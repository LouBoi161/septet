//! One set of fonts for everyone. egui keeps a single `FontDefinitions` per context and every app
//! used to replace it with its own; hosted, they don't (see each app's `hosted` module), and the
//! shell installs the union of what they all asked for.

use egui::{FontDefinitions, FontFamily};

use crate::kinds::APPS;

pub fn merged() -> FontDefinitions {
    let mut out = FontDefinitions::default();
    let mut proportional: Vec<String> = Vec::new();
    let mut monospace: Vec<String> = Vec::new();
    for app in APPS {
        let defs = (app.entry.font_definitions)();
        for (name, data) in defs.font_data {
            out.font_data.entry(name).or_insert(data);
        }
        for (family, list) in defs.families {
            let target = match &family {
                FontFamily::Proportional => &mut proportional,
                FontFamily::Monospace => &mut monospace,
                FontFamily::Name(_) => out.families.entry(family.clone()).or_default(),
            };
            // An app's first choice keeps its place; everyone else's become fallbacks.
            for font in list {
                if !target.contains(&font) {
                    target.push(font);
                }
            }
        }
    }
    out.families.insert(FontFamily::Proportional, proportional);
    out.families.insert(FontFamily::Monospace, monospace);
    crate::theme::add_shell_fonts(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    /// The apps' craft-fonts tables all point at the one copy in `shared/craft-fonts`; a face
    /// embedded twice shows up at a second address. Without `CRAFT_FONTS_DIR` the tables are empty.
    #[test]
    fn craft_fonts_are_embedded_once() {
        let mut seen: HashMap<(&str, &str), *const u8> = HashMap::new();
        let mut check = |family: &'static str, style: &'static str, bytes: &'static [u8]| {
            let first = *seen.entry((family, style)).or_insert(bytes.as_ptr());
            assert_eq!(first, bytes.as_ptr(), "{family} {style} is embedded more than once");
        };
        #[cfg(feature = "photocraft")]
        photocraft_text::CRAFT_FONTS.iter().for_each(|f| check(f.family, f.style, f.bytes));
        #[cfg(feature = "vectorcraft")]
        vectorcraft_text::CRAFT_FONTS.iter().for_each(|f| check(f.family, f.style, f.bytes));
        #[cfg(feature = "lightcraft")]
        lightcraft_engine::CRAFT_FONTS.iter().for_each(|f| check(f.family, f.style, f.bytes));
        #[cfg(feature = "designcraft")]
        designcraft_fonts::CRAFT_FONTS.iter().for_each(|f| check(f.family, f.style, f.bytes));
        #[cfg(feature = "pdfcraft")]
        pdfcraft_fonts::CRAFT_FONTS.iter().for_each(|f| check(f.family, f.style, f.bytes));
        #[cfg(feature = "filmcraft")]
        filmcraft_text::fonts::CRAFT_FONTS.iter().for_each(|f| check(f.family, f.style, f.bytes));
        #[cfg(feature = "wordcraft")]
        wordcraft_fonts::CRAFT_FONTS.iter().for_each(|f| check(f.family, f.style, f.bytes));
    }
}
