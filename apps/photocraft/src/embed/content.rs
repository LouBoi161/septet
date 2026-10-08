//! Files handed to other apps in the host: dragged layers and "Send to" (see `Embedded`).
//!
//! They are written as Save As writes them (`photocraft_io::export`, crash-guarded, written
//! atomically), in the first format the target takes that PhotoCraft can write: a PSD keeps the
//! layers, everything else is the flattened image with its transparency (JPEG over white).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use photocraft_doc::{Document, LayerId};
use photocraft_engine::Session;

/// Formats PhotoCraft writes (its Save As formats).
const WRITABLE: [&str; 10] = ["psd", "psb", "tif", "tiff", "png", "webp", "jpg", "jpeg", "tga", "exr"];

/// The layers `ids` of `doc` alone on a copy of its canvas (same colour, profile and resolution),
/// as File › Export › Layers to Files does: the outermost of them, bottom to top, shown and
/// unclipped. `None` when none of them is in `doc`.
pub fn layers_alone(doc: &Document, ids: &[LayerId]) -> Option<Document> {
    let walk = doc.walk();
    let picked: Vec<_> = walk.iter().filter(|(_, _, l)| ids.contains(&l.id)).collect();
    let layers: Vec<_> = picked
        .iter()
        .filter(|(p, _, _)| !picked.iter().any(|(q, _, _)| q.len() < p.len() && p.starts_with(q)))
        .map(|(_, _, l)| {
            let mut l = (*l).clone();
            l.visible = true;
            l.clipped = false;
            l
        })
        .collect();
    if layers.is_empty() {
        return None;
    }
    let mut one = doc.clone();
    one.layers = layers;
    one.selection = None;
    one.quick_mask = None;
    one.channels.clear();
    one.layer_comps.clear();
    one.last_applied_comp = None;
    one.last_document_state = None;
    Some(one)
}

/// `doc` cropped to what it shows (Image › Trim on transparent pixels); unchanged when nothing
/// shows.
pub fn trimmed(doc: Document) -> Document {
    let mut s = Session::new();
    s.add_document(doc.clone(), None);
    if let Err(e) = s.execute("image.trim", serde_json::json!({})) {
        log::info!("handing over the whole canvas: {e}");
        return doc;
    }
    s.close(0).map_or(doc, |st| Arc::unwrap_or_clone(st.doc))
}

/// Write `doc` into `dir` as `name` in the first format of `accept` (lowercase extensions, best
/// first) PhotoCraft can write; a format that fails moves on to the next. Never replaces a file:
/// one handed over earlier may be linked from another document.
pub fn write(doc: &Document, name: &str, accept: &[&str], dir: &Path) -> Option<PathBuf> {
    let name = file_name(name);
    // Like Export As, flat files carry no XMP packet: it can hold the text of every type layer (#647).
    let opts = photocraft_io::ExportOptions { xmp: photocraft_io::XmpEmbed::None, ..Default::default() };
    for ext in accept.iter().map(|e| e.trim_start_matches('.').to_ascii_lowercase()) {
        if !WRITABLE.contains(&ext.as_str()) {
            continue;
        }
        let path = unique_path(dir, &name, &ext);
        let written = crate::crash_guard::guard("Export", || photocraft_io::export(doc, &ext, &opts).map(|r| r.bytes).map_err(|e| e.to_string()))
            .and_then(|bytes| photocraft_format::atomic_write(&path, &bytes).map_err(|e| e.to_string()));
        match written {
            Ok(()) => return Some(path),
            Err(e) => log::warn!("couldn't write {}: {e}", path.display()),
        }
    }
    None
}

/// Whether `path`'s extension is one of `accept`.
pub fn accepted(path: &Path, accept: &[&str]) -> bool {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).is_some_and(|e| accept.iter().any(|a| a.eq_ignore_ascii_case(&e)))
}

/// A document's name without its extension ("Beach.psd" → "Beach").
pub fn stem(name: &str) -> String {
    Path::new(name).file_stem().map_or_else(|| name.to_string(), |s| s.to_string_lossy().into_owned())
}

/// `name` as a file name on every platform: no separators, reserved or control characters, no
/// leading or trailing dots or spaces.
fn file_name(name: &str) -> String {
    let clean: String =
        name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).take(120).collect();
    let clean = clean.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if clean.is_empty() { "Untitled".to_string() } else { clean.to_string() }
}

