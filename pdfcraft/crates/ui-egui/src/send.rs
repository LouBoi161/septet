//! Pages leaving PdfCraft for another app of a host window (Septet): pages dragged out of the
//! Organize grid or the Pages panel, and Send to with the current page. They go as one PDF of
//! the pages when the other app takes PDFs, otherwise as an image of each page.

use std::path::{Path, PathBuf};

use pdfcraft_engine::DocId;
use pdfcraft_engine::export::{Exporter, ImageFormat};

use crate::PdfCraftApp;

/// The resolution pages are sent at as images (Export ▸ Image's default).
const IMAGE_DPI: f64 = 150.0;

/// JPEG quality for pages sent as JPEG (Export ▸ Image's default).
const JPEG_QUALITY: u8 = 85;

impl PdfCraftApp {
    /// The active document's pages in a drag that may leave the app: dragged in the Organize grid
    /// or the Pages panel, or just let go outside PdfCraft's pages.
    fn outgoing_pages(&self) -> Option<(DocId, Vec<usize>)> {
        let (i, id) = self.active_ids()?;
        let view = self.views.get(i)?;
        let pages = view.org_drag.clone().or_else(|| view.panel_drag.clone()).or_else(|| view.dropped_outside.clone())?;
        (!pages.is_empty()).then_some((id, pages))
    }

    /// While pages are being dragged in the Organize grid or (in a host window) the Pages panel:
    /// what to call them on the drag's ghost, "Page 4" or "3 pages".
    pub fn outgoing_drag(&self) -> Option<String> {
        let (i, id) = self.active_ids()?;
        let view = self.views.get(i)?;
        let pages = view.org_drag.as_ref().or(view.panel_drag.as_ref())?;
        match pages.as_slice() {
            [] => None,
            [page] => {
                let label = self.session.get(id).and_then(|d| d.info.pages.get(*page)).map_or_else(|| (page + 1).to_string(), |p| p.label.clone());
                Some(crate::i18n::fmt(tl!("Page {label}"), &[("label", &label)]))
            }
            many => Some(crate::i18n::fmt(tl!("{n} pages"), &[("n", &many.len().to_string())])),
        }
    }

    /// The dragged pages as files in `dir` for an app that places `accept` (lowercase file
    /// extensions, best first): one PDF of them, or an image of each. Empty when no pages are
    /// being dragged or none of `accept` can be made.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        match self.outgoing_pages() {
            Some((id, pages)) => self.write_pages(id, &pages, accept, dir),
            None => Vec::new(),
        }
    }

    /// The drag went to another app: forget it, so the pages don't move or stay dragged when
    /// PdfCraft shows again.
    pub fn cancel_outgoing_drag(&mut self) {
        for view in &mut self.views {
            view.org_drag = None;
            view.panel_drag = None;
            view.dropped_outside = None;
        }
    }

    /// Send to: the active document's current page as one file in `dir` (see
    /// [`Self::take_outgoing_files`]); `None` on Home or when none of `accept` can be made.
    pub fn send_current_page(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        let (i, id) = self.active_ids()?;
        let page = self.views.get(i)?.current;
        self.write_pages(id, &[page], accept, dir).into_iter().next()
    }

    /// Write `pages` (0-based) of document `id` in the first of `accept` PdfCraft can make.
    fn write_pages(&self, id: DocId, pages: &[usize], accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        let Some(doc) = self.session.get(id) else { return Vec::new() };
        let stem = file_stem(crate::files::strip_pdf(&doc.name));
        for ext in accept {
            let format = match *ext {
                "pdf" => None,
                "png" => Some(ImageFormat::Png),
                "jpg" | "jpeg" => Some(ImageFormat::Jpeg { quality: JPEG_QUALITY }),
                "tif" | "tiff" => Some(ImageFormat::Tiff),
                _ => continue,
            };
            let written = match format {
                None => self.session.extract(id, pages).map_err(|e| e.to_string()).and_then(|bytes| {
                    let name = match pages {
                        [p] => format!("{stem} (page {})", p + 1),
                        [first, .., last] if pages.windows(2).all(|w| w[1] == w[0] + 1) => format!("{stem} (pages {}-{})", first + 1, last + 1),
                        _ => format!("{stem} ({} pages)", pages.len()),
                    };
                    write_new(dir, &name, ext, &bytes).map(|path| vec![path])
                }),
                Some(format) => {
                    let mut exporter = Exporter::new(doc);
                    pages
                        .iter()
                        .map(|p| {
                            exporter.image(*p, IMAGE_DPI, format).and_then(|bytes| write_new(dir, &format!("{stem} (page {})", p + 1), ext, &bytes))
                        })
                        .collect()
                }
            };
            match written {
                Ok(paths) => return paths,
                // A document that forbids copying its pages still renders: try the next format.
                Err(e) => log::warn!("sending pages as {ext}: {e}"),
            }
        }
        Vec::new()
    }
}

/// A document name made safe as a file name stem.
fn file_stem(name: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).take(120).collect();
    let safe = safe.trim().trim_start_matches('.');
    if safe.is_empty() { "Document".to_string() } else { safe.to_string() }
}

/// Write `bytes` to `dir/<name>.<ext>`, or `<name> 2.<ext>` and so on when that exists: files
/// sent earlier may still be in use (placed and linked) in the other app.
fn write_new(dir: &Path, name: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let mut path = dir.join(format!("{name}.{ext}"));
    let mut n = 2;
    while path.exists() {
        if n > 9999 {
            return Err(format!("too many files named {name} in {}", dir.display()));
        }
        path = dir.join(format!("{name} {n}.{ext}"));
        n += 1;
    }
    crate::editing::write_atomically(&path.to_string_lossy(), bytes).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_stems_are_safe() {
        assert_eq!(super::file_stem("Report: Q3/Q4"), "Report_ Q3_Q4");
        assert_eq!(super::file_stem("  ..hidden "), "hidden");
        assert_eq!(super::file_stem(""), "Document");
    }
}
