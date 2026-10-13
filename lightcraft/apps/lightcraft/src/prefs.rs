//! The app settings file (`<config>/ui.json`): UI state and app settings, including the library
//! to open at launch.

use lightcraft_ui_egui::{LightcraftApp, UiState};

/// LightCraft's settings folder (`ui.json`, logs, the GPU init marker, the SAM 3, face and
/// denoise models): the host's data root when one is set (`lightcraft_engine::paths`).
pub fn config_dir() -> Option<std::path::PathBuf> {
    lightcraft_engine::config::config_dir()
}

/// `<config>/ui.json`, the saved UI state and app settings — `None` when nothing is read or
/// written: `LIGHTCRAFT_NO_PREFS` (tests, scripts) or no home folder.
pub fn prefs_path() -> Option<std::path::PathBuf> {
    if std::env::var_os("LIGHTCRAFT_NO_PREFS").is_some() {
        return None;
    }
    config_dir().map(|d| d.join("ui.json"))
}

/// The saved UI state and app settings (`<config>/ui.json`), if any, and a warning for the user
/// when the file exists but can't be used (issue #103): a damaged file is kept as
/// `ui.json.corrupt-<unix time>` first, so the next save can't lose the library location in it;
/// one that can't be read at all is not written this session (`keep_file`).
///
/// A `--memory` session reads the settings (so it looks like the user's app) but writes nothing
/// (issue #164), so a damaged file is left as it is there, not set aside.
pub fn load_prefs(in_memory: bool) -> (Option<UiState>, Option<String>, bool) {
    let Some(path) = prefs_path() else { return (None, None, false) };
    load_prefs_at(&path, !in_memory)
}

pub fn load_prefs_at(path: &std::path::Path, set_aside_damaged: bool) -> (Option<UiState>, Option<String>, bool) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (None, None, false),
        Err(e) => {
            let msg = format!("The app settings ({}) couldn't be read: {e}. Defaults are used, and the file isn't overwritten.", path.display());
            return (None, Some(msg), true);
        }
    };
    match serde_json::from_slice::<UiState>(&bytes) {
        Ok(ui) => (Some(ui.sanitized()), None, false),
        Err(e) if !set_aside_damaged => {
            let msg = format!("The app settings ({}) are damaged ({e}). Defaults are used; nothing is saved in this session.", path.display());
            (None, Some(msg), true)
        }
        Err(e) => {
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let keep = path.with_file_name(format!("ui.json.corrupt-{secs}"));
            let kept = std::fs::rename(path, &keep);
            let what = match &kept {
                Ok(()) => format!("it was kept as {}", keep.display()),
                Err(r) => format!("it couldn't be set aside ({r})"),
            };
            let msg = format!(
                "The app settings ({}) are damaged ({e}); {what}. Defaults are used — reopen your library with Settings → Open Library… if it isn't shown.",
                path.display()
            );
            (None, Some(msg), kept.is_err())
        }
    }
}

/// Write `bytes` to `path` atomically: a temp file, fsynced, renamed over it (a crash or a full
/// disk leaves the old file or the new one, never a truncated one).
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent()
        && let Ok(d) = std::fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Saves `ui.json` (app settings, incl. the library to open at launch): as soon as the library
/// changes, every few seconds when anything else changed, and at exit.
///
/// Nothing is written while the session is one that promises to save nothing: `--memory`
/// (`path` is `None`; issue #164), or the one running because the library couldn't be opened — the
/// question window and Continue Without Saving (issue #100; `app.openLibrary` ends it, and the
/// library it opened is saved then).
#[derive(Default)]
pub struct PrefsWriter {
    /// Where to write; `None` writes nothing (`--memory`, `LIGHTCRAFT_NO_PREFS`, no home folder).
    path: Option<std::path::PathBuf>,
    written: Vec<u8>,
    library: String,
    checked: f64,
    /// The last write failed (reported once until a write works).
    failing: bool,
    /// `ui.json` couldn't be read at launch: never overwrite it this session.
    keep_file: bool,
}

impl PrefsWriter {
    /// A writer for `app`'s settings as they are now (what's on disk: only changes are written)
    /// into `path` (`None`: nothing is written, see [`prefs_path`]); `keep_file`: `ui.json`
    /// couldn't be read at launch (see [`load_prefs`]).
    pub fn new(app: &LightcraftApp, keep_file: bool, path: Option<std::path::PathBuf>) -> Self {
        PrefsWriter {
            path,
            written: serde_json::to_vec_pretty(&app.ui).unwrap_or_default(),
            library: app.ui.settings.library_path.clone(),
            keep_file,
            ..Default::default()
        }
    }

    pub fn save(&mut self, app: &LightcraftApp) -> Result<(), String> {
        self.save_ui(&app.ui, app.library_problem.is_some())
    }

