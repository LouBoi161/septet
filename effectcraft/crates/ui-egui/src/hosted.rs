//! Running as a tab of a host window (Septet) instead of owning the window: the host installs
//! the fonts, owns the UI zoom and the window's size and chrome, so the app leaves them alone.
//! Off (the standalone app) unless the embedding host turns it on before building the app.
//!
//! Hosted, EffectCraft also leaves out its community Discord and website links
//! ([`hides_command`]): nothing in a host suggests the ArtCraft team made or endorses it
//! (`docs/brand/LICENSE-brand.txt`). The host may also set the theme for all the apps it shows
//! ([`set_theme`]).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static HOSTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static THEME: Cell<Option<crate::theme::ThemeKind>> = const { Cell::new(None) };
    static OPEN_APPEARANCE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// The host's theme, shown instead of Settings ▸ Appearance and never saved as it; `None` (the
/// default) shows EffectCraft's own.
pub fn set_theme(theme: Option<crate::theme::ThemeKind>) {
    THEME.set(theme);
}

/// The host's theme ([`set_theme`]), if it sets one.
pub fn theme() -> Option<crate::theme::ThemeKind> {
    THEME.get()
}

/// The host's appearance settings, which the theme commands open while the host sets the theme.
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
        t.accent_hover = recolor(t.accent_hover, from, to);
        t.focus = recolor(t.focus, from, to);
        t.row_selected = recolor(t.row_selected, from, to);
    }
    t
}

/// Mark the process as hosting EffectCraft in another app's window (`true`) or not.
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// Whether EffectCraft runs inside a host's window (see [`set_hosted`]).
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// Keep everything EffectCraft stores per user under `root` (a portable install) instead of the
/// platform's folders; `None` = the platform's. Set it before building the app.
pub fn set_data_root(root: Option<PathBuf>) {
    effectcraft_engine::config::set_data_root(root);
}

/// The portable data root, when one is set.
pub fn data_root() -> Option<PathBuf> {
    effectcraft_engine::config::data_root()
}

/// Commands whose menu entries, buttons and links a hosted build leaves out: the community
/// Discord and the getartcraft.com pages (the website, EffectCraft's page there and its online
/// tutorials). EffectCraft's GitHub, documentation and feedback links stay.
pub fn hides_command(id: &str) -> bool {
    is_hosted() && matches!(id, "help.discord" | "help.website" | "help.appPage" | "help.onlineTutorials")
}
