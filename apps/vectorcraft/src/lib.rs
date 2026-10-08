//! The VectorCraft desktop app as a library: what the `vectorcraft` binary gives the app on the
//! desktop (services, preferences), and [`Embedded`], the app as a tab of another app's window.

mod clipboard;
pub mod desktop;
#[cfg(feature = "wgpu")]
pub mod embed;
pub mod prefs;
mod printing;

#[cfg(feature = "wgpu")]
pub use embed::Embedded;
