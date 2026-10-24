//! WordCraft's desktop app as a library: the platform services `main.rs` uses (file dialogs,
//! printing, the system clipboard, the animation setting, default apps, the system region), and
//! [`Embedded`], WordCraft as a tab inside another eframe app (Septet).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod animations;
pub mod clipboard;
pub mod default_apps;
pub mod embed;
pub mod file_dialogs;
pub mod print;
pub mod region;

pub use embed::Embedded;
