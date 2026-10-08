//! Where the desktop app keeps its per-user data: preferences, brush presets, crash-recovery
//! autosaves and the GPU startup marker all live under the one directory [`config_dir`] returns.
//!
//! In order:
//! 0. A host app's data root (`photocraft_ui_egui::hosted::data_root`, e.g. a portable host's
//!    `<exe dir>/Data/Photocraft`), used like the portable data folder below: created, and if it
//!    can't be written the app warns and goes on down this list.
//! 1. `PHOTOCRAFT_CONFIG_DIR`, when set (tests, agents, custom setups).
//! 2. **Portable mode** (#228): a marker file ([`PORTABLE_MARKERS`]) beside the executable puts
//!    everything in `<exe dir>/PhotoCraftData`, so a portable copy on a USB stick leaves nothing in
//!    `%APPDATA%`. The Windows portable zip ships with `portable.txt`; the MSI doesn't. If that
//!    folder can't be written (read-only medium, Program Files), the app warns and falls back.
//! 3. The platform convention: macOS `~/Library/Application Support/Photocraft`, Windows
//!    `%APPDATA%\Photocraft`, Linux `$XDG_CONFIG_HOME/photocraft` or `~/.config/photocraft`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Files beside the executable that switch on portable mode (either one; contents are ignored).
pub const PORTABLE_MARKERS: [&str; 2] = ["portable.txt", "PhotoCraft.portable"];
/// The data folder created beside the executable in portable mode.
pub const PORTABLE_DATA_DIR: &str = "PhotoCraftData";

/// How [`DataDir::dir`] was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `PHOTOCRAFT_CONFIG_DIR`.
    Override,
    /// `<exe dir>/PhotoCraftData`.
    Portable,
    /// The platform's per-user config directory.
    Platform,
    /// A host app's data root (`photocraft_ui_egui::hosted::data_root`).
    HostRoot,
}

/// The resolved data directory.
#[derive(Clone, Debug)]
pub struct DataDir {
    /// `None` only when no platform directory is known either (no `HOME`/`APPDATA`).
    pub dir: Option<PathBuf>,
    pub mode: Mode,
    /// Set when a portable marker was found but its data folder isn't writable.
    pub warning: Option<String>,
}

static DIR: OnceLock<DataDir> = OnceLock::new();

/// The data directory for this process, resolved once (the portable check probes the disk).
pub fn current() -> &'static DataDir {
    DIR.get_or_init(|| {
        let exe = std::env::current_exe().ok();
        let root = photocraft_ui_egui::hosted::data_root();
        let d = resolve_in(root.as_deref(), |k| std::env::var_os(k), exe.as_deref().and_then(Path::parent));
        if let Some(w) = &d.warning {
            log::warn!("{w}");
            eprintln!("photocraft: {w}");
        }
        log::info!("settings directory ({:?}): {:?}", d.mode, d.dir);
        d
    })
}

/// The per-user settings directory (see the module docs).
pub fn config_dir() -> Option<PathBuf> {
    current().dir.clone()
}

/// Whether [`current`] has been resolved already (a data root set afterwards no longer applies).
pub fn resolved() -> bool {
    DIR.get().is_some()
}

/// [`resolve`] with a host app's data root (`root`) first, kept like the portable data folder:
/// created and checked writable, else a warning and the rest of the list.
pub fn resolve_in(root: Option<&Path>, env: impl Fn(&str) -> Option<OsString>, exe_dir: Option<&Path>) -> DataDir {
    let Some(root) = root else { return resolve(env, exe_dir) };
    match ensure_writable(root) {
        Ok(()) => DataDir { dir: Some(root.to_path_buf()), mode: Mode::HostRoot, warning: None },
        Err(e) => {
            let mut d = resolve(env, exe_dir);
            let fallback = d.dir.as_ref().map_or_else(|| "nowhere (no user folder found)".to_string(), |p| p.display().to_string());
            let warning = format!("the data folder {} isn't writable ({e}); settings are kept in {fallback} instead", root.display());
            d.warning = Some(match d.warning {
                Some(w) => format!("{warning}; {w}"),
                None => warning,
            });
            d
        }
    }
}