    /// Write `ui` if it changed since the last write; `temporary` (the library couldn't be
    /// opened) saves nothing.
    pub fn save_ui(&mut self, ui: &UiState, temporary: bool) -> Result<(), String> {
        if self.keep_file || temporary {
            return Ok(());
        }
        let Some(path) = self.path.clone() else { return Ok(()) };
        let bytes = serde_json::to_vec_pretty(ui).map_err(|e| e.to_string())?;
        if bytes == self.written {
            return Ok(());
        }
        path.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| write_atomic(&path, &bytes))
            .map_err(|e| format!("saving the app settings failed: {e}"))?;
        self.written = bytes;
        self.library = ui.settings.library_path.clone();
        Ok(())
    }

    pub fn tick(&mut self, app: &mut LightcraftApp, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let moved = app.ui.settings.library_path != self.library;
        if !moved && now - self.checked < 3.0 {
            return;
        }
        self.checked = now;
        match self.save(app) {
            Ok(()) => self.failing = false,
            Err(e) => {
                if !self.failing {
                    log::warn!("{e}");
                    app.notices.push(format!("{e}. LightCraft keeps trying."));
                }
                self.failing = true;
                // don't retry every frame
                self.library = app.ui.settings.library_path.clone();
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("lc-app-prefs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Issue #103: a damaged ui.json (which holds the library location) is kept aside and
    /// reported, not silently replaced by the defaults.
    #[test]
    fn damaged_ui_json_is_kept_and_reported() {
        let d = dir("damaged");
        let path = d.join("ui.json");
        std::fs::write(&path, br#"{"settings": {"library_path": "/Volumes/Photos/Lib"#).unwrap();
        let (ui, warning, keep) = load_prefs_at(&path, true);
        assert!(ui.is_none() && !keep);
        let warning = warning.unwrap();
        assert!(warning.contains("damaged") && warning.contains("ui.json.corrupt-"), "{warning}");
        assert!(!path.exists());
        let kept: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert!(kept.len() == 1 && kept[0].starts_with("ui.json.corrupt-"), "{kept:?}");
        // a missing file is just "no settings yet"
        assert_eq!(load_prefs_at(&path, true).1, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Issue #164: a `--memory` session reads the settings but writes nothing — a damaged file is
    /// reported and left where it is, not set aside.
    #[test]
    fn memory_session_leaves_a_damaged_ui_json_alone() {
        let d = dir("damaged-memory");
        let path = d.join("ui.json");
        let bytes = br#"{"settings": {"library_path": "/Volumes/Photos/Lib"#;
        std::fs::write(&path, bytes).unwrap();
        let (ui, warning, keep) = load_prefs_at(&path, false);
        assert!(ui.is_none() && keep);
        let warning = warning.unwrap();
        assert!(warning.contains("damaged") && warning.contains("nothing is saved"), "{warning}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Issue #164: the writer saves changed settings to its file — and nothing at all without one
    /// (`--memory`) or while the session is the temporary one offered when the library can't open.
    #[test]
    fn prefs_writer_writes_nothing_in_a_memory_or_temporary_session() {
        let d = dir("writer");
        let path = d.join("sub").join("ui.json");
        let mut ui = UiState::default();
        // an ordinary session: a change is written, the folder created on the way
        let mut w = PrefsWriter { path: Some(path.clone()), ..Default::default() };
        ui.thumb_size += 50.0;
        w.save_ui(&ui, false).unwrap();
        let saved = std::fs::read(&path).unwrap();
        assert!(String::from_utf8_lossy(&saved).contains("thumbSize"), "{}", String::from_utf8_lossy(&saved));
        // the temporary session (library problem): a further change is not written…
        ui.thumb_size += 50.0;
        w.save_ui(&ui, true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        // …until a library opens
        w.save_ui(&ui, false).unwrap();
        assert_ne!(std::fs::read(&path).unwrap(), saved);
        // --memory: no path, so no file and no folder, whatever changes
        let d2 = dir("writer-memory");
        let _ = std::fs::remove_dir_all(&d2);
        let mut w = PrefsWriter { path: None, ..Default::default() };
        ui.thumb_size += 50.0;
        w.save_ui(&ui, false).unwrap();
        assert!(w.written.is_empty() && !d2.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ui_json_is_written_atomically() {
        let d = dir("atomic");
        let path = d.join("ui.json");
        std::fs::write(&path, b"old").unwrap();
        write_atomic(&path, b"{\"new\": true}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"new\": true}");
        assert!(!d.join("ui.json.tmp").exists());
        // a failed write leaves the old file
        let missing = d.join("gone").join("ui.json");
        assert!(write_atomic(&missing, b"x").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"new\": true}");
        let ui = UiState::default();
        write_atomic(&path, &serde_json::to_vec_pretty(&ui).unwrap()).unwrap();
        let (loaded, warning, _) = load_prefs_at(&path, true);
        assert!(loaded.is_some() && warning.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
