//! Art leaving the app for another app sharing its window ([`crate::hosted`]: Septet's tabs):
//! objects dragged off the canvas with the Selection tool or rows dragged in the Layers panel,
//! and the active document sent to another app, each written as one file the other app places.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use vectorcraft_doc::{Document, Node, NodeId};
use vectorcraft_engine::DocState;
use vectorcraft_engine::cmd::fileio;

use crate::VectorcraftApp;
use crate::widgets::PanelDrag;

/// Objects dragged out of the app: their document and ids (a layer stands for its art).
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    /// The document they are in ([`DocState::uid`]).
    pub doc: u64,
    pub ids: Vec<NodeId>,
}

/// Rasters are written at whole multiples of 72 ppi: as many as bring the longer side to this
/// many pixels (at most [`MAX_SCALE`] times)…
const RASTER_SIDE: f64 = 2048.0;
const MAX_SCALE: f64 = 4.0;
/// …and never more pixels a side than this.
const MAX_RASTER_SIDE: f64 = 8192.0;

impl VectorcraftApp {
    /// The drag in progress that another app could take: art dragged off the canvas with the
    /// Selection tool, or rows (or the selected-art square) dragged in the Layers panel. Cheap: it
    /// only looks at egui's drag payload.
    pub fn outgoing_drag(&self, ctx: &egui::Context) -> Option<Outgoing> {
        let st = self.session.active()?;
        let ids = match egui::DragAndDrop::payload::<PanelDrag>(ctx) {
            Some(d) => match &*d {
                PanelDrag::Art(ids) => ids.clone(),
                _ => return None,
            },
            None => crate::panels::layers::dragged(ctx, &st.selection.objects)?,
        };
        (!ids.is_empty()).then_some(Outgoing { doc: st.uid, ids })
    }

    /// A short name for `out`, for the drag's ghost: `Layer “Sky”`, `Ellipse`, `3 objects`.
    pub fn outgoing_label(&self, out: &Outgoing) -> String {
        let doc = self.session.documents().iter().find(|st| st.uid == out.doc).map(|st| &*st.doc);
        match (&out.ids[..], doc) {
            ([id], Some(doc)) => match doc.node(*id) {
                Some(n) if n.is_layer() => format!("Layer “{}”", n.display_name()),
                Some(n) => object_name(n),
                None => "1 object".into(),
            },
            (ids, Some(doc)) if ids.iter().all(|id| doc.node(*id).is_some_and(Node::is_layer)) => format!("{} layers", ids.len()),
            (ids, _) => format!("{} objects", ids.len()),
        }
    }

    /// Forget the drag another app took ([`Self::outgoing_drag`]): nothing happens when the button
    /// goes up, nor when the app shows again.
    pub fn cancel_outgoing_drag(&mut self, ctx: &egui::Context) {
        if egui::DragAndDrop::has_payload_of_type::<PanelDrag>(ctx) {
            egui::DragAndDrop::clear_payload(ctx);
        }
        crate::panels::layers::forget_drag(ctx);
        crate::canvas::forget_art_drag(ctx);
    }

    /// Write `out`'s art, cropped to its bounds, as one file of the first format in `accept`
    /// (lower-case extensions, best first) the app writes, into `dir` → its path.
    pub fn write_outgoing(&self, out: &Outgoing, accept: &[&str], dir: &Path) -> Result<PathBuf, String> {
        let st = self.session.documents().iter().find(|st| st.uid == out.doc).ok_or("the document was closed")?;
        let (f, ext) = writable(accept)?;
        let name = match &out.ids[..] {
            [id] => st.doc.node(*id).map(object_name),
            _ => None,
        }
        .unwrap_or_else(|| format!("{} art", doc_name(st)));
        let ids = layer_art(&st.doc, &out.ids);
        let (doc, bounds) = fileio::objects_document(st, &ids, &name).ok_or("the art has no extent")?;
        let bytes = fileio::encode(&doc, f.id, &options(f, bounds.width(), bounds.height(), json!({}))).map_err(|e| e.to_string())?;
        write_new(dir, &name, ext, &bytes)
    }

