//! The data root: one folder that holds everything VectorCraft keeps per user (preferences, swatch
//! and graphic style libraries, Data Recovery copies, logs, templates) instead of the per-user OS
//! folders, for a portable install. Set by the host before the app starts; none by default.

use std::path::PathBuf;
use std::sync::RwLock;

static ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Keep the app's per-user data under `root` (made if missing) instead of the per-user OS
/// folders; `None` goes back to those.
pub fn set_data_root(root: Option<PathBuf>) {
    if let Some(r) = &root {
        // A folder that can't be made shows up as the files in it failing to save.
        let _ = std::fs::create_dir_all(r);
    }
    *ROOT.write().unwrap_or_else(|e| e.into_inner()) = root;
}

/// The data root ([`set_data_root`]), if there is one.
pub fn data_root() -> Option<PathBuf> {
    ROOT.read().unwrap_or_else(|e| e.into_inner()).clone()
}
