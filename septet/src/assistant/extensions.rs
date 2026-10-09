//! Skills, plugins and MCP servers for Claude Code: Septet's own (bundled in the binary) and the user's.
//!
//! Every plugin goes to Claude Code as a `--plugin-dir`; that works with `--setting-sources ""`, so the
//! user's personal Claude Code setup stays out unless they opt in (`Settings::own_setup`).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

mod bundled {
    include!(concat!(env!("OUT_DIR"), "/assistant_plugin.rs"));
}

/// Unpack Septet's plugin (`septet/assistant-plugin`) once per version and return its folder.
pub fn bundled_dir() -> std::io::Result<PathBuf> {
    let root = crate::recent::data_dir("Assistant").ok_or_else(|| std::io::Error::other("no data folder"))?;
    let dir = root.join(format!("plugin-{}", bundled::HASH));
    if !dir.is_dir() {
        let tmp = root.join(format!("plugin-{}.part-{}", bundled::HASH, std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for (rel, data) in bundled::FILES {
            let path = tmp.join(rel);
            std::fs::create_dir_all(path.parent().expect("file in a folder"))?;
            std::fs::write(path, data)?;
        }
        // Another Septet may have unpacked the same version meanwhile; then its copy is as good.
        if std::fs::rename(&tmp, &dir).is_err() {
            let _ = std::fs::remove_dir_all(&tmp);
            if !dir.is_dir() {
                return Err(std::io::Error::other(format!("could not unpack {}", dir.display())));
            }
        }
    }
    // Earlier versions' copies.
    for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("plugin-") && e.path() != dir && !name.contains(".part-") {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    Ok(dir)
}

fn user_root() -> Option<PathBuf> {
    crate::recent::config_dir().map(|d| d.join("assistant"))
}

/// The user's own skills, a plugin named "my": `my-skills/skills/<name>/SKILL.md`.
pub fn my_skills_dir() -> Option<PathBuf> {
    user_root().map(|r| r.join("my-skills"))
}

/// The user's plugins, one folder each.
pub fn plugins_dir() -> Option<PathBuf> {
    user_root().map(|r| r.join("plugins"))
}

/// Create the folders the settings dialog opens, with a manifest for the "my" plugin.
pub fn create_folders() -> std::io::Result<()> {
    let (Some(my), Some(plugins)) = (my_skills_dir(), plugins_dir()) else { return Ok(()) };
    std::fs::create_dir_all(my.join("skills"))?;
    std::fs::create_dir_all(my.join(".claude-plugin"))?;
    std::fs::create_dir_all(plugins)?;
    let manifest = my.join(".claude-plugin/plugin.json");
    if !manifest.is_file() {
        std::fs::write(manifest, r#"{"name": "my", "description": "Your own skills for Claude in Septet."}"#)?;
    }
    Ok(())
}

/// A plugin the user added.
#[derive(Clone, Debug, PartialEq)]
pub struct UserPlugin {
    pub name: String,
    pub dir: PathBuf,
    /// Hooks run commands without asking; the settings dialog says so.
    pub has_hooks: bool,
}

/// What the user added: skill names and plugins.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub skills: Vec<String>,
    pub plugins: Vec<UserPlugin>,
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A folder Claude Code would load as a plugin: a manifest or at least one of the parts a plugin has.
fn is_plugin(dir: &Path) -> bool {
    [".claude-plugin/plugin.json", "skills", "commands", "agents", "hooks", ".mcp.json"].iter().any(|p| dir.join(p).exists())
}

fn has_hooks(dir: &Path) -> bool {
    let in_manifest = std::fs::read(dir.join(".claude-plugin/plugin.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|m| m.get("hooks").is_some());
    in_manifest || dir.join("hooks").exists()
}

pub fn scan() -> Found {
    scan_in(my_skills_dir().as_deref(), plugins_dir().as_deref())
}

fn scan_in(my: Option<&Path>, plugins: Option<&Path>) -> Found {
    let skills =
        my.map(|m| subdirs(&m.join("skills"))).unwrap_or_default().into_iter().filter(|d| d.join("SKILL.md").is_file()).map(|d| file_name(&d)).collect();
    let plugins = plugins
        .map(subdirs)
        .unwrap_or_default()
        .into_iter()
        .filter(|d| is_plugin(d))
        .map(|d| UserPlugin { name: file_name(&d), has_hooks: has_hooks(&d), dir: d })
        .collect();
    Found { skills, plugins }
}

/// Every `--plugin-dir` for a conversation: Septet's own first, then the user's.
pub fn plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match bundled_dir() {
        Ok(d) => dirs.push(d),
        Err(e) => eprintln!("septet: Claude skills unavailable: {e}"),
    }
    let found = scan();
    if !found.skills.is_empty()
        && let Some(my) = my_skills_dir()
    {
        dirs.push(my);
    }
    dirs.extend(found.plugins.into_iter().map(|p| p.dir));
    dirs
}

/// An MCP server the user added in the settings.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct McpServer {
    pub name: String,
    /// A command line (the server talks over stdin/stdout) or an http(s) URL.
    pub target: String,
    pub enabled: bool,
}

impl McpServer {
    /// The name Claude Code knows it by (its tools are `mcp__<name>__<tool>`).
    pub fn key(&self) -> String {
        let k: String = self.name.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        if k.is_empty() || k == "septet" { format!("user-{k}") } else { k }
    }

    /// Its entry in `--mcp-config`, or why it can't have one.
    pub fn config(&self) -> Result<Value, String> {
        let t = self.target.trim();
        if t.starts_with("http://") || t.starts_with("https://") {
            return Ok(json!({"type": "http", "url": t}));
        }
        let mut words = split_command(t)?;
        if words.is_empty() {
            return Err("Enter a command or a URL.".into());
        }
        let command = words.remove(0);
        Ok(json!({"type": "stdio", "command": command, "args": words}))
    }
}

/// Split a command line into words: whitespace separates, '…' and "…" group, \ escapes outside '…'.
fn split_command(s: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => words.extend(word.take()),
            '\'' => {
                let w = word.get_or_insert_default();
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => w.push(c),
                        None => return Err("A quote is not closed.".into()),
                    }
                }
            }
            '"' => {
                let w = word.get_or_insert_default();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => w.extend(chars.next()),
                        Some(c) => w.push(c),
                        None => return Err("A quote is not closed.".into()),
                    }
                }
            }
            // Windows paths keep their backslashes.
            '\\' if !cfg!(windows) => word.get_or_insert_default().extend(chars.next()),
            c => word.get_or_insert_default().push(c),
        }
    }
    words.extend(word);
    Ok(words)
}

