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

/// Whether the host's accent changed since the last call: the theme is applied again.
pub fn take_restyle() -> bool {
    RESTYLE.replace(false)
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

/// Tokens `t` with the host's accent ([`set_accent`]) in place of the theme's.
pub(crate) fn with_accent(mut t: crate::theme::Tokens) -> crate::theme::Tokens {
    if let Some(to) = accent() {
        let from = t.accent;
        t.accent = recolor(t.accent, from, to);
        t.accent_strong = recolor(t.accent_strong, from, to);
        t.row_selected = recolor(t.row_selected, from, to);
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
