//! Hosted mode: LightCraft runs as a tab inside a host application (Septet) that owns the
//! window, the egui context's fonts and the UI zoom. Set once by the host before the app is built
//! (`lightcraft_embed::Embedded::new`); a standalone LightCraft never sets it.
//!
//! While hosted, LightCraft never installs fonts (`ctx.set_fonts`: the host installs one merged set
//! that includes ours, see `theme::font_definitions`), never changes the zoom factor, never sizes
//! or places the window, and draws no window chrome of its own. It also shows no ArtCraft marks or
//! promotions (a derived build may not: `docs/brand/LICENSE-brand.txt`; see `links::shown`).
//!
//! A host may also keep LightCraft's per-user files in a folder of its own ([`set_data_root`]).

use std::sync::atomic::{AtomicBool, Ordering};

static HOSTED: AtomicBool = AtomicBool::new(false);

/// Mark LightCraft as running inside a host (process-wide).
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// LightCraft runs inside a host application.
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// Keep everything LightCraft writes or reads as per-user state (settings, camera profiles, the
/// SAM 3 model, the default library and export folder) under `root` instead of the per-user OS
/// folders — a host's portable install. `None`: the OS folders (the default). Set it before
/// LightCraft is built; see [`lightcraft_engine::paths`].
pub fn set_data_root(root: Option<std::path::PathBuf>) {
    lightcraft_engine::paths::set_data_root(root);
}

/// The folder set by [`set_data_root`], if any.
pub fn data_root() -> Option<std::path::PathBuf> {
    lightcraft_engine::paths::data_root()
}
