//! Whether PdfCraft runs as a tab inside a host window (Septet) instead of in its own window.
//!
//! A host owns the window: its chrome, the fonts (one merged set for every app it shows) and the
//! UI zoom. While hosted, PdfCraft leaves those alone: it never calls `ctx.set_fonts` (the host
//! installed [`crate::theme::installed_font_definitions`] among its own) and its tab strip
//! doesn't move or maximize the window.
//!
//! In a host build PdfCraft doesn't invite people to the original authors' community (their
//! Discord, their website) ([`shows_community_links`]); PdfCraft's own page and source repository
//! stay. (The ArtCraft marks are gone from this modified tree altogether: `docs/brand/README.md`.)
//! And a host may keep everything PdfCraft stores per user in a folder of its own
//! ([`set_data_root`]). Standalone, nothing changes.

use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

static HOSTED: AtomicBool = AtomicBool::new(false);

static DATA_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Mark the app as running inside a host window (before the app is built).
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// `true` when a host window owns the window, its fonts and its zoom.
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// Keep everything PdfCraft stores per user (crash recovery, the digital IDs it creates) under
/// `root` instead of the user's data folders, which is created if missing (a portable install);
/// `None` goes back to those folders. Set it before the app is built.
pub fn set_data_root(root: Option<PathBuf>) {
    if let Some(dir) = &root
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        log::warn!("data folder {}: {e}", dir.display());
    }
    match DATA_ROOT.write() {
        Ok(mut r) => *r = root,
        Err(poisoned) => *poisoned.into_inner() = root,
    }
}

/// The folder set by [`set_data_root`], if any.
pub fn data_root() -> Option<PathBuf> {
    match DATA_ROOT.read() {
        Ok(r) => r.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Where PdfCraft keeps its per-user folder `name` ("Recovery", "Digital IDs"): under the data
/// root when one is set, else where `default` says (the user's data folders). Every per-user
/// folder is looked up through here.
pub fn user_dir(name: &str, default: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    match data_root() {
        Some(root) => Some(root.join(name)),
        None => default(),
    }
}

/// Whether the community links (Discord, the original authors' website) are shown: not in a host
/// build.
pub fn shows_community_links() -> bool {
    !is_hosted()
}

/// The Help commands that open those community links, left out of the menus and the command
/// palette when [`shows_community_links`] is off.
pub fn is_community_command(id: &str) -> bool {
    matches!(id, "help.discord" | "help.website")
}