    /// The active document as one file of the first format in `accept` (lower-case extensions,
    /// best first) the app writes, into `dir` → its path (none without a document). Unmodified,
    /// it is the document's own file when `accept` takes its type; otherwise PDF writes every
    /// artboard, other formats the one in view.
    pub fn write_active(&self, accept: &[&str], dir: &Path) -> Option<Result<PathBuf, String>> {
        let st = self.session.active()?;
        if !st.is_dirty()
            && let Some(path) = st.path.as_deref()
            && accept.contains(&fileio::extension(path).as_str())
            && Path::new(path).is_file()
        {
            return Some(Ok(PathBuf::from(path)));
        }
        Some(self.export_active(st, accept, dir))
    }

    fn export_active(&self, st: &DocState, accept: &[&str], dir: &Path) -> Result<PathBuf, String> {
        let (f, ext) = writable(accept)?;
        let artboard = self.view().map_or(0, |v| v.artboard).min(st.doc.artboards.len().saturating_sub(1));
        let (w, h) = st.doc.artboards.get(artboard).map_or((0.0, 0.0), |a| (a.rect.width(), a.rect.height()));
        let pick = if f.id == "pdf" { json!({}) } else { json!({ "artboard": artboard }) };
        let bytes = fileio::encode(&st.doc, f.id, &options(f, w, h, pick)).map_err(|e| e.to_string())?;
        write_new(dir, &doc_name(st), ext, &bytes)
    }
}

/// The first of `accept` the app writes as one picture (text files only hold type) → its format
/// and that extension.
fn writable<'a>(accept: &[&'a str]) -> Result<(&'static fileio::Format, &'a str), String> {
    accept
        .iter()
        .find_map(|ext| fileio::format(ext).filter(|f| f.write && f.extensions.contains(ext) && f.id != "txt").map(|f| (f, *ext)))
        .ok_or_else(|| format!("VectorCraft can't write any of {}", accept.join(", ")))
}

/// `pick` (the artboard to write) with the options of format `f` for art of `w` × `h` points:
/// rasters transparent (formats without alpha flatten on white) at whole multiples of 72 ppi, PDF
/// without VectorCraft's own data.
fn options(f: &fileio::Format, w: f64, h: f64, mut pick: Value) -> Value {
    if f.raster {
        let side = w.max(h);
        let scale = if side.is_finite() && side > 0.0 { (RASTER_SIDE / side).ceil().clamp(1.0, MAX_SCALE).min(MAX_RASTER_SIDE / side) } else { 1.0 };
        pick["ppi"] = json!(72.0 * scale);
        pick["background"] = json!("transparent");
    } else if f.id == "pdf" {
        pick["preserveEditing"] = json!(false);
    }
    pick
}

/// The layers among `ids` replaced by their art (shown sublayers' too), for the export, which
/// takes objects.
fn layer_art(doc: &Document, ids: &[NodeId]) -> Vec<NodeId> {
    fn add(n: &Node, top: bool, out: &mut Vec<NodeId>) {
        if !n.is_layer() {
            out.push(n.id);
        } else if top || n.visible {
            for c in n.children().into_iter().flatten() {
                add(c, false, out);
            }
        }
    }
    let mut out = vec![];
    for n in ids.iter().filter_map(|id| doc.node(*id)) {
        add(n, true, &mut out);
    }
    out
}

/// An object's name: its own, else its kind (`Ellipse`), or a type object's first words.
fn object_name(n: &Node) -> String {
    let name = n.display_name();
    match name.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        Some(kind) if n.name.is_none() => kind.to_string(),
        _ => name,
    }
}

/// The document's name without its extension.
fn doc_name(st: &DocState) -> String {
    st.path.as_deref().map_or_else(|| st.doc.title.clone(), fileio::file_stem)
}

