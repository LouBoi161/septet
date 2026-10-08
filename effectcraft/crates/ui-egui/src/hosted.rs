//! Running as a tab of a host window (Septet) instead of owning the window: the host installs
//! the fonts, owns the UI zoom and the window's size and chrome, so the app leaves them alone.
//! Off (the standalone app) unless the embedding host turns it on before building the app.
//!
//! Hosted, EffectCraft also leaves out its community Discord and website links
//! ([`hides_command`]): nothing in a host suggests the ArtCraft team made or endorses it
//! (`docs/brand/LICENSE-brand.txt`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static HOSTED: AtomicBool = AtomicBool::new(false);

/// Mark the process as hosting EffectCraft in another app's window (`true`) or not.
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// Whether EffectCraft runs inside a host's window (see [`set_hosted`]).
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// Keep everything EffectCraft stores per user under `root` (a portable install) instead of the
/// platform's folders; `None` = the platform's. Set it before building the app.
pub fn set_data_root(root: Option<PathBuf>) {
    effectcraft_engine::config::set_data_root(root);
}

/// The portable data root, when one is set.
pub fn data_root() -> Option<PathBuf> {
    effectcraft_engine::config::data_root()
}

/// Commands whose menu entries, buttons and links a hosted build leaves out: the community
/// Discord and the getartcraft.com pages (the website, EffectCraft's page there and its online
/// tutorials). EffectCraft's GitHub, documentation and feedback links stay.
pub fn hides_command(id: &str) -> bool {
    is_hosted() && matches!(id, "help.discord" | "help.website" | "help.appPage" | "help.onlineTutorials")
}
