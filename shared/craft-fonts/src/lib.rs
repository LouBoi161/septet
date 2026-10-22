//! The craft-fonts faces, embedded once (see `build.rs`). Not used directly: the apps' generated
//! `CRAFT_FONTS` tables point at these statics when the suite builds them with their
//! `shared-craft-fonts` feature.

#![allow(missing_docs)]

include!(concat!(env!("OUT_DIR"), "/faces.rs"));