/// Add the user's enabled MCP servers to Septet's `--mcp-config` (a JSON object with `mcpServers`).
pub fn with_user_servers(septet_config: &str, servers: &[McpServer]) -> String {
    let mut cfg: Value = serde_json::from_str(septet_config).unwrap_or_else(|_| json!({"mcpServers": {}}));
    for s in servers.iter().filter(|s| s.enabled) {
        match s.config() {
            Ok(entry) => cfg["mcpServers"][s.key()] = entry,
            Err(e) => eprintln!("septet: MCP server {:?} skipped: {e}", s.name),
        }
    }
    cfg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_plugin_is_complete() {
        assert!(bundled::FILES.iter().any(|(p, _)| *p == ".claude-plugin/plugin.json"));
        assert!(bundled::FILES.iter().any(|(p, _)| *p == "skills/canvas-design/SKILL.md"));
        assert_eq!(bundled::HASH.len(), 16);
    }

    #[test]
    fn commands() {
        assert_eq!(split_command(r#"npx -y "@scope/server" --dir '/a b'"#).unwrap(), ["npx", "-y", "@scope/server", "--dir", "/a b"]);
        assert!(split_command("echo 'open").is_err());
        let s = McpServer { name: "My Files!".into(), target: "uvx mcp-server-fetch".into(), enabled: true };
        assert_eq!(s.key(), "My-Files-");
        assert_eq!(s.config().unwrap(), json!({"type": "stdio", "command": "uvx", "args": ["mcp-server-fetch"]}));
        let s = McpServer { name: "septet".into(), target: "https://example.com/mcp".into(), enabled: true };
        assert_eq!(s.key(), "user-septet", "must not replace Septet's own server");
        assert_eq!(s.config().unwrap()["type"], "http");
    }

    #[test]
    fn config_keeps_septet() {
        let servers = [
            McpServer { name: "fetch".into(), target: "uvx mcp-server-fetch".into(), enabled: true },
            McpServer { name: "off".into(), target: "x".into(), enabled: false },
        ];
        let cfg: Value = serde_json::from_str(&with_user_servers(r#"{"mcpServers":{"septet":{"type":"http"}}}"#, &servers)).unwrap();
        assert_eq!(cfg["mcpServers"]["septet"]["type"], "http");
        assert_eq!(cfg["mcpServers"]["fetch"]["command"], "uvx");
        assert!(cfg["mcpServers"].get("off").is_none());
    }

    #[test]
    fn finds_user_skills_and_plugins() {
        let root = std::env::temp_dir().join(format!("septet-ext-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let my = root.join("my-skills");
        std::fs::create_dir_all(my.join("skills/logo")).unwrap();
        std::fs::write(my.join("skills/logo/SKILL.md"), "---\nname: logo\n---\n").unwrap();
        std::fs::create_dir_all(my.join("skills/empty")).unwrap();
        let plugins = root.join("plugins");
        std::fs::create_dir_all(plugins.join("a/skills")).unwrap();
        std::fs::create_dir_all(plugins.join("b/hooks")).unwrap();
        std::fs::create_dir_all(plugins.join("not-a-plugin")).unwrap();
        let f = scan_in(Some(&my), Some(&plugins));
        assert_eq!(f.skills, ["logo"]);
        let names: Vec<(&str, bool)> = f.plugins.iter().map(|p| (p.name.as_str(), p.has_hooks)).collect();
        assert_eq!(names, [("a", false), ("b", true)]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
