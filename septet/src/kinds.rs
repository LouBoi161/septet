//! The apps Septet hosts, as one table ([`APPS`]): names, brand colours, icons, the files each one
//! opens and takes, and its entry points. Adding an app is one [`AppKind`] variant and one entry
//! here; the shell, the clipboard and content bridges and the assistant all read from the table.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use egui::{Color32, Context, FontDefinitions};

use crate::hosted::HostedApp;

/// An app of the suite. Saved sessions and recent files name apps by these variants, so a kind
/// stays even when its app isn't built in or the user uninstalled it (`AppKind::installed`); one
/// this build has never heard of (a newer build's) reads as [`AppKind::Unknown`]. All of them are
/// skipped where sessions are restored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum AppKind {
    Photocraft,
    Vectorcraft,
    Lightcraft,
    Designcraft,
    Pdfcraft,
    Filmcraft,
    Effectcraft,
    Wordcraft,
    Deckcraft,
    #[serde(other)]
    Unknown,
}

/// How an app takes a paste from another app's clipboard (`bridge.rs`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // in builds without the apps that use a variant
pub enum Paste {
    /// It reads everything it understands itself: the shell leaves pastes alone.
    Itself,
    /// It reads images and copied files itself; SVG arrives as a PNG.
    ItselfImages,
    /// SVG stays SVG; images and files arrive as files to place.
    Svg,
    /// Everything arrives as a file to place, SVG rasterized to PNG.
    Files,
}

/// The calls into an app's `Embedded` (each app's `apps/<app>/src/embed.rs`).
pub struct EntryPoints {
    pub create: fn(&Context, Option<&eframe::egui_wgpu::RenderState>, Option<&dyn eframe::Storage>) -> Box<dyn HostedApp>,
    /// Portable mode: keep the app's settings and data under this folder (call before `create`).
    pub set_data_root: fn(Option<PathBuf>),
    /// The fonts the app would install in a context of its own (`fonts.rs` merges them).
    pub font_definitions: fn() -> FontDefinitions,
}

pub struct AppSpec {
    pub kind: AppKind,
    pub name: &'static str,
    /// Two-letter badge, the way the suite's apps are told apart at a glance.
    pub badge: &'static str,
    pub tagline: &'static str,
    /// What the app is for, in the assistant's system prompt.
    pub role: &'static str,
    /// The brand colour of the app icon (`<app>/assets/app-icon/<app>-small.svg`).
    pub color: Color32,
    pub icon: egui::ImageSource<'static>,
    /// Lower-case file extensions it opens as documents (the first app in [`APPS`] that lists an
    /// extension is its default; see [`AppKind::for_path`]).
    pub extensions: &'static [&'static str],
    /// Other apps' files it opens too (lists of extensions), offered after theirs in "Open in…" menus.
    pub also_opens: &'static [&'static [&'static str]],
    /// Formats it places well, best first, when content is dragged or sent to it (`content.rs`).
    pub accepts: &'static [&'static str],
    pub paste: Paste,
    /// It asks about unsaved work itself when its window closes; otherwise the shell asks.
    pub own_close_prompt: bool,
    /// Files dropped from the file manager are placed where they landed (winit drops carry no
    /// position, so the shell reads the pointer and hands it over); otherwise the app handles drops.
    pub drops_at_pointer: bool,
    /// What `app_inspect` reads in this app (`what` values), for the tool description.
    pub inspect: &'static str,
    /// What `app_render` draws in this app (`target` values), for the tool description.
    pub render: &'static str,
    /// What Claude should know before using the app, or "".
    pub limits: &'static str,
    pub entry: EntryPoints,
}

/// The raster formats Photocraft opens; Lightcraft, Vectorcraft, Designcraft and Pdfcraft take them too.
const RASTER: &[&str] = &[
    "pcraft", "psd", "psb", "png", "jpg", "jpeg", "webp", "gif", "bmp", "tga", "ico", "pnm", "pbm", "pgm", "ppm", "exr", "hdr", "avif", "qoi", "tif", "tiff",
    "heic", "heif", "jxl",
];

