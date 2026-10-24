//! ComfyUI's HTTP API, as far as Septet needs it: is it there, which nodes and models it has,
//! upload an image, queue a graph, wait for it (with the step progress from its WebSocket when
//! there is one), fetch the results, cancel, and unload the models.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{Value, json};

/// The largest result or answer Septet reads (an 8-bit 2816 × 2816 PNG is far below it).
const MAX_BODY: u64 = 256 << 20;

pub struct Client {
    /// `http://127.0.0.1:8188`, without a trailing slash.
    base: String,
    agent: ureq::Agent,
    /// Tells this client's WebSocket messages from those of other clients (the user's browser).
    pub id: String,
}

/// A ComfyUI answer that is not what Septet expected, in words for the user.
pub type Result<T> = std::result::Result<T, String>;

impl Client {
    pub fn new(base: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(3)))
            .timeout_global(Some(Duration::from_secs(120)))
            // ComfyUI says what is wrong with a graph in the body of a 400.
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .new_agent();
        Client { base: base.trim_end_matches('/').to_owned(), agent, id: format!("septet-{}", uuid::Uuid::new_v4().simple()) }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn get(&self, path: &str) -> Result<(u16, Vec<u8>)> {
        let mut resp = self.agent.get(format!("{}{path}", self.base)).call().map_err(|e| self.unreachable(e))?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().with_config().limit(MAX_BODY).read_to_vec().map_err(|e| e.to_string())?;
        Ok((status, body))
    }

    fn post(&self, path: &str, content_type: &str, body: &[u8]) -> Result<(u16, Vec<u8>)> {
        let mut resp = self.agent.post(format!("{}{path}", self.base)).header("Content-Type", content_type).send(body).map_err(|e| self.unreachable(e))?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().with_config().limit(MAX_BODY).read_to_vec().map_err(|e| e.to_string())?;
        Ok((status, body))
    }

    fn post_json(&self, path: &str, body: &Value) -> Result<(u16, Vec<u8>)> {
        self.post(path, "application/json", body.to_string().as_bytes())
    }

    fn unreachable(&self, e: ureq::Error) -> String {
        format!("ComfyUI at {} does not answer ({e}).", self.base)
    }

    /// Whether a ComfyUI answers there, and its version.
    pub fn version(&self) -> Result<String> {
        let (status, body) = self.get("/system_stats")?;
        let v: Value = json_body(status, &body)?;
        Ok(v["system"]["comfyui_version"].as_str().unwrap_or("unknown").to_owned())
    }

    /// The free memory of the graphics card ComfyUI uses, in MiB (`None`: it doesn't say).
    pub fn vram_free_mib(&self) -> Option<u64> {
        let (status, body) = self.get("/system_stats").ok()?;
        let v: Value = json_body(status, &body).ok()?;
        Some(v["devices"].get(0)?["vram_free"].as_u64()? >> 20)
    }

    /// Whether it answers within a moment (for start-up and idle checks).
    pub fn up(&self) -> bool {
        self.version().is_ok()
    }

    /// The node classes of `graph` this ComfyUI lacks, and the model files it names that are not
    /// in the loader's list.
    pub fn missing(&self, graph: &Value) -> Result<Vec<String>> {
        let mut missing = Vec::new();
        let Some(nodes) = graph.as_object() else { return Ok(missing) };
        let mut classes: Vec<&str> = nodes.values().filter_map(|n| n["class_type"].as_str()).collect();
        classes.sort_unstable();
        classes.dedup();
        for class in classes {
            let (status, body) = self.get(&format!("/object_info/{class}"))?;
            let info: Value = json_body(status, &body)?;
            let Some(spec) = info.get(class) else {
                missing.push(format!("the node “{class}” (update ComfyUI)"));
                continue;
            };
            // Model pickers list their files as the first element of the input's spec.
            for node in nodes.values().filter(|n| n["class_type"] == class) {
                let Some(inputs) = node["inputs"].as_object() else { continue };
                for (name, value) in inputs {
                    let Some(file) = value.as_str().filter(|f| f.ends_with(".safetensors") || f.ends_with(".gguf") || f.ends_with(".ckpt")) else { continue };
                    let choices = &spec["input"]["required"][name][0];
                    if choices.as_array().is_some_and(|c| !c.iter().any(|x| x == file)) {
                        missing.push(format!("the model “{file}”"));
                    }
                }
            }
        }
        Ok(missing)
    }

    /// Upload a PNG to ComfyUI's input folder → the name a `LoadImage` node takes.
    pub fn upload_png(&self, name: &str, png: &[u8]) -> Result<String> {
        let boundary = format!("septet{}", uuid::Uuid::new_v4().simple());
        let mut body = Vec::with_capacity(png.len() + 512);
        let _ = write!(body, "--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{name}\"\r\nContent-Type: image/png\r\n\r\n");
        body.extend_from_slice(png);
        let _ = write!(body, "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"overwrite\"\r\n\r\ntrue\r\n--{boundary}--\r\n");
        let (status, answer) = self.post("/upload/image", &format!("multipart/form-data; boundary={boundary}"), &body)?;
        let v: Value = json_body(status, &answer)?;
        let file = v["name"].as_str().ok_or("ComfyUI did not take the image.")?;
        Ok(match v["subfolder"].as_str().filter(|s| !s.is_empty()) {
            Some(sub) => format!("{sub}/{file}"),
            None => file.to_owned(),
        })
    }

    /// Queue `graph` → its prompt id.
    pub fn queue(&self, graph: &Value) -> Result<String> {
        let (status, body) = self.post_json("/prompt", &json!({ "prompt": graph, "client_id": self.id }))?;
        let v: Value = serde_json::from_slice(&body).map_err(|_| format!("ComfyUI answered {status} to the job."))?;
        if status != 200 {
            return Err(graph_error(&v));
        }
        v["prompt_id"].as_str().map(str::to_owned).ok_or_else(|| "ComfyUI did not queue the job.".into())
    }

    /// The finished job's state: `None` while it is queued or running.
    pub fn history(&self, prompt: &str) -> Result<Option<Value>> {
        let (status, body) = self.get(&format!("/history/{prompt}"))?;
        let v: Value = json_body(status, &body)?;
        Ok(v.get(prompt).cloned())
    }

    /// The images a finished job saved, in node order: `(filename, subfolder, type)`.
    pub fn outputs(entry: &Value) -> Result<Vec<(String, String, String)>> {
        let status = &entry["status"];
        if status["status_str"] == "error" {
            let message = status["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|m| m[0] == "execution_error")
                .map(|m| {
                    let node = m[1]["node_type"].as_str().unwrap_or("a node");
                    let what = m[1]["exception_message"].as_str().unwrap_or("failed").trim();
                    format!("{node}: {what}")
                })
                .next()
                .unwrap_or_else(|| "The job failed in ComfyUI.".into());
            return Err(friendly(&message));
        }
        let mut files = Vec::new();
        let mut nodes: Vec<(&String, &Value)> = entry["outputs"].as_object().map(|o| o.iter().collect()).unwrap_or_default();
        nodes.sort_by_key(|(id, _)| id.parse::<u64>().unwrap_or(u64::MAX));
        for (_, out) in nodes {
            for img in out["images"].as_array().into_iter().flatten() {
                if let Some(name) = img["filename"].as_str() {
                    files.push((name.to_owned(), img["subfolder"].as_str().unwrap_or("").to_owned(), img["type"].as_str().unwrap_or("output").to_owned()));
                }
            }
        }
        Ok(files)
    }

    /// One saved image's bytes.
    pub fn view(&self, file: &(String, String, String)) -> Result<Vec<u8>> {
        let (name, sub, kind) = file;
        let (status, body) = self.get(&format!("/view?filename={}&subfolder={}&type={}", encode(name), encode(sub), encode(kind)))?;
        if status != 200 {
            return Err(format!("ComfyUI could not hand over {name} ({status})."));
        }
        Ok(body)
    }

    /// Stop the running job and take Septet's waiting ones off the queue.
    pub fn cancel(&self, prompt: &str) {
        let _ = self.post_json("/queue", &json!({ "delete": [prompt] }));
        let _ = self.post_json("/interrupt", &json!({ "prompt_id": prompt }));
    }

    /// Unload every model and give back the memory ComfyUI holds.
    pub fn free(&self) -> Result<()> {
        let (status, _) = self.post_json("/free", &json!({ "unload_models": true, "free_memory": true }))?;
        if status == 200 { Ok(()) } else { Err(format!("ComfyUI answered {status} to unloading.")) }
    }

    /// ComfyUI's progress messages for this client, if its WebSocket opens.
    pub fn progress(&self) -> Option<Progress> {
        Progress::connect(&self.base, &self.id)
    }
}

