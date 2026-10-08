//! The UI preferences file (`ui.json`): where it lives, reading it at startup and writing it when
//! the app quits.

use std::path::{Path, PathBuf};

use vectorcraft_engine::cmd::fileio;
use vectorcraft_ui_egui::{UiState, VectorcraftApp};

/// The folder VectorCraft keeps what it stores per user in (the UI preferences, the User Defined
/// swatch and graphic style libraries, Data Recovery copies, logs): the data root when the host
/// set one (`hosted::data_root`, a portable install), else ~/Library/Application
/// Support/VectorCraft (macOS), %APPDATA%\VectorCraft (Windows), $XDG_CONFIG_HOME or
/// ~/.config/vectorcraft (Linux).
pub fn data_dir() -> Option<PathBuf> {
    vectorcraft_ui_egui::hosted::data_root().or_else(|| user_dir("VectorCraft", "vectorcraft"))
}

/// Where UI preferences live: `ui.json` in [`data_dir`].
pub fn prefs_path() -> Option<PathBuf> {
    Some(data_dir()?.join("ui.json"))
}

/// The same place under the project's former name (DrawCraft): read once if there are no
/// VectorCraft preferences yet, so settings survive the rename. Not with a data root, which
/// holds everything the app reads.
fn legacy_prefs_path() -> Option<PathBuf> {
    if vectorcraft_ui_egui::hosted::data_root().is_some() {
        return None;
    }
    Some(user_dir("DrawCraft", "drawcraft")?.join("ui.json"))
}

/// The per-user OS folder of an app called `name` (`lower` on Linux).
fn user_dir(name: &str, lower: &str) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support").join(name))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(name))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join(lower))
    }
}

/// Runs without preferences (`VECTORCRAFT_NO_PREFS`, agents' test runs) neither read nor write them.
pub fn prefs_enabled() -> bool {
    std::env::var_os("VECTORCRAFT_NO_PREFS").is_none()
}

/// The saved UI preferences, read before the window opens (they hold its size and position).
pub fn read_prefs() -> Option<UiState> {
    if !prefs_enabled() {
        return None;
    }
    let bytes = prefs_path().and_then(|p| std::fs::read(p).ok()).or_else(|| legacy_prefs_path().and_then(|p| std::fs::read(p).ok()))?;
    serde_json::from_slice(&bytes).ok()
}

pub fn load_prefs(app: &mut VectorcraftApp, saved: Option<UiState>) {
    if !prefs_enabled() {
        return;
    }
    if let Some(ui) = saved {
        app.ui = ui.sanitized();
    }
    vectorcraft_ui_egui::prefs_dialog::restore(app);
}

pub fn save_prefs(app: &VectorcraftApp) {
    if !prefs_enabled() {
        return;
    }
    if let Some(p) = prefs_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
        let mut ui = app.ui.clone();
        ui.engine_prefs = app.session.prefs.to_json();
        if let Ok(bytes) = serde_json::to_vec_pretty(&ui) {
            // Preferences are best effort: a failed write keeps the previous file.
            let _ = fileio::write_atomic(&p, &bytes);
        }
    }
}
