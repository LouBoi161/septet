//! The user's own Claude Code: finding it, asking whether it is signed in, starting it.
//!
//! Septet never reads, stores or passes on Claude credentials. Signing in happens in Claude Code
//! itself (`claude auth login`, Anthropic's own browser flow); Septet only starts the program the
//! user installed and signed in to, and reads the sign-in *state* from `claude auth status`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where to send people who don't have Claude Code yet.
pub const INSTALL_URL: &str = "https://code.claude.com/docs/en/setup";

/// MCP tool calls time out after about a minute by default; Septet's `approve` waits for the user and
/// renders can take long, so the child gets an hour (in milliseconds).
const MCP_TOOL_TIMEOUT_MS: &str = "3600000";

#[cfg(windows)]
const NAMES: &[&str] = &["claude.exe", "claude.cmd"];
#[cfg(not(windows))]
const NAMES: &[&str] = &["claude"];

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// The `claude` executable: the configured path, else the first on PATH, else the usual install
/// locations (apps started from a desktop launcher often don't see the shell's PATH, macOS especially).
pub fn find(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = configured {
        return p.is_file().then(|| p.to_path_buf());
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(h) = home() {
        dirs.push(h.join(".local/bin"));
        dirs.push(h.join(".claude/local"));
        if cfg!(windows) {
            if let Some(appdata) = std::env::var_os("APPDATA") {
                dirs.push(PathBuf::from(appdata).join("npm"));
            }
        } else {
            dirs.push(h.join(".npm-global/bin"));
        }
    }
    if cfg!(target_os = "macos") {
        dirs.push("/opt/homebrew/bin".into());
        dirs.push("/usr/local/bin".into());
    }
    dirs.iter().flat_map(|d| NAMES.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// A command for `claude`, with an environment that makes it use the subscription: an API key in
/// Septet's environment would otherwise take precedence and bill the API account instead.
pub fn command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    for var in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"] {
        cmd.env_remove(var);
    }
    cmd.env("MCP_TOOL_TIMEOUT", MCP_TOOL_TIMEOUT_MS);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Signed in, and how. Deliberately without e-mail address or organization.
#[derive(Clone, Debug, PartialEq)]
pub struct Auth {
    pub logged_in: bool,
    /// "claude.ai" for a subscription, "console"/API key otherwise.
    pub method: String,
    /// "pro", "max", … when signed in with a subscription.
    pub subscription: Option<String>,
}

impl Auth {
    pub fn uses_subscription(&self) -> bool {
        self.logged_in && self.method == "claude.ai"
    }

    fn parse(json: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        Some(Auth {
            logged_in: v.get("loggedIn")?.as_bool()?,
            method: v.get("authMethod").and_then(|m| m.as_str()).unwrap_or_default().to_owned(),
            subscription: v.get("subscriptionType").and_then(|m| m.as_str()).map(str::to_owned),
        })
    }
}

/// What `claude --version` and `claude auth status` said.
#[derive(Clone, Debug, PartialEq)]
pub enum Probe {
    NotInstalled,
    Ready { exe: PathBuf, version: String, auth: Auth },
    Failed { exe: PathBuf, error: String },
}

fn output(exe: &Path, args: &[&str]) -> Result<String, String> {
    let out = command(exe).args(args).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if err.is_empty() { format!("{} exited with {}", exe.display(), out.status) } else { err })
    }
}

/// Look for Claude Code and ask whether it is signed in. Takes a second or two: call it off the UI thread.
pub fn probe(configured: Option<&Path>) -> Probe {
    let Some(exe) = find(configured) else { return Probe::NotInstalled };
    let version = match output(&exe, &["--version"]) {
        // "2.1.295 (Claude Code)"
        Ok(v) => v.split_whitespace().next().unwrap_or_default().to_owned(),
        Err(error) => return Probe::Failed { exe, error },
    };
    // `auth status` exits non-zero when signed out but still prints the JSON.
    let out = command(&exe).args(["auth", "status", "--json"]).stdin(Stdio::null()).output();
    match out.as_ref().ok().and_then(|o| Auth::parse(&String::from_utf8_lossy(&o.stdout))) {
        Some(auth) => Probe::Ready { exe, version, auth },
        None => Probe::Failed { exe, error: "Could not read the sign-in status from `claude auth status`.".into() },
    }
}

