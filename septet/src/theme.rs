//! The shell's own look: the tab strip and the Home screen. Colours follow the active app's
//! panel colour, so the selected tab flows into that app's menu bar the way a browser tab flows
//! into its page.

use egui::{Color32, FontData, FontDefinitions, FontFamily, Visuals};
use std::sync::Arc;

/// Font family names the shell uses (registered next to the apps' own, see `fonts`).
pub const SHELL_MEDIUM: &str = "septet-medium";
pub const SHELL_SEMIBOLD: &str = "septet-semibold";

#[derive(Clone, Copy, Debug)]
pub struct ShellColors {
    /// Behind the tabs (the window's title strip).
    pub strip: Color32,
    /// The selected tab: the active app's chrome colour.
    pub tab_active: Color32,
    pub tab_hover: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub separator: Color32,
    pub close_hover: Color32,
    pub accent: Color32,
    pub home_bg: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub card_border: Color32,
}

impl ShellColors {
    /// Colours for a strip sitting on top of an app whose panels use `visuals`.
    pub fn for_app(visuals: &Visuals, accent: Color32) -> Self {
        let base = visuals.panel_fill;
        let dark = visuals.dark_mode;
        let strip = if dark { darken(base, 0.42) } else { darken(base, 0.12) };
        ShellColors {
            strip,
            tab_active: base,
            tab_hover: mix(strip, base, 0.5),
            text: visuals.strong_text_color(),
            text_dim: visuals.weak_text_color(),
            separator: if dark { lighten(strip, 0.10) } else { darken(strip, 0.15) },
            close_hover: Color32::from_rgb(0xc4, 0x2b, 0x1c),
            accent,
            home_bg: if dark { Color32::from_rgb(0x1b, 0x1b, 0x1d) } else { Color32::from_rgb(0xf3, 0xf3, 0xf5) },
            card: if dark { Color32::from_rgb(0x26, 0x26, 0x29) } else { Color32::WHITE },
            card_hover: if dark { Color32::from_rgb(0x30, 0x30, 0x34) } else { Color32::from_rgb(0xea, 0xea, 0xee) },
            card_border: if dark { Color32::from_rgb(0x36, 0x36, 0x3a) } else { Color32::from_rgb(0xdd, 0xdd, 0xe2) },
        }
    }

    /// The Home screen and empty windows: Septet's own dark look.
    pub fn home() -> Self {
        let mut v = Visuals::dark();
        v.panel_fill = Color32::from_rgb(0x1b, 0x1b, 0x1d);
        // Septet's mark is all seven app colours; its own accent stays neutral.
        Self::for_app(&v, Color32::from_rgb(0xf4, 0xf4, 0xf6))
    }
}

pub fn darken(c: Color32, amount: f32) -> Color32 {
    let f = 1.0 - amount;
    Color32::from_rgba_premultiplied((c.r() as f32 * f) as u8, (c.g() as f32 * f) as u8, (c.b() as f32 * f) as u8, c.a())
}

pub fn lighten(c: Color32, amount: f32) -> Color32 {
    let l = |v: u8| (v as f32 + (255.0 - v as f32) * amount) as u8;
    Color32::from_rgba_premultiplied(l(c.r()), l(c.g()), l(c.b()), c.a())
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

/// Inter, as all seven apps use it, for the shell's own families.
pub fn add_shell_fonts(defs: &mut FontDefinitions) {
    let medium = include_bytes!("../../photocraft/assets/fonts/Inter-Medium.ttf");
    let semibold = include_bytes!("../../photocraft/assets/fonts/Inter-SemiBold.ttf");
    defs.font_data.insert(SHELL_MEDIUM.into(), Arc::new(FontData::from_static(medium)));
    defs.font_data.insert(SHELL_SEMIBOLD.into(), Arc::new(FontData::from_static(semibold)));
    let fallback = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, font) in [(SHELL_MEDIUM, SHELL_MEDIUM), (SHELL_SEMIBOLD, SHELL_SEMIBOLD)] {
        let mut list = vec![font.to_string()];
        list.extend(fallback.iter().filter(|f| *f != font).cloned());
        defs.families.insert(FontFamily::Name(family.into()), list);
    }
}

pub fn medium(size: f32) -> egui::FontId {
    egui::FontId::new(size, FontFamily::Name(SHELL_MEDIUM.into()))
}

pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, FontFamily::Name(SHELL_SEMIBOLD.into()))
}
