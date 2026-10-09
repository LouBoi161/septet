//! Septet's MCP server for Claude Code: Streamable HTTP on 127.0.0.1, JSON responses only.
//!
//! One server per Septet process, on a random port, with a fresh 244-bit bearer token. Each
//! connection gets a thread; a `tools/call` is handed to the UI thread (which owns the shell) and the
//! connection waits for its answer, so a tool may take as long as it needs (Claude Code's own limit
//! is raised through `MCP_TOOL_TIMEOUT`, see `cli.rs`).

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::{Value, json};

/// Largest request body accepted (tool arguments are small; images only flow the other way).
const MAX_BODY: usize = 16 << 20;

/// A tool call waiting for the UI thread.
pub struct ToolCall {
    pub name: String,
    pub args: Value,
    pub reply: Sender<ToolReply>,
}

#[derive(Clone, Debug)]
pub struct ToolReply {
    /// MCP content blocks: `{"type":"text","text":…}` or `{"type":"image","data":…,"mimeType":…}`.
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolReply {
    pub fn text(text: impl Into<String>) -> Self {
        ToolReply { content: vec![json!({"type": "text", "text": text.into()})], is_error: false }
    }

    pub fn json(value: &Value) -> Self {
        Self::text(serde_json::to_string_pretty(value).unwrap_or_default())
    }

    pub fn error(text: impl Into<String>) -> Self {
        ToolReply { is_error: true, ..Self::text(text) }
    }
}

pub struct Server {
    pub port: u16,
    token: String,
    pub calls: Receiver<ToolCall>,
    _stop: StopOnDrop,
}

impl Server {
    /// Listen on a random loopback port. `tools` is the `tools/list` answer; `wake` runs when a call arrives.
    pub fn start(tools: Vec<Value>, wake: impl Fn() + Send + Sync + 'static) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let (tx, calls) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared { token: token.clone(), tools, wake: Box::new(wake) });
        let stopping = stop.clone();
        std::thread::Builder::new().name("septet-mcp".into()).spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (shared, tx) = (shared.clone(), tx.clone());
                let _ = std::thread::Builder::new().name("septet-mcp-conn".into()).spawn(move || serve(stream, &shared, &tx));
            }
        })?;
        Ok(Server { port, token, calls, _stop: StopOnDrop { flag: stop, port } })
    }

    /// The `--mcp-config` for Claude Code. Holds the token: write it only where other users can't read it.
    pub fn config(&self) -> String {
        json!({"mcpServers": {"septet": {
            "type": "http",
            "url": format!("http://127.0.0.1:{}/mcp", self.port),
            "headers": {"Authorization": format!("Bearer {}", self.token)},
        }}})
        .to_string()
    }
}

/// Ends the accept loop when the server goes away.
struct StopOnDrop {
    flag: Arc<AtomicBool>,
    port: u16,
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::Relaxed);
        // Wake the accept loop so it sees the flag.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

struct Shared {
    token: String,
    tools: Vec<Value>,
    wake: Box<dyn Fn() + Send + Sync>,
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn read_request(r: &mut impl BufRead) -> Option<Request> {
    let mut line = String::new();
    if r.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_owned(), parts.next()?.to_owned());
    let mut headers = Vec::new();
    loop {
        line.clear();
        r.read_line(&mut line).ok()?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let (k, v) = l.split_once(':')?;
        headers.push((k.trim().to_owned(), v.trim().to_owned()));
        if headers.len() > 100 {
            return None;
        }
    }
    let mut req = Request { method, path, headers, body: Vec::new() };
    let len: usize = req.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return None;
    }
    req.body.resize(len, 0);
    r.read_exact(&mut req.body).ok()?;
    Some(req)
}

fn respond(w: &mut impl Write, status: &str, extra: &[(&str, &str)], body: &[u8]) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    w.write_all(head.as_bytes())?;
    w.write_all(body)?;
    w.flush()
}

