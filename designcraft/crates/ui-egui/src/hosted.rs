//! Hosted mode: DesignCraft runs as a tab inside another eframe app (Septet) that owns the
//! window, the UI fonts and the UI zoom. While hosted the app never calls `ctx.set_fonts` or
//! `ctx.set_zoom_factor`, and doesn't promote the upstream community (its Discord and website:
//! nothing may suggest the upstream team made or endorses a derived build,
//! `docs/brand/LICENSE-brand.txt`); standalone nothing changes. A portable data root
//! ([`set_data_root`]) keeps the app's per-user files together, and the host may set the
//! interface brightness for all the apps it shows ([`set_brightness`]).

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::theme::Brightness;

static HOSTED: AtomicBool = AtomicBool::new(false);

/// Mark the app as hosted (`true`) or standalone (`false`, the default).
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// The app runs inside a host that owns the window, the fonts and the UI zoom.
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// The upstream community (its Discord, its website and app pages) may be promoted: only
/// standalone.
pub fn show_community() -> bool {
    !is_hosted()
}

/// Help commands that open the upstream community's Discord, website and app pages: hidden from
/// the menus and the command palette when [`show_community`] is false.
pub fn is_community_command(id: &str) -> bool {
    matches!(id, "help.discord" | "help.website" | "help.appPage" | "help.app")
}