/// Every app built into this binary (cargo features, one per app), in Home's order.
pub static APPS: &[AppSpec] = &[
    #[cfg(feature = "photocraft")]
    AppSpec {
        kind: AppKind::Photocraft,
        name: "Photocraft",
        badge: "Ph",
        tagline: "Photos, compositing and painting",
        role: "photo editing",
        color: Color32::from_rgb(0x2f, 0x7b, 0xf5),
        icon: egui::include_image!("../../photocraft/assets/app-icon/photocraft-small.svg"),
        extensions: RASTER,
        also_opens: &[&["pdf", "svg", "ai", "eps"]],
        accepts: &["psd", "tif", "tiff", "png", "jpg", "jpeg", "webp"],
        paste: Paste::ItselfImages,
        own_close_prompt: true,
        drops_at_pointer: true,
        inspect: "document (layer tree), layer (`id`), selection, history, documents",
        render: "document, layer (`id`, alone), selection",
        limits: "Photocraft opens no SVG or PDF",
        entry: crate::hosted::entry_points!(photocraft_embed::Embedded),
    },
    #[cfg(feature = "vectorcraft")]
    AppSpec {
        kind: AppKind::Vectorcraft,
        name: "Vectorcraft",
        badge: "Vc",
        tagline: "Vector graphics and illustration",
        role: "vector illustration",
        color: Color32::from_rgb(0xe8, 0x57, 0x3f),
        icon: egui::include_image!("../../vectorcraft/assets/app-icon/vectorcraft-small.svg"),
        extensions: &["vectorcraft", "drawcraft", "svg", "svgz", "ai", "eps", "epsf", "epsi", "emf", "wmf", "dxf", "cdr"],
        also_opens: &[RASTER, &["pdf"]],
        accepts: &["svg", "pdf", "ai", "eps", "png", "jpg", "jpeg"],
        paste: Paste::Itself,
        own_close_prompt: true,
        drops_at_pointer: false,
        inspect: "document (layer tree; `depth`), object (`id`), selection, find (params name/kind/text), history, documents",
        render: "document (artboard in view, or `page`), object or layer (`id`, cut out alone), selection",
        limits: "Vectorcraft drops most SVG filters and uses installed fonts only, so outline text you want exact",
        entry: crate::hosted::entry_points!(vectorcraft_embed::Embedded),
    },
    #[cfg(feature = "lightcraft")]
    AppSpec {
        kind: AppKind::Lightcraft,
        name: "Lightcraft",
        badge: "Lc",
        tagline: "Raw photo library and development",
        role: "raw photo library",
        color: Color32::from_rgb(0xf2, 0xa5, 0x16),
        icon: egui::include_image!("../../lightcraft/assets/app-icon/lightcraft-small.svg"),
        extensions: &[
            "dng", "arw", "srf", "sr2", "cr2", "cr3", "crw", "nef", "nrw", "raf", "orf", "rw2", "rwl", "pef", "srw", "x3f", "3fr", "fff", "iiq", "mos", "mef",
            "mrw", "erf", "kdc", "dcr", "lrcat", "lccat",
        ],
        also_opens: &[RASTER],
        accepts: &["dng", "tif", "tiff", "jpg", "jpeg", "png", "webp"],
        paste: Paste::Itself,
        own_close_prompt: true,
        drops_at_pointer: false,
        inspect: "document, photos (params filter/limit), photo, develop (with masks), controls (slider ids), albums, history",
        render: "photo (`id`), before (unedited), mask (`mask` id, `view`)",
        limits: "",
        entry: crate::hosted::entry_points!(lightcraft_embed::Embedded),
    },
    #[cfg(feature = "designcraft")]
    AppSpec {
        kind: AppKind::Designcraft,
        name: "Designcraft",
        badge: "Dc",
        tagline: "Page layout and publishing",
        role: "page layout",
        color: Color32::from_rgb(0x7b, 0xb5, 0x1c),
        icon: egui::include_image!("../../designcraft/assets/app-icon/designcraft-small.svg"),
        extensions: &["designcraft", "dcraft", "idml", "indd", "epub"],
        also_opens: &[RASTER, &["pdf", "svg", "ai", "eps"]],
        accepts: &["pdf", "svg", "psd", "tif", "tiff", "png", "jpg", "jpeg", "txt"],
        paste: Paste::Svg,
        own_close_prompt: false,
        drops_at_pointer: true,
        inspect: "document (pages; params.page), page, object (`id`), story (`id`: its text), selection, history, documents",
        render: "page (`page`), object (`id`), selection, layer (`id`)",
        limits: "Designcraft opens only .designcraft and .idml documents (make a new layout with app_execute `file.new`)",
        entry: crate::hosted::entry_points!(designcraft_embed::Embedded),
    },
    #[cfg(feature = "pdfcraft")]
    AppSpec {
        kind: AppKind::Pdfcraft,
        name: "Pdfcraft",
        badge: "Pd",
        tagline: "Read, edit, sign and organize PDFs",
        role: "PDF editing",
        color: Color32::from_rgb(0x12, 0xa5, 0x8a),
        icon: egui::include_image!("../../pdfcraft/assets/app-icon/pdfcraft-small.svg"),
        extensions: &["pdf", "xfdf", "fdf"],
        also_opens: &[RASTER],
        accepts: &["pdf", "png", "jpg", "jpeg"],
        paste: Paste::Files,
        own_close_prompt: true,
        drops_at_pointer: false,
        inspect: "document, documents, page (params.page: its text), comments, fields, bookmarks, links, history",
        render: "page (`page`)",
        limits: "",
        entry: crate::hosted::PDFCRAFT,
    },
    #[cfg(feature = "filmcraft")]
    AppSpec {
        kind: AppKind::Filmcraft,
        name: "Filmcraft",
        badge: "Fc",
        tagline: "Video editing",
        role: "video editing",
        color: Color32::from_rgb(0x8b, 0x5c, 0xf6),
        icon: egui::include_image!("../../filmcraft/assets/app-icon/filmcraft-small.svg"),
        extensions: &[
            "filmcraft", "fcproj", "mp4", "m4v", "mov", "mkv", "webm", "mxf", "mts", "m2ts", "ts", "avi", "mpg", "mpeg", "ogv", "wav", "mp3", "aif", "aiff",
            "flac", "m4a", "aac", "ogg", "opus", "srt", "vtt", "edl", "fcpxml", "otio",
        ],
        also_opens: &[],
        accepts: &["mp4", "mov", "mkv", "webm", "wav", "mp3", "m4a", "png", "jpg", "jpeg"],
        paste: Paste::Files,
        own_close_prompt: false,
        drops_at_pointer: true,
        inspect: "document (bins), sequence, clip (`id`), selection, history",
        render: "frame (`time`, else the playhead), clip (`id`), selection, item (`id`)",
        limits: "in Filmcraft, opening or placing media only adds it to the bin: put clips on the timeline with app_execute (`timeline.place`), or build whole edits as OTIO/FCPXML/EDL",
        entry: crate::hosted::entry_points!(filmcraft_embed::Embedded),
    },
    #[cfg(feature = "effectcraft")]
    AppSpec {
        kind: AppKind::Effectcraft,
        name: "Effectcraft",
        badge: "Ec",
        tagline: "Motion graphics and visual effects",
        role: "motion graphics",
        color: Color32::from_rgb(0xe0, 0x36, 0x8f),
        icon: egui::include_image!("../../effectcraft/assets/app-icon/effectcraft-small.svg"),
        extensions: &["effectcraft", "ecproj", "ectemplate", "lottie", "aep"],
        also_opens: &[&["svg", "ai", "eps"]],
        accepts: &["mp4", "mov", "mkv", "webm", "wav", "mp3", "svg", "psd", "pdf", "png", "jpg", "jpeg"],
        paste: Paste::Svg,
        own_close_prompt: true,
        drops_at_pointer: true,
        inspect: "document, comp, layer (`id`: property tree), property (params layer/path), selection, history",
        render: "frame (`comp`, `time`), layer (`id`, alone), selection",
        limits: "Effectcraft imports Lottie only with app_execute `file.importLottie`, not through septet_open",
        entry: crate::hosted::entry_points!(effectcraft_embed::Embedded),
    },
    #[cfg(feature = "wordcraft")]
    AppSpec {
        kind: AppKind::Wordcraft,
        name: "Wordcraft",
        badge: "Wc",
        tagline: "Word processing and documents",
        role: "word processing",
        color: Color32::from_rgb(0x3b, 0x5b, 0xdb),
        icon: egui::include_image!("../../wordcraft/assets/app-icon/wordcraft.svg"),
        extensions: &["docx", "docm", "dotx", "dotm", "doc", "dot", "rtf", "odt", "md", "markdown", "txt", "html", "htm", "tex", "latex", "ltx"],
        also_opens: &[],
        accepts: &["docx", "rtf", "odt", "html", "md", "txt", "png", "jpg", "jpeg", "gif", "webp"],
        paste: Paste::Itself,
        own_close_prompt: true,
        drops_at_pointer: false,
        inspect: "document (paragraphs and tables with styles, runs and text; `text: false` leaves it out), text (`story`), paragraph (`path`), selection, pages, history",
        render: "page (`page`, 1-based)",
        limits: "Wordcraft shows one document at a time: septet_open replaces it (asking to save first); make a new one with \
                 app_execute `file.new`. Commands act at the caret or selection: select text first (`select.text`, `caret.*`)",
        entry: crate::hosted::entry_points!(wordcraft_embed::Embedded),
    },
    #[cfg(feature = "deckcraft")]
    AppSpec {
        kind: AppKind::Deckcraft,
        name: "Deckcraft",
        badge: "Dk",
        tagline: "Presentations and slide shows",
        role: "presentations and slide shows",
        color: Color32::from_rgb(0xf2, 0x6b, 0x1d),
        icon: egui::include_image!("../../deckcraft/assets/app-icon/hicolor/scalable/apps/ai.storyteller.deckcraft.svg"),
        extensions: &["deckcraft", "pptx", "potx", "ppsx"],
        also_opens: &[],
        accepts: &["svg", "png", "jpg", "jpeg", "webp", "gif"],
        paste: Paste::Itself,
        own_close_prompt: true,
        drops_at_pointer: false,
        inspect: "document (slides with titles, layouts, shape counts; selection; size; theme), slide (`index`), shape (`id`), text (`id`), \
                  selection, animations (`slide`), comments, history, documents, show",
        render: "slide (`slide`, 0-based; the current one by default), deck (all slides in a grid, up to 24 from `from`)",
        limits: "Deckcraft keeps several presentations open and shows one (app_inspect `documents`): septet_open opens another, \
                 `file.new` makes one. Slide indices are 0-based in every command. A full-screen slide show (`show.start`) opens a \
                 window of its own; `show.end` ends it",
        entry: crate::hosted::entry_points!(deckcraft_embed::Embedded),
    },
];

