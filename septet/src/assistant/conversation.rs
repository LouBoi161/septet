//! A running conversation: the Claude Code process, what has been said, and pending approvals.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use serde_json::{Value, json};

use super::mcp::ToolReply;
use super::protocol::{Block, Delta, Event, Part, RateLimit, TurnResult};
use super::session::{Output, Session};

/// One item in the transcript.
#[derive(Clone, Debug)]
pub enum Entry {
    User(String),
    Text(String),
    /// Claude thought (the CLI does not pass the text on).
    Thinking,
    Tool {
        id: String,
        name: String,
        input: Value,
        result: Option<(Vec<Part>, bool)>,
    },
    /// Something Septet says: errors, "stopped", limits.
    Note(String),
}

/// A tool waiting for the user's yes or no.
pub struct Approval {
    pub tool: String,
    pub input: Value,
    pub reply: Sender<ToolReply>,
}

/// What the user chose for an approval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Choice {
    Once,
    /// The same request again in this conversation (for Bash: the same command).
    Always,
    Deny,
}

/// The answer `--permission-prompt-tool` expects.
pub fn decision(allow: bool, input: &Value) -> ToolReply {
    let d = if allow { json!({"behavior": "allow", "updatedInput": input}) } else { json!({"behavior": "deny", "message": "The user declined this."}) };
    ToolReply::text(d.to_string())
}

/// What "allow for this conversation" covers: a Bash command exactly, other tools by name and path.
pub fn approval_key(tool: &str, input: &Value) -> String {
    match tool {
        "Bash" => format!("Bash:{}", input["command"].as_str().unwrap_or_default()),
        // "For this conversation" covers the whole site.
        "WebFetch" => format!("WebFetch:{}", input["url"].as_str().and_then(host).unwrap_or_default()),
        _ => match input.get("file_path").or_else(|| input.get("path")).and_then(Value::as_str) {
            Some(p) => format!("{tool}:{p}"),
            None => tool.to_owned(),
        },
    }
}

/// The host of an http(s) URL, lower-case.
pub fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host.starts_with('[') { host.split_inclusive(']').next()? } else { host.split(':').next()? };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

pub struct Conversation {
    pub session: Session,
    pub workspace: PathBuf,
    /// The `--mcp-config` file (it holds the server token); removed when the conversation ends.
    pub config_file: Option<PathBuf>,
    pub entries: Vec<Entry>,
    /// A turn is running.
    pub busy: bool,
    pub approvals: Vec<Approval>,
    pub allowed: HashSet<String>,
    pub model: String,
    pub rate_limit: Option<RateLimit>,
    pub last_result: Option<TurnResult>,
    pub ended: bool,
    /// The model and effort Claude Code was started with.
    pub started_with: (Option<String>, Option<String>),
    /// The text entry the streamed deltas of each content block go to (block index → entry index).
    streaming: Vec<(usize, usize)>,
}

impl Conversation {
    pub fn new(session: Session, workspace: PathBuf, config_file: Option<PathBuf>) -> Self {
        Conversation {
            session,
            workspace,
            config_file,
            entries: Vec::new(),
            busy: false,
            approvals: Vec::new(),
            allowed: HashSet::new(),
            model: String::new(),
            rate_limit: None,
            last_result: None,
            ended: false,
            started_with: (None, None),
            streaming: Vec::new(),
        }
    }

    pub fn send(&mut self, text: &str) {
        match self.session.send(text, &[]) {
            Ok(()) => {
                self.entries.push(Entry::User(text.to_owned()));
                self.busy = true;
            }
            Err(e) => self.entries.push(Entry::Note(format!("Could not reach Claude Code: {e}"))),
        }
    }

    pub fn interrupt(&mut self) {
        // Pending approvals are declined: Claude Code waits for them before it can stop.
        for a in self.approvals.drain(..) {
            let _ = a.reply.send(decision(false, &a.input));
        }
        let _ = self.session.interrupt();
    }

