//! The desktop app's library side: its settings and the system preferences it starts from
//! (`reopen`, `motion`), shared with `main.rs`, and [`Embedded`], PdfCraft
//! built to run as a tab of a host window (Septet) instead of in its own.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod embed;
#[cfg(target_os = "windows")]
pub mod motion;
pub mod reopen;
pub mod settings;

pub use embed::Embedded;
