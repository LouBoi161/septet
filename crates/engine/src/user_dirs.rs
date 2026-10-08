//! Where DesignCraft keeps its per-user state: the preferences (`ui.json`, `prefs.json`) and the
//! crash-recovery folder. Every lookup of a per-user folder goes through here, so a portable
//! install (or a host app) can keep all of it under one data root instead of the OS folders.

use std::path::PathBuf;
use std::sync::{PoisonError, RwLock};

static DATA_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Keep every per-user file under `root` (created when missing) instead of the per-user OS
/// folders: the preferences in `root` itself, crash recovery in `root/Recovery`. `None` (the
/// default) uses the OS folders.
pub fn set_data_root(root: Option<PathBuf>) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(r) = &root
        && let Err(e) = std::fs::create_dir_all(r)
    {
        log::warn!("data folder {}: {e}", r.display());
    }
    *DATA_ROOT.write().unwrap_or_else(PoisonError::into_inner) = root;
}

/// The data root set by [`set_data_root`], if any.
pub fn data_root() -> Option<PathBuf> {
    DATA_ROOT.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The folder of the preferences: the data root, else the platform's per-user config folder
/// (`~/Library/Application Support/DesignCraft`, `%APPDATA%\DesignCraft`,
/// `$XDG_CONFIG_HOME/designcraft` or `~/.config/designcraft`).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(root) = data_root() {
        return Some(root);
    }
    if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/DesignCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DesignCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".config"))).map(|c| c.join("designcraft"))
    }
}

/// The crash-recovery folder: `Recovery` in the data root, else the platform's per-user one
/// (`~/Library/Application Support/DesignCraft/Recovery`, `%APPDATA%\DesignCraft\Recovery`,
/// `$XDG_DATA_HOME/designcraft/recovery` or `~/.local/share/designcraft/recovery`).
pub fn recovery_dir() -> Option<PathBuf> {
    if let Some(root) = data_root() {
        return Some(root.join("Recovery"));
    }
    if cfg!(target_os = "macos") {
        return home().map(|h| h.join("Library/Application Support/DesignCraft/Recovery"));
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DesignCraft").join("Recovery"));
    }
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".local/share"))).map(|d| d.join("designcraft/recovery"))
}
