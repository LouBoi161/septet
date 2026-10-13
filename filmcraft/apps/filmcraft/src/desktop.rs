//! What the desktop app sets up around [`FilmcraftApp`], shared by the `filmcraft` binary and
//! [`crate::Embedded`]: the session with auto-save and the microphone, the project a launch opens,
//! the native file dialogs, the OS hooks and the GPU export renderer.

use chrono::TimeZone;
use filmcraft_engine::Session;
use filmcraft_engine::autosave::{AutosaveConfig, default_data_dir};
use filmcraft_ui_egui::FilmcraftApp;
use serde_json::json;

use crate::{appearance, audio_in, file_filters};

/// Put the OS hardware video decoders in front of our own. Registered in a statement of its own:
/// a log macro does not evaluate its arguments while no logger takes its level, which left the
/// hardware decoders out of every run.
pub fn register_hardware_decoders() -> filmcraft_platform::Availability {
    let hardware = filmcraft_platform::register();
    log::info!("hardware decoding: {hardware:?}");
    hardware
}

/// The GPU export frame renderer (filmcraft-gpu's off-screen compositor behind filmcraft-export's
/// frame-renderer hook): exports with GPU rendering Auto composite on the GPU and fall back to the
/// CPU reference renderer wherever it cannot.
struct GpuFrameRenderer(filmcraft_gpu::ExportRenderer);

impl filmcraft_export::FrameRenderer for GpuFrameRenderer {
    fn render(
        &mut self,
        project: &filmcraft_project::Project,
        seq: filmcraft_project::ItemId,
        t: filmcraft_time::Tick,
        opts: filmcraft_render::RenderOptions,
        sources: &dyn filmcraft_render::SourceProvider,
    ) -> Option<filmcraft_render::Image> {
        self.0.render(project, seq, t, opts, sources)
    }
}

/// Register the GPU export frame renderer for the whole process (registering it again is
/// harmless).
pub fn register_gpu_frame_renderer() {
    filmcraft_export::register_frame_renderer(|| {
        filmcraft_gpu::ExportRenderer::new().map(|r| Box::new(GpuFrameRenderer(r)) as Box<dyn filmcraft_export::FrameRenderer>)
    });
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
            log::warn!("auto-save and crash recovery unavailable: {e}");
        }
    }
    // voice-over recording reads the microphone through cpal
    session.voiceover.input = Some(Box::new(audio_in::CpalIn::new(&session.prefs.audio_hardware.device_class)));
    session
}

/// Whether a file given to the app is a project (opened) rather than media (imported). The
/// extension's case does not matter (`CUT.FCPROJ`, #850).
pub fn is_project(file: &str) -> bool {
    std::path::Path::new(file).extension().is_some_and(|e| e.eq_ignore_ascii_case("fcproj"))
}

