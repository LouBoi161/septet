//! The EffectCraft desktop app as a library: what the `effectcraft` window and an embedding host
//! (Septet, which shows EffectCraft as a tab: [`Embedded`]) set up the same way.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod audio_out;
pub mod desktop;
mod embed;

pub use embed::Embedded;
