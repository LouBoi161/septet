//! Where LightCraft keeps its per-user state: the OS's per-user folders, or — when a host runs
//! LightCraft from a portable install — one folder of the host's choosing ([`set_data_root`]).
//!
//! With a data root, [`crate::config::config_dir`] (settings, logs, camera profiles, the SAM 3,
//! face and denoise models, the GPU init marker) is the root itself, and the default library is
//! `<root>/Library` ([`crate::library::default_dir`]). Explicit overrides (`LIGHTCRAFT_LIBRARY`,
//! `LIGHTCRAFT_CAMERA_PROFILES`, `LIGHTCRAFT_SAM3_DIR`, `LIGHTCRAFT_FACE_MODELS`,
//! `LIGHTCRAFT_DENOISE_MODELS`) still win; `LIGHTCRAFT_CONFIG_DIR` (the whole settings folder)
//! does not: the data root replaces it.

use std::path::PathBuf;
use std::sync::{PoisonError, RwLock};

static DATA_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Keep everything LightCraft writes or reads as per-user state under `root` (created if missing)
/// instead of the per-user OS folders; `None`: the OS folders (the default). Process-wide; set it
/// before anything looks a folder up.
pub fn set_data_root(root: Option<PathBuf>) {
    if let Some(r) = &root
        && let Err(e) = std::fs::create_dir_all(r)
    {
        log::warn!("data folder {}: {e}", r.display());
    }
    *DATA_ROOT.write().unwrap_or_else(PoisonError::into_inner) = root;
}

/// The folder set by [`set_data_root`], if any.
pub fn data_root() -> Option<PathBuf> {
    DATA_ROOT.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Where per-user state lives: the data root when one is set, else what `os` gives (the OS
/// folder, `None` when it can't be found).
pub fn state_dir(os: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    data_root().or_else(os)
}