fn json_body(status: u16, body: &[u8]) -> Result<Value> {
    if status != 200 {
        return Err(format!("ComfyUI answered {status}."));
    }
    serde_json::from_slice(body).map_err(|_| "ComfyUI's answer is not JSON.".into())
}

/// What ComfyUI says is wrong with a graph, shortest first.
fn graph_error(v: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(e) = v["error"]["message"].as_str() {
        lines.push(e.to_owned());
    }
    for (_, node) in v["node_errors"].as_object().into_iter().flatten() {
        let class = node["class_type"].as_str().unwrap_or("node");
        for e in node["errors"].as_array().into_iter().flatten() {
            let what = e["details"].as_str().filter(|d| !d.is_empty()).or_else(|| e["message"].as_str()).unwrap_or("invalid");
            lines.push(format!("{class}: {what}"));
        }
    }
    if lines.is_empty() { "ComfyUI did not take the job.".into() } else { friendly(&lines.join("; ")) }
}

/// Out-of-memory errors in words the user can act on.
fn friendly(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("out of memory") || lower.contains("allocation on device") {
        return format!("The graphics card ran out of memory. Close other programs that use it, or try a smaller size. ({message})");
    }
    message.to_owned()
}

/// Percent-encoding for a query value.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// ComfyUI's WebSocket, read for `progress` messages only. A plain socket on the local machine:
/// Septet never talks to a ComfyUI elsewhere over it (the address may be remote, then there is
/// no step count, only the elapsed time).
pub struct Progress {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl Progress {
    fn connect(base: &str, client: &str) -> Option<Progress> {
        let host = base.strip_prefix("http://")?;
        let host = host.split('/').next()?;
        let addr = std::net::ToSocketAddrs::to_socket_addrs(host).ok()?.next()?;
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, uuid::Uuid::new_v4().as_bytes());
        write!(stream, "GET /ws?clientId={client} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").ok()?;
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() > 8192 || stream.read(&mut byte).ok()? == 0 {
                return None;
            }
            head.push(byte[0]);
        }
        if !head.starts_with(b"HTTP/1.1 101") {
            return None;
        }
        stream.set_read_timeout(Some(Duration::from_millis(200))).ok()?;
        Some(Progress { stream, buf: Vec::new() })
    }

    /// The text messages that arrived since the last call (binary previews are skipped). `None`:
    /// the socket closed.
    pub fn messages(&mut self) -> Option<Vec<Value>> {
        let mut chunk = [0u8; 16384];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => return None,
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    if self.buf.len() > (64 << 20) {
                        return None;
                    }
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
                Err(_) => return None,
            }
        }
        let mut out = Vec::new();
        while let Some((opcode, payload, used)) = frame(&self.buf) {
            if opcode == 1
                && let Ok(v) = serde_json::from_slice::<Value>(&payload)
            {
                out.push(v);
            }
            self.buf.drain(..used);
            if opcode == 8 {
                return None;
            }
        }
        Some(out)
    }
}

