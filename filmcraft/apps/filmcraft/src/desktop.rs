//! What the desktop app sets up around [`FilmcraftApp`], shared by the `filmcraft` binary and
//! [`crate::Embedded`]: the session with auto-save and the microphone, the project a launch opens,
//! the native file dialogs and the OS hooks.

use chrono::TimeZone;
use filmcraft_engine::Session;
use filmcraft_engine::autosave::{AutosaveConfig, default_data_dir};
use filmcraft_ui_egui::FilmcraftApp;
use serde_json::json;

use crate::audio_in;

/// Put the OS hardware video decoders in front of our own. Registered in a statement of its own:
/// a log macro does not evaluate its arguments while no logger takes its level, which left the
/// hardware decoders out of every run.
pub fn register_hardware_decoders() -> filmcraft_platform::Availability {
    let hardware = filmcraft_platform::register();
    log::info!("hardware decoding: {hardware:?}");
    hardware
}

/// Local UTC offset at a unix time (auto-save file names and recovery times use local time).
pub fn local_offset(unix: i64) -> i32 {
    chrono::Local.timestamp_opt(unix, 0).single().map(|d| d.offset().local_minus_utc()).unwrap_or(0)
}

/// A new session with auto-save and the crash-recovery journal in `data_dir` (else
/// [`default_data_dir`]: a portable data root, `FILMCRAFT_DATA_DIR` or the per-user application
/// data folder; this also loads the settings), recording voice-overs from the microphone.
pub fn session(data_dir: Option<std::path::PathBuf>) -> Session {
    let mut session = Session::default();
    if let Some(dir) = data_dir.or_else(default_data_dir) {
        let mut cfg = AutosaveConfig::new(dir);
        cfg.local_offset = local_offset;
        if let Err(e) = session.start_autosave(cfg) {
            eprintln!("filmcraft: auto-save and crash recovery unavailable: {e}");
        }
    }
    // voice-over recording reads the microphone through cpal
    session.voiceover.input = Some(Box::new(audio_in::CpalIn::new(&session.prefs.audio_hardware.device_class)));
    session
}

/// Whether a file given to the app is a project (opened) rather than media (imported).
pub fn is_project(file: &str) -> bool {
    file.ends_with(".fcproj")
}

/// Open what a launch asks for: the first project in `files`, else what Settings ▸ General ▸ At
/// Startup says (unless `--demo` / `--empty` was given: `startup_flag`), else the demo project when
/// `demo`; then import the media in `files`.
pub fn open_at_startup(session: &mut Session, files: &[String], demo: bool, startup_flag: bool) {
    let project = files.iter().find(|f| is_project(f)).cloned();
    if let Some(p) = project {
        if let Err(e) = session.execute("file.open", json!({"path": p})) {
            eprintln!("filmcraft: {e}");
        }
    } else if !startup_flag && session.prefs.general.at_startup == "openMostRecent" {
        // Settings ▸ General ▸ At Startup ▸ Open Most Recent
        let recent = session.prefs.general.recent_projects.iter().find(|p| std::path::Path::new(p).exists()).cloned();
        match recent {
            Some(p) => {
                if let Err(e) = session.execute("file.open", json!({"path": p})) {
                    eprintln!("filmcraft: {e}");
                }
            }
            None => {
                let _ = session.execute("file.openDemoProject", json!({}));
            }
        }
    } else if !startup_flag && session.prefs.general.at_startup == "emptyProject" {
    } else if demo {
        let _ = session.execute("file.openDemoProject", json!({}));
    }
    let media: Vec<String> = files.iter().filter(|f| !is_project(f)).cloned().collect();
    if !media.is_empty() {
        let _ = session.execute("file.import", json!({"paths": media}));
    }
}

/// The native file dialogs (rfd): import, relink, open and save.
pub fn install_file_dialogs(app: &mut FilmcraftApp) {
    app.hooks.pick_files = Some(Box::new(|exts: &[&str]| {
        rfd::FileDialog::new().add_filter("Media", exts).pick_files().unwrap_or_default().into_iter().map(|p| p.to_string_lossy().to_string()).collect()
    }));
    // Link Media ▸ Locate…, Attach Proxies, Reconnect Full Resolution: one path, not imported.
    app.hooks.pick_file_for_relink =
        Some(Box::new(|exts: &[&str], _hint| rfd::FileDialog::new().add_filter("Media", exts).pick_file().map(|p| p.to_string_lossy().to_string())));
    app.hooks.pick_save = Some(Box::new(|name: &str| {
        rfd::FileDialog::new().add_filter("FilmCraft Project", &["fcproj"]).set_file_name(name).save_file().map(|p| p.to_string_lossy().to_string())
    }));
    app.hooks.pick_save_as = Some(Box::new(|filter: &str, exts: &[&str], name: &str| {
        rfd::FileDialog::new().add_filter(filter, exts).set_file_name(name).save_file().map(|p| p.to_string_lossy().to_string())
    }));
    app.hooks.pick_folder = Some(Box::new(|| rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().to_string())));
    app.hooks.pick_open_file =
        Some(Box::new(|filter: &str, exts: &[&str]| rfd::FileDialog::new().add_filter(filter, exts).pick_file().map(|p| p.to_string_lossy().to_string())));
    app.hooks.pick_open_project =
        Some(Box::new(|| rfd::FileDialog::new().add_filter("FilmCraft Project", &["fcproj"]).pick_file().map(|p| p.to_string_lossy().to_string())));
}

/// Open a file in its default application, or reveal it in the file manager (Edit Original,
/// Reveal Log Files).
pub fn open_path(path: &str, reveal: bool) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        if reveal {
            c.arg("-R");
        }
        c.arg(path);
        c
    } else if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("explorer");
        if reveal {
            c.arg(format!("/select,{path}"));
        } else {
            c.arg(path);
        }
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        let p = std::path::Path::new(path);
        c.arg(if reveal { p.parent().unwrap_or(p) } else { p });
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| format!("can't open {path}: {e}"))
}

#[cfg(test)]
mod tests {
    /// Start-up registers the hardware decoders whether or not anything is logged (no logger is
    /// installed here, as in a normal run).
    #[test]
    fn startup_registers_the_hardware_decoders_without_a_logger() {
        assert!(!log::log_enabled!(log::Level::Info));
        let hardware = super::register_hardware_decoders();
        assert_eq!(filmcraft_platform::registered(), cfg!(any(target_os = "macos", target_os = "windows")), "{hardware:?}");
    }
}
