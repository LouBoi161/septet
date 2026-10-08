//! Hosted mode: the app drawn as a tab inside another app's window (Septet), which owns the
//! window, the fonts and the UI zoom. The host sets it once, before it makes the app.
//!
//! While hosted the app never replaces the fonts (`ctx.set_fonts`: the host installs one set for
//! every app it shows, this app's [`crate::theme::font_definitions`] among them), never sets the
//! zoom factor, and never sizes, places or decorates the window. Nor does it invite to the
//! ArtCraft team's community or website ([`ARTCRAFT_LINKS`]): a build they didn't make may only
//! say, in plain text, that it is based on VectorCraft by the ArtCraft team, nothing suggesting
//! they made or endorse it (`docs/brand/LICENSE-brand.txt`).
//!
//! The host may also keep everything the app stores per user under one folder, the data root
//! ([`set_data_root`]: a portable install).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static HOSTED: AtomicBool = AtomicBool::new(false);

/// The app runs inside a host's window (`true`) rather than in its own.
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// Does the app run inside a host's window ([`set_hosted`])?
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// The commands that link to the ArtCraft team's Discord community, their website and
/// VectorCraft's page there: no menu, palette entry or button offers them while hosted.
pub const ARTCRAFT_LINKS: [&str; 3] = ["help.discord", "help.website", "help.appPage"];

/// Does the UI offer command `id`? Everything but [`ARTCRAFT_LINKS`] while hosted.
pub fn offers(id: &str) -> bool {
    !(is_hosted() && ARTCRAFT_LINKS.contains(&id))
}

/// Keep everything the app stores per user (preferences, libraries, Data Recovery, logs,
/// templates) under `root`, made if missing, instead of the per-user OS folders; `None` (the
/// default) uses those. Set before the app is made.
pub fn set_data_root(root: Option<PathBuf>) {
    vectorcraft_engine::data_root::set_data_root(root);
}

/// The data root ([`set_data_root`]), if there is one.
pub fn data_root() -> Option<PathBuf> {
    vectorcraft_engine::data_root::data_root()
}
