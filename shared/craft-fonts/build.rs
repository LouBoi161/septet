//! Embeds every face of the craft-fonts checkout in `CRAFT_FONTS_DIR` (its `fonts/manifest.txt`,
//! and `fonts/latin-manifest.txt` where the checkout has one: DeckCraft's presentation fonts) as one
//! `pub static` each. The apps' own build scripts read the same manifest and, with their
//! `shared-craft-fonts` feature, refer to these statics instead of embedding a copy each: six
//! copies of the CJK faces were ~200 MB of the suite binary.
//!
//! Errors are left to the apps' build scripts, which already report a bad checkout (as a warning,
//! or an error with `CRAFT_FONTS_REQUIRED=1`); this one just embeds nothing then. Use an absolute
//! `CRAFT_FONTS_DIR`: the apps resolve a relative one each from their own directory.
use std::fmt::Write as _;
use std::path::PathBuf;

#[path = "names.rs"]
mod names;

fn main() {
    println!("cargo::rerun-if-changed=names.rs");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    let mut src = String::new();
    if let Some(dir) = std::env::var_os("CRAFT_FONTS_DIR").filter(|d| !d.is_empty()).map(PathBuf::from) {
        let mut seen = std::collections::HashSet::new();
        for manifest in ["fonts/latin-manifest.txt", "fonts/manifest.txt"].map(|m| dir.join(m)) {
            println!("cargo::rerun-if-changed={}", manifest.display());
            for line in std::fs::read_to_string(&manifest).unwrap_or_default().lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some(file) = line.split(" | ").nth(2).map(str::trim) else { continue };
                if !seen.insert(file.to_string()) {
                    continue;
                }
                let Ok(path) = dir.join(file).canonicalize() else { continue };
                println!("cargo::rerun-if-changed={}", path.display());
                let Ok(len) = std::fs::metadata(&path).map(|m| m.len()) else { continue };
                // An array, not a `&[u8]`: the apps take its address. Reading a `&[u8]` static of
                // another crate in a static's initializer copies the bytes into that crate.
                let _ = writeln!(src, "pub static {}: [u8; {len}] = *include_bytes!({:?});", names::ident(file), path.display().to_string());
            }
        }
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("faces.rs");
    if std::fs::read_to_string(&out).is_ok_and(|old| old == src) {
        return;
    }
    if let Err(e) = std::fs::write(&out, src) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}