/// The apps the user uninstalled (Home › Apps), one bit per [`AppKind`]. Every app stays built in:
/// uninstalling takes it out of Home, the + menu, "Open in…", file types and Claude's tools, and
/// installing it again brings it back with its settings (`manage.rs`).
#[cfg(not(test))]
static UNINSTALLED: AtomicU32 = AtomicU32::new(0);
// Per test thread, so tests that uninstall an app don't change the lists under the others.
#[cfg(test)]
thread_local! {
    static UNINSTALLED: AtomicU32 = const { AtomicU32::new(0) };
}

fn uninstalled_mask() -> u32 {
    #[cfg(not(test))]
    return UNINSTALLED.load(Ordering::Relaxed);
    #[cfg(test)]
    UNINSTALLED.with(|m| m.load(Ordering::Relaxed))
}

/// Set the uninstalled apps (from the saved settings, or the Apps page).
pub fn set_uninstalled(kinds: &[AppKind]) {
    let mask = kinds.iter().fold(0, |m, k| m | k.bit());
    #[cfg(not(test))]
    UNINSTALLED.store(mask, Ordering::Relaxed);
    #[cfg(test)]
    UNINSTALLED.with(|m| m.store(mask, Ordering::Relaxed));
}

/// The apps built in but uninstalled, for saving.
pub fn uninstalled() -> Vec<AppKind> {
    AppKind::built().filter(|k| !k.installed()).collect()
}

