//! FilmCraft's desktop pieces as a library: cpal audio output and voice-over input, the session and
//! project a launch starts with, the native file dialogs and their filters, the system appearance
//! and other OS hooks (all used by the `filmcraft` binary), and [`Embedded`], FilmCraft as a tab
//! inside a host shell (Septet).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

pub mod appearance;
pub mod audio;
pub mod audio_in;
pub mod desktop;
mod embed;
pub mod file_filters;

pub use embed::Embedded;