    pub fn answer(&mut self, index: usize, choice: Choice) {
        if index >= self.approvals.len() {
            return;
        }
        let a = self.approvals.remove(index);
        if choice == Choice::Always {
            self.allowed.insert(approval_key(&a.tool, &a.input));
        }
        let _ = a.reply.send(decision(choice != Choice::Deny, &a.input));
    }

    /// Take in what Claude Code sent since the last frame.
    pub fn update(&mut self) {
        for out in self.session.drain() {
            match out {
                Output::Event(e) => self.event(e),
                Output::Noise(_line) => {
                    #[cfg(debug_assertions)]
                    eprintln!("claude: {_line}");
                }
                Output::Exited { stderr } => {
                    self.ended = true;
                    self.busy = false;
                    let why = if stderr.is_empty() { String::new() } else { format!(": {stderr}") };
                    self.entries.push(Entry::Note(format!("Claude Code stopped{why}")));
                }
            }
        }
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::Init(i) => self.model = i.model,
            Event::Delta(Delta::MessageStart) => self.streaming.clear(),
            Event::Delta(Delta::BlockStart { index, kind, .. }) if kind == "text" => {
                self.entries.push(Entry::Text(String::new()));
                self.streaming.push((index, self.entries.len() - 1));
            }
            Event::Delta(Delta::Text { index, text }) => {
                if let Some(&(_, at)) = self.streaming.iter().find(|(i, _)| *i == index)
                    && let Some(Entry::Text(t)) = self.entries.get_mut(at)
                {
                    t.push_str(&text);
                }
            }
            // Text arrived through the deltas already.
            Event::Block(Block::Text(text)) => {
                if !self.entries.iter().rev().take(3).any(|e| matches!(e, Entry::Text(t) if *t == text)) {
                    self.entries.push(Entry::Text(text));
                }
            }
            Event::Block(Block::Thinking(_)) => self.entries.push(Entry::Thinking),
            Event::Block(Block::ToolUse { id, name, input }) => self.entries.push(Entry::Tool { id, name, input, result: None }),
            Event::ToolResult { tool_use_id, content, is_error } => {
                for e in self.entries.iter_mut().rev() {
                    if let Entry::Tool { id, result, .. } = e
                        && *id == tool_use_id
                    {
                        *result = Some((content, is_error));
                        break;
                    }
                }
            }
            Event::RateLimit(r) => self.rate_limit = Some(r),
            Event::Result(r) => {
                self.busy = false;
                if r.interrupted() {
                    self.entries.push(Entry::Note("Stopped.".into()));
                } else if r.is_error {
                    self.entries.push(Entry::Note(r.text.clone().unwrap_or_else(|| format!("Claude Code reported an error ({}).", r.subtype))));
                }
                self.last_result = Some(r);
            }
            _ => {}
        }
    }
}

impl Drop for Conversation {
    fn drop(&mut self) {
        for a in self.approvals.drain(..) {
            let _ = a.reply.send(decision(false, &a.input));
        }
        self.session.stop();
        if let Some(f) = &self.config_file {
            let _ = std::fs::remove_file(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(approval_key("Bash", &json!({"command": "ls -la"})), "Bash:ls -la");
        assert_eq!(approval_key("Write", &json!({"file_path": "/tmp/a"})), "Write:/tmp/a");
        assert_eq!(approval_key("WebFetch", &json!({"url": "https://User@Example.com:8080/a?b"})), "WebFetch:example.com");
        assert_eq!(approval_key("WebFetch", &json!({"url": "https://example.com"})), "WebFetch:example.com");
        assert_eq!(approval_key("WebFetch", &json!({"url": "x"})), "WebFetch:");
        assert_eq!(approval_key("WebSearch", &json!({"query": "fonts"})), "WebSearch");
        let allow: Value = serde_json::from_str(decision(true, &json!({"a": 1})).content[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(allow, json!({"behavior": "allow", "updatedInput": {"a": 1}}));
    }
}
