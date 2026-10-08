//! Files opened through Septet (for Home) and the window layout to restore at the next start.

use std::path::{Path, PathBuf};

use crate::kinds::AppKind;
use crate::shell::TabKind;

/// Portable mode: a `portable.txt` next to the executable (or `--portable`, or `SEPTET_PORTABLE`) keeps
/// all of Septet's and the apps' data in a `Data` folder beside it, e.g. on a USB stick.
pub fn portable_root() -> Option<PathBuf> {
    static ROOT: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
        let asked = std::env::args().any(|a| a == "--portable") || std::env::var_os("SEPTET_PORTABLE").is_some();
        (asked || exe_dir.join("portable.txt").is_file()).then(|| exe_dir.join("Data"))
    })
    .clone()
}

/// Septet's own settings folder (the apps keep theirs where they always do, or under the portable
/// `Data` folder).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SEPTET_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    if let Some(root) = portable_root() {
        return Some(root.join("Septet"));
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.map(|b| b.join(if cfg!(any(target_os = "windows", target_os = "macos")) { "Septet" } else { "septet" }))
}

/// Where files handed between apps live (`sub`: "Shared", "Pasted"). Placed files are often linked
/// rather than copied, so this is not a temp folder.
pub fn data_dir(sub: &str) -> Option<PathBuf> {
    let base = match portable_root() {
        Some(root) => root.join("Septet"),
        None => {
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            let base = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
                .map(|b| b.join("septet"));
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            let base = config_dir();
            base?
        }
    };
    let dir = base.join(sub);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecentFile {
    pub path: PathBuf,
    pub app: AppKind,
    /// Seconds since the Unix epoch.
    pub opened: u64,
}

#[derive(Default)]
pub struct Recent {
    pub files: Vec<RecentFile>,
    dirty: bool,
}

const MAX: usize = 40;

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Recent {
    fn file() -> Option<PathBuf> {
        config_dir().map(|d| d.join("recent.json"))
    }

    pub fn load() -> Self {
        let files = Self::file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice::<Vec<RecentFile>>(&b).ok()).unwrap_or_default();
        Recent { files, dirty: false }
    }

    pub fn add(&mut self, path: &Path, app: AppKind) {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.files.retain(|f| f.path != path);
        self.files.insert(0, RecentFile { path, app, opened: now() });
        self.files.truncate(MAX);
        self.dirty = true;
    }

    pub fn remove(&mut self, path: &Path) {
        self.files.retain(|f| f.path != path);
        self.dirty = true;
    }

    pub fn save(&self) {
        if !self.dirty {
            return;
        }
        let Some(file) = Self::file() else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&self.files) {
            let tmp = file.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(tmp, file);
            }
        }
    }
}

/// "3 min ago", "yesterday", …
pub fn ago(secs: u64) -> String {
    let d = now().saturating_sub(secs);
    match d {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", d / 60),
        3600..86_400 => format!("{} h ago", d / 3600),
        86_400..172_800 => "yesterday".into(),
        _ => format!("{} days ago", d / 86_400),
    }
}

/// The windows and their tabs, to reopen next time.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub windows: Vec<SavedWindow>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SavedWindow {
    pub tabs: Vec<TabKind>,
    pub active: usize,
    /// Outer position and inner size in points, where the platform reports them.
    #[serde(default)]
    pub geometry: Option<([f32; 2], [f32; 2])>,
}
