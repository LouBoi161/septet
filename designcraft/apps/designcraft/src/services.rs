//! Desktop platform services: native file dialogs (rfd) with a file-type filter per purpose, and
//! file reads and writes through `std::fs`.

use designcraft_ui_egui::Services;

/// One file-type row in an open dialog. `open_filters` is what `pick_open` applies.
pub struct OpenFilter {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

/// The file-type rows of the open dialog for `purpose` (`open`, `place`, `swatches` …).
pub fn open_filters(purpose: &str) -> &'static [OpenFilter] {
    match purpose {
        "swatches" => &[OpenFilter { name: "Swatch Exchange (ASE)", extensions: &["ase"] }],
        "script" => &[OpenFilter { name: "Script", extensions: &["dcscript", "txt", "json"] }],
        "icc" => &[OpenFilter { name: "ICC profile", extensions: &["icc", "icm"] }],
        "book" => &[OpenFilter { name: "Book", extensions: &["dcbook"] }],
        "xml" => &[OpenFilter { name: "XML", extensions: &["xml"] }],
        "library" => &[OpenFilter { name: "Object Library", extensions: &["dclib"] }],
        "dataMerge" => &[OpenFilter { name: "Data source (CSV, TSV, text, Excel)", extensions: &["csv", "tsv", "tab", "txt", "xlsx"] }],
        // Relink (Links panel) picks any file that can be placed.
        "place" | "relink" => &[
            OpenFilter {
                name: "Graphics and text",
                extensions: &[
                    "png",
                    "jpg",
                    "jpeg",
                    "gif",
                    "webp",
                    "tif",
                    "tiff",
                    "bmp",
                    "psd",
                    "svg",
                    "pdf",
                    "ai",
                    "eps",
                    "txt",
                    "docx",
                    "rtf",
                    "md",
                    "xlsx",
                    "idml",
                    "designcraft",
                    "mp4",
                    "m4v",
                    "mov",
                    "webm",
                    "mp3",
                    "m4a",
                    "wav",
                    "ogg",
                ],
            },
            OpenFilter {
                name: "Graphics",
                extensions: &["png", "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp", "psd", "svg", "pdf", "ai", "eps"],
            },
            OpenFilter { name: "Text (Word, RTF, plain, Excel)", extensions: &["docx", "rtf", "txt", "md", "xlsx"] },
            OpenFilter { name: "Video and sound", extensions: &["mp4", "m4v", "mov", "webm", "mp3", "m4a", "wav", "ogg"] },
        ],
        _ => &[
            OpenFilter { name: "DesignCraft or IDML", extensions: &["designcraft", "idml"] },
            OpenFilter { name: "DesignCraft", extensions: &["designcraft"] },
            OpenFilter { name: "InDesign Markup (IDML)", extensions: &["idml"] },
        ],
    }
}

/// The desktop services: rfd open/save dialogs and `std::fs` reads and writes.
pub fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|purpose: &str| {
            let mut dialog = rfd::FileDialog::new();
            for filter in open_filters(purpose) {
                dialog = dialog.add_filter(filter.name, filter.extensions);
            }
            dialog.pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str| rfd::FileDialog::new().set_file_name(name).save_file().map(|p| p.to_string_lossy().to_string()))),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn data_merge_open_dialog_lists_table_extensions() {
        let filters = super::open_filters("dataMerge");
        let exts: Vec<&str> = filters.iter().flat_map(|filter| filter.extensions.iter().copied()).collect();
        for ext in ["csv", "tsv", "tab", "txt", "xlsx"] {
            assert!(exts.contains(&ext), "{ext} is missing from the dataMerge dialog: {exts:?}");
        }
        assert!(!exts.iter().any(|ext| *ext == "designcraft" || *ext == "idml"), "dataMerge must not fall through to the document filters: {exts:?}");
        let place = super::open_filters("place");
        assert!(place.len() > 1, "the place dialog keeps a filter for each kind of file");
        assert!(place.iter().any(|filter| filter.extensions.contains(&"png")));
        let documents = super::open_filters("");
        assert!(documents.iter().any(|filter| filter.extensions.contains(&"designcraft")));
    }

    #[test]
    fn relink_dialog_lists_placeable_files() {
        let relink = super::open_filters("relink");
        assert!(relink.iter().any(|filter| filter.extensions.contains(&"png")), "relink must offer graphics, not only documents");
    }
}
