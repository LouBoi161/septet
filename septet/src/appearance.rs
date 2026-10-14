//! One look for Septet and every app in it (`docs/suite/PLAN.md`, A4): a brightness level, which
//! Septet hands to each app instead of the app's own theme ([`crate::hosted::HostedApp::set_appearance`]),
//! and one UI size for all of them. The apps never save it as their own; their theme menus open
//! this dialog instead.

use egui::{Color32, Context, Id, RichText};
use serde::{Deserialize, Serialize};

use crate::theme::{self, ShellColors};

/// The levels the apps know, darkest first. An app with fewer variants maps them onto its own
/// (Pdfcraft: Dark for both dark levels, Light for both light ones).
pub const LEVELS: [&str; 4] = ["Darkest", "Dark", "Light", "Lightest"];

/// The brightness the user picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Theme {
    /// Each app shows the theme set in its own preferences (Septet's look before A4).
    AppsOwn,
    /// Dark or Light, as the desktop says.
    System,
    Darkest,
    #[default]
    Dark,
    Light,
    Lightest,
}

impl Theme {
    const ALL: [Theme; 6] = [Theme::System, Theme::Darkest, Theme::Dark, Theme::Light, Theme::Lightest, Theme::AppsOwn];

    fn label(self) -> &'static str {
        match self {
            Theme::AppsOwn => "Each app's own",
            Theme::System => "Same as the desktop",
            Theme::Darkest => LEVELS[0],
            Theme::Dark => LEVELS[1],
            Theme::Light => LEVELS[2],
            Theme::Lightest => LEVELS[3],
        }
    }
}

/// Saved in Septet's storage (`septet/appearance`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: Theme,
    /// The whole UI's zoom, every app's included (1.0 = 100 %).
    pub ui_scale: f32,
    /// Buttons, selections and focus in every app (one of [`ACCENTS`]); `None`: each app's own.
    pub accent: Option<[u8; 3]>,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance { theme: Theme::default(), ui_scale: 1.0, accent: None }
    }
}

/// The accent colours on offer. The apps write white on their accent (primary buttons, selected
/// rows), so each of these keeps white text readable; a free colour picker could not.
pub const ACCENTS: [(&str, [u8; 3]); 8] = [
    ("Blue", [0x25, 0x63, 0xeb]),
    ("Purple", [0x7c, 0x3a, 0xed]),
    ("Pink", [0xdb, 0x27, 0x77]),
    ("Red", [0xdc, 0x26, 0x26]),
    ("Orange", [0xc2, 0x41, 0x0c]),
    ("Green", [0x15, 0x80, 0x3d]),
    ("Teal", [0x0f, 0x76, 0x6e]),
    ("Graphite", [0x52, 0x52, 0x5b]),
];

pub const STORAGE_KEY: &str = "septet/appearance";

/// The UI sizes on offer.
const SCALES: [f32; 7] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75];

impl Appearance {
    /// The level the apps get, 0 (darkest) to 3 (lightest); `None`: each app its own theme.
    /// `desktop_dark` is the desktop's preference, if it says (`None`: dark, as Septet was).
    pub fn level(&self, desktop_dark: Option<bool>) -> Option<u8> {
        match self.theme {
            Theme::AppsOwn => None,
            Theme::System => Some(if desktop_dark.unwrap_or(true) { 1 } else { 2 }),
            Theme::Darkest => Some(0),
            Theme::Dark => Some(1),
            Theme::Light => Some(2),
            Theme::Lightest => Some(3),
        }
    }

    pub fn accent(&self) -> Option<Color32> {
        self.accent.map(|[r, g, b]| Color32::from_rgb(r, g, b))
    }

    pub fn ui_scale(&self) -> f32 {
        if self.ui_scale.is_finite() { self.ui_scale.clamp(SCALES[0], SCALES[SCALES.len() - 1]) } else { 1.0 }
    }
}

/// What the desktop says about dark or light: the XDG Settings portal on Linux (winit doesn't
/// know there), the window system elsewhere.
pub struct Desktop {
    #[cfg(target_os = "linux")]
    portal: Option<pdfcraft_platform::desktop_theme::DesktopTheme>,
}

impl Desktop {
    pub fn new() -> Self {
        Desktop {
            #[cfg(target_os = "linux")]
            portal: None,
        }
    }

