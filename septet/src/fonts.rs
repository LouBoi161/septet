//! One set of fonts for everyone. egui keeps a single `FontDefinitions` per context and every app
//! used to replace it with its own; hosted, they don't (see each app's `hosted` module), and the
//! shell installs the union of what they all asked for.

use egui::{FontDefinitions, FontFamily};

use crate::hosted;
use crate::kinds::AppKind;

pub fn merged() -> FontDefinitions {
    let mut out = FontDefinitions::default();
    let mut proportional: Vec<String> = Vec::new();
    let mut monospace: Vec<String> = Vec::new();
    for kind in AppKind::ALL {
        let defs = hosted::font_definitions(kind);
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
