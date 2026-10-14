//! Hosted mode: the app drawn as a tab inside another app's window (Septet), which owns the
//! window, the fonts and the UI zoom. The host sets it once, before it makes the app.
//!
//! While hosted the app never replaces the fonts (`ctx.set_fonts`: the host installs one set for
//! every app it shows, this app's [`crate::theme::font_definitions`] among them), never sets the
//! zoom factor, and never sizes, places or decorates the window. Nor does it invite to the
//! ArtCraft team's community or website ([`ARTCRAFT_LINKS`]): a build they didn't make may only
//! say, in plain text, that it is based on VectorCraft by the ArtCraft team, nothing suggesting
//! they made or endorse it (`docs/brand/LICENSE-brand.txt`).
//!
//! The host may also keep everything the app stores per user under one folder, the data root
//! ([`set_data_root`]: a portable install), and set the UI brightness for all the apps it shows
//! ([`set_brightness`]).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::theme::Brightness;

static HOSTED: AtomicBool = AtomicBool::new(false);

/// The app runs inside a host's window (`true`) rather than in its own.
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// Does the app run inside a host's window ([`set_hosted`])?
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// The commands that link to the ArtCraft team's Discord community, their website and
/// VectorCraft's page there: no menu, palette entry or button offers them while hosted.
pub const ARTCRAFT_LINKS: [&str; 3] = ["help.discord", "help.website", "help.appPage"];

/// Does the UI offer command `id`? Everything but [`ARTCRAFT_LINKS`] while hosted.
pub fn offers(id: &str) -> bool {
    !(is_hosted() && ARTCRAFT_LINKS.contains(&id))
}

thread_local! {
    static BRIGHTNESS: Cell<Option<Brightness>> = const { Cell::new(None) };
    static OPEN_APPEARANCE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// The host's UI brightness, shown instead of Preferences › User Interface › Brightness and never
/// saved as it; `None` (the default) shows the app's own.
pub fn set_brightness(brightness: Option<Brightness>) {
    BRIGHTNESS.set(brightness);
}

/// The host's UI brightness ([`set_brightness`]), if it sets one.
pub fn brightness() -> Option<Brightness> {
    BRIGHTNESS.get()
}

/// The host's appearance settings, which the brightness commands open while the host sets the
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

/// Keep everything the app stores per user (preferences, libraries, Data Recovery, logs,
/// templates) under `root`, made if missing, instead of the per-user OS folders; `None` (the
/// default) uses those. Set before the app is made.
pub fn set_data_root(root: Option<PathBuf>) {
    vectorcraft_engine::data_root::set_data_root(root);
}

/// The data root ([`set_data_root`]), if there is one.
pub fn data_root() -> Option<PathBuf> {
    vectorcraft_engine::data_root::data_root()
}
