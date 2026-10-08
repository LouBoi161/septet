//! FilmCraft's desktop pieces as a library: cpal audio output and voice-over input, the session and
//! project a launch starts with, the native file dialogs and OS hooks (all used by the `filmcraft`
//! binary), and [`Embedded`], FilmCraft as a tab inside a host shell (Septet).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

pub mod audio;
pub mod audio_in;
pub mod desktop;
mod embed;

pub use embed::Embedded;
