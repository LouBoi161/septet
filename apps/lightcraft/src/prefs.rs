//! The app settings file (`<config>/ui.json`): UI state and app settings, including the library
//! to open at launch.

use lightcraft_ui_egui::{LightcraftApp, UiState};

/// LightCraft's settings folder (`ui.json`, the GPU init marker, the SAM 3 model).
pub fn config_dir() -> Option<std::path::PathBuf> {
    lightcraft_engine::camera_profiles::config_dir()
}

/// The saved UI state and app settings (`<config>/ui.json`), if any, and a warning for the user
/// when the file exists but can't be used (issue #103): a damaged file is kept as
/// `ui.json.corrupt-<unix time>` first, so the next save can't lose the library location in it;
/// one that can't be read at all is not written this session (`keep_file`).
pub fn load_prefs() -> (Option<UiState>, Option<String>, bool) {
    if std::env::var_os("LIGHTCRAFT_NO_PREFS").is_some() {
        return (None, None, false);
    }
    let Some(path) = config_dir().map(|d| d.join("ui.json")) else { return (None, None, false) };
    load_prefs_at(&path)
}

pub fn load_prefs_at(path: &std::path::Path) -> (Option<UiState>, Option<String>, bool) {
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
#[derive(Default)]
pub struct PrefsWriter {
    written: Vec<u8>,
    library: String,
    checked: f64,
    /// The last write failed (reported once until a write works).
    failing: bool,
    /// `ui.json` couldn't be read at launch: never overwrite it this session.
    keep_file: bool,
}

impl PrefsWriter {
    /// A writer for `app`'s settings as they are now (what's on disk: only changes are written);
    /// `keep_file`: `ui.json` couldn't be read at launch (see [`load_prefs`]).
    pub fn new(app: &LightcraftApp, keep_file: bool) -> Self {
        PrefsWriter {
            written: serde_json::to_vec_pretty(&app.ui).unwrap_or_default(),
            library: app.ui.settings.library_path.clone(),
            keep_file,
            ..Default::default()
        }
    }

    pub fn save(&mut self, app: &LightcraftApp) -> Result<(), String> {
        if self.keep_file || std::env::var_os("LIGHTCRAFT_NO_PREFS").is_some() {
            return Ok(());
        }
        let Some(d) = config_dir() else { return Ok(()) };
        let bytes = serde_json::to_vec_pretty(&app.ui).map_err(|e| e.to_string())?;
        if bytes == self.written {
            return Ok(());
        }
        std::fs::create_dir_all(&d)
            .and_then(|()| write_atomic(&d.join("ui.json"), &bytes))
            .map_err(|e| format!("saving the app settings failed: {e}"))?;
        self.written = bytes;
        self.library = app.ui.settings.library_path.clone();
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
                eprintln!("lightcraft: {e}");
                if !self.failing {
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
        let (ui, warning, keep) = load_prefs_at(&path);
        assert!(ui.is_none() && !keep);
        let warning = warning.unwrap();
        assert!(warning.contains("damaged") && warning.contains("ui.json.corrupt-"), "{warning}");
        assert!(!path.exists());
        let kept: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert!(kept.len() == 1 && kept[0].starts_with("ui.json.corrupt-"), "{kept:?}");
        // a missing file is just "no settings yet"
        assert_eq!(load_prefs_at(&path).1, None);
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
        let (loaded, warning, _) = load_prefs_at(&path);
        assert!(loaded.is_some() && warning.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