/// Write `bytes` into `dir` as `name`.`ext`, numbered (`name 2.ext`…) rather than replacing a file
/// an earlier drag left there, which another app may still link to → the path.
fn write_new(dir: &Path, name: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let clean: String = name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.');
    let stem: String = if clean.is_empty() { "Untitled".into() } else { clean.chars().take(80).collect() };
    let path = (1..10_000)
        .map(|i| dir.join(if i == 1 { format!("{stem}.{ext}") } else { format!("{stem} {i}.{ext}") }))
        .find(|p| !p.exists())
        .ok_or_else(|| format!("{} holds too many files named {stem}", dir.display()))?;
    fileio::write_atomic(&path, bytes).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vectorcraft_engine::Session;

    use super::*;
    use crate::Services;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vectorcraft-outgoing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn app_with_art() -> (VectorcraftApp, NodeId, NodeId) {
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 400, "height": 300})).unwrap();
        let a = app.run("shape.rectangle", json!({"x": 10, "y": 20, "width": 100, "height": 50})).unwrap();
        let b = app.run("shape.ellipse", json!({"x": 160, "y": 120, "width": 80, "height": 60})).unwrap();
        let id = |v: &Value| NodeId(v["id"].as_u64().unwrap());
        (app, id(&a), id(&b))
    }

    #[test]
    fn art_dragged_off_the_canvas_goes_out_as_one_cropped_file() {
        let (mut app, a, b) = app_with_art();
        let ctx = egui::Context::default();
        assert_eq!(app.outgoing_drag(&ctx), None);
        egui::DragAndDrop::set_payload(&ctx, PanelDrag::Art(vec![b, a]));
        let out = app.outgoing_drag(&ctx).unwrap();
        assert_eq!(app.outgoing_label(&out), "2 objects");
        let dir = temp_dir("art");
        // The first format the app writes: SVG, cropped to the two objects (their 1 pt strokes
        // included).
        let svg = app.write_outgoing(&out, &["kra", "svg", "png"], &dir).unwrap();
        assert_eq!(svg.extension().unwrap(), "svg");
        let text = std::fs::read_to_string(&svg).unwrap();
        assert!(text.contains("viewBox=\"0 0 231 161\""), "{text}");
        // A second drag of the same art doesn't replace the first file.
        let again = app.write_outgoing(&out, &["svg"], &dir).unwrap();
        assert_ne!(again, svg);
        // PNG: transparent, at a whole multiple of 72 ppi.
        let png = app.write_outgoing(&out, &["png"], &dir).unwrap();
        let img = image::open(&png).unwrap();
        assert_eq!((img.width(), img.height()), (231 * 4, 161 * 4));
        assert_eq!(img.to_rgba8().get_pixel(0, img.height() - 1)[3], 0, "bottom left: no art");
        assert!(app.write_outgoing(&out, &["kra"], &dir).is_err());
        // Taken by another app: forgotten.
        app.cancel_outgoing_drag(&ctx);
        assert_eq!(app.outgoing_drag(&ctx), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_dragged_layer_goes_out_as_its_art() {
        let (app, a, _) = app_with_art();
        let st = app.session.active().unwrap();
        let layer = st.doc.ancestry(a).unwrap()[0];
        let out = Outgoing { doc: st.uid, ids: vec![layer] };
        assert_eq!(app.outgoing_label(&out), "Layer “Layer 1”");
        let dir = temp_dir("layer");
        let pdf = app.write_outgoing(&out, &["pdf"], &dir).unwrap();
        assert_eq!(pdf.file_name().unwrap(), "Layer 1.pdf");
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        let one = Outgoing { doc: st.uid, ids: vec![a] };
        assert_eq!(app.outgoing_label(&one), "Rectangle");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_active_document_goes_out_as_one_file() {
        let (app, ..) = app_with_art();
        let dir = temp_dir("active");
        let svg = app.write_active(&["svg", "pdf"], &dir).unwrap().unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().contains("viewBox=\"0 0 400 300\""));
        let png = app.write_active(&["tif", "png"], &dir).unwrap().unwrap();
        assert_eq!(png.extension().unwrap(), "tif");
        let empty = VectorcraftApp::new(Session::new(), Services::default());
        assert!(empty.write_active(&["svg"], &dir).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
