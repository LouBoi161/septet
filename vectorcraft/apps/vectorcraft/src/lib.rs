//! The VectorCraft desktop app as a library: what the `vectorcraft` binary gives the app on the
//! desktop (services, preferences), and [`Embedded`], the app as a tab of another app's window.

mod clipboard;
// Also the binary's: `cursor_theme::apply` before any thread starts.
pub mod cursor_theme;
pub mod desktop;
#[cfg(feature = "wgpu")]
pub mod embed;
#[cfg(target_os = "macos")]
mod mac_fonts;
#[cfg(target_os = "macos")]
mod mac_save_format;
#[cfg(target_os = "macos")]
mod mac_screen_color;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "dragonfly", target_os = "netbsd", target_os = "openbsd", test))]
mod portal_save;
pub mod prefs;
// Shared with `vectorcraft-cli`, whose `#[path]` includes the file where it is.
pub mod prefs_dir;
mod printing;
#[cfg(all(windows, not(target_vendor = "win7")))]
mod system_fonts;

#[cfg(feature = "wgpu")]
pub use embed::Embedded;