/// Open what a launch asks for: `project`, else what Settings ▸ General ▸ At Startup says (unless
/// `--demo` / `--empty` was given: `startup_flag`), else the demo project when `demo`; then import
/// `media`.
pub fn open_at_startup(session: &mut Session, project: Option<String>, media: Vec<String>, demo: bool, startup_flag: bool) {
    if let Some(p) = project {
        if let Err(e) = session.execute("file.open", json!({"path": p})) {
            log::error!("{e}");
        }
    } else if !startup_flag && session.prefs.general.at_startup == "openMostRecent" {
        // Settings ▸ General ▸ At Startup ▸ Open Most Recent
        let recent = session.prefs.general.recent_projects.iter().find(|p| std::path::Path::new(p).exists()).cloned();
        match recent {
            Some(p) => {
                if let Err(e) = session.execute("file.open", json!({"path": p})) {
                    log::error!("{e}");
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
    if !media.is_empty() {
        let _ = session.execute("file.import", json!({"paths": media}));
    }
}

/// The native file dialogs (rfd): import, relink, open and save. Filter names follow the
/// interface language; extension filters match any case ([`file_filters`]). On Linux the dialogs
/// run on their own thread (`run_dialog`, #260).
pub fn install_file_dialogs(app: &mut FilmcraftApp) {
    app.hooks.pick_files = Some(Box::new(|exts: &[&str]| {
        rfd::FileDialog::new()
            .add_filter(filmcraft_ui_egui::i18n::t("Media"), &file_filters::extensions(exts))
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    }));
    // Link Media ▸ Locate…, Attach Proxies, Reconnect Full Resolution: one path, not imported.
    app.hooks.pick_file_for_relink = Some(Box::new(|exts: &[&str], _hint| {
        rfd::FileDialog::new()
            .add_filter(filmcraft_ui_egui::i18n::t("Media"), &file_filters::extensions(exts))
            .pick_file()
            .map(|p| p.to_string_lossy().to_string())
    }));
    app.hooks.pick_save = Some(Box::new(|name: &str| {
        rfd::FileDialog::new()
            .add_filter(filmcraft_ui_egui::i18n::t("FilmCraft Project"), &file_filters::extensions(&["fcproj"]))
            .set_file_name(name)
            .save_file()
            .map(|p| p.to_string_lossy().to_string())
    }));
    app.hooks.pick_save_as = Some(Box::new(|filter: &str, exts: &[&str], name: &str| {
        // a full path (Quick Export, Export mode) opens the dialog in its folder
        let path = std::path::Path::new(name);
        let mut dialog = rfd::FileDialog::new()
            .add_filter(filmcraft_ui_egui::i18n::t(filter), &file_filters::extensions(exts))
            .set_file_name(path.file_name().map_or(name.into(), |n| n.to_string_lossy()));
        if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.save_file().map(|p| p.to_string_lossy().to_string())
    }));
    app.hooks.pick_folder = Some(Box::new(|| rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().to_string())));
    app.hooks.pick_folder_at =
        Some(Box::new(|dir: &str| rfd::FileDialog::new().set_directory(dir).pick_folder().map(|p| p.to_string_lossy().into_owned())));
    // Linux: the XDG portal call blocks until the user chooses, which would stop the frame
    // loop (GNOME offers to force-quit the window after about five seconds, #260). Show the
    // dialog on its own thread instead; the app keeps running and acts on the answer.
    #[cfg(target_os = "linux")]
    {
        app.hooks.run_dialog = Some(Box::new(run_dialog_off_thread));
    }
    app.hooks.pick_open_file = Some(Box::new(|filter: &str, exts: &[&str]| {
        rfd::FileDialog::new()
            .add_filter(filmcraft_ui_egui::i18n::t(filter), &file_filters::extensions(exts))
            .pick_file()
            .map(|p| p.to_string_lossy().to_string())
    }));
    app.hooks.pick_open_project = Some(Box::new(|| {
        rfd::FileDialog::new()
            .add_filter(filmcraft_ui_egui::i18n::t("FilmCraft Project"), &file_filters::extensions(&["fcproj"]))
            .pick_file()
            .map(|p| p.to_string_lossy().to_string())
    }));
}

/// Show `dialog` with rfd on a worker thread and answer on the returned channel (#260). Filter
/// names are translated here, on the UI thread (the interface language is per thread). A dialog
/// thread that can't start or panics drops the sender: the app takes that as cancelled.
#[cfg(target_os = "linux")]
fn run_dialog_off_thread(dialog: filmcraft_ui_egui::native_dialogs::FileDialog) -> std::sync::mpsc::Receiver<Vec<String>> {
    use filmcraft_ui_egui::native_dialogs::FileDialog;
    let t = |s: &str| filmcraft_ui_egui::i18n::t(s).to_string();
    let media = t("Media");
    let project = t("FilmCraft Project");
    let dialog = match dialog {
        FileDialog::OpenFile { filter, exts } => FileDialog::OpenFile { filter: t(&filter), exts },
        FileDialog::SaveAs { filter, exts, name } => FileDialog::SaveAs { filter: t(&filter), exts, name },
        other => other,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("file-dialog".into()).spawn(move || {
        let shown = std::panic::catch_unwind(move || {
            let path = |p: std::path::PathBuf| p.to_string_lossy().into_owned();
            match dialog {
                FileDialog::OpenFiles { exts } => {
                    rfd::FileDialog::new().add_filter(&media, &exts).pick_files().unwrap_or_default().into_iter().map(path).collect()
                }
                FileDialog::OpenFile { filter, exts } => rfd::FileDialog::new().add_filter(&filter, &exts).pick_file().map(path).into_iter().collect(),
                FileDialog::Relink { exts, .. } => rfd::FileDialog::new().add_filter(&media, &exts).pick_file().map(path).into_iter().collect(),
                FileDialog::OpenProject => rfd::FileDialog::new().add_filter(&project, &["fcproj"]).pick_file().map(path).into_iter().collect(),
                FileDialog::SaveProject { name } => {
                    rfd::FileDialog::new().add_filter(&project, &["fcproj"]).set_file_name(name).save_file().map(path).into_iter().collect()
                }
                FileDialog::SaveAs { filter, exts, name } => {
                    rfd::FileDialog::new().add_filter(&filter, &exts).set_file_name(name).save_file().map(path).into_iter().collect()
                }
                FileDialog::Folder { at } => {
                    let d = rfd::FileDialog::new();
                    let d = match at {
                        Some(dir) if !dir.is_empty() => d.set_directory(dir),
                        _ => d,
                    };
                    d.pick_folder().map(path).into_iter().collect::<Vec<_>>()
                }
            }
        });
        match shown {
            Ok(paths) => {
                let _ = tx.send(paths);
            }
            Err(_) => log::warn!("the file dialog thread panicked; taken as cancelled"),
        }
    });
    if let Err(e) = spawned {
        log::warn!("could not start the file dialog thread: {e}");
    }
    rx
}

/// The system's preferred languages, for Settings ▸ General ▸ Interface Language ▸ System Language
/// (#218).
pub fn install_system_languages(app: &mut FilmcraftApp) {
    app.hooks.system_languages = Some(Box::new(|| sys_locale::get_locales().collect()));
}

/// The other OS hooks: links in the system browser (#642), files and images on the system
/// clipboard for Paste (#611), the cursor position during an OS file drop (Windows) and the system
/// appearance for Appearance Mode ▸ Sync with system (Linux, [`appearance`]).
pub fn install_os_hooks(app: &mut FilmcraftApp) {
    // Help menu, About and Discord links (#642): eframe is built without its `links`
    // feature, so `ctx.open_url` does nothing on the desktop. Detached, so a launcher
    // that waits for the browser never freezes the UI.
    app.hooks.open_url = Some(Box::new(|url: &str| open::that_detached(url).map_err(|e| e.to_string())));
    app.hooks.clipboard_media = Some(Box::new(clipboard_media));
    app.hooks.cursor_screen_position = Some(Box::new(filmcraft_platform::cursor::cursor_screen_position));
    // Settings ▸ Appearance ▸ Appearance Mode ▸ Sync with system on Linux desktops whose
    // compositor reports no theme to winit (no polling: see appearance.rs).
    app.hooks.system_theme = appearance::service();
}

/// Files copied in the file manager, else an image, on the system clipboard (Paste, #611).
fn clipboard_media() -> Option<filmcraft_ui_egui::ClipboardMedia> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    if let Ok(files) = clipboard.get().file_list()
        && !files.is_empty()
    {
        return Some(filmcraft_ui_egui::ClipboardMedia::Files(files.iter().map(|p| p.to_string_lossy().into_owned()).collect()));
    }
    let image = clipboard.get_image().ok()?;
    Some(filmcraft_ui_egui::ClipboardMedia::Image { width: image.width, height: image.height, rgba: image.bytes.into_owned() })
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
        // Linux file managers have no select-file flag: open the parent folder instead.
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
        // always on macOS and Windows; on Linux when a VA-API driver or NVIDIA's driver (NVDEC) is there
        let expected = cfg!(any(target_os = "macos", target_os = "windows"))
            || (cfg!(target_os = "linux") && matches!(hardware, filmcraft_platform::Availability::Available(_)));
        assert_eq!(filmcraft_platform::registered(), expected, "{hardware:?}");
    }
}
