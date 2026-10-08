//! FilmCraft hosted as a tab inside another app's window (Septet).
//!
//! The host owns the window and the `egui::Context` it shares with its other apps: it installs one
//! set of fonts for all of them (FilmCraft's from [`crate::theme::font_definitions`]), owns the UI
//! zoom and draws the window chrome. While hosted, FilmCraft leaves those alone. Standalone
//! (the default) nothing changes. The host can also keep FilmCraft's per-user data in a folder of
//! its own ([`set_data_root`], a portable install) and take project items dragged out of FilmCraft
//! into its other apps ([`dragged_items`]).

use std::sync::atomic::{AtomicBool, Ordering};

use filmcraft_media::MediaKind;
use filmcraft_project::{ItemId, ItemKind, MediaRef, Project};

static HOSTED: AtomicBool = AtomicBool::new(false);

/// Say whether FilmCraft runs inside a host (set once, before the app is built).
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// Whether FilmCraft runs inside a host that owns the fonts, the zoom and the window.
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// Keep everything FilmCraft keeps per user (settings, workspaces, auto-save and crash recovery,
/// shortcuts, presets, logs, the media cache, downloaded speech models) in `root`, e.g. next to a
/// portable install; None = the per-user data folder. Process-wide; set it before the session
/// starts. Every lookup goes through [`filmcraft_engine::autosave::default_data_dir`].
pub fn set_data_root(root: Option<std::path::PathBuf>) {
    filmcraft_engine::autosave::set_data_root(root);
}

/// The data root set with [`set_data_root`], if any.
pub fn data_root() -> Option<std::path::PathBuf> {
    filmcraft_engine::autosave::data_root()
}

/// The project items being dragged out of the Project panel, a bin or the Media Browser, for a host
/// that lets them be dropped into another app: the selection when the dragged item is part of it,
/// every file of a Media Browser drag. Empty when no such drag is in progress.
pub fn dragged_items(app: &crate::FilmcraftApp, ctx: &egui::Context) -> Vec<ItemId> {
    let Some(item) = crate::panels::dragged_item(ctx) else { return Vec::new() };
    if let Some(files) = crate::panels::media_browser::drag_items(ctx).filter(|f| f.first() == Some(&item.0)) {
        return files.into_iter().map(ItemId).filter(|i| app.session.project.item(*i).is_some()).collect();
    }
    let sel = &app.session.state.project_selection;
    if sel.contains(&item) { sel.clone() } else { vec![item] }
}

/// The drag of [`dragged_items`] ended in another app, which took it: forget it as a drop on nothing
/// does, without acting on the release (an import the Media Browser made for it is taken back).
pub fn cancel_drag(app: &mut crate::FilmcraftApp, ctx: &egui::Context) {
    crate::panels::cancel_drag(app, ctx);
}

/// The movie, audio or still file on disk an item plays (a subclip's master clip's), as imported.
/// None for sequences, generators, image sequences and other items without one such file.
pub fn source_file(project: &Project, item: ItemId) -> Option<String> {
    let mut id = item;
    // a damaged project's subclip chain can be cyclic or arbitrarily deep
    for _ in 0..32 {
        match &project.item(id)?.kind {
            ItemKind::Media(m) => {
                return match &m.media {
                    MediaRef::File { path } if matches!(m.info.kind, MediaKind::Movie | MediaKind::AudioOnly | MediaKind::Still) => Some(path.clone()),
                    _ => None,
                };
            }
            ItemKind::Subclip { parent, .. } => id = *parent,
            _ => return None,
        }
    }
    None
}
