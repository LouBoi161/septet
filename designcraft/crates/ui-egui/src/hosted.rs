//! Hosted mode: DesignCraft runs as a tab inside another eframe app (Septet) that owns the
//! window, the UI fonts and the UI zoom. While hosted the app never calls `ctx.set_fonts` or
//! `ctx.set_zoom_factor`, and doesn't promote the upstream community (its Discord and website:
//! nothing may suggest the upstream team made or endorses a derived build,
//! `docs/brand/LICENSE-brand.txt`); standalone nothing changes. A portable data root
//! ([`set_data_root`]) keeps the app's per-user files together.

use std::sync::atomic::{AtomicBool, Ordering};

static HOSTED: AtomicBool = AtomicBool::new(false);

/// Mark the app as hosted (`true`) or standalone (`false`, the default).
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// The app runs inside a host that owns the window, the fonts and the UI zoom.
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// The upstream community (its Discord, its website and app pages) may be promoted: only
/// standalone.
pub fn show_community() -> bool {
    !is_hosted()
}

/// Help commands that open the upstream community's Discord, website and app pages: hidden from
/// the menus and the command palette when [`show_community`] is false.
pub fn is_community_command(id: &str) -> bool {
    matches!(id, "help.discord" | "help.website" | "help.appPage" | "help.app")
}

/// Keep every per-user file (preferences, crash recovery) under `root`, created when missing,
/// instead of the per-user OS folders (portable install); `None` = the OS folders (default).
/// Set it before the app starts.
pub fn set_data_root(root: Option<std::path::PathBuf>) {
    designcraft_engine::user_dirs::set_data_root(root);
}

/// The data root set by [`set_data_root`], if any.
pub fn data_root() -> Option<std::path::PathBuf> {
    designcraft_engine::user_dirs::data_root()
}
