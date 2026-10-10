//! Where the desktop app keeps its settings, shared by `main.rs` and the embedded build
//! ([`crate::embed`]).

use pdfcraft_ui_egui::PdfCraftApp;

/// The eframe storage key PdfCraft's settings are saved under ([`PdfCraftApp`]'s `save`).
pub const STORAGE_KEY: &str = "pdfcraft";

/// The app was called PrintCraft before; settings saved then are under this key.
pub const LEGACY_STORAGE_KEY: &str = "printcraft";

/// The settings folder: `app.ron` and the `logs` folder (docs/development.md). In portable mode
/// it is `PdfCraftData` beside the executable (#157). eframe would otherwise derive it from the
/// app id; keep it under "PdfCraft". A host's data root, when set, takes the place of both
/// (`pdfcraft_ui_egui::hosted::set_data_root`; a host build is never portable on its own, see
/// `pdfcraft_ui_egui::portable::current`).
pub fn settings_dir() -> Option<std::path::PathBuf> {
    if let Some(root) = pdfcraft_ui_egui::hosted::data_root() {
        return Some(root);
    }
    if let Some(dir) = pdfcraft_ui_egui::portable::data_dir() {
        return Some(dir.to_path_buf());
    }
    eframe::storage_dir("PdfCraft")
}

/// Restore the settings saved in `storage` (under the PrintCraft key if that's all there is).
pub fn restore(app: &mut PdfCraftApp, storage: Option<&dyn eframe::Storage>) {
    if let Some(json) = storage.and_then(|s| s.get_string(STORAGE_KEY).or_else(|| s.get_string(LEGACY_STORAGE_KEY))) {
        app.restore(&json);
    }
}

/// Move the settings and crash-recovery folders of the app's former name, PrintCraft, to the new
/// name once, so an upgrade keeps recent files, preferences and unsaved work. Best effort: a
/// folder is left alone when the new one already exists or the move fails. A portable copy, and
/// a host's data root, leave the per-user folders alone.
pub fn migrate_legacy_folders() {
    // A data root is the host's (or the portable copy's): the user's folders stay as they are.
    if pdfcraft_ui_egui::hosted::data_root().is_some() || pdfcraft_ui_egui::portable::data_dir().is_some() {
        return;
    }
    let mut moves = vec![(eframe::storage_dir("PrintCraft"), settings_dir())];
    // Recovery lives in the settings folder except on Windows, where it is under %LOCALAPPDATA%.
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
        moves.push((local.as_ref().map(|d| d.join("PrintCraft")), local.map(|d| d.join("PdfCraft"))));
    }
    for (old, new) in moves {
        let (Some(old), Some(new)) = (old, new) else { continue };
        if !old.is_dir() || new.exists() {
            continue;
        }
        if let Some(parent) = new.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::rename(&old, &new) {
            Ok(()) => log::info!("moved {} to {}", old.display(), new.display()),
            Err(e) => log::warn!("moving {} to {}: {e}", old.display(), new.display()),
        }
    }
}