/// Resolve the data directory from the environment (`env`) and the executable's folder.
pub fn resolve(env: impl Fn(&str) -> Option<OsString>, exe_dir: Option<&Path>) -> DataDir {
    if let Some(d) = env("PHOTOCRAFT_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return DataDir { dir: Some(PathBuf::from(d)), mode: Mode::Override, warning: None };
    }
    let platform = platform_dir(&env);
    let Some((exe_dir, marker)) = exe_dir.and_then(|d| portable_marker(d).map(|m| (d, m))) else {
        return DataDir { dir: platform, mode: Mode::Platform, warning: None };
    };
    let data = exe_dir.join(PORTABLE_DATA_DIR);
    match ensure_writable(&data) {
        Ok(()) => DataDir { dir: Some(data), mode: Mode::Portable, warning: None },
        Err(e) => {
            let fallback = platform.as_ref().map_or_else(|| "nowhere (no user folder found)".to_string(), |p| p.display().to_string());
            let warning =
                format!("portable mode ({} found) but {} isn't writable ({e}); settings are kept in {fallback} instead", marker.display(), data.display());
            DataDir { dir: platform, mode: Mode::Platform, warning: Some(warning) }
        }
    }
}

/// The platform convention, from `env` (so tests can fake `HOME`/`APPDATA`/`XDG_CONFIG_HOME`).
pub fn platform_dir(env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let home = env("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Application Support/Photocraft"));
    }
    if cfg!(windows) {
        return env("APPDATA").filter(|a| !a.is_empty()).map(|a| PathBuf::from(a).join("Photocraft"));
    }
    env("XDG_CONFIG_HOME").filter(|x| !x.is_empty()).map(PathBuf::from).or_else(|| home.map(|h| h.join(".config"))).map(|c| c.join("photocraft"))
}

/// The first portable marker file in `exe_dir`, if any.
pub fn portable_marker(exe_dir: &Path) -> Option<PathBuf> {
    PORTABLE_MARKERS.iter().map(|m| exe_dir.join(m)).find(|p| p.is_file())
}

