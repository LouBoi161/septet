//! The Photocraft desktop app as a library: the platform services, settings directory, GPU
//! startup policy, display profiles and pen tablet input that the binary (`main.rs`) wires into
//! the UI, and [`Embedded`], the app built for a tab inside a host app (the Septet shell).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod app_dirs;
mod appearance;
pub mod caps_lock;
// The window caption style the desktop asks for; main.rs hands it to the standalone app.
pub mod caption_style;
pub mod crash_guard;
mod embed;
// Used by services.rs (an empty file dialog answer explained, #3354); main.rs starts it.
pub mod file_dialog_probe;
pub mod gpu_startup;
#[cfg(target_os = "linux")]
mod icon_theme;
// Pure logic is tested on every platform; only Linux runs the check.
#[cfg(any(target_os = "linux", test))]
pub mod linux_libs;
pub mod monitor_profile;
mod photoshop_settings;
// Save As through the desktop portal, which reports the file type picked (#6121); services.rs
// uses it.
#[cfg(target_os = "linux")]
mod portal_save;
// The print portal (File › Print on Linux); main.rs and `Embedded` hand its spooler to the
// session. It needs the session bus (zbus, Linux only); its pure helpers are tested everywhere.
#[cfg(any(target_os = "linux", test))]
pub mod print_portal;
pub mod services;
// Tablet support (macOS AppKit monitor, Linux X11/Wayland, Windows barrel button).
pub mod tablet;

pub use embed::{Embedded, Generate, GenerativeSource};

/// Tests that spawn child processes hold this for reading, tests that check who holds a file lock
/// for writing ([`no_child_spawns`]): until its `exec`, a child holds a copy of every open file,
/// `flock`s included, so a spawn in another test could keep a lock this process just dropped held
/// for a moment. (main.rs has its own for the binary's tests.)
#[cfg(test)]
pub(crate) static CHILD_SPAWNS: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Held by a test that checks who holds a file lock: no other test spawns a child meanwhile.
#[cfg(test)]
pub(crate) fn no_child_spawns() -> std::sync::RwLockWriteGuard<'static, ()> {
    CHILD_SPAWNS.write().unwrap_or_else(std::sync::PoisonError::into_inner)
}