    /// `Some(true)` for dark. The portal is asked only while the user follows the desktop.
    pub fn dark(&mut self, ctx: &Context, following: bool) -> Option<bool> {
        #[cfg(target_os = "linux")]
        {
            if following && self.portal.is_none() {
                let ctx = ctx.clone();
                self.portal = Some(pdfcraft_platform::desktop_theme::DesktopTheme::start(move || ctx.request_repaint()));
            }
            if let Some(portal) = &self.portal {
                portal.set_active(following);
                if let Some(a) = portal.get() {
                    return Some(a == pdfcraft_platform::desktop_theme::DesktopAppearance::Dark);
                }
            }
        }
        let _ = following;
        ctx.system_theme().map(|t| t == egui::Theme::Dark)
    }
}

/// The Appearance dialog. Changes apply at once; `appearance` is what Septet saves.
pub fn dialog(open: &mut bool, ctx: &Context, appearance: &mut Appearance) {
    if !*open {
        return;
    }
    let c = ShellColors::home();
    let resp = egui::Modal::new(Id::new("septet-appearance")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.label(RichText::new("Appearance").font(theme::semibold(18.0)));
        ui.add_space(4.0);
        ui.label(
            RichText::new("For Septet and every app in it. The apps' own theme settings stay as they are for when they run on their own.").color(c.text_dim),
        );
        ui.add_space(12.0);
        ui.label(RichText::new("Brightness").font(theme::semibold(13.0)));
        for t in Theme::ALL {
            ui.radio_value(&mut appearance.theme, t, t.label());
        }
        if appearance.theme == Theme::AppsOwn {
            ui.label(RichText::new("Septet's own screens stay dark.").color(c.text_dim));
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Accent colour").font(theme::semibold(13.0)));
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut appearance.accent, None, "Each app's own");
            ui.add_space(6.0);
            for (name, rgb) in ACCENTS {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                let [r, g, b] = rgb;
                ui.painter().circle_filled(rect.center(), 9.0, Color32::from_rgb(r, g, b));
                if appearance.accent == Some(rgb) {
                    ui.painter().circle_stroke(rect.center(), 10.5, egui::Stroke::new(2.0, c.text));
                }
                if resp.on_hover_text(name).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    appearance.accent = Some(rgb);
                }
            }
        });
        ui.add_space(12.0);
        ui.label(RichText::new("UI size").font(theme::semibold(13.0)));
        let current = appearance.ui_scale();
        egui::ComboBox::from_id_salt("septet-ui-scale").selected_text(format!("{:.0} %", current * 100.0)).show_ui(ui, |ui| {
            for s in SCALES {
                if ui.selectable_label((s - current).abs() < 1e-3, format!("{:.0} %", s * 100.0)).clicked() {
                    appearance.ui_scale = s;
                }
            }
        });
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new("Done").color(c.on_accent)).fill(c.accent)).clicked() {
                ui.close();
            }
        });
    });
    if resp.should_close() {
        *open = false;
    }
}

/// Septet's own look at `level` (`None`: dark, as before A4) with `accent`: the shell's colours and
/// egui's style for its own widgets.
pub fn apply_to_shell(ctx: &Context, level: Option<u8>, accent: Option<Color32>) {
    theme::set_level(level, accent);
    let light = level.is_some_and(|l| l >= 2);
    ctx.set_theme(if light { egui::ThemePreference::Light } else { egui::ThemePreference::Dark });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_the_choice_and_the_desktop() {
        let at = |theme, desktop| Appearance { theme, ..Default::default() }.level(desktop);
        assert_eq!(at(Theme::AppsOwn, Some(true)), None);
        assert_eq!(at(Theme::Darkest, Some(false)), Some(0));
        assert_eq!(at(Theme::Lightest, Some(true)), Some(3));
        assert_eq!(at(Theme::System, Some(false)), Some(2));
        assert_eq!(at(Theme::System, Some(true)), Some(1));
        // A desktop that says nothing: dark, as Septet always was.
        assert_eq!(at(Theme::System, None), Some(1));
    }

    #[test]
    fn saved_settings_read_back_and_bad_sizes_are_clamped() {
        let a = Appearance { theme: Theme::Light, ui_scale: 1.25, accent: Some(ACCENTS[2].1) };
        let back: Appearance = ron::from_str(&ron::to_string(&a).unwrap()).unwrap();
        assert_eq!(back, a);
        let partial: Appearance = ron::from_str("(theme: Darkest)").unwrap();
        assert_eq!(partial.ui_scale(), 1.0);
        assert_eq!(Appearance { ui_scale: 9.0, ..Default::default() }.ui_scale(), 1.75);
        assert_eq!(Appearance { ui_scale: f32::NAN, ..Default::default() }.ui_scale(), 1.0);
    }
}