/// Start Anthropic's sign-in flow (it opens the browser). Septet waits for it and probes again.
pub fn login(exe: &Path) -> std::io::Result<std::process::Child> {
    command(exe).args(["auth", "login", "--claudeai"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
}

/// How to start a conversation.
#[derive(Clone, Debug, Default)]
pub struct SessionArgs {
    pub session_id: String,
    /// Continue an earlier conversation with this id instead of starting one.
    pub resume: bool,
    /// Alias ("sonnet", "opus", …) or full model name; None: Claude Code's default.
    pub model: Option<String>,
    /// "low" … "max"; None: Claude Code's default.
    pub effort: Option<String>,
    pub mcp_config: Option<PathBuf>,
    pub system_prompt: Option<String>,
    /// Plugins with skills (`--plugin-dir`); Claude may read their files without asking.
    pub plugin_dirs: Vec<PathBuf>,
    /// Load the user's own Claude Code settings, skills, plugins and hooks (`--setting-sources user`).
    pub own_setup: bool,
}

/// A permission rule for reading everything below `dir` (absolute paths start with `//` in rules).
fn read_rule(dir: &Path) -> String {
    let p = dir.to_string_lossy().replace('\\', "/");
    format!("Read(/{}{}/**)", if p.starts_with('/') { "" } else { "/" }, p.trim_end_matches('/'))
}

impl SessionArgs {
    pub fn to_args(&self) -> Vec<OsString> {
        let mut a: Vec<OsString> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--tools",
            "Read,Write,Edit,Glob,Grep,Bash,Skill,WebSearch,WebFetch",
            // Edits inside the workspace (the working directory) need no prompt; anything else asks.
            "--permission-mode",
            "acceptEdits",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        // Without the user's hooks, plugins, CLAUDE.md and skills, Septet's assistant behaves the same everywhere.
        // Never "project": Claude could write a .claude/settings.json with hooks into the workspace.
        a.extend(["--setting-sources".into(), if self.own_setup { "user" } else { "" }.into()]);
        for dir in &self.plugin_dirs {
            a.extend(["--plugin-dir".into(), dir.clone().into_os_string()]);
        }
        let reads: Vec<String> = self.plugin_dirs.iter().map(|d| read_rule(d)).collect();
        if let Some(cfg) = &self.mcp_config {
            a.extend(["--mcp-config".into(), cfg.clone().into_os_string(), "--strict-mcp-config".into()]);
            a.extend(["--allowedTools".into(), "mcp__septet".into()]);
            a.extend(reads.into_iter().map(OsString::from));
            a.extend(["--permission-prompt-tool", "mcp__septet__approve"].map(OsString::from));
        } else {
            // Without Septet's MCP server nobody could answer a prompt: decline instead of hanging.
            a.extend(["--strict-mcp-config", "--permission-prompts", "none"].map(OsString::from));
            if !reads.is_empty() {
                a.push("--allowedTools".into());
                a.extend(reads.into_iter().map(OsString::from));
            }
        }
        a.push(if self.resume { "--resume" } else { "--session-id" }.into());
        a.push(self.session_id.clone().into());
        if let Some(m) = &self.model {
            a.extend(["--model".into(), m.into()]);
        }
        if let Some(e) = &self.effort {
            a.extend(["--effort".into(), e.into()]);
        }
        if let Some(p) = &self.system_prompt {
            a.extend(["--append-system-prompt".into(), p.into()]);
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_status() {
        let a = Auth::parse(r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","email":"x@y","subscriptionType":"max"}"#).unwrap();
        assert!(a.uses_subscription());
        assert_eq!(a.subscription.as_deref(), Some("max"));
        let a = Auth::parse(r#"{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}"#).unwrap();
        assert!(!a.uses_subscription());
        assert!(Auth::parse("Not logged in").is_none());
    }

    #[test]
    fn args() {
        let a = SessionArgs { session_id: "abc".into(), resume: true, model: Some("sonnet".into()), ..Default::default() }.to_args();
        let a: Vec<&str> = a.iter().map(|s| s.to_str().unwrap()).collect();
        assert!(a.windows(2).any(|w| w == ["--resume", "abc"]));
        assert!(a.windows(2).any(|w| w == ["--model", "sonnet"]));
        assert!(a.windows(2).any(|w| w == ["--permission-prompts", "none"]));
        assert!(!a.contains(&"--bare"), "--bare would skip the subscription sign-in");
        assert!(!a.contains(&"--disable-slash-commands"), "it would turn off all skills");
        assert!(a.windows(2).any(|w| w == ["--setting-sources", ""]));
        assert!(a.iter().any(|s| s.contains("Skill")));
    }

    #[test]
    fn plugins_and_own_setup() {
        let args = SessionArgs {
            session_id: "abc".into(),
            mcp_config: Some("/tmp/m.json".into()),
            plugin_dirs: vec!["/data/plugin-1".into(), "/cfg/my-skills/".into()],
            own_setup: true,
            ..Default::default()
        }
        .to_args();
        let a: Vec<&str> = args.iter().map(|s| s.to_str().unwrap()).collect();
        assert!(a.windows(2).any(|w| w == ["--plugin-dir", "/data/plugin-1"]));
        assert!(a.windows(2).any(|w| w == ["--setting-sources", "user"]));
        let allowed = a.iter().position(|s| *s == "--allowedTools").unwrap();
        assert_eq!(&a[allowed + 1..allowed + 4], ["mcp__septet", "Read(//data/plugin-1/**)", "Read(//cfg/my-skills/**)"]);
        assert_eq!(read_rule(Path::new(r"C:\Users\a")), "Read(//C:/Users/a/**)");
    }

    #[test]
    fn missing_configured_path() {
        assert_eq!(find(Some(Path::new("/nonexistent/claude"))), None);
    }
}
