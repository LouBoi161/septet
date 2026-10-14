// Shared by this crate's build script and the apps' (`#[path]` module): both name a face's static
// the same way, so an app's generated `CRAFT_FONTS` can point at the one copy embedded here.

/// The static holding a manifest file's bytes: `fonts/biz-ud-pgothic/BIZUDPGothic-Regular.ttf`
/// becomes `FONTS_BIZ_UD_PGOTHIC_BIZUDPGOTHIC_REGULAR_TTF`.
pub fn ident(file: &str) -> String {
    file.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect()
}

/// The `bytes:` expression of an app's `CraftFont` entry. With the app crate's
/// `shared-craft-fonts` feature (the suite turns it on), the bytes are this crate's copy;
/// otherwise the app embeds the file itself, as upstream does.
#[allow(dead_code)]
pub fn bytes_expr(file: &str, path: &std::path::Path) -> String {
    if std::env::var_os("CARGO_FEATURE_SHARED_CRAFT_FONTS").is_some() {
        format!("&shared_craft_fonts::{}", ident(file))
    } else {
        format!("include_bytes!({:?})", path.display().to_string())
    }
}
