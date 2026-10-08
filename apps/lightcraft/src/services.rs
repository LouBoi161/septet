//! The desktop's platform services for the UI: file dialogs (rfd), opening files, links and
//! folders in other applications, and the file writers.

use lightcraft_ui_egui::Services;

/// The desktop services.
pub fn services() -> Services {
    Services {
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
                std::process::Command::new("explorer").arg(url).status()
            } else {
                std::process::Command::new("xdg-open").arg(url).status()
            };
            status.map(|_| ()).map_err(|e| e.to_string())
        })),
        reveal: Some(Box::new(|path: &str| {
            let status = if cfg!(target_os = "macos") {
                std::process::Command::new("open").args(["-R", path]).status()
            } else if cfg!(target_os = "windows") {
                std::process::Command::new("explorer").arg(format!("/select,{path}")).status()
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
                .add_filter(
                    lightcraft_ui_egui::i18n::tr("Photos"),
                    &[
                        "jpg", "jpeg", "png", "tif", "tiff", "webp", "dng", "cr2", "cr3", "nef", "nrw", "arw", "raf", "orf", "rw2", "rwl", "raw",
                        "pef", "psd", "jxl", "gif", "bmp",
                    ],
                )
                .pick_files()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })),
        pick_preset_files: Some(Box::new(|| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Import Presets"))
                .add_filter(
                    lightcraft_ui_egui::i18n::tr("Presets & Profiles"),
                    &["lcpreset", "xmp", "lrtemplate", "zip", "dng", "lmp", "mplumpack", "cube"],
                )
                .pick_files()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })),
        pick_tracklog: Some(Box::new(|| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Auto-Tag from Tracklog"))
                .add_filter(lightcraft_ui_egui::i18n::tr("GPS Track Log"), &["gpx"])
                .pick_file()
                .map(|p| vec![p.to_string_lossy().to_string()])
                .unwrap_or_default()
        })),
        save_preset_file: Some(Box::new(|name: &str| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Export Presets"))
                .add_filter(lightcraft_ui_egui::i18n::tr("LightCraft Preset"), &["lcpreset"])
                .set_file_name(name)
                .save_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        pick_curve_preset_files: Some(Box::new(|| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Import Point Curve Presets"))
                .add_filter(lightcraft_ui_egui::i18n::tr("Point Curve Presets"), &["lccurve", "json"])
                .pick_files()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })),
        save_curve_preset_file: Some(Box::new(|name: &str| {
            rfd::FileDialog::new()
                .set_title(lightcraft_ui_egui::i18n::tr("Export Point Curve Presets"))
                .add_filter(lightcraft_ui_egui::i18n::tr("Point Curve Presets"), &["lccurve"])
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
