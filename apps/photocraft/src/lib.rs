//! The Photocraft desktop app as a library: the platform services, settings directory, GPU
//! startup policy, display profiles and pen tablet input that the binary (`main.rs`) wires into
//! the UI, and [`Embedded`], the app built for a tab inside a host app (the Septet shell).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod app_dirs;
pub mod crash_guard;
mod embed;
pub mod gpu_startup;
pub mod monitor_profile;
pub mod services;
// Windows gets pen pressure from winit (WM_POINTER); the web runner has its own listener.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
pub mod tablet;

pub use embed::Embedded;
