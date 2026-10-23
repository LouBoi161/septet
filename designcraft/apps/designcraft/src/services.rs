//! Desktop platform services: native file dialogs (rfd) with a file-type filter per purpose (and
//! the suggested extension added to a saved name typed without it), file
//! reads through `std::fs`, atomic writes, and the clipboard's text.

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
        "shortcuts" => &[OpenFilter { name: "Shortcut set (text)", extensions: &["txt"] }],
        "icc" => &[OpenFilter { name: "ICC profile", extensions: &["icc", "icm"] }],
        "book" => &[OpenFilter { name: "Book", extensions: &["dcbook"] }],
        "xml" => &[OpenFilter { name: "XML", extensions: &["xml"] }],
        "library" => &[OpenFilter { name: "Object Library", extensions: &["dclib"] }],
        "relink" => &[OpenFilter {
            name: "Graphics",
            extensions: &["png", "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp", "psd", "svg", "pdf", "ai", "eps"],
        }],
        "dataMerge" => &[OpenFilter { name: "Data source (CSV, TSV, text, Excel)", extensions: &["csv", "tsv", "tab", "txt", "xlsx"] }],
        "place" => &[
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
            OpenFilter { name: "DesignCraft, IDML or InDesign", extensions: &["designcraft", "idml", "indd", "indt"] },
            OpenFilter { name: "DesignCraft", extensions: &["designcraft"] },
            OpenFilter { name: "InDesign Markup (IDML)", extensions: &["idml"] },
            OpenFilter { name: "InDesign document or template (INDD, INDT)", extensions: &["indd", "indt"] },
            // Files saved without an extension; `file.open` tells the formats apart by content.
            OpenFilter { name: "All files", extensions: &[ALL_FILES] },
        ],
    }
}

/// The "All files" pattern. The macOS panel merges every filter into one list of allowed types,
/// which takes type identifiers as well as extensions: `public.data` admits any file.
pub const ALL_FILES: &str = if cfg!(target_os = "macos") { "public.data" } else { "*" };

/// The extension of the file name a save dialog suggests (`Brochure.designcraft`), if it has one.
/// The dialog filters on it, so the macOS and Windows dialogs add it to a name typed without it.
fn suggested_extension(name: &str) -> Option<&str> {
    std::path::Path::new(name).extension().and_then(|e| e.to_str()).filter(|e| !e.is_empty() && !e.contains(char::is_whitespace))
}

/// The file a save dialog's answer writes: `picked` with the suggested extension added when it has
/// none (Linux dialogs don't add it). A document always ends in `.designcraft`, as `file.save`
/// writes it.
fn picked_save_path(picked: std::path::PathBuf, ext: Option<&str>) -> std::path::PathBuf {
    let Some(ext) = ext else { return picked };
    if ext.eq_ignore_ascii_case("designcraft") {
        let p = picked.to_string_lossy();
        return designcraft_engine::cmd::with_document_extension(&p).into();
    }
    if picked.extension().is_some() || picked.file_name().is_none() {
        return picked;
    }
    let mut s = picked.into_os_string();
    s.push(".");
    s.push(ext);
    s.into()
}

/// The save dialog: the suggested name's extension as its filter, and the extension added to a
/// name typed without it. Adding it names another file than the one the dialog checked, so
/// replacing an existing one asks again.
fn pick_save(name: &str) -> Option<String> {
    let ext = suggested_extension(name);
    let mut dialog = rfd::FileDialog::new().set_file_name(name);
    if let Some(ext) = ext {
        dialog = dialog.add_filter(ext.to_ascii_uppercase(), &[ext]);
    }
    let picked = dialog.save_file()?;
    let path = picked_save_path(picked.clone(), ext);
    if path != picked && path.exists() {
        let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Replace File?")
            .set_description(format!("“{file}” already exists. Do you want to replace it?"))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if answer != rfd::MessageDialogResult::Yes {
            return None;
        }
    }
    Some(path.to_string_lossy().to_string())
}

/// The desktop services: rfd open/save dialogs, `std::fs` reads, atomic writes, the clipboard's text.
pub fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|purpose: &str| {
            let mut dialog = rfd::FileDialog::new();
            for filter in open_filters(purpose) {
                dialog = dialog.add_filter(filter.name, filter.extensions);
            }
            dialog.pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(pick_save)),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| designcraft_engine::atomic::write_atomic(std::path::Path::new(p), b).map_err(|e| e.to_string()))),
        clipboard_text: Some(Box::new(|| arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok())),
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
        for ext in ["designcraft", "idml", "indd", "indt"] {
            assert!(documents.first().is_some_and(|filter| filter.extensions.contains(&ext)), "{ext} is missing from the first Open filter");
        }
        assert!(documents.last().is_some_and(|filter| filter.extensions == [super::ALL_FILES]), "files without an extension can be picked");
    }

    #[test]
    fn relink_open_dialog_lists_graphics() {
        let filters = super::open_filters("relink");
        let exts: Vec<&str> = filters.iter().flat_map(|filter| filter.extensions.iter().copied()).collect();
        for ext in ["png", "jpg", "tif", "psd", "svg", "pdf", "ai", "eps"] {
            assert!(exts.contains(&ext), "{ext} is missing from the relink dialog: {exts:?}");
        }
        assert!(!exts.contains(&"designcraft") && !exts.contains(&"idml"));
    }

    #[test]
    fn save_dialog_answers_get_the_suggested_extension() {
        use std::path::PathBuf;
        let save = |picked: &str, name: &str| super::picked_save_path(PathBuf::from(picked), super::suggested_extension(name));
        assert_eq!(save("/d/Brochure", "Untitled.designcraft"), PathBuf::from("/d/Brochure.designcraft"));
        assert_eq!(save("/d/Brochure.idml", "Untitled.designcraft"), PathBuf::from("/d/Brochure.idml.designcraft"));
        assert_eq!(save("/d/Brochure.designcraft", "Untitled.designcraft"), PathBuf::from("/d/Brochure.designcraft"));
        assert_eq!(save("/d/Brochure", "Untitled.pdf"), PathBuf::from("/d/Brochure.pdf"));
        assert_eq!(save("/d/Brochure.txt", "Untitled.rtf"), PathBuf::from("/d/Brochure.txt"), "another export format is kept");
        assert_eq!(save("/d/Package", "Untitled 1.2 Folder"), PathBuf::from("/d/Package"), "a folder has no extension");
    }
}
