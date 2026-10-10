//! What the desktop gives the app ([`services`]): file dialogs, files, the system clipboard, the
//! browser, the file manager, the default apps and the printers. Plus opening the files the app
//! was handed and the folders it keeps next to its preferences.

use vectorcraft_engine::cmd::fileio;
use vectorcraft_ui_egui::picks::PickRequest;
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

/// Have the font scans also list the fonts font services load in place, from files outside the
/// font folders (#579): DirectWrite's system font collection on Windows, CoreText's font manager on
/// macOS. Before the first font scan (the app's start); only the first call counts.
pub fn install_platform_fonts() {
    #[cfg(all(windows, not(target_vendor = "win7")))]
    crate::system_fonts::install();
    #[cfg(target_os = "macos")]
    crate::mac_fonts::install();
}

/// The folders kept next to the preferences ([`prefs::data_dir`]): the User Defined swatch and
/// graphic style libraries, the Libraries panel's libraries and Data Recovery's copies.
pub fn set_user_folders(app: &mut VectorcraftApp) {
    let folder = |name: &str| prefs::data_dir().map(|d| d.join(name).to_string_lossy().to_string());
    // User Defined swatch and graphic style libraries live next to the preferences.
    app.session.swatch_libraries.set_user_dir(folder("Swatches"));
    app.session.style_libraries.set_user_dir(folder("Graphic Styles"));
    // So do the Libraries panel's libraries; runs without preferences (agents' test runs) keep
    // theirs for the session only, never touching the user's.
    if prefs::prefs_enabled() {
        app.session.libraries.set_dir(folder("Libraries"));
    }
    // Data Recovery copies live next to the preferences too (none for runs without
    // preferences, such as agents' test runs, unless the recoveryFolder preference is set).
    if std::env::var_os("VECTORCRAFT_NO_PREFS").is_none() {
        app.session.recovery.set_default_folder(folder("Data Recovery"));
    }
}

/// The window file dialogs belong to (none in a host's window, which the app doesn't own).
pub type Parent = Option<std::sync::Arc<winit::window::Window>>;

/// A native file dialog for `request` over `parent` (Windows and Linux, where the portal then
/// shows it over the window and keeps it in front; macOS shows them as before).
fn file_dialog(request: &PickRequest, parent: &Parent) -> rfd::FileDialog {
    let d = match parent {
        Some(w) if !cfg!(target_os = "macos") => rfd::FileDialog::new().set_parent(&**w),
        _ => rfd::FileDialog::new(),
    };
    let pick = match request {
        PickRequest::Open(pick) | PickRequest::Save(pick) => pick,
        PickRequest::OpenMany => return fileio::place_filters().fold(d.set_title("Place"), |d, (name, exts)| d.add_filter(name, exts)),
        PickRequest::Folder => return d,
    };
    let d = pick.filters.iter().fold(d, |d, (name, exts)| d.add_filter(*name, exts));
    let d = match &pick.folder {
        Some(folder) => d.set_directory(folder),
        None => d,
    };
    if pick.name.is_empty() { d } else { d.set_file_name(&pick.name) }
}

/// Show `dialog` for `request` (on the calling thread, until it closes) → the paths picked.
fn show_dialog(dialog: rfd::FileDialog, request: &PickRequest) -> Vec<String> {
    let paths = match request {
        PickRequest::Open(_) => dialog.pick_file().into_iter().collect(),
        PickRequest::OpenMany => dialog.pick_files().unwrap_or_default(),
        PickRequest::Save(pick) => {
            // The Templates folder may not exist yet.
            if let Some(folder) = &pick.folder {
                let _ = std::fs::create_dir_all(folder);
            }
            dialog.save_file().into_iter().collect()
        }
        PickRequest::Folder => dialog.pick_folder().into_iter().collect(),
    };
    paths.into_iter().map(|p| p.to_string_lossy().to_string()).collect()
}

/// Show the dialog for `request` over `parent` now → the paths picked.
fn pick_now(request: PickRequest, parent: &Parent) -> Vec<String> {
    show_dialog(file_dialog(&request, parent), &request)
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

/// Linux: show the dialog for `request` over `parent` on a thread of its own, answering on the
/// receiver. Shown on the UI thread, nothing would answer the compositor meanwhile, which then
/// offers to kill the window as not responding (#592).
fn start_pick(request: PickRequest, parent: &Parent) -> Option<std::sync::mpsc::Receiver<Vec<String>>> {
    let dialog = file_dialog(&request, parent);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("file-dialog".into())
        .spawn(move || {
            // The app gone meanwhile has no use for the answer.
            let _ = tx.send(show_dialog(dialog, &request));
        })
        .ok()?;
    Some(rx)
}

/// The desktop's services, file dialogs over `parent`.
pub fn services(parent: Parent) -> Services {
    let (p1, p2, p3, p4, p5) = (parent.clone(), parent.clone(), parent.clone(), parent.clone(), parent);
    Services {
        pick_open: Some(Box::new(move |pick: &FilePick| pick_now(PickRequest::Open(pick.clone()), &p1).into_iter().next())),
        pick_open_multi: Some(Box::new(move || pick_now(PickRequest::OpenMany, &p2))),
        pick_save: Some(Box::new(move |pick: &FilePick| pick_now(PickRequest::Save(pick.clone()), &p3).into_iter().next())),
        // Windows and macOS dialogs run the window's events while they are open; Linux's don't.
        start_pick: cfg!(all(unix, not(target_os = "macos")))
            .then(|| Box::new(move |request: PickRequest| start_pick(request, &p5)) as vectorcraft_ui_egui::picks::StartPick),
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
        pick_folder: Some(Box::new(move || pick_now(PickRequest::Folder, &p4).into_iter().next())),
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