/// One complete server frame at the start of `buf` → (opcode, payload, bytes used). Server frames
/// are not masked; ComfyUI does not fragment its JSON.
fn frame(buf: &[u8]) -> Option<(u8, Vec<u8>, usize)> {
    let (&b0, &b1) = (buf.first()?, buf.get(1)?);
    let masked = b1 & 0x80 != 0;
    let (len, mut at) = match b1 & 0x7f {
        126 => (u64::from(u16::from_be_bytes([*buf.get(2)?, *buf.get(3)?])), 4),
        127 => (u64::from_be_bytes(buf.get(2..10)?.try_into().ok()?), 10),
        n => (u64::from(n), 2),
    };
    let mask = if masked {
        let m: [u8; 4] = buf.get(at..at + 4)?.try_into().ok()?;
        at += 4;
        Some(m)
    } else {
        None
    };
    let end = at.checked_add(usize::try_from(len).ok()?)?;
    let mut payload = buf.get(at..end)?.to_vec();
    if let Some(m) = mask {
        payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= m[i % 4]);
    }
    Some((b0 & 0x0f, payload, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_parse_short_long_and_partial() {
        let mut buf = vec![0x81, 5];
        buf.extend_from_slice(b"hello");
        assert_eq!(frame(&buf), Some((1, b"hello".to_vec(), 7)));
        assert_eq!(frame(&buf[..4]), None, "incomplete");
        let long = vec![b'x'; 300];
        let mut buf = vec![0x82, 126, 1, 44];
        buf.extend_from_slice(&long);
        assert_eq!(frame(&buf).map(|(o, p, n)| (o, p.len(), n)), Some((2, 300, 304)));
        assert_eq!(frame(&[0x81, 127, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]), None, "absurd length");
    }

    #[test]
    fn errors_read_as_sentences() {
        let v = json!({"error": {"message": "Prompt outputs failed validation"}, "node_errors": {"1": {"class_type": "UNETLoader", "errors": [{"message": "Value not in list", "details": "unet_name: 'x' not in []"}]}}});
        assert_eq!(graph_error(&v), "Prompt outputs failed validation; UNETLoader: unet_name: 'x' not in []");
        assert!(friendly("CUDA out of memory. Tried to allocate").starts_with("The graphics card ran out of memory"));
        let entry = json!({"status": {"status_str": "error", "messages": [["execution_start", {}], ["execution_error", {"node_type": "VAEDecode", "exception_message": "boom\n"}]]}});
        assert_eq!(Client::outputs(&entry), Err("VAEDecode: boom".into()));
        let entry = json!({"status": {"status_str": "success"}, "outputs": {"9": {"images": [{"filename": "b.png", "subfolder": "", "type": "output"}]}, "8": {"images": [{"filename": "a.png", "subfolder": "s", "type": "temp"}]}}});
        assert_eq!(Client::outputs(&entry).unwrap(), vec![("a.png".into(), "s".into(), "temp".into()), ("b.png".into(), "".into(), "output".into())]);
        assert_eq!(encode("a b/ü.png"), "a%20b%2F%C3%BC.png");
    }
}
