//! Claude in Septet: talks to the user's own Claude Code (see `docs/assistant/PLAN.md`).
//!
//! Off by default. The settings dialog finds Claude Code, shows whether it is signed in and starts
//! Anthropic's sign-in flow when it is not; Septet itself never touches the credentials.

pub mod apps;
pub mod assets;
pub mod cli;
pub mod conversation;
pub mod extensions;
pub mod history;
pub mod markdown;
pub mod mcp;
pub mod panel;
pub mod protocol;
pub mod session;
pub mod settings;
pub mod tools;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use egui::{Context, ViewportId};

use cli::Probe;
use protocol::Event;
use session::{Output, Session};

pub use conversation::{Approval, Choice, Conversation, approval_key, decision};

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// Claude Code's executable, when it is not found on its own.
    pub claude_path: Option<PathBuf>,
    /// An alias from [`MODELS`] (always that family's newest model); None: Claude Code's default.
    pub model: Option<String>,
    /// One of [`EFFORTS`]; None: Claude Code's default.
    pub effort: Option<String>,
    /// Also load the user's own Claude Code settings, skills, plugins and hooks.
    pub own_setup: bool,
    /// MCP servers the user added; their tools ask before each use.
    pub mcp_servers: Vec<extensions::McpServer>,
}

/// The model families to choose from. Claude Code resolves each alias to the newest model of the family.
pub const MODELS: &[(&str, &str)] = &[("haiku", "Haiku"), ("sonnet", "Sonnet"), ("opus", "Opus"), ("fable", "Fable")];

/// How hard Claude thinks (`claude --effort`).
pub const EFFORTS: &[(&str, &str)] = &[("low", "Low"), ("medium", "Medium"), ("high", "High"), ("xhigh", "Extra high"), ("max", "Max")];

pub fn model_name(alias: Option<&str>) -> &'static str {
    alias.and_then(|a| MODELS.iter().find(|(m, _)| *m == a)).map_or("Default", |(_, n)| n)
}

pub fn effort_name(effort: Option<&str>) -> &'static str {
    effort.and_then(|e| EFFORTS.iter().find(|(x, _)| *x == e)).map_or("Default", |(_, n)| n)
}

impl Settings {
    fn file() -> Option<PathBuf> {
        crate::recent::config_dir().map(|d| d.join("assistant").join("settings.json"))
    }

    pub fn load() -> Self {
        Self::file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(file) = Self::file() else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(file, json);
        }
    }
}

/// Where conversations work: Claude reads and writes files here without asking.
pub fn workspace_root() -> Option<PathBuf> {
    crate::recent::data_dir("Assistant").map(|d| d.join("workspace"))
}

pub enum ProbeState {
    Checking(Receiver<Probe>),
    Done(Probe),
}

/// A one-message conversation from the settings dialog, to show that everything works.
pub struct TestRun {
    pub session: Session,
    pub started: std::time::Instant,
    pub reply: String,
    /// Seconds until the answer.
    pub took: f32,
    pub outcome: Option<Result<String, String>>,
}

pub struct Assistant {
    pub settings: Settings,
    pub probe: ProbeState,
    /// `claude auth login`, while it runs.
    login: Option<std::process::Child>,
    pub login_error: Option<String>,
    pub test: Option<TestRun>,
    /// The settings dialog, in this window.
    pub dialog: Option<ViewportId>,
    /// Septet's MCP server, started with the first conversation.
    pub mcp: Option<mcp::Server>,
    pub conversation: Option<Conversation>,
    /// The window showing the Claude panel.
    pub panel: Option<ViewportId>,
    pub panel_width: f32,
    /// The message field's height last frame (it grows with the text).
    pub composer_h: f32,
    /// What the user is typing.
    pub input: String,
    pub panel_error: Option<String>,
    pub history: history::History,
    /// Images from tool results, by tool call.
    pub thumbs: std::collections::HashMap<String, egui::TextureHandle>,
    /// The user's skills and plugins, and when the folders were last looked at.
    found: Option<(std::time::Instant, extensions::Found)>,
    /// App tool calls waiting for their app to start or for the user's approval.
    pub waiting: Vec<apps::Waiting>,
}

impl Assistant {
    pub fn new(ctx: &Context) -> Self {
        let settings = Settings::load();
        let probe = Self::start_probe(ctx, &settings);
        Assistant {
            settings,
            probe,
            login: None,
            login_error: None,
            test: None,
            dialog: None,
            mcp: None,
            conversation: None,
            panel: None,
            panel_width: 380.0,
            composer_h: 70.0,
            input: String::new(),
            panel_error: None,
            history: history::History::load(),
            thumbs: Default::default(),
            found: None,
            waiting: Vec::new(),
        }
    }

