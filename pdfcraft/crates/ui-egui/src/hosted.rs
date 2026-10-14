//! Whether PdfCraft runs as a tab inside a host window (Septet) instead of in its own window.
//!
//! A host owns the window: its chrome, the fonts (one merged set for every app it shows), the
//! UI zoom and egui's input options. While hosted, PdfCraft leaves those alone: it never calls
//! `ctx.set_fonts` with fonts of its own (the host installed
//! [`crate::theme::installed_font_definitions`] among its own; the text editor only adds a
//! document's fonts to the host's set), never sets the zoom factor (Preferences ▸ Interface size
//! is hidden) or `line_scroll_speed` (the Scroll speed preference scales PdfCraft's own wheel
//! input instead, [`crate::scroll_speed`]), and its tab strip doesn't move or maximize the
//! window.
//!
//! In a host build PdfCraft doesn't invite people to the original authors' community (their
//! Discord, their website) ([`shows_community_links`]); PdfCraft's own page and source repository
//! stay. (The ArtCraft marks are gone from this modified tree altogether: `docs/brand/README.md`.)
//! And a host may keep everything PdfCraft stores per user in a folder of its own
//! ([`set_data_root`]), and set light or dark for all the apps it shows ([`set_theme`]).
//! Standalone, nothing changes.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

static HOSTED: AtomicBool = AtomicBool::new(false);

static DATA_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

thread_local! {
    static THEME: Cell<Option<crate::theme::ThemeKind>> = const { Cell::new(None) };
    static OPEN_APPEARANCE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// The host's theme, shown instead of Preferences ▸ Appearance mode and never saved as it;
/// `None` (the default) shows PdfCraft's own.
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
        t.accent_text = recolor(t.accent_text, from, to);
        t.accent_soft = recolor(t.accent_soft, from, to);
        t.selected = recolor(t.selected, from, to);
    }
    t
}

/// Mark the app as running inside a host window (before the app is built).
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// `true` when a host window owns the window, its fonts and its zoom.
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// Keep everything PdfCraft stores per user (crash recovery, the digital IDs it creates, the
/// copied images it hands the clipboard) under `root` instead of the user's data folders, which
/// is created if missing (a portable install); `None` goes back to those folders. Set it before the app is built. PdfCraft's own portable
/// mode ([`crate::portable`]) is off in a host window and whenever a data root is set.
pub fn set_data_root(root: Option<PathBuf>) {
    if let Some(dir) = &root
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        log::warn!("data folder {}: {e}", dir.display());
    }
    match DATA_ROOT.write() {
        Ok(mut r) => *r = root,
        Err(poisoned) => *poisoned.into_inner() = root,
    }
}

/// The folder set by [`set_data_root`], if any.
pub fn data_root() -> Option<PathBuf> {
    match DATA_ROOT.read() {
        Ok(r) => r.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Where PdfCraft keeps its per-user folder `name` ("Recovery", "Digital IDs", "Clipboard"): under
/// the data root when one is set, else where `default` says (the user's data folders). Every per-user
/// folder is looked up through here.
pub fn user_dir(name: &str, default: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    match data_root() {
        Some(root) => Some(root.join(name)),
        None => default(),
    }
}

/// Whether the community links (Discord, the original authors' website) are shown: not in a host
/// build.
pub fn shows_community_links() -> bool {
    !is_hosted()
}

/// The Help commands that open those community links, left out of the menus and the command
/// palette when [`shows_community_links`] is off.
pub fn is_community_command(id: &str) -> bool {
    matches!(id, "help.discord" | "help.website")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ThemeKind, ThemePreference};

    /// A host's accent takes the place of the theme's blues: same hue as the accent, each token as
    /// much lighter or paler as it was than the theme's accent; greys and white stay.
    #[test]
    fn the_host_accent_recolours_the_theme_blues_only() {
        use egui::Color32;
        let pink = Color32::from_rgb(0xdb, 0x27, 0x77);
        let light = crate::theme::Tokens::for_kind(ThemeKind::Light);
        set_accent(Some(pink));
        let t = with_accent(light);
        set_accent(None);
        assert_eq!(t.accent, pink);
        let hue = |c: Color32| egui::ecolor::HsvaGamma::from(c).h;
        for (name, c, was) in [
            ("accent_text", t.accent_text, light.accent_text),
            ("accent_soft", t.accent_soft, light.accent_soft),
            ("selected", t.selected, light.selected),
        ] {
            assert!((hue(c) - hue(pink)).abs() < 0.02, "{name} has the accent's hue");
            let v = |c: Color32| egui::ecolor::HsvaGamma::from(c).v;
            assert!((v(c) > v(pink)) == (v(was) > v(light.accent)), "{name} keeps its brightness relative to the accent");
        }
        assert_eq!(recolor(Color32::WHITE, light.accent, pink), Color32::WHITE);
        assert_eq!(recolor(Color32::from_gray(90), light.accent, pink), Color32::from_gray(90));
        assert_eq!(with_accent(light).accent, light.accent, "no host accent: the theme's own");
    }

    /// While a host (Septet) sets the theme, its theme shows and the theme commands open the host's
    /// settings without touching PdfCraft's own preference.
    #[test]
    fn a_host_theme_shows_and_theme_commands_open_its_settings() {
        let ctx = egui::Context::default();
        let mut app = crate::PdfCraftApp::new();
        app.sync_theme(&ctx);
        let own = app.theme;
        let opened = std::rc::Rc::new(Cell::new(0));
        let count = opened.clone();
        set_open_appearance(Box::new(move || count.set(count.get() + 1)));
        set_theme(Some(ThemeKind::Dark));
        app.sync_theme(&ctx);
        assert_eq!(app.theme, ThemeKind::Dark);
        app.execute("view.theme.light");
        app.execute("view.theme");
        app.sync_theme(&ctx);
        assert_eq!(opened.get(), 2);
        assert_eq!(app.theme, ThemeKind::Dark);
        assert_eq!(app.theme_preference, ThemePreference::Light);
        set_theme(None);
        app.sync_theme(&ctx);
        assert_eq!(app.theme, own);
    }
}
