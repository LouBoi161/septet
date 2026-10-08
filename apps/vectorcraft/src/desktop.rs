//! What the desktop gives the app ([`services`]): file dialogs, files, the system clipboard, the
//! browser, the file manager, the default apps and the printers. Plus opening the files the app
//! was handed and the folders it keeps next to its preferences.

use vectorcraft_engine::cmd::fileio;
use vectorcraft_ui_egui::{ClipboardProbeFactory, FilePick, Services, VectorcraftApp};

use crate::{clipboard, prefs, printing};

/// Open files handed to the app (command line, macOS Finder and Dock) as documents. A file that
/// can't be opened is reported in the status bar and on stderr; the others still open.
pub fn open_files(app: &mut VectorcraftApp, files: Vec<String>) {
    for f in files {
        if let Err(e) = vectorcraft_ui_egui::io::open_path(app, &f) {
            eprintln!("vectorcraft: {f}: {e}");
            app.status(format!("Couldn't open {}: {e}", fileio::file_name(&f)));
        }
    }
}

/// The folders kept next to the preferences ([`prefs::data_dir`]): the User Defined swatch and
/// graphic style libraries, and Data Recovery's copies.
pub fn set_user_folders(app: &mut VectorcraftApp) {
    let folder = |name: &str| prefs::data_dir().map(|d| d.join(name).to_string_lossy().to_string());
    // User Defined swatch and graphic style libraries live next to the preferences.
    app.session.swatch_libraries.set_user_dir(folder("Swatches"));
    app.session.style_libraries.set_user_dir(folder("Graphic Styles"));
    // Data Recovery copies live next to the preferences too (none for runs without
    // preferences, such as agents' test runs, unless the recoveryFolder preference is set).
    if std::env::var_os("VECTORCRAFT_NO_PREFS").is_none() {
        app.session.recovery.set_default_folder(folder("Data Recovery"));
    }
}

/// A native file dialog showing `pick`'s file types, folder and suggested name.
fn file_dialog(pick: &FilePick) -> rfd::FileDialog {
    let d = pick.filters.iter().fold(rfd::FileDialog::new(), |d, (name, exts)| d.add_filter(*name, exts));
    let d = match &pick.folder {
        Some(folder) => d.set_directory(folder),
        None => d,
    };
    if pick.name.is_empty() { d } else { d.set_file_name(&pick.name) }
}

/// File → Show in Folder: select `path` in Finder / Explorer, or open its folder elsewhere.
fn reveal(path: &str) -> Result<(), String> {
    reveal_command(path).spawn().map(|_| ()).map_err(|e| format!("can't show {path}: {e}"))
}

#[cfg(target_os = "macos")]
fn reveal_command(path: &str) -> std::process::Command {
    let mut c = std::process::Command::new("open");
    c.args(["-R", path]);
    c
}

#[cfg(windows)]
fn reveal_command(path: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt as _;
    // Explorer reads `/select,"path"` itself (the usual argument quoting breaks paths with spaces)
    // and needs backslashes.
    let mut c = std::process::Command::new("explorer");
    c.raw_arg(format!("/select,\"{}\"", path.replace('/', "\\")));
    c
}

#[cfg(not(any(target_os = "macos", windows)))]
fn reveal_command(path: &str) -> std::process::Command {
    let folder = std::path::Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    let mut c = std::process::Command::new("xdg-open");
    c.arg(folder);
    c
}

/// Write a file the safe way: a failed write keeps the old file ([`fileio::write_atomic`]).
fn write_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    fileio::write_atomic(std::path::Path::new(path), bytes).map_err(|e| e.to_string())
}

pub fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|pick: &FilePick| file_dialog(pick).pick_file().map(|p| p.to_string_lossy().to_string()))),
        pick_open_multi: Some(Box::new(|| {
            fileio::place_filters()
                .fold(rfd::FileDialog::new().set_title("Place"), |d, (name, exts)| d.add_filter(name, exts))
                .pick_files()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })),
        pick_save: Some(Box::new(|pick: &FilePick| {
            // The Templates folder may not exist yet.
            if let Some(folder) = &pick.folder {
                let _ = std::fs::create_dir_all(folder);
            }
            file_dialog(pick).save_file().map(|p| p.to_string_lossy().to_string())
        })),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(write_file)),
        // Background Save and Export write from a worker thread.
        write_shared: Some(std::sync::Arc::new(write_file)),
        // Every format Copy offers and Paste reads (menu-bar Paste never sees egui's Paste event).
        system_clipboard: Some(clipboard::system_clipboard()),
        // Linux checks whether Paste has something to take on a background thread: an X11 clipboard
        // owner that never answers holds a read for up to 4 s. Windows only asks which formats the
        // clipboard holds and macOS asks the pasteboard server, so they check in line.
        clipboard_probe: cfg!(target_os = "linux").then(|| Box::new(clipboard::system_clipboard) as ClipboardProbeFactory),
        // Help → Discord / website / GitHub, the Discord button, About and Home links.
        open_url: Some(Box::new(open_url)),
        reveal: Some(Box::new(reveal)),
        // Links panel: Edit Original; Package: Show Package. Relink to Folder and Package pick folders.
        open_file: Some(Box::new(open_file)),
        pick_folder: Some(Box::new(|| rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().to_string()))),
        // File → Print: the system's printers and print queue.
        print: Some(Box::new(printing::SystemPrint)),
        ..Default::default()
    }
}

/// Open `url` in the system browser.
pub fn open_url(url: &str) {
    let _ = webbrowser::open(url);
}

/// Edit Original, Show Package: open `path` (a file or a folder) in the system's default app for it.
pub fn open_file(path: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut c = {
        use std::os::windows::process::CommandExt as _;
        let mut c = std::process::Command::new("explorer");
        c.raw_arg(format!("\"{}\"", path.replace('/', "\\")));
        c
    };
    #[cfg(target_os = "macos")]
    let mut c = std::process::Command::new("open");
    #[cfg(not(any(target_os = "macos", windows)))]
    let mut c = std::process::Command::new("xdg-open");
    #[cfg(not(windows))]
    c.arg(path);
    c.spawn().map(|_| ()).map_err(|e| format!("can't open {path}: {e}"))
}

/// "name (backend, kind)" of the adapter the window renders with, for Help › About and bug reports.
#[cfg(feature = "wgpu")]
pub fn adapter_summary(info: &eframe::wgpu::AdapterInfo) -> String {
    format!("{} ({:?}, {:?})", info.name.trim(), info.backend, info.device_type)
}

/// Tell the macOS input method to drop its composition (the Type tool kept the marked text as
/// typed). winit's IME toggle only clears its own copy, so the IME would type it again.
#[cfg(target_os = "macos")]
pub fn discard_marked_text() {
    if let Some(mtm) = objc2::MainThreadMarker::new()
        && let Some(ic) = objc2_app_kit::NSTextInputContext::currentInputContext(mtm)
    {
        ic.discardMarkedText();
    }
}