/// `dir/name.ext`, or `dir/name 2.ext`, `name 3.ext` … when it exists.
fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{name}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..10_000).map(|n| dir.join(format!("{name} {n}.{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A 40×30 document: a white Background, a red 10×10 square at (4, 6) on "paint", and a group
    /// "set" holding a hidden layer "inner".
    fn document() -> (Document, LayerId, LayerId, LayerId) {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 40, "height": 30, "name": "Beach.psd"})).unwrap();
        let id = |v: serde_json::Value| LayerId(v["layer"].as_u64().unwrap());
        let paint = id(s.execute("layer.new.layer", json!({"name": "paint"})).unwrap());
        s.execute("select.rect", json!({"x": 4, "y": 6, "width": 10, "height": 10})).unwrap();
        s.execute("edit.fill", json!({"color": "#ff0000"})).unwrap();
        s.execute("select.deselect", json!({})).unwrap();
        let inner = id(s.execute("layer.new.layer", json!({"name": "inner"})).unwrap());
        let set = id(s.execute("layer.groupLayers", json!({})).unwrap());
        s.execute("layer.setProps", json!({"layer": inner.0, "visible": false})).unwrap();
        (Document::clone(&s.active().unwrap().doc), paint, inner, set)
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("photocraft-embed-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn dragged_layers_stand_alone_outermost_first_and_shown() {
        let (doc, paint, inner, set) = document();
        let one = layers_alone(&doc, &[paint]).unwrap();
        assert_eq!(one.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["paint"]);
        assert_eq!((one.size, one.mode, one.depth), (doc.size, doc.mode, doc.depth));
        // A group and a layer inside it: the group carries the layer.
        let both = layers_alone(&doc, &[inner, set, paint]).unwrap();
        assert_eq!(both.layers.len(), 2);
        assert_eq!(both.layers[0].id, paint, "bottom to top, as in the document");
        // A layer from inside a group comes out on its own, shown.
        let alone = layers_alone(&doc, &[inner]).unwrap();
        assert_eq!(alone.layers.len(), 1);
        assert!(alone.layers[0].visible);
        assert!(layers_alone(&doc, &[LayerId(u64::MAX)]).is_none());
    }

    #[test]
    fn a_dragged_layer_is_trimmed_to_what_it_shows() {
        let (doc, paint, _, _) = document();
        let t = trimmed(layers_alone(&doc, &[paint]).unwrap());
        assert_eq!((t.size.width, t.size.height), (10, 10));
        // Nothing shows: the canvas as it is.
        let empty = Document::new("empty", doc.size, doc.mode, doc.depth);
        assert_eq!(trimmed(empty).size, doc.size);
    }

    #[test]
    fn files_take_the_first_writable_format_and_never_replace_one() {
        let dir = temp("write");
        let (doc, paint, _, _) = document();
        let one = trimmed(layers_alone(&doc, &[paint]).unwrap());
        let png = write(&one, "paint", &["svg", "pdf", "png", "jpg"], &dir).unwrap();
        assert_eq!(png, dir.join("paint.png"));
        let img = photocraft_codecs::decode(&std::fs::read(&png).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (10, 10));
        assert_eq!(write(&one, "paint", &["png"], &dir), Some(dir.join("paint 2.png")));
        let psd = write(&doc, "Beach", &["psd", "png"], &dir).unwrap();
        assert!(photocraft_io::is_psd(&std::fs::read(&psd).unwrap()));
        assert_eq!(write(&doc, "a/b:c", &["TIF"], &dir), Some(dir.join("a_b_c.tif")));
        assert_eq!(write(&doc, "x", &["svg", "pdf"], &dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_and_extensions() {
        assert_eq!(stem("Beach.psd"), "Beach");
        assert_eq!(stem("Untitled-1"), "Untitled-1");
        assert_eq!(file_name(" ..hidden. "), "hidden");
        assert_eq!(file_name("..."), "Untitled");
        assert!(accepted(Path::new("/a/photo.JPG"), &["png", "jpg"]));
        assert!(!accepted(Path::new("/a/photo.pcraft"), &["png", "jpg"]));
        assert!(!accepted(Path::new("/a/noext"), &["png"]));
    }
}
