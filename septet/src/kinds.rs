//! The seven apps Septet hosts: names, brand colours, icons and the files each one opens.

use std::path::Path;

use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum AppKind {
    Photocraft,
    Vectorcraft,
    Lightcraft,
    Designcraft,
    Pdfcraft,
    Filmcraft,
    Effectcraft,
}

impl AppKind {
    pub const ALL: [AppKind; 7] =
        [AppKind::Photocraft, AppKind::Vectorcraft, AppKind::Lightcraft, AppKind::Designcraft, AppKind::Pdfcraft, AppKind::Filmcraft, AppKind::Effectcraft];

    pub fn name(self) -> &'static str {
        match self {
            AppKind::Photocraft => "Photocraft",
            AppKind::Vectorcraft => "Vectorcraft",
            AppKind::Lightcraft => "Lightcraft",
            AppKind::Designcraft => "Designcraft",
            AppKind::Pdfcraft => "Pdfcraft",
            AppKind::Filmcraft => "Filmcraft",
            AppKind::Effectcraft => "Effectcraft",
        }
    }

    /// Two-letter badge, the way the suite's apps are told apart at a glance.
    pub fn badge(self) -> &'static str {
        match self {
            AppKind::Photocraft => "Ph",
            AppKind::Vectorcraft => "Vc",
            AppKind::Lightcraft => "Lc",
            AppKind::Designcraft => "Dc",
            AppKind::Pdfcraft => "Pd",
            AppKind::Filmcraft => "Fc",
            AppKind::Effectcraft => "Ec",
        }
    }

    pub fn tagline(self) -> &'static str {
        match self {
            AppKind::Photocraft => "Photos, compositing and painting",
            AppKind::Vectorcraft => "Vector graphics and illustration",
            AppKind::Lightcraft => "Raw photo library and development",
            AppKind::Designcraft => "Page layout and publishing",
            AppKind::Pdfcraft => "Read, edit, sign and organize PDFs",
            AppKind::Filmcraft => "Video editing",
            AppKind::Effectcraft => "Motion graphics and visual effects",
        }
    }

    /// The brand colour of the app icon (`<app>/assets/app-icon/<app>-small.svg`).
    pub fn color(self) -> Color32 {
        match self {
            AppKind::Photocraft => Color32::from_rgb(0x2f, 0x7b, 0xf5),
            AppKind::Vectorcraft => Color32::from_rgb(0xe8, 0x57, 0x3f),
            AppKind::Lightcraft => Color32::from_rgb(0xf2, 0xa5, 0x16),
            AppKind::Designcraft => Color32::from_rgb(0x7b, 0xb5, 0x1c),
            AppKind::Pdfcraft => Color32::from_rgb(0x12, 0xa5, 0x8a),
            AppKind::Filmcraft => Color32::from_rgb(0x8b, 0x5c, 0xf6),
            AppKind::Effectcraft => Color32::from_rgb(0xe0, 0x36, 0x8f),
        }
    }

    pub fn icon(self) -> egui::ImageSource<'static> {
        match self {
            AppKind::Photocraft => egui::include_image!("../../photocraft/assets/app-icon/photocraft-small.svg"),
            AppKind::Vectorcraft => egui::include_image!("../../vectorcraft/assets/app-icon/vectorcraft-small.svg"),
            AppKind::Lightcraft => egui::include_image!("../../lightcraft/assets/app-icon/lightcraft-small.svg"),
            AppKind::Designcraft => egui::include_image!("../../designcraft/assets/app-icon/designcraft-small.svg"),
            AppKind::Pdfcraft => egui::include_image!("../../pdfcraft/assets/app-icon/pdfcraft-small.svg"),
            AppKind::Filmcraft => egui::include_image!("../../filmcraft/assets/app-icon/filmcraft-small.svg"),
            AppKind::Effectcraft => egui::include_image!("../../effectcraft/assets/app-icon/effectcraft-small.svg"),
        }
    }

    /// Lower-case file extensions this app opens as documents (the first app that lists an
    /// extension in [`AppKind::ALL`] order is its default; see [`AppKind::for_path`]).
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            AppKind::Photocraft => &[
                "pcraft", "psd", "psb", "png", "jpg", "jpeg", "webp", "gif", "bmp", "tga", "ico", "pnm", "pbm", "pgm", "ppm", "exr", "hdr", "avif", "qoi",
                "tif", "tiff", "heic", "heif", "jxl",
            ],
            AppKind::Vectorcraft => &["vectorcraft", "drawcraft", "svg", "svgz", "ai", "eps", "epsf", "epsi", "emf", "wmf", "dxf", "cdr"],
            AppKind::Lightcraft => &[
                "dng", "arw", "srf", "sr2", "cr2", "cr3", "crw", "nef", "nrw", "raf", "orf", "rw2", "rwl", "pef", "srw", "x3f", "3fr", "fff", "iiq", "mos",
                "mef", "mrw", "erf", "kdc", "dcr", "lrcat", "lccat",
            ],
            AppKind::Designcraft => &["designcraft", "dcraft", "idml", "indd", "epub"],
            AppKind::Pdfcraft => &["pdf", "xfdf", "fdf"],
            AppKind::Filmcraft => &[
                "filmcraft",
                "fcproj",
                "mp4",
                "m4v",
                "mov",
                "mkv",
                "webm",
                "mxf",
                "mts",
                "m2ts",
                "ts",
                "avi",
                "mpg",
                "mpeg",
                "ogv",
                "wav",
                "mp3",
                "aif",
                "aiff",
                "flac",
                "m4a",
                "aac",
                "ogg",
                "opus",
                "srt",
                "vtt",
                "edl",
                "fcpxml",
                "otio",
            ],
            AppKind::Effectcraft => &["effectcraft", "ecproj", "ectemplate", "lottie", "aep"],
        }
    }

    /// The app that opens `path` by default, by its extension.
    pub fn for_path(path: &Path) -> Option<AppKind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        AppKind::ALL.into_iter().find(|k| k.extensions().contains(&ext.as_str()))
    }

    /// Every app that can open `path` (the default first), for "Open in…" menus.
    pub fn all_for_path(path: &Path) -> Vec<AppKind> {
        let Some(ext) = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase) else {
            return Vec::new();
        };
        let mut v: Vec<AppKind> = AppKind::ALL.into_iter().filter(|k| k.extensions().contains(&ext.as_str())).collect();
        // Raster images are also good Lightcraft/Designcraft/Vectorcraft inputs.
        if AppKind::Photocraft.extensions().contains(&ext.as_str()) {
            for k in [AppKind::Lightcraft, AppKind::Vectorcraft, AppKind::Designcraft, AppKind::Pdfcraft] {
                if !v.contains(&k) {
                    v.push(k);
                }
            }
        }
        if ext == "pdf" {
            for k in [AppKind::Vectorcraft, AppKind::Designcraft, AppKind::Photocraft] {
                if !v.contains(&k) {
                    v.push(k);
                }
            }
        }
        if ext == "svg" || ext == "ai" || ext == "eps" {
            for k in [AppKind::Photocraft, AppKind::Designcraft, AppKind::Effectcraft] {
                if !v.contains(&k) {
                    v.push(k);
                }
            }
        }
        v
    }
}
