//! Hosted mode: PhotoCraft running as a tab inside another eframe app (the Septet shell), which
//! owns the window, the fonts and the UI zoom of the shared `egui::Context`.
//!
//! While hosted the shell never calls `ctx.set_fonts` (the host installs one merged set, see
//! [`crate::theme::font_definitions`]), never sets the zoom factor, never resizes or maximizes
//! the window, and shows none of the original makers' community links and promotions
//! (`crate::links::COMMUNITY_COMMANDS`): a modified version may not present itself as theirs
//! (`docs/brand/LICENSE-brand.txt`). Standalone (the default) nothing changes.
//!
//! A host can also keep everything PhotoCraft stores per user under one folder of its own
//! ([`set_data_root`]), as PhotoCraft's portable mode does.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{PoisonError, RwLock};

static HOSTED: AtomicBool = AtomicBool::new(false);
static DATA_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Where this modified version of PhotoCraft comes from, in plain text (the About box): the
/// ArtCraft marks are removed from it (`docs/brand/README.md`).
pub const ATTRIBUTION: &str = "Based on PhotoCraft by the ArtCraft team (MIT OR Apache-2.0)";

/// Switch hosted mode on (before the app is built) or off.
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// Whether PhotoCraft runs inside a host app (see the module docs).
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// Keep the preferences, brush presets, crash recovery and every other per-user file under
/// `root` instead of the per-user OS folder (`None`: the usual folder). Set it before the app
/// is built: the desktop app resolves its settings folder once.
pub fn set_data_root(root: Option<PathBuf>) {
    *DATA_ROOT.write().unwrap_or_else(PoisonError::into_inner) = root;
}

/// The folder a host app keeps PhotoCraft's per-user files in, if it chose one ([`set_data_root`]).
pub fn data_root() -> Option<PathBuf> {
    DATA_ROOT.read().unwrap_or_else(PoisonError::into_inner).clone()
}