/// Compare without leaking how much of the token matched.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn serve(stream: TcpStream, shared: &Shared, calls: &Sender<ToolCall>) {
    let Ok(mut out) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    while let Some(req) = read_request(&mut reader) {
        let authorized = req.header("authorization").and_then(|v| v.strip_prefix("Bearer ")).is_some_and(|t| same(t.as_bytes(), shared.token.as_bytes()));
        let result = if req.path.split('?').next() != Some("/mcp") {
            respond(&mut out, "404 Not Found", &[], b"")
        } else if !authorized {
            respond(&mut out, "401 Unauthorized", &[], b"")
        } else {
            match req.method.as_str() {
                "POST" => match handle(&req.body, shared, calls) {
                    Some((body, session)) => {
                        let extra: &[(&str, &str)] = if session { &[("Mcp-Session-Id", "septet")] } else { &[] };
                        respond(&mut out, "200 OK", extra, body.as_bytes())
                    }
                    None => respond(&mut out, "202 Accepted", &[], b""),
                },
                // No server-initiated messages: no SSE stream to open.
                "GET" => respond(&mut out, "405 Method Not Allowed", &[("Allow", "POST, DELETE")], b""),
                "DELETE" => respond(&mut out, "200 OK", &[], b""),
                _ => respond(&mut out, "405 Method Not Allowed", &[("Allow", "POST, DELETE")], b""),
            }
        };
        if result.is_err() || req.header("connection").is_some_and(|c| c.eq_ignore_ascii_case("close")) {
            break;
        }
    }
    let _ = out.shutdown(Shutdown::Both);
}

/// One JSON-RPC message. Returns the response body (None for notifications) and whether it was `initialize`.
fn handle(body: &[u8], shared: &Shared, calls: &Sender<ToolCall>) -> Option<(String, bool)> {
    let msg: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return Some((json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}).to_string(), false)),
    };
    let id = msg.get("id")?.clone();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "septet", "version": env!("CARGO_PKG_VERSION")},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": shared.tools})),
        "tools/call" => Ok(call(&params, shared, calls)),
        _ => Err((-32601, format!("Unknown method {method}"))),
    };
    let reply = match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    };
    Some((reply.to_string(), method == "initialize"))
}

fn call(params: &Value, shared: &Shared, calls: &Sender<ToolCall>) -> Value {
    let (reply, rx) = channel();
    let call = ToolCall {
        name: params.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
        args: params.get("arguments").cloned().unwrap_or(json!({})),
        reply,
    };
    let r = if calls.send(call).is_ok() {
        (shared.wake)();
        rx.recv().unwrap_or_else(|_| ToolReply::error("Septet stopped before answering."))
    } else {
        ToolReply::error("Septet is shutting down.")
    };
    json!({"content": r.content, "isError": r.is_error})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn post(port: u16, token: &str, body: &str) -> (String, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "POST /mcp HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        (head.lines().next().unwrap().to_owned(), body.to_owned())
    }

    #[test]
    fn round_trip() {
        let tools = vec![json!({"name": "echo", "inputSchema": {"type": "object"}})];
        let server = Server::start(tools, || {}).unwrap();
        let token = server.token.clone();
        let port = server.port;

        let (status, _) = post(port, "wrong", r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        assert!(status.contains("401"));

        let (status, body) = post(port, &token, r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#);
        assert!(status.contains("200"));
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2025-11-25");

        let (status, _) = post(port, &token, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        assert!(status.contains("202"));

        let (_, body) = post(port, &token, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        assert!(body.contains("\"echo\""));

        // A tool call is answered by whoever drains `calls` (the UI thread in Septet).
        let calls = server.calls;
        let t = std::thread::spawn(move || {
            let c = calls.recv().unwrap();
            c.reply.send(ToolReply::text(format!("hi {}", c.args["who"]))).unwrap();
        });
        let (_, body) = post(
            port,
            &token,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"who":"you"},"_meta":{"claudecode/toolUseId":"toolu_1"}}}"#,
        );
        t.join().unwrap();
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["result"]["content"][0]["text"], "hi \"you\"");
        assert_eq!(v["result"]["isError"], false);
    }
}
