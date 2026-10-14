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
