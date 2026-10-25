//! DeckCraft's desktop app as a library: the platform services `main.rs` uses (audio output, the
//! system pasteboard), and [`Embedded`], DeckCraft as a tab inside another eframe app (Septet).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod audio;
pub mod embed;
pub mod pasteboard;

pub use embed::Embedded;