/// The entries of the installed apps, in Home's order.
pub fn installed_specs() -> impl Iterator<Item = &'static AppSpec> {
    APPS.iter().filter(|a| a.kind.installed())
}

impl AppKind {
    fn bit(self) -> u32 {
        1 << (self as u32)
    }

    /// The installed apps, in Home's order.
    pub fn all() -> impl Iterator<Item = AppKind> {
        installed_specs().map(|a| a.kind)
    }

    /// Every app built into this Septet, installed or not.
    pub fn built() -> impl Iterator<Item = AppKind> {
        APPS.iter().map(|a| a.kind)
    }

    /// Whether this build has the app (sessions and recent files may name one it hasn't).
    pub fn is_built(self) -> bool {
        APPS.iter().any(|a| a.kind == self)
    }

    /// Whether the app is built in and not uninstalled.
    pub fn installed(self) -> bool {
        self.is_built() && uninstalled_mask() & self.bit() == 0
    }

    /// The app called `name` (any case), if it is installed.
    pub fn by_name(name: &str) -> Option<AppKind> {
        installed_specs().find(|a| a.name.eq_ignore_ascii_case(name)).map(|a| a.kind)
    }

    /// The app's entry in [`APPS`]. Only for apps built in: the places that read kinds from disk
    /// (session, recent files) drop the others.
    pub fn spec(self) -> &'static AppSpec {
        APPS.iter().find(|a| a.kind == self).unwrap_or_else(|| panic!("{self:?} is not built into this Septet"))
    }

    pub fn name(self) -> &'static str {
        self.spec().name
    }

    pub fn badge(self) -> &'static str {
        self.spec().badge
    }

    pub fn tagline(self) -> &'static str {
        self.spec().tagline
    }

    pub fn color(self) -> Color32 {
        self.spec().color
    }

    pub fn icon(self) -> egui::ImageSource<'static> {
        self.spec().icon.clone()
    }

    pub fn extensions(self) -> &'static [&'static str] {
        self.spec().extensions
    }

    /// The installed app that opens `path` by default, by its extension.
    pub fn for_path(path: &Path) -> Option<AppKind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        installed_specs().find(|a| a.extensions.contains(&ext.as_str())).map(|a| a.kind)
    }

    /// Every app that can open `path` (the default first), for "Open in…" menus.
    pub fn all_for_path(path: &Path) -> Vec<AppKind> {
        let Some(ext) = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase) else {
            return Vec::new();
        };
        let ext = ext.as_str();
        let own = installed_specs().filter(|a| a.extensions.contains(&ext));
        let also = installed_specs().filter(|a| !a.extensions.contains(&ext) && a.also_opens.iter().any(|list| list.contains(&ext)));
        own.chain(also).map(|a| a.kind).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "default")]
    fn open_in_lists_the_owner_first() {
        let png = AppKind::all_for_path(Path::new("a.PNG"));
        assert_eq!(png.first(), Some(&AppKind::Photocraft));
        assert!(png.contains(&AppKind::Lightcraft) && png.contains(&AppKind::Pdfcraft));
        assert_eq!(AppKind::all_for_path(Path::new("a.pdf"))[0], AppKind::Pdfcraft);
        assert_eq!(AppKind::for_path(Path::new("cut.otio")), Some(AppKind::Filmcraft));
        assert!(AppKind::all_for_path(Path::new("noext")).is_empty());
    }

    #[test]
    #[cfg(feature = "default")]
    fn uninstalled_apps_leave_the_lists() {
        set_uninstalled(&[AppKind::Photocraft]);
        assert!(!AppKind::Photocraft.installed() && AppKind::Photocraft.is_built());
        assert!(!AppKind::all().any(|k| k == AppKind::Photocraft));
        assert_eq!(AppKind::by_name("photocraft"), None);
        assert_eq!(AppKind::for_path(Path::new("a.psd")), None);
        assert_eq!(AppKind::all_for_path(Path::new("a.png"))[0], AppKind::Vectorcraft);
        assert_eq!(uninstalled(), [AppKind::Photocraft]);
        set_uninstalled(&[]);
        assert_eq!(AppKind::for_path(Path::new("a.psd")), Some(AppKind::Photocraft));
    }

    #[test]
    fn unknown_apps_read_as_unknown() {
        let tabs: Vec<AppKind> = ron::from_str("[Pdfcraft, Gridcraft]").unwrap();
        assert_eq!(tabs, [AppKind::Pdfcraft, AppKind::Unknown]);
        assert!(!AppKind::Unknown.installed());
    }
}
