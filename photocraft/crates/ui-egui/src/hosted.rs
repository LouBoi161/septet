//! Hosted mode: PhotoCraft running as a tab inside another eframe app (the Septet shell), which
//! owns the window, the fonts and the UI zoom of the shared `egui::Context`.
//!
//! While hosted the shell never calls `ctx.set_fonts` (the host installs one merged set, see
//! [`crate::theme::font_definitions`]), never sets the zoom factor, never resizes or maximizes
//! the window, and shows none of the original makers' community links and promotions
//! (`crate::links::COMMUNITY_COMMANDS`): a modified version may not present itself as theirs
//! (`docs/brand/LICENSE-brand.txt`). Standalone (the default) nothing changes.
//!
//! A host can also keep everything PhotoCraft stores per user under one folder of its own
//! ([`set_data_root`]), as PhotoCraft's portable mode does, and set the interface brightness for
//! all the apps it shows ([`set_theme`]). A host with an image generator adds Generative Fill and
//! Generate Image to the Edit menu ([`set_generate`]).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{PoisonError, RwLock};

static HOSTED: AtomicBool = AtomicBool::new(false);
static DATA_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Where this modified version of PhotoCraft comes from, in plain text (the About box): the
/// ArtCraft marks are removed from it (`docs/brand/README.md`).
pub const ATTRIBUTION: &str = "Based on PhotoCraft by the ArtCraft team (MIT OR Apache-2.0)";

thread_local! {
    static THEME: Cell<Option<crate::theme::ThemeKind>> = const { Cell::new(None) };
    static OPEN_APPEARANCE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// The host's theme, shown instead of Preferences › Interface › Appearance and never saved as it;
/// `None` (the default) shows PhotoCraft's own.
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

/// What the host's image generator is asked for ([`set_generate`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generate {
    /// Edit › Generative Fill…: fill the pixel selection.
    Fill,
    /// Edit › Generate Image…: a new image as a new layer.
    Image,
}

/// Edit › Generative Fill… and the selection tools' context menu row, while a host generates.
pub const GENERATIVE_FILL: &str = "edit.generativeFill";
/// Edit › Generate Image…, while a host generates.
pub const GENERATE_IMAGE: &str = "edit.generateImage";

/// The generation commands: id, label, kind. Photoshop lists them right after Content-Aware Fill.
pub(crate) const GENERATE_COMMANDS: [(&str, &str, Generate); 2] =
    [(GENERATIVE_FILL, "Generative Fill…", Generate::Fill), (GENERATE_IMAGE, "Generate Image…", Generate::Image)];

/// The host's image generator ([`set_generate`]).
type GenerateHandler = Box<dyn FnMut(Generate)>;

thread_local! {
    static GENERATE: RefCell<Option<GenerateHandler>> = const { RefCell::new(None) };
}

/// The host's image generator: while it is set, Edit › Generative Fill… (with a pixel
/// selection), Edit › Generate Image… (with a document) and the selection tools' Generative Fill…
/// row are shown and call `handler`. Without it (standalone) none of them exist.
pub fn set_generate(handler: Box<dyn FnMut(Generate)>) {
    GENERATE.with(|g| match g.try_borrow_mut() {
        Ok(mut slot) => *slot = Some(handler),
        // Called from inside the running handler.
        Err(_) => log::warn!("the image generator is in use; keeping the previous one"),
    });
}

/// Ask the host's image generator ([`set_generate`]) for `kind` → whether there is one.
pub fn generate(kind: Generate) -> bool {
    // A handler that asks again from inside itself is already running: nothing more to do.
    GENERATE.with(|g| g.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(|handler| handler(kind))).is_some())
}

/// Whether a host generates images ([`set_generate`]), so the generation commands exist.
pub fn generates() -> bool {
    GENERATE.with(|g| g.try_borrow().map_or(true, |g| g.is_some()))
}

/// The generation command `id` names, while a host generates ([`generates`]).
pub fn generate_command(id: &str) -> Option<Generate> {
    GENERATE_COMMANDS.iter().find(|(c, ..)| *c == id).map(|&(.., kind)| kind).filter(|_| generates())
}

#[cfg(test)]
pub(crate) fn clear_generate() {
    GENERATE.set(None);
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
        t.accent_soft = recolor(t.accent_soft, from, to);
        t.accent_border = recolor(t.accent_border, from, to);
        t.accent_text = recolor(t.accent_text, from, to);
        t.primary_bg = recolor(t.primary_bg, from, to);
        t.row_selected = recolor(t.row_selected, from, to);
    }
    if contrast() {
        t.text_dim = t.text_dim.lerp_to_gamma(t.text, 0.6);
        t.text_faint = t.text_faint.lerp_to_gamma(t.text, 0.35);
        t.field_border = t.field_border.lerp_to_gamma(t.text, 0.35);
        t.card_border = t.card_border.lerp_to_gamma(t.text, 0.35);
        t.separator = t.separator.lerp_to_gamma(t.text, 0.35);
        t.icon = t.icon.lerp_to_gamma(t.text, 0.5);
    }
    t
}

/// Switch hosted mode on (before the app is built) or off.
pub fn set_hosted(hosted: bool) {
    HOSTED.store(hosted, Ordering::Relaxed);
}

/// Whether PhotoCraft runs inside a host app (see the module docs).
pub fn is_hosted() -> bool {
    HOSTED.load(Ordering::Relaxed)
}

/// Keep the preferences, brush presets, crash recovery and every other per-user file under
/// `root` instead of the per-user OS folder (`None`: the usual folder). Set it before the app
/// is built: the desktop app resolves its settings folder once.
pub fn set_data_root(root: Option<PathBuf>) {
    *DATA_ROOT.write().unwrap_or_else(PoisonError::into_inner) = root;
}

/// The folder a host app keeps PhotoCraft's per-user files in, if it chose one ([`set_data_root`]).
pub fn data_root() -> Option<PathBuf> {
    DATA_ROOT.read().unwrap_or_else(PoisonError::into_inner).clone()
}

#[cfg(test)]
mod contrast_tests {
    use super::*;
    use crate::theme::{ThemeKind, Tokens};

    /// More contrast brings secondary text, borders and icons closer to the text colour, dark and
    /// light alike, leaves the rest as the theme has it, and restyles the app.
    #[test]
    fn more_contrast_brings_secondary_colours_towards_the_text() {
        let off = |c: egui::Color32, text: egui::Color32| {
            [(c.r(), text.r()), (c.g(), text.g()), (c.b(), text.b())].iter().map(|&(a, b)| i32::from(a).abs_diff(i32::from(b))).sum::<u32>()
        };
        for which in [ThemeKind::Pro, ThemeKind::ProLight] {
            let own = Tokens::for_kind(which);
            take_restyle();
            set_contrast(true);
            assert!(take_restyle(), "switching restyles");
            let t = with_accent(own);
            set_contrast(false);
            for (name, now, was) in [
                ("text_dim", t.text_dim, own.text_dim),
                ("text_faint", t.text_faint, own.text_faint),
                ("field_border", t.field_border, own.field_border),
                ("card_border", t.card_border, own.card_border),
                ("separator", t.separator, own.separator),
                ("icon", t.icon, own.icon),
            ] {
                assert!(off(now, own.text) < off(was, own.text) || was == own.text, "{name} is closer to the text in {which:?}");
            }
            assert_eq!((t.text, t.accent), (own.text, own.accent));
            assert_eq!(with_accent(own).text_dim, own.text_dim, "without it: the theme's own");
        }
    }
}
