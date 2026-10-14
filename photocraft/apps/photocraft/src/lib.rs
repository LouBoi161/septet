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
pub mod services;
// Windows gets pen pressure from winit (WM_POINTER); the web runner has its own listener.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
pub mod tablet;

pub use embed::Embedded;
