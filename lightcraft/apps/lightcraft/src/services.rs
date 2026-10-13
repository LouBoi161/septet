//! The desktop's platform services for the UI: file dialogs (rfd), opening files, links and
//! folders in other applications, and the file writers.

use crate::dialog_filter::FileDialogExt as _;
use lightcraft_ui_egui::Services;

/// The Windows command that opens `url` in the default browser. `explorer <url>` opened File
/// Explorer instead for links with a query or fragment, like the Map button's OpenStreetMap link
/// (issue #234). The URL protocol handler hands the URL to the browser as is; no shell runs, so
/// `?`, `&` and `#` need no escaping.
fn windows_open_url_command(url: &str) -> std::process::Command {
    let mut c = std::process::Command::new("rundll32");
    c.args(["url.dll,FileProtocolHandler", url]);
    c
}

/// Explorer's `/select,<path>` switch (issue #272). Explorer parses its own command line and only
/// selects the file when the path after the comma is quoted (`/select,"C:\My Photos\a.jpg"`),
/// spelled with backslashes and free of the verbatim prefix (`\\?\`, `\\?\UNC\`) that
/// `canonicalize` adds; given anything else it opens Documents. Windows file names can't contain
/// `"`; a path that does anyway (a hostile catalog) has them dropped, so the raw command line
/// always holds exactly this one argument.
fn explorer_select_arg(path: &str) -> String {
    let path = path.replace('/', "\\").replace('"', "");
    let path = match path.strip_prefix(r"\\?\UNC\") {
        Some(unc) => format!(r"\\{unc}"),
        None => path.strip_prefix(r"\\?\").unwrap_or(path.as_str()).to_string(),
    };
    format!("/select,\"{path}\"")
}

/// `explorer /select,"<path>"`. Rust's own argument quoting wraps the whole switch in quotes
/// (`"/select,C:\My Photos\a.jpg"`) whenever the path has a space, which Explorer doesn't
/// recognise, so on Windows the switch goes on the command line exactly as built.
fn explorer_select_command(path: &str) -> std::process::Command {
    let mut c = std::process::Command::new("explorer");
    let arg = explorer_select_arg(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.raw_arg(arg);
    }
    #[cfg(not(windows))]
    {
        c.arg(arg);
    }
    c
}

/// The platform services; `ctx` repaints when a file dialog closes, and `log_file` is
/// for Help ▸ Open Log Folder.
pub fn services(ctx: egui::Context, log_file: Option<&std::path::Path>) -> Services {
    Services {
        // Commands' file dialogs run on their own thread (#191): a dialog on the UI thread
        // stopped the window answering the compositor, which then reported the app as hung.
        picker: Some(Box::new(move |req: lightcraft_ui_egui::pick::PickRequest| {
            use lightcraft_ui_egui::pick::PickKind;
            let (tx, rx) = std::sync::mpsc::channel();
            let repaint = ctx.clone();
            std::thread::Builder::new()
                .name("file-dialog".into())
                .spawn(move || {
                    let mut d = rfd::FileDialog::new();
                    if let Some(t) = &req.title {
                        d = d.set_title(t);
                    }
                    if let Some((name, exts)) = &req.filter {
                        d = d.add_filter_nocase(name.clone(), exts);
                    }
                    if let Some(n) = &req.file_name {
                        d = d.set_file_name(n);
                    }
                    let s = |p: std::path::PathBuf| p.to_string_lossy().to_string();
                    let paths: Vec<String> = match req.kind {
                        PickKind::Files => d.pick_files().unwrap_or_default().into_iter().map(s).collect(),
                        PickKind::File => d.pick_file().map(s).into_iter().collect(),
                        PickKind::Folder => d.pick_folder().map(s).into_iter().collect(),
                        PickKind::Save => d.save_file().map(s).into_iter().collect(),
                    };
                    // the app may have quit meanwhile: nobody to tell
                    let _ = tx.send(paths);
                    repaint.request_repaint();
                })
                .map_err(|e| format!("could not start the file dialog: {e}"))?;
            Ok(rx)
        })),
        log_file: log_file.map(|p| p.display().to_string()),
        pick_lightroom_catalog: Some(Box::new(|| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Import Lightroom Catalog"))
                .add_filter_nocase(lightcraft_ui_egui::i18n::tr("Lightroom Classic Catalog"), &["lrcat"])
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        pick_folder: Some(Box::new(|| {
            rfd::FileDialog::new().set_title(lightcraft_ui_egui::i18n::tr("Open Library")).pick_folder().map(|p| p.to_string_lossy().to_string())
        })),
        open_with: Some(Box::new(|path: &str, app: &str| {
            // spawned, never waited for: the editor runs alongside
            let app = app.trim();
            let mut c = if cfg!(target_os = "macos") {
                let mut c = std::process::Command::new("open");
                if !app.is_empty() {
                    c.args(["-a", app]);
                }
                c
            } else if app.is_empty() {
                if cfg!(target_os = "windows") {
                    let mut c = std::process::Command::new("cmd");
                    c.args(["/C", "start", ""]);
                    c
                } else {
                    std::process::Command::new("xdg-open")
                }
            } else {
                std::process::Command::new(app)
            };
            c.arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
        })),
        open_url: Some(Box::new(|url: &str| {
            if !url.starts_with("https://") {
                return Err("only https links are opened".into());
            }
            let status = if cfg!(target_os = "macos") {
                std::process::Command::new("open").arg(url).status()
            } else if cfg!(target_os = "windows") {
                windows_open_url_command(url).status()
            } else {
                std::process::Command::new("xdg-open").arg(url).status()
            };
            status.map(|_| ()).map_err(|e| e.to_string())
        })),
        reveal: Some(Box::new(|path: &str| {
            let status = if cfg!(target_os = "macos") {
                std::process::Command::new("open").args(["-R", path]).status()
            } else if cfg!(target_os = "windows") {
                explorer_select_command(path).status()
            } else {
                let dir = std::path::Path::new(path).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_else(|| ".".into());
                std::process::Command::new("xdg-open").arg(dir).status()
            };
            status
                .map_err(|e| e.to_string())
                .and_then(|s| if s.success() || cfg!(target_os = "windows") { Ok(()) } else { Err(format!("reveal failed: {s}")) })
        })),
        pick_files: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter_nocase(lightcraft_ui_egui::i18n::tr("Photos"), lightcraft_engine::import::EXTENSIONS)
                .pick_files()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })),
        pick_denoise_model: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter_nocase(lightcraft_ui_egui::i18n::tr("ONNX model"), &["onnx"])
                .pick_file()
                .map(|p| vec![p.to_string_lossy().into_owned()])
                .unwrap_or_default()
        })),
        pick_preset_files: Some(Box::new(|| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Import Presets"))
                .add_filter_nocase(
                    lightcraft_ui_egui::i18n::tr("Presets & Profiles"),
                    &["lcpreset", "xmp", "lrtemplate", "zip", "dng", "lmp", "mplumpack", "cube"],
                )
                .pick_files()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })),
        pick_model_file: Some(Box::new(|| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Add a Face Recognition Model"))
                .add_filter_nocase(lightcraft_ui_egui::i18n::tr("Face models (ONNX)"), &["onnx"])
                .pick_file()
                .map(|p| vec![p.to_string_lossy().to_string()])
                .unwrap_or_default()
        })),
        pick_tracklog: Some(Box::new(|| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Auto-Tag from Tracklog"))
                .add_filter_nocase(lightcraft_ui_egui::i18n::tr("GPS Track Log"), &["gpx"])
                .pick_file()
                .map(|p| vec![p.to_string_lossy().to_string()])
                .unwrap_or_default()
        })),
        save_preset_file: Some(Box::new(|name: &str| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Export Presets"))
                .add_filter_nocase(lightcraft_ui_egui::i18n::tr("LightCraft Preset"), &["lcpreset"])
                .set_file_name(name)
                .save_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        pick_curve_preset_files: Some(Box::new(|| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Import Point Curve Presets"))
                .add_filter_nocase(lightcraft_ui_egui::i18n::tr("Point Curve Presets"), &["lccurve", "json"])
                .pick_files()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })),
        save_curve_preset_file: Some(Box::new(|name: &str| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Export Point Curve Presets"))
                .add_filter_nocase(lightcraft_ui_egui::i18n::tr("Point Curve Presets"), &["lccurve"])
                .set_file_name(name)
                .save_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        // atomic (temp file + sync + rename): a failed write never leaves a truncated file
        write_shared: Some(std::sync::Arc::new(lightcraft_engine::export::write_file)),
        write: Some(Box::new(lightcraft_engine::export::write_file)),
        png: Some(Box::new(|img: &lightcraft_raster::Rgba8| {
            lightcraft_codecs::encode_png(&lightcraft_codecs::EncodeImage::rgba8(img), &lightcraft_codecs::EncodeMeta::default()).unwrap_or_default()
        })),
        // the library is a folder on disk: backed up with the user's other files
        backup_library: None,
        restore_library: None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Issue #234: Windows opens links with the URL protocol handler (the default browser), not
    /// File Explorer, and passes the whole map link, `?`, `&` and `#` included, as one argument.
    #[test]
    fn windows_links_open_in_the_browser() {
        let url = "https://www.openstreetmap.org/?mlat=48.856600&mlon=2.352200#map=15/48.856600/2.352200";
        let c = windows_open_url_command(url);
        assert_eq!(c.get_program(), "rundll32");
        let args: Vec<&std::ffi::OsStr> = c.get_args().collect();
        assert_eq!(args, ["url.dll,FileProtocolHandler", url]);
    }

    /// Issue #272: Explorer gets the file to select quoted after `/select,`, with backslashes and
    /// without the verbatim prefix, even when the path has spaces (else it opens Documents).
    #[test]
    fn explorer_select_quotes_the_path() {
        assert_eq!(explorer_select_arg(r"C:\Users\Me\My Photos\IMG 1.jpg"), r#"/select,"C:\Users\Me\My Photos\IMG 1.jpg""#);
        assert_eq!(explorer_select_arg("D:/Example/Photos/IMG_0001.CR3"), r#"/select,"D:\Example\Photos\IMG_0001.CR3""#);
        assert_eq!(explorer_select_arg(r"\\server\share\My Photos\a.jpg"), r#"/select,"\\server\share\My Photos\a.jpg""#);
        assert_eq!(explorer_select_arg(r"\\?\D:\My Photos\a.jpg"), r#"/select,"D:\My Photos\a.jpg""#);
        assert_eq!(explorer_select_arg(r"\\?\UNC\server\share\a.jpg"), r#"/select,"\\server\share\a.jpg""#);
        // a `"` can't end the quoted path and add arguments to the raw command line
        assert_eq!(explorer_select_arg(r#"C:\a" "C:\b.jpg"#), r#"/select,"C:\a C:\b.jpg""#);
        let c = explorer_select_command(r"C:\My Photos\a.jpg");
        assert_eq!(c.get_program(), "explorer");
        let args: Vec<&std::ffi::OsStr> = c.get_args().collect();
        assert_eq!(args, [r#"/select,"C:\My Photos\a.jpg""#]);
    }
}