/// Create `dir` and prove a file can be written in it (a folder can exist yet be read-only).
fn ensure_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(format!(".write-test-{}", std::process::id()));
    std::fs::write(&probe, b"")?;
    // A leftover probe is harmless; don't fail portable mode over it.
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("photocraft-app-dirs-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn env_of(pairs: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let m: HashMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), v.as_os_str().to_owned())).collect();
        move |k| m.get(k).cloned()
    }

    /// A fake user folder that every platform's convention resolves under.
    fn fake_home(root: &Path) -> impl Fn(&str) -> Option<OsString> + use<> {
        let home = root.join("home");
        let appdata = root.join("appdata");
        env_of(&[("HOME", &home), ("APPDATA", &appdata)])
    }

    #[test]
    fn no_marker_uses_the_platform_dir() {
        let root = temp("platform");
        let exe = root.join("bin");
        std::fs::create_dir_all(&exe).unwrap();
        let env = fake_home(&root);
        let d = resolve(&env, Some(&exe));
        assert_eq!(d.mode, Mode::Platform);
        assert!(d.warning.is_none());
        let dir = d.dir.unwrap();
        assert_eq!(Some(dir.clone()), platform_dir(&env));
        assert!(!dir.starts_with(&exe));
        assert!(!exe.join(PORTABLE_DATA_DIR).exists(), "no portable folder without a marker");
        // The convention per platform.
        if cfg!(target_os = "macos") {
            assert_eq!(dir, root.join("home/Library/Application Support/Photocraft"));
        } else if cfg!(windows) {
            assert_eq!(dir, root.join("appdata").join("Photocraft"));
        } else {
            assert_eq!(dir, root.join("home/.config/photocraft"));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn either_marker_keeps_everything_beside_the_exe() {
        for marker in PORTABLE_MARKERS {
            let root = temp("portable");
            let exe = root.join("PhotoCraft");
            std::fs::create_dir_all(&exe).unwrap();
            std::fs::write(exe.join(marker), b"").unwrap();
            let d = resolve(fake_home(&root), Some(&exe));
            assert_eq!(d.mode, Mode::Portable, "{marker}");
            assert!(d.warning.is_none());
            let dir = d.dir.unwrap();
            assert_eq!(dir, exe.join(PORTABLE_DATA_DIR));
            assert!(dir.is_dir(), "the data folder is created");
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the write probe is removed");
            // Every derived path sits under the exe dir.
            for sub in ["preferences.json", "Presets", "Recovery", crate::gpu_startup::MARKER_FILE] {
                assert!(dir.join(sub).starts_with(&exe));
            }
            assert!(!root.join("home").exists() && !root.join("appdata").exists(), "nothing written to the user folder");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn a_host_data_root_holds_everything_and_wins() {
        let root = temp("host-root");
        let exe = root.join("bin");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(exe.join("portable.txt"), b"").unwrap();
        let data = root.join("Data").join("Photocraft");
        let override_dir = root.join("override");
        let env = env_of(&[("PHOTOCRAFT_CONFIG_DIR", &override_dir), ("HOME", &root.join("home"))]);
        let d = resolve_in(Some(&data), &env, Some(&exe));
        assert_eq!((d.mode, d.dir.as_deref(), d.warning.as_deref()), (Mode::HostRoot, Some(data.as_path()), None));
        assert!(data.is_dir(), "created");
        assert_eq!(std::fs::read_dir(&data).unwrap().count(), 0, "the write probe is removed");
        assert!(!exe.join(PORTABLE_DATA_DIR).exists(), "the exe's portable folder isn't used");
        // Without a root: as before.
        assert_eq!(resolve_in(None, &env, Some(&exe)).mode, Mode::Override);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unwritable_host_data_root_falls_back_with_a_warning() {
        let root = temp("host-root-unwritable");
        // A file where the data folder should go: it can't be created, on every platform.
        let data = root.join("Data");
        std::fs::write(&data, b"in the way").unwrap();
        let d = resolve_in(Some(&data), fake_home(&root), None);
        assert_eq!(d.mode, Mode::Platform);
        assert_eq!(d.dir, platform_dir(&fake_home(&root)));
        assert!(d.warning.as_deref().is_some_and(|w| w.contains("isn't writable")), "{:?}", d.warning);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_marker_folder_is_not_a_marker() {
        let root = temp("dir-marker");
        let exe = root.join("bin");
        std::fs::create_dir_all(exe.join("portable.txt")).unwrap();
        assert_eq!(resolve(fake_home(&root), Some(&exe)).mode, Mode::Platform);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unwritable_portable_folder_falls_back_with_a_warning() {
        let root = temp("unwritable");
        let exe = root.join("bin");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(exe.join("portable.txt"), b"").unwrap();
        // A file where the data folder should go: it can't be created, on every platform.
        std::fs::write(exe.join(PORTABLE_DATA_DIR), b"in the way").unwrap();
        let env = fake_home(&root);
        let d = resolve(&env, Some(&exe));
        assert_eq!(d.mode, Mode::Platform);
        assert_eq!(d.dir, platform_dir(&env));
        let w = d.warning.unwrap();
        assert!(w.contains("portable") && w.contains("isn't writable"), "{w}");
        // No user folder at all: still no panic, just no directory.
        let d = resolve(|_| None, Some(&exe));
        assert!(d.dir.is_none() && d.warning.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn override_wins_over_portable() {
        let root = temp("override");
        let exe = root.join("bin");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(exe.join("portable.txt"), b"").unwrap();
        let custom = root.join("custom");
        let d = resolve(env_of(&[("PHOTOCRAFT_CONFIG_DIR", &custom)]), Some(&exe));
        assert_eq!((d.mode, d.dir), (Mode::Override, Some(custom)));
        assert!(!exe.join(PORTABLE_DATA_DIR).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_exe_dir_or_env_never_panics() {
        let d = resolve(|_| None, None);
        assert_eq!((d.mode, d.dir), (Mode::Platform, None));
        let empty = PathBuf::new();
        let d = resolve(env_of(&[("PHOTOCRAFT_CONFIG_DIR", &empty), ("HOME", &empty), ("APPDATA", &empty)]), None);
        assert_eq!(d.dir, None, "empty variables are ignored");
    }
}