thread_local! {
    static BRIGHTNESS: Cell<Option<Brightness>> = const { Cell::new(None) };
    static OPEN_APPEARANCE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// The host's interface brightness, shown instead of Window › Interface Color Theme and never
/// saved as it; `None` (the default) shows the app's own.
pub fn set_brightness(brightness: Option<Brightness>) {
    BRIGHTNESS.set(brightness);
}

/// The host's interface brightness ([`set_brightness`]), if it sets one.
pub fn brightness() -> Option<Brightness> {
    BRIGHTNESS.get()
}

/// The host's appearance settings, which the theme commands open while the host sets the
/// brightness.
pub fn set_open_appearance(open: Box<dyn FnMut()>) {
    OPEN_APPEARANCE.set(Some(open));
}

/// Open the host's appearance settings ([`set_open_appearance`]) → whether there are any.
pub fn open_appearance() -> bool {
    OPEN_APPEARANCE.with_borrow_mut(|open| open.as_mut().map(|open| open()).is_some())
}

thread_local! {
    static ACCENT: Cell<Option<egui::Color32>> = const { Cell::new(None) };
    static RESTYLE: Cell<bool> = const { Cell::new(false) };
}

/// The host's accent colour, shown instead of the theme's (buttons, selections, focus) and never
/// saved as it; `None` (the default) keeps the theme's.
pub fn set_accent(accent: Option<egui::Color32>) {
    if ACCENT.replace(accent) != accent {
        RESTYLE.set(true);
    }
}

/// The host's accent colour ([`set_accent`]), if it sets one.
pub fn accent() -> Option<egui::Color32> {
    ACCENT.get()
}

/// Whether the host's accent or contrast changed since the last call: the theme is applied again.
pub fn take_restyle() -> bool {
    RESTYLE.replace(false)
}

thread_local! {
    static CONTRAST: Cell<bool> = const { Cell::new(false) };
}

/// More contrast, as the host asks: secondary text, borders and icons closer to the text colour,
/// at any brightness and accent; never saved. `false` (the default) keeps the theme's.
pub fn set_contrast(more: bool) {
    if CONTRAST.replace(more) != more {
        RESTYLE.set(true);
    }
}

/// Whether the host asks for more contrast ([`set_contrast`]).
pub fn contrast() -> bool {
    CONTRAST.get()
}

/// `c` recoloured from the theme's accent `from` to `to`: `to`'s hue, with `c`'s saturation and
/// brightness relative to `from` and its own alpha. Greys (white text, grey fills) stay.
pub(crate) fn recolor(c: egui::Color32, from: egui::Color32, to: egui::Color32) -> egui::Color32 {
    use egui::ecolor::HsvaGamma;
    let (c, from, to) = (HsvaGamma::from(c), HsvaGamma::from(from), HsvaGamma::from(to));
    if c.s < 0.04 || from.s < 0.05 || from.v < 0.05 {
        return c.into();
    }
    HsvaGamma { h: to.h, s: (to.s * c.s / from.s).clamp(0.0, 1.0), v: (to.v * c.v / from.v).clamp(0.0, 1.0), a: c.a }.into()
}

/// Tokens `t` with the host's accent ([`set_accent`]) in place of the theme's, and with more
/// contrast if the host asks for it ([`set_contrast`]).
pub(crate) fn with_accent(mut t: crate::theme::Tokens) -> crate::theme::Tokens {
    if let Some(to) = accent() {
        let from = t.accent;
        t.accent = recolor(t.accent, from, to);
        t.accent_strong = recolor(t.accent_strong, from, to);
        t.row_selected = recolor(t.row_selected, from, to);
    }
    if contrast() {
        t.text_dim = t.text_dim.lerp_to_gamma(t.text, 0.6);
        t.text_disabled = t.text_disabled.lerp_to_gamma(t.text, 0.35);
        t.border = t.border.lerp_to_gamma(t.text, 0.35);
        t.divider = t.divider.lerp_to_gamma(t.text, 0.35);
        t.field_border = t.field_border.lerp_to_gamma(t.text, 0.35);
        t.input_border = t.input_border.lerp_to_gamma(t.text, 0.35);
        t.section_divider = t.section_divider.lerp_to_gamma(t.text, 0.35);
        t.icon = t.icon.lerp_to_gamma(t.text, 0.5);
    }
    t
}

/// Keep every per-user file (preferences, crash recovery) under `root`, created when missing,
/// instead of the per-user OS folders (portable install); `None` = the OS folders (default).
/// Set it before the app starts.
pub fn set_data_root(root: Option<std::path::PathBuf>) {
    designcraft_engine::user_dirs::set_data_root(root);
}

/// The data root set by [`set_data_root`], if any.
pub fn data_root() -> Option<std::path::PathBuf> {
    designcraft_engine::user_dirs::data_root()
}

#[cfg(test)]
mod contrast_tests {
    use super::*;
    use crate::theme::{Brightness, Tokens};

    /// More contrast brings secondary text, borders and icons closer to the text colour, dark and
    /// light alike, leaves the rest as the theme has it, and restyles the app.
    #[test]
    fn more_contrast_brings_secondary_colours_towards_the_text() {
        let off = |c: egui::Color32, text: egui::Color32| {
            [(c.r(), text.r()), (c.g(), text.g()), (c.b(), text.b())].iter().map(|&(a, b)| i32::from(a).abs_diff(i32::from(b))).sum::<u32>()
        };
        for which in [Brightness::Dark, Brightness::Light] {
            let own = Tokens::for_brightness(which);
            take_restyle();
            set_contrast(true);
            assert!(take_restyle(), "switching restyles");
            let t = with_accent(own);
            set_contrast(false);
            for (name, now, was) in [
                ("text_dim", t.text_dim, own.text_dim),
                ("text_disabled", t.text_disabled, own.text_disabled),
                ("border", t.border, own.border),
                ("divider", t.divider, own.divider),
                ("field_border", t.field_border, own.field_border),
                ("input_border", t.input_border, own.input_border),
                ("section_divider", t.section_divider, own.section_divider),
                ("icon", t.icon, own.icon),
            ] {
                assert!(off(now, own.text) < off(was, own.text) || was == own.text, "{name} is closer to the text in {which:?}");
            }
            assert_eq!((t.text, t.accent), (own.text, own.accent));
            assert_eq!(with_accent(own).text_dim, own.text_dim, "without it: the theme's own");
        }
    }
}