    fn start_probe(ctx: &Context, settings: &Settings) -> ProbeState {
        let (tx, rx) = channel();
        let path = settings.claude_path.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(cli::probe(path.as_deref()));
            ctx.request_repaint();
        });
        ProbeState::Checking(rx)
    }

    /// The user's skills and plugins; the folders are looked at every two seconds while asked for.
    pub fn extensions(&mut self) -> extensions::Found {
        match &self.found {
            Some((at, found)) if at.elapsed() < std::time::Duration::from_secs(2) => found.clone(),
            _ => {
                let found = extensions::scan();
                self.found = Some((std::time::Instant::now(), found.clone()));
                found
            }
        }
    }

    pub fn recheck(&mut self, ctx: &Context) {
        self.probe = Self::start_probe(ctx, &self.settings);
    }

    pub fn ready(&self) -> Option<(&std::path::Path, &cli::Auth)> {
        match &self.probe {
            ProbeState::Done(Probe::Ready { exe, auth, .. }) if auth.logged_in => Some((exe, auth)),
            _ => None,
        }
    }

    pub fn logging_in(&self) -> bool {
        self.login.is_some()
    }

    pub fn login(&mut self) {
        let exe = match &self.probe {
            ProbeState::Done(Probe::Ready { exe, .. } | Probe::Failed { exe, .. }) => exe.clone(),
            _ => return,
        };
        self.login_error = None;
        match cli::login(&exe) {
            Ok(child) => self.login = Some(child),
            Err(e) => self.login_error = Some(e.to_string()),
        }
    }

    pub fn cancel_login(&mut self) {
        if let Some(mut child) = self.login.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn start_test(&mut self, ctx: &Context) {
        let Some((exe, _)) = self.ready() else { return };
        let Some(root) = workspace_root() else { return };
        let args = cli::SessionArgs {
            session_id: uuid::Uuid::new_v4().to_string(),
            model: self.settings.model.clone(),
            effort: self.settings.effort.clone(),
            ..Default::default()
        };
        let c = ctx.clone();
        let result = Session::start(exe, &args, &root.join("connection-test"), move || c.request_repaint()).and_then(|mut session| {
            session.send("Reply with exactly the word: ready", &[])?;
            Ok(session)
        });
        self.test = Some(match result {
            Ok(session) => TestRun { session, started: std::time::Instant::now(), reply: String::new(), took: 0.0, outcome: None },
            Err(e) => {
                self.login_error = Some(format!("Could not start Claude Code: {e}"));
                return;
            }
        });
    }

    /// Start a new conversation (or continue `resume`, an earlier conversation's id) with Septet's tools.
    pub fn start_conversation(&mut self, ctx: &Context, resume: Option<String>) -> Result<(), String> {
        let (exe, _) = self.ready().ok_or("Claude Code is not ready; see the Claude settings.")?;
        let exe = exe.to_path_buf();
        let root = workspace_root().ok_or("No folder for Claude's workspace.")?;
        if self.mcp.is_none() {
            let c = ctx.clone();
            self.mcp =
                Some(mcp::Server::start(tools::definitions(), move || c.request_repaint()).map_err(|e| format!("Could not start Septet's tool server: {e}"))?);
        }
        let server = self.mcp.as_ref().expect("started above");
        let id = resume.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let workspace = root.join(&id);
        std::fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;
        // Outside the workspace, so Claude doesn't see the token among its files.
        let config_dir = root.parent().unwrap_or(&root).join("sessions");
        std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
        let config_file = config_dir.join(format!("{id}.mcp.json"));
        let mcp_config = extensions::with_user_servers(&server.config(), &self.settings.mcp_servers);
        write_private(&config_file, mcp_config.as_bytes()).map_err(|e| e.to_string())?;
        let args = cli::SessionArgs {
            session_id: id,
            resume: resume.is_some(),
            model: self.settings.model.clone(),
            effort: self.settings.effort.clone(),
            mcp_config: Some(config_file.clone()),
            system_prompt: Some(tools::system_prompt(&workspace)),
            plugin_dirs: extensions::plugin_dirs(),
            own_setup: self.settings.own_setup,
        };
        let c = ctx.clone();
        let session = Session::start(&exe, &args, &workspace, move || c.request_repaint()).map_err(|e| format!("Could not start Claude Code: {e}"))?;
        let mut conv = Conversation::new(session, workspace, Some(config_file));
        conv.started_with = (self.settings.model.clone(), self.settings.effort.clone());
        self.conversation = Some(conv);
        Ok(())
    }

    /// Send a message, starting a conversation first if there is none.
    pub fn ask(&mut self, ctx: &Context, text: &str) {
        self.panel_error = None;
        let wanted = (self.settings.model.clone(), self.settings.effort.clone());
        let switch = self.conversation.as_ref().is_some_and(|c| !c.ended && c.started_with != wanted);
        if switch || self.conversation.as_ref().is_none_or(|c| c.ended) {
            // Claude Code stopped (crashed, or was ended), or another model or effort was picked:
            // carry on with the same conversation in a new process.
            let previous = self.conversation.take();
            let resume = previous.as_ref().map(|c| c.session.id.clone());
            if let Err(e) = self.start_conversation(ctx, resume) {
                self.panel_error = Some(e);
                self.input = text.to_owned();
                return;
            }
            if let (Some(mut old), Some(new)) = (previous, self.conversation.as_mut()) {
                new.entries = std::mem::take(&mut old.entries);
                new.allowed = std::mem::take(&mut old.allowed);
                if switch {
                    let (m, e) = &new.started_with;
                    new.entries.push(conversation::Entry::Note(format!(
                        "Now using {} · {} effort",
                        model_name(m.as_deref()),
                        effort_name(e.as_deref()).to_lowercase()
                    )));
                }
            }
        }
        let conv = self.conversation.as_mut().expect("started above");
        self.history.touch(&conv.session.id, text);
        conv.send(text);
    }

    /// Continue an earlier conversation. Its messages stay with Claude Code; the panel starts empty.
    pub fn resume(&mut self, ctx: &Context, past: &history::Past) {
        self.conversation = None;
        self.thumbs.clear();
        self.panel_error = None;
        match self.start_conversation(ctx, Some(past.id.clone())) {
            Ok(()) => {
                let conv = self.conversation.as_mut().expect("started");
                conv.entries.push(conversation::Entry::Note(format!("Continuing “{}”. Claude remembers what was said.", past.title)));
            }
            Err(e) => self.panel_error = Some(e),
        }
    }

    /// Show or hide the panel in `viewport`.
    pub fn toggle_panel(&mut self, ctx: &Context, viewport: ViewportId) {
        if self.panel == Some(viewport) {
            self.panel = None;
        } else {
            self.panel = Some(viewport);
            if matches!(self.probe, ProbeState::Done(_)) && self.ready().is_none() {
                self.recheck(ctx);
            }
            ctx.memory_mut(|m| m.request_focus(panel::input_id(viewport)));
        }
    }

    pub fn logic(&mut self, ctx: &Context) {
        if let Some(c) = &mut self.conversation {
            c.update();
        }
        if let ProbeState::Checking(rx) = &self.probe
            && let Ok(p) = rx.try_recv()
        {
            self.probe = ProbeState::Done(p);
        }
        if let Some(child) = &mut self.login {
            match child.try_wait() {
                Ok(None) => ctx.request_repaint_after(std::time::Duration::from_millis(500)),
                Ok(Some(status)) => {
                    if !status.success() {
                        let mut err = String::new();
                        if let Some(mut e) = child.stderr.take() {
                            let _ = std::io::Read::read_to_string(&mut e, &mut err);
                        }
                        self.login_error = Some(if err.trim().is_empty() { format!("Sign-in ended ({status}).") } else { err.trim().to_owned() });
                    }
                    self.login = None;
                    self.recheck(ctx);
                }
                Err(e) => {
                    self.login_error = Some(e.to_string());
                    self.login = None;
                }
            }
        }
        if let Some(test) = &mut self.test {
            for out in test.session.drain() {
                match out {
                    Output::Event(Event::Delta(protocol::Delta::Text { text, .. })) => test.reply.push_str(&text),
                    Output::Event(Event::Result(r)) if test.outcome.is_none() => {
                        test.took = test.started.elapsed().as_secs_f32();
                        test.outcome = Some(if r.is_error { Err(r.text.unwrap_or(r.subtype)) } else { Ok(r.text.unwrap_or_default()) });
                        test.session.stop();
                    }
                    Output::Exited { stderr } if test.outcome.is_none() => {
                        test.outcome = Some(Err(if stderr.is_empty() { "Claude Code stopped without an answer.".into() } else { stderr }));
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn exit(&mut self) {
        self.cancel_login();
        self.test = None;
        self.conversation = None;
        self.mcp = None;
    }
}

/// Write a file only the current user can read (it holds the MCP token).
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    std::io::Write::write_all(&mut opts.open(path)?, data)
}
