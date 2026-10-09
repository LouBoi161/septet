//! Claude Code's `stream-json` protocol (`claude -p --input-format stream-json --output-format stream-json`).
//!
//! Parsed from `serde_json::Value` rather than strict types: the CLI adds fields and event types
//! between versions, and anything unknown must pass through as [`Event::Other`]. See
//! `docs/assistant/protocol-notes.md` for what each event means.

use serde_json::{Value, json};

/// One thing that happened, decoded from a line of output. A line can carry several.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Sent at the start of every turn.
    Init(Init),
    /// `system/status`, e.g. "requesting" before each API call.
    Status(String),
    /// A streamed piece of the message being written (`--include-partial-messages`).
    Delta(Delta),
    /// A finished content block of Claude's message.
    Block(Block),
    /// The result of a tool Claude called.
    ToolResult {
        tool_use_id: String,
        content: Vec<Part>,
        is_error: bool,
    },
    /// Text the CLI put on the user side, e.g. "[Request interrupted by user]".
    UserText(String),
    RateLimit(RateLimit),
    /// The reply to a `control_request` we sent.
    ControlResponse {
        request_id: String,
        ok: bool,
    },
    /// The end of a turn.
    Result(TurnResult),
    /// Anything this version of Septet does not know about.
    Other,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Init {
    pub session_id: String,
    pub model: String,
    pub version: String,
    /// "none" when the subscription is used (no API key in the environment).
    pub api_key_source: String,
    pub permission_mode: String,
    pub tools: Vec<String>,
    /// MCP servers and their status ("connected", "failed", …).
    pub mcp_servers: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Delta {
    /// A new message starts (one per API call).
    MessageStart,
    /// A content block starts at `index`; `kind` is "text", "thinking" or "tool_use".
    BlockStart {
        index: usize,
        kind: String,
        tool: Option<(String, String)>,
    },
    Text {
        index: usize,
        text: String,
    },
    Thinking {
        index: usize,
        text: String,
    },
    ToolInput {
        index: usize,
        json: String,
    },
    BlockStop {
        index: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Text(String),
    /// The CLI sends thinking blocks with empty text; they only show that Claude thought.
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

/// Part of a tool result.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Image { media_type: String, data: String },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RateLimit {
    /// "allowed", or something else once a limit is hit.
    pub status: String,
    pub kind: String,
    /// Unix seconds.
    pub resets_at: Option<u64>,
    /// 0.0..=1.0 of the current window, when reported.
    pub utilization: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnResult {
    pub subtype: String,
    pub is_error: bool,
    /// The final text, absent when the turn was interrupted.
    pub text: Option<String>,
    pub num_turns: u64,
    pub cost_usd: f64,
    pub terminal_reason: String,
    /// Tools the user declined during this turn.
    pub denials: Vec<String>,
}

impl TurnResult {
    pub fn interrupted(&self) -> bool {
        self.terminal_reason.starts_with("aborted")
    }
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn idx(v: &Value) -> usize {
    v.get("index").and_then(Value::as_u64).unwrap_or(0) as usize
}

/// Decode one line of `stream-json` output. Lines that are not JSON give `Err`.
pub fn parse_line(line: &str) -> Result<Vec<Event>, serde_json::Error> {
    let v: Value = serde_json::from_str(line)?;
    Ok(parse(&v))
}

pub fn parse(v: &Value) -> Vec<Event> {
    match v.get("type").and_then(Value::as_str).unwrap_or_default() {
        "system" => vec![match v.get("subtype").and_then(Value::as_str) {
            Some("init") => Event::Init(Init {
                session_id: s(v, "session_id"),
                model: s(v, "model"),
                version: s(v, "claude_code_version"),
                api_key_source: s(v, "apiKeySource"),
                permission_mode: s(v, "permissionMode"),
                tools: v.get("tools").and_then(Value::as_array).into_iter().flatten().filter_map(|t| t.as_str().map(str::to_owned)).collect(),
                mcp_servers: v.get("mcp_servers").and_then(Value::as_array).into_iter().flatten().map(|m| (s(m, "name"), s(m, "status"))).collect(),
            }),
            Some("status") => Event::Status(s(v, "status")),
            _ => Event::Other,
        }],
        "stream_event" => vec![parse_stream_event(v.get("event").unwrap_or(&Value::Null))],
        "assistant" => content(v).iter().filter_map(parse_block).map(Event::Block).collect(),
        "user" => {
            let msg = v.get("message").and_then(|m| m.get("content"));
            if let Some(text) = msg.and_then(Value::as_str) {
                return vec![Event::UserText(text.to_owned())];
            }
            content(v)
                .iter()
                .map(|b| match b.get("type").and_then(Value::as_str) {
                    Some("tool_result") => Event::ToolResult {
                        tool_use_id: s(b, "tool_use_id"),
                        content: parts(b.get("content").unwrap_or(&Value::Null)),
                        is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                    },
                    Some("text") => Event::UserText(s(b, "text")),
                    _ => Event::Other,
                })
                .collect()
        }
        "rate_limit_event" => {
            let info = v.get("rate_limit_info").unwrap_or(&Value::Null);
            let kind = s(info, "rateLimitType");
            vec![Event::RateLimit(RateLimit {
                status: s(info, "status"),
                resets_at: info.get("resetsAt").and_then(Value::as_u64),
                utilization: info.pointer(&format!("/unifiedWindows/{kind}/utilization")).and_then(Value::as_f64),
                kind,
            })]
        }
        "control_response" => {
            let r = v.get("response").unwrap_or(&Value::Null);
            vec![Event::ControlResponse { request_id: s(r, "request_id"), ok: r.get("subtype").and_then(Value::as_str) == Some("success") }]
        }
        "result" => vec![Event::Result(TurnResult {
            subtype: s(v, "subtype"),
            is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            text: v.get("result").and_then(Value::as_str).map(str::to_owned),
            num_turns: v.get("num_turns").and_then(Value::as_u64).unwrap_or(0),
            cost_usd: v.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
            terminal_reason: s(v, "terminal_reason"),
            denials: v.get("permission_denials").and_then(Value::as_array).into_iter().flatten().map(|d| s(d, "tool_name")).collect(),
        })],
        _ => vec![Event::Other],
    }
}

fn content(v: &Value) -> &[Value] {
    v.pointer("/message/content").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

fn parse_block(b: &Value) -> Option<Block> {
    match b.get("type")?.as_str()? {
        "text" => Some(Block::Text(s(b, "text"))),
        "thinking" => Some(Block::Thinking(s(b, "thinking"))),
        "tool_use" => Some(Block::ToolUse { id: s(b, "id"), name: s(b, "name"), input: b.get("input").cloned().unwrap_or(Value::Null) }),
        _ => None,
    }
}

/// A tool result's content: a plain string or a list of text and image blocks.
fn parts(c: &Value) -> Vec<Part> {
    match c {
        Value::String(t) => vec![Part::Text(t.clone())],
        Value::Array(items) => items
            .iter()
            .filter_map(|p| match p.get("type")?.as_str()? {
                "text" => Some(Part::Text(s(p, "text"))),
                "image" => {
                    let src = p.get("source")?;
                    Some(Part::Image { media_type: s(src, "media_type"), data: s(src, "data") })
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_stream_event(e: &Value) -> Event {
    let delta = match e.get("type").and_then(Value::as_str).unwrap_or_default() {
        "message_start" => Delta::MessageStart,
        "content_block_start" => {
            let b = e.get("content_block").unwrap_or(&Value::Null);
            let kind = s(b, "type");
            let tool = (kind == "tool_use").then(|| (s(b, "id"), s(b, "name")));
            Delta::BlockStart { index: idx(e), kind, tool }
        }
        "content_block_delta" => {
            let d = e.get("delta").unwrap_or(&Value::Null);
            match d.get("type").and_then(Value::as_str).unwrap_or_default() {
                "text_delta" => Delta::Text { index: idx(e), text: s(d, "text") },
                "thinking_delta" => Delta::Thinking { index: idx(e), text: s(d, "thinking") },
                "input_json_delta" => Delta::ToolInput { index: idx(e), json: s(d, "partial_json") },
                _ => return Event::Other,
            }
        }
        "content_block_stop" => Delta::BlockStop { index: idx(e) },
        _ => return Event::Other,
    };
    Event::Delta(delta)
}

/// An image to send along with a message.
#[derive(Clone, Debug)]
pub struct ImageInput {
    pub media_type: String,
    /// Base64.
    pub data: String,
}

/// A user message, as one line for the CLI's stdin.
pub fn user_message(session_id: &str, text: &str, images: &[ImageInput]) -> String {
    let content = if images.is_empty() {
        json!(text)
    } else {
        let mut blocks = vec![json!({"type": "text", "text": text})];
        blocks.extend(images.iter().map(|i| json!({"type": "image", "source": {"type": "base64", "media_type": i.media_type, "data": i.data}})));
        Value::Array(blocks)
    };
    json!({"type": "user", "message": {"role": "user", "content": content}, "parent_tool_use_id": null, "session_id": session_id}).to_string()
}

/// Stop the running turn. The process stays alive for the next message.
pub fn interrupt(request_id: &str) -> String {
    json!({"type": "control_request", "request_id": request_id, "request": {"subtype": "interrupt"}}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(fixture: &str) -> Vec<Event> {
        let path = format!("{}/tests/fixtures/assistant/{fixture}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap().lines().flat_map(|l| parse_line(l).unwrap()).collect()
    }

    #[test]
    fn conversation() {
        let ev = events("conversation.jsonl");
        let inits: Vec<&Init> = ev.iter().filter_map(|e| if let Event::Init(i) = e { Some(i) } else { None }).collect();
        assert_eq!(inits.len(), 5, "one init per turn");
        assert_eq!(inits[0].api_key_source, "none");
        assert_eq!(inits[0].model, "claude-haiku-5-5");
        assert_eq!(inits[0].mcp_servers, vec![("septet".to_owned(), "connected".to_owned())]);
        assert!(inits[0].tools.iter().any(|t| t == "mcp__septet__septet_screenshot"));

        let results: Vec<&TurnResult> = ev.iter().filter_map(|e| if let Event::Result(r) = e { Some(r) } else { None }).collect();
        assert_eq!(results.len(), 5);
        assert!(results[0].text.as_deref().unwrap().contains("Red"));
        assert_eq!(results[1].denials, vec!["Bash"]);
        assert_eq!(results[2].text.as_deref(), Some("**Blue**"));
        assert!(results[3].is_error && results[3].interrupted() && results[3].text.is_none());
        assert_eq!(results[4].text.as_deref(), Some("still-alive"));

        // The screenshot tool's result is an image; the declined Bash call an error.
        let image = ev.iter().any(|e| matches!(e, Event::ToolResult { content, .. } if content.iter().any(|p| matches!(p, Part::Image { media_type, .. } if media_type == "image/png"))));
        assert!(image);
        assert!(
            ev.iter().any(|e| matches!(e, Event::ToolResult { is_error: true, content, .. } if content == &[Part::Text("The user declined this.".into())]))
        );
        assert!(ev.iter().any(|e| matches!(e, Event::Block(Block::ToolUse { name, input, .. }) if name == "Bash" && input["command"] == "echo spike-ok")));
        assert!(ev.iter().any(|e| matches!(e, Event::ControlResponse { request_id, ok: true } if request_id == "int-1")));
        assert!(ev.iter().any(|e| matches!(e, Event::UserText(t) if t == "[Request interrupted by user]")));
        assert!(ev.iter().any(|e| matches!(e, Event::RateLimit(r) if r.status == "allowed" && r.utilization.is_some())));

        // The streamed text of the interrupted story adds up to the text block that followed it.
        let streamed: String = ev.iter().filter_map(|e| if let Event::Delta(Delta::Text { text, .. }) = e { Some(text.as_str()) } else { None }).collect();
        assert!(streamed.contains("The Keeper's Last Winter"));
    }

    #[test]
    fn resume() {
        let ev = events("resume.jsonl");
        assert!(ev.iter().any(|e| matches!(e, Event::Result(r) if r.text.as_deref() == Some("Red"))));
    }

    #[test]
    fn unknown_lines_pass() {
        assert_eq!(parse_line(r#"{"type":"something_new","x":1}"#).unwrap(), vec![Event::Other]);
        assert_eq!(parse_line(r#"{"type":"system","subtype":"thinking_tokens"}"#).unwrap(), vec![Event::Other]);
        assert!(parse_line("not json").is_err());
    }

    #[test]
    fn messages() {
        let m: Value = serde_json::from_str(&user_message("s1", "hi", &[])).unwrap();
        assert_eq!(m["message"]["content"], "hi");
        let m: Value = serde_json::from_str(&user_message("s1", "look", &[ImageInput { media_type: "image/png".into(), data: "AAA".into() }])).unwrap();
        assert_eq!(m["message"]["content"][1]["source"]["data"], "AAA");
        let i: Value = serde_json::from_str(&interrupt("r1")).unwrap();
        assert_eq!(i["request"]["subtype"], "interrupt");
    }
}
