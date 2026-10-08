//! LightCraft's desktop app as a library: the pieces `main.rs` builds the window from (settings
//! file, platform services, library session), and [`Embedded`], LightCraft as a tab of a host
//! application (Septet) that owns the window.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod alloc_release;
mod embed;
pub mod prefs;
pub mod services;
pub mod session;

pub use embed::Embedded;
