//! Hosted mode: LightCraft runs as a tab inside a host application (Septet) that owns the
//! window, the egui context's fonts and the UI zoom. Set once by the host before the app is built
//! (`lightcraft_embed::Embedded::new`); a standalone LightCraft never sets it.
//!
//! While hosted, LightCraft never installs fonts (`ctx.set_fonts`: the host installs one merged set
//! that includes ours, see `theme::font_definitions`), never changes the zoom factor, never sizes
//! or places the window, and draws no window chrome of its own. It also shows no ArtCraft marks or
//! promotions (a derived build may not: `docs/brand/LICENSE-brand.txt`; see `links::shown`).
//!
//! A host may also keep LightCraft's per-user files in a folder of its own ([`set_data_root`]), and
//! set the theme and accent colour for all the apps it shows ([`set_theme`], [`set_accent`]).

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::theme::ThemeKind;

static HOSTED: AtomicBool = AtomicBool::new(false);

/// Mark LightCraft as running inside a host (process-wide).
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// LightCraft runs inside a host application.
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

thread_local! {
    static THEME: Cell<Option<ThemeKind>> = const { Cell::new(None) };
    static ACCENT: Cell<Option<egui::Color32>> = const { Cell::new(None) };
    static RESTYLE: Cell<bool> = const { Cell::new(false) };
    static OPEN_APPEARANCE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// The host's theme, shown instead of the one Settings ▸ Interface ▸ Appearance selects and never
/// saved as it; `None` (the default) shows LightCraft's own.
pub fn set_theme(theme: Option<ThemeKind>) {
    THEME.set(theme);
}

/// The host's theme ([`set_theme`]), if it sets one.
pub fn theme() -> Option<ThemeKind> {
    THEME.get()
}

/// The host's appearance settings, which the appearance commands and Settings open while the host
/// sets the theme.
pub fn set_open_appearance(open: Box<dyn FnMut()>) {
    OPEN_APPEARANCE.set(Some(open));
}

/// Open the host's appearance settings ([`set_open_appearance`]) → whether there are any.
pub fn open_appearance() -> bool {
    OPEN_APPEARANCE.with_borrow_mut(|open| open.as_mut().map(|open| open()).is_some())
}

/// The host's accent colour, shown instead of the theme's (selections, focus, active controls) and
/// never saved as it; `None` (the default) keeps the theme's.
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
        t.accent = recolor(t.accent, t.accent, to);
    }
    t
}

/// Keep everything LightCraft writes or reads as per-user state (settings, camera profiles, the
/// SAM 3 model, the default library and export folder) under `root` instead of the per-user OS
/// folders — a host's portable install. `None`: the OS folders (the default). Set it before
/// LightCraft is built; see [`lightcraft_engine::paths`].
pub fn set_data_root(root: Option<std::path::PathBuf>) {
    lightcraft_engine::paths::set_data_root(root);
}

/// The folder set by [`set_data_root`], if any.
pub fn data_root() -> Option<std::path::PathBuf> {
    lightcraft_engine::paths::data_root()
}
