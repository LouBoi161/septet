//! Whether DeckCraft runs as a tab inside a host window (Septet) instead of in its own window.
//!
//! A host owns the window: its chrome, the fonts (one merged set for every app it shows), the
//! UI zoom and dropped files. While hosted, DeckCraft leaves those alone: it never calls
//! `ctx.set_fonts` (the host installed [`crate::theme::font_definitions`] among its own), never
//! sets the zoom factor, doesn't read dropped files itself (the host places them), and its title
//! bar doesn't move or maximize the window. A full-screen slide show opens a window of its own
//! instead of making the host's window full screen. A host may also set light or dark
//! ([`set_dark`]), an accent colour ([`set_accent`]) and more contrast ([`set_contrast`]) for all
//! the apps it shows; none of them is saved as DeckCraft's own. Standalone, nothing changes.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

static HOSTED: AtomicBool = AtomicBool::new(false);

/// Mark the app as running inside a host window (before the app is built).
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// `true` when a host window owns the window, its fonts and its zoom.
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

thread_local! {
    static DARK: Cell<Option<bool>> = const { Cell::new(None) };
    static ACCENT: Cell<Option<egui::Color32>> = const { Cell::new(None) };
    static CONTRAST: Cell<bool> = const { Cell::new(false) };
    static RESTYLE: Cell<bool> = const { Cell::new(false) };
    static OPEN_APPEARANCE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// The host's theme, dark (`true`) or light, shown instead of the Dark Mode setting and never
/// saved as it; `None` (the default) shows DeckCraft's own.
pub fn set_dark(dark: Option<bool>) {
    if DARK.replace(dark) != dark {
        RESTYLE.set(true);
    }
}

/// The host's theme ([`set_dark`]), if it sets one.
pub fn dark() -> Option<bool> {
    DARK.get()
}

/// The host's accent colour, shown instead of DeckCraft's orange (tabs, selected thumbnails,
/// primary buttons) and never saved as it; `None` (the default) keeps the orange.
pub fn set_accent(accent: Option<egui::Color32>) {
    if ACCENT.replace(accent) != accent {
        RESTYLE.set(true);
    }
}

/// More contrast, as the host asks: secondary text and borders closer to the text colour; never
/// saved. `false` (the default) keeps the theme's.
pub fn set_contrast(more: bool) {
    if CONTRAST.replace(more) != more {
        RESTYLE.set(true);
    }
}

/// Whether the host's theme, accent or contrast changed since the last call: the theme is applied
/// again.
pub fn take_restyle() -> bool {
    RESTYLE.replace(false)
}

/// The host's appearance settings, which Dark Mode opens while the host sets the theme.
pub fn set_open_appearance(open: Box<dyn FnMut()>) {
    OPEN_APPEARANCE.set(Some(open));
}

/// Open the host's appearance settings ([`set_open_appearance`]) when the host sets the theme →
/// whether it did.
pub fn open_appearance() -> bool {
    dark().is_some() && OPEN_APPEARANCE.with_borrow_mut(|open| open.as_mut().map(|open| open()).is_some())
}

thread_local! {
    static AGENT_CALL: Cell<bool> = const { Cell::new(false) };
    static DIALOG_REFUSED: Cell<bool> = const { Cell::new(false) };
}

/// Run `f`, a command the host's agent asked for: the file pickers it would open stay closed
/// ([`refuse_dialog`]) → `f`'s result, and whether it asked for one.
pub fn agent_call<R>(f: impl FnOnce() -> R) -> (R, bool) {
    AGENT_CALL.set(true);
    DIALOG_REFUSED.set(false);
    let r = f();
    AGENT_CALL.set(false);
    (r, DIALOG_REFUSED.replace(false))
}

/// A command asks for a file picker: refused (and noted) during [`agent_call`].
pub fn refuse_dialog() -> bool {
    let refuse = AGENT_CALL.get();
    if refuse {
        DIALOG_REFUSED.set(true);
    }
    refuse
}

/// What asked for a file picker, to run again with the picked file: a host shows the picker
/// without blocking its window ([`current_replay`]) and then calls `SlideApp::replay_pick`.
#[derive(Clone, Debug, PartialEq)]
pub enum Replay {
    /// File › Open.
    Open,
    /// Pick a file for a command (insert a picture, audio, video; change a picture; a theme…).
    Run { purpose: String, cmd: String, extra: serde_json::Value },
    /// Reuse Slides.
    Reuse { keep: bool },
    /// Save As.
    SaveAs { format: String },
    /// Export (a copy in another format).
    Export { fmt: String, extra: serde_json::Value },
}

thread_local! {
    static REPLAY: RefCell<Option<Replay>> = const { RefCell::new(None) };
    static NO_DEFER: Cell<u32> = const { Cell::new(0) };
}

/// While alive, a picker asked for can be shown later and `replay` run again with its answer.
pub struct Replayable(Option<Replay>);

impl Replayable {
    pub fn new(replay: Replay) -> Self {
        Replayable(REPLAY.replace(Some(replay)))
    }
}

impl Drop for Replayable {
    fn drop(&mut self) {
        REPLAY.set(self.0.take());
    }
}

/// While alive, pickers answer at once (Save in the Quit dialog must finish before it closes).
pub(crate) struct NoDefer;

impl NoDefer {
    pub(crate) fn new() -> Self {
        NO_DEFER.set(NO_DEFER.get().saturating_add(1));
        NoDefer
    }
}

impl Drop for NoDefer {
    fn drop(&mut self) {
        NO_DEFER.set(NO_DEFER.get().saturating_sub(1));
    }
}

/// The action asking for a file picker now, if the host may show the picker later and run it
/// again with the answer; `None` when the picker must answer at once.
pub fn current_replay() -> Option<Replay> {
    if NO_DEFER.get() > 0 {
        return None;
    }
    REPLAY.with_borrow(Clone::clone)
}

/// `c` recoloured from the theme's accent `from` to `to`: `to`'s hue, with `c`'s saturation and
/// brightness relative to `from` and its own alpha. Greys (white text, grey fills) stay.
fn recolor(c: egui::Color32, from: egui::Color32, to: egui::Color32) -> egui::Color32 {
    use egui::ecolor::HsvaGamma;
    let (c, from, to) = (HsvaGamma::from(c), HsvaGamma::from(from), HsvaGamma::from(to));
    if c.s < 0.04 || from.s < 0.05 || from.v < 0.05 {
        return c.into();
    }
    HsvaGamma { h: to.h, s: (to.s * c.s / from.s).clamp(0.0, 1.0), v: (to.v * c.v / from.v).clamp(0.0, 1.0), a: c.a }.into()
}

/// Tokens `t` with the host's accent ([`set_accent`]) in place of the theme's, and with more
/// contrast if the host asks for it ([`set_contrast`]). Slide guides keep their colour: they mark
/// positions on the slide, not the interface.
pub(crate) fn with_accent(mut t: crate::theme::Tokens) -> crate::theme::Tokens {
    if let Some(to) = ACCENT.get() {
        let from = t.accent;
        t.accent = recolor(t.accent, from, to);
        t.contextual = recolor(t.contextual, from, to);
        // The text on the accent stays readable: white on a dark accent, dark on a light one.
        t.accent_text = if egui::Rgba::from(t.accent).intensity() > 0.5 { egui::Color32::from_rgb(0x11, 0x16, 0x2E) } else { egui::Color32::WHITE };
    }
    if CONTRAST.get() {
        t.text_dim = t.text_dim.lerp_to_gamma(t.text, 0.6);
        t.text_faint = t.text_faint.lerp_to_gamma(t.text, 0.4);
        t.border = t.border.lerp_to_gamma(t.text, 0.35);
        t.separator = t.separator.lerp_to_gamma(t.text, 0.3);
        t.card_border = t.card_border.lerp_to_gamma(t.text, 0.3);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Tokens;

    /// A host's accent takes the place of the theme's orange (same hue), greys stay, and without
    /// one the theme's own shows.
    #[test]
    fn the_host_accent_recolours_the_theme_orange_only() {
        let blue = egui::Color32::from_rgb(0x27, 0x5b, 0xdb);
        let light = Tokens::light();
        set_accent(Some(blue));
        let t = with_accent(light);
        set_accent(None);
        assert_eq!(t.accent, blue);
        let hue = |c: egui::Color32| egui::ecolor::HsvaGamma::from(c).h;
        assert!((hue(t.contextual) - hue(blue)).abs() < 0.02);
        assert_eq!(t.text, light.text);
        assert_eq!(t.guide, light.guide);
        assert_eq!(recolor(egui::Color32::from_gray(90), light.accent, blue), egui::Color32::from_gray(90));
        assert_eq!(with_accent(light).accent, light.accent);
    }

    /// More contrast brings secondary text and borders closer to the text colour, and restyles.
    #[test]
    fn more_contrast_brings_secondary_colours_towards_the_text() {
        let off = |c: egui::Color32, text: egui::Color32| {
            [(c.r(), text.r()), (c.g(), text.g()), (c.b(), text.b())].iter().map(|&(a, b)| u32::from(a.abs_diff(b))).sum::<u32>()
        };
        for own in [Tokens::light(), Tokens::dark()] {
            take_restyle();
            set_contrast(true);
            assert!(take_restyle());
            let t = with_accent(own);
            set_contrast(false);
            assert!(off(t.text_dim, own.text) < off(own.text_dim, own.text));
            assert!(off(t.border, own.text) < off(own.border, own.text));
            assert_eq!(t.text, own.text);
        }
    }
}
