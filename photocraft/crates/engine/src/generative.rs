//! Generative Fill and Generate Image as a host app does them: PhotoCraft hands the host what it
//! sends to the image generator ([`source`]: the visible composite around the selection, as 8-bit
//! sRGB, and the selection itself) and places the generated pixels back as a new layer
//! ([`place`]), one undoable step like any other edit. The generator itself is the host's.
//!
//! Pixels cross the boundary as 8-bit sRGB, whatever the document's colour mode and bit depth:
//! the composite is converted from the profile it is rendered in
//! ([`crate::color_cmds::composite_profile`]), and generated pixels are converted into the
//! document's profile and depth as layers copied in from another document are.

use photocraft_algo::resample::{Resample, resize_surface_in_canvas};
use photocraft_cms::{Builtin, TransformOptions};
use photocraft_color::{ColorMode, PixelFormat};
use photocraft_doc::{DocId, Document, Layer, LayerContent, LayerId, LayerMask};
use photocraft_geom::{Rect, Size};
use photocraft_raster::Surface;

use crate::{EngineError, Result, Session};

/// Most pixels a generative area or a generated image may have (a 16384² image): more is no
/// image generator's output and would only exhaust memory.
pub const MAX_PIXELS: u64 = 1 << 28;
/// Longest side of a generative area or a generated image.
pub const MAX_SIDE: u32 = 65_536;

/// What a host sends to the image generator ([`source`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    /// The document's id ([`DocId`]), for placing the result back into it ([`place`]).
    pub document: u64,
    /// The document's size in pixels.
    pub size: [u32; 2],
    /// x, y, width, height: the selection's bounds grown by the margin, within the canvas (the
    /// whole canvas without a selection).
    pub rect: [u32; 4],
    /// The visible composite over `rect`: 8-bit sRGB RGBA, unpremultiplied, row-major.
    pub rgba: Vec<u8>,
    /// The selection over `rect`, one coverage byte per pixel (soft edges kept); `None` without a
    /// selection.
    pub mask: Option<Vec<u8>>,
}

/// The active document's composite and selection for an image generator: the selection's bounds
/// grown by `margin` pixels on each side and clamped to the canvas (the whole canvas without a
/// selection), the visible composite there as 8-bit sRGB and the selection's coverage.
pub fn source(s: &Session, margin: u32) -> Result<Source> {
    let st = s.active().ok_or_else(|| EngineError::Other("No document is open.".into()))?;
    let doc = &st.doc;
    let canvas = doc.bounds();
    if canvas.is_empty() {
        return Err(EngineError::Other("The document has no pixels.".into()));
    }
    let rect = match &doc.selection {
        Some(sel) => {
            let bounds = sel.content_bounds().intersect(&canvas);
            if bounds.is_empty() {
                return Err(EngineError::Other("The selection is empty: select the area to fill first.".into()));
            }
            bounds.inflate(i32::try_from(margin).unwrap_or(i32::MAX)).intersect(&canvas)
        }
        None => canvas,
    };
    check_area(rect.width(), rect.height(), "The area to generate")?;
    let rgba = composite_srgb8(s, doc, rect)?;
    let mask = match &doc.selection {
        Some(sel) => Some(coverage8(sel, rect)?),
        None => None,
    };
    let [x, y] = [rect.x0, rect.y0].map(|v| u32::try_from(v).unwrap_or(0));
    Ok(Source { document: doc.id.0, size: [doc.size.width, doc.size.height], rect: [x, y, rect.width(), rect.height()], rgba, mask })
}

/// Generated pixels for [`place`]: `width`×`height` 8-bit sRGB RGBA, unpremultiplied, row-major.
#[derive(Clone, Copy, Debug)]
pub struct Image<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// Place generated pixels in the open document `document` ([`Source::document`]; it need not be
/// the active one): one new pixel layer named `name` above that document's active layer, holding
/// `image` scaled to `rect` (x, y, width, height) at its position, with a layer mask that is
/// `mask` (one byte per pixel of `rect`) inside `rect` and hides everything outside it; no layer
/// mask without `mask`. One undo step (Generative Fill with a mask, Generate Image without); the
/// new layer becomes active. The active document stays the active one. Returns the new layer.
pub fn place(s: &mut Session, document: u64, rect: [u32; 4], image: Image<'_>, mask: Option<&[u8]>, name: &str) -> Result<LayerId> {
    let Image { width, height, rgba } = image;
    let index = s
        .documents()
        .iter()
        .position(|d| d.doc.id == DocId(document))
        .ok_or_else(|| EngineError::Other("The document the image was generated for is closed.".into()))?;
    let [x, y, w, h] = rect;
    check_area(w, h, "The area to fill")?;
    check_area(width, height, "The generated image")?;
    let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
        return Err(EngineError::Other("The area to fill is outside the document.".into()));
    };
    let target = Rect::from_xywh(x, y, w, h);
    let pixels = (width as usize).saturating_mul(height as usize);
    if rgba.len() != pixels.saturating_mul(4) {
        return Err(EngineError::Other(format!(
            "The generated image has {} bytes, not {} for {width} × {height} pixels.",
            rgba.len(),
            pixels.saturating_mul(4)
        )));
    }
    let area = (w as usize).saturating_mul(h as usize);
    if let Some(m) = mask
        && m.len() != area
    {
        return Err(EngineError::Other(format!("The mask has {} bytes, not {area} for {w} × {h} pixels.", m.len())));
    }
    let dest = s.documents().get(index).map(|d| d.doc.clone()).ok_or(EngineError::NoDocument)?;
    let surface = document_pixels(s, &dest, scaled(rgba, width, height, w, h)?, target)?;
    let mut layer = Layer::raster(if name.trim().is_empty() { dest.next_layer_name("Layer") } else { name.to_string() }, surface.format());
    *crate::pixels_mut(&mut layer)? = surface;
    if let Some(m) = mask {
        let mut cover = Surface::with_default(PixelFormat::GRAY8, &[0.0]);
        cover.write_interleaved(target, m);
        let format = cover.format().with_sample(dest.depth);
        if cover.format() != format {
            cover = cover.try_convert(format)?;
        }
        cover.prune();
        layer.mask = Some(LayerMask { surface: cover, ..LayerMask::hide_all() });
    }
    let label = if mask.is_some() { "Generative Fill" } else { "Generate Image" };
    let previous = s.active_index();
    s.set_active(index);
    let placed = s.edit(label, |doc, active| {
        let id = doc.insert_above(*active, layer);
        *active = Some(id);
        Ok(id)
    });
    if let Ok(id) = placed {
        crate::layer_multi_cmds::reselect(s, vec![id], Some(id));
    }
    if let Some(i) = previous {
        s.set_active(i);
    }
    placed
}

/// `what` (a sentence's subject) must be an area an image generator can work with.
fn check_area(w: u32, h: u32, what: &str) -> Result<()> {
    if w == 0 || h == 0 {
        return Err(EngineError::Other(format!("{what} is empty.")));
    }
    if w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(EngineError::Other(format!("{what} is too large ({w} × {h} pixels).")));
    }
    Ok(())
}

/// The document's visible composite over `rect` as 8-bit sRGB RGBA (unpremultiplied).
fn composite_srgb8(s: &Session, doc: &Document, rect: Rect) -> Result<Vec<u8>> {
    let buf = photocraft_compose::render(doc, rect);
    let n = (rect.width() as usize).saturating_mul(rect.height() as usize);
    if buf.px.len() != n {
        return Err(EngineError::Other("The area to generate is too large to render.".into()));
    }
    let mut px = crate::allocation::filled(n.saturating_mul(4), 0.0, "reading the composite")?;
    for (out, p) in px.as_chunks_mut::<4>().0.iter_mut().zip(&buf.px) {
        *out = *p;
    }
    drop(buf);
    let from = crate::color_cmds::composite_profile(doc);
    let srgb = Builtin::Srgb.profile();
    if from.content_hash() != srgb.content_hash() {
        let opts = TransformOptions { intent: s.color.settings.intent(), bpc: s.color.settings.bpc, ..TransformOptions::default() };
        let t = photocraft_cms::cached(&from, srgb, opts).map_err(|e| EngineError::Other(format!("The composite can't be converted to sRGB: {e}")))?;
        t.apply(&mut px, 4);
    }
    Ok(px.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect())
}

/// `sel`'s coverage over `rect` as bytes, one per pixel.
fn coverage8(sel: &Surface, rect: Rect) -> Result<Vec<u8>> {
    let values = crate::allocation::read_region(sel, rect, "reading the selection")?;
    let n = sel.channels().max(1);
    Ok(values.chunks_exact(n).map(|p| (p.first().copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect())
}

/// `rgba` (`width`×`height`) as an 8-bit RGBA surface over `(0, 0, w, h)`, resampled with Bicubic
/// Automatic when the sizes differ.
fn scaled(rgba: &[u8], width: u32, height: u32, w: u32, h: u32) -> Result<Surface> {
    let source = Rect::from_xywh(0, 0, width, height);
    let image = Surface::from_interleaved(PixelFormat::RGBA8, source, rgba);
    if (width, height) == (w, h) {
        return Ok(image);
    }
    let (sx, sy) = (f64::from(w) / f64::from(width), f64::from(h) / f64::from(height));
    let resized = resize_surface_in_canvas(&image, sx, sy, Resample::BicubicAutomatic, source);
    // The scaled rect can round a pixel wider: keep exactly `w`×`h`.
    let area = Rect::from_xywh(0, 0, w, h);
    let bytes = resized.try_to_interleaved(area)?;
    Ok(Surface::from_interleaved(PixelFormat::RGBA8, area, &bytes))
}

/// `image` (8-bit sRGB over `(0, 0, w, h)`) at `target`, in `dest`'s colour mode, profile and bit
/// depth: through the colour engine as layers copied from another document, for the modes that
/// convert so; a plain format conversion otherwise, as a paste.
fn document_pixels(s: &Session, dest: &Document, image: Surface, target: Rect) -> Result<Surface> {
    let fmt = dest.pixel_format();
    let format = PixelFormat::new(fmt.mode, fmt.sample, true);
    let area = Rect::from_xywh(0, 0, target.width(), target.height());
    let bytes = image.try_to_interleaved(area)?;
    let mut surface = Surface::from_interleaved(PixelFormat::RGBA8, target, &bytes);
    drop(image);
    if crate::layer_copy_cmds::layered(dest.mode) {
        // At the document's depth first, so 16- and 32-bit documents convert without 8-bit steps.
        let deep = PixelFormat::RGBA8.with_sample(dest.depth);
        if surface.format() != deep {
            surface = surface.try_convert(deep)?;
        }
        let mut scratch = Document::new("", Size::new(target.width(), target.height()), ColorMode::Rgb, dest.depth);
        scratch.icc_profile = Some(Builtin::Srgb.profile().to_bytes());
        scratch.layers.push(Layer::new("", LayerContent::Raster(surface)));
        let profile = crate::color_cmds::document_profile(dest);
        crate::color_cmds::convert_document(&mut scratch, &profile, s.color.settings.intent(), s.color.settings.bpc)?;
        surface = match scratch.layers.pop().map(|l| l.content) {
            Some(LayerContent::Raster(surface)) => surface,
            _ => return Err(EngineError::Other("The generated pixels were lost converting them to the document's colours.".into())),
        };
    }
    if surface.format() != format {
        surface = surface.try_convert(format)?;
    }
    surface.prune();
    Ok(surface)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A white `w`×`h` document of `mode` and `depth`, a red layer over it.
    fn session(w: u32, h: u32, mode: &str, depth: u32) -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": w, "height": h, "mode": mode, "depth": depth, "background": "white"})).unwrap();
        s
    }

    fn px(src: &Source, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * src.rect[2] + x) * 4) as usize;
        src.rgba[i..i + 4].try_into().unwrap()
    }

    fn close(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tol)
    }

    #[test]
    fn the_source_is_the_selection_grown_by_the_margin_within_the_canvas() {
        let mut s = session(64, 48, "rgb", 8);
        s.execute("layer.new.layer", json!({})).unwrap();
        s.execute("select.rect", json!({"x": 0, "y": 0, "width": 32, "height": 48})).unwrap();
        s.execute("edit.fill", json!({"color": "#ff0000"})).unwrap();
        s.execute("select.rect", json!({"x": 10, "y": 8, "width": 20, "height": 10})).unwrap();
        let src = source(&s, 4).unwrap();
        assert_eq!(src.document, s.active().unwrap().doc.id.0);
        assert_eq!((src.size, src.rect), ([64, 48], [6, 4, 28, 18]));
        assert_eq!(src.rgba.len(), 28 * 18 * 4);
        // The composite: red on the left half, the white background on the right.
        assert_eq!(px(&src, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&src, 27, 17), [255, 255, 255, 255]);
        let mask = src.mask.as_ref().unwrap();
        assert_eq!(mask.len(), 28 * 18);
        let m = |x: u32, y: u32| mask[(y * 28 + x) as usize];
        assert_eq!((m(4, 4), m(23, 13), m(3, 4), m(4, 3), m(24, 13), m(0, 0)), (255, 255, 0, 0, 0, 0));
        // Near an edge the margin stops at the canvas.
        s.execute("select.rect", json!({"x": 2, "y": 40, "width": 60, "height": 8})).unwrap();
        assert_eq!(source(&s, 16).unwrap().rect, [0, 24, 64, 24]);
        // A huge margin is the whole canvas.
        assert_eq!(source(&s, u32::MAX).unwrap().rect, [0, 0, 64, 48]);
    }

    #[test]
    fn soft_selection_edges_are_kept_in_the_mask() {
        let mut s = session(64, 64, "rgb", 8);
        s.execute("select.rect", json!({"x": 16, "y": 16, "width": 32, "height": 32})).unwrap();
        s.execute("select.modify.feather", json!({"radius": 4})).unwrap();
        let src = source(&s, 0).unwrap();
        let sel = s.active().unwrap().doc.selection.clone().unwrap();
        let mask = src.mask.unwrap();
        assert!(mask.iter().any(|v| (1..255).contains(v)), "soft edges");
        let [x0, y0, w, _] = src.rect;
        for (i, v) in mask.iter().enumerate() {
            let (x, y) = (x0 + i as u32 % w, y0 + i as u32 / w);
            let want = (sel.sample_channel(x as i32, y as i32, 0) * 255.0 + 0.5) as u8;
            assert!(v.abs_diff(want) <= 1, "({x}, {y}): {v} vs {want}");
        }
    }

    #[test]
    fn without_a_selection_the_source_is_the_whole_canvas() {
        let s = session(20, 10, "rgb", 8);
        let src = source(&s, 8).unwrap();
        assert_eq!((src.rect, src.mask), ([0, 0, 20, 10], None));
        assert!(src.rgba.as_chunks::<4>().0.iter().all(|p| *p == [255, 255, 255, 255]));
    }

    #[test]
    fn the_source_is_srgb_at_every_depth_and_mode() {
        for (mode, depth) in [("rgb", 16), ("rgb", 32), ("grayscale", 8), ("cmyk", 8), ("lab", 16)] {
            let mut s = session(16, 16, mode, depth);
            s.execute("select.rect", json!({"x": 0, "y": 0, "width": 8, "height": 16})).unwrap();
            s.execute("edit.fill", json!({"color": "#000000"})).unwrap();
            s.execute("select.deselect", json!({})).unwrap();
            let src = source(&s, 0).unwrap();
            assert!(close(px(&src, 12, 8), [255, 255, 255, 255], 3), "{mode} {depth}: white {:?}", px(&src, 12, 8));
            // CMYK's rich black is not quite black in sRGB.
            assert!(close(px(&src, 2, 8), [0, 0, 0, 255], 40), "{mode} {depth}: black {:?}", px(&src, 2, 8));
        }
    }

    #[test]
    fn no_document_or_an_empty_selection_is_an_error() {
        assert!(source(&Session::new(), 0).unwrap_err().to_string().contains("No document"));
        let mut s = session(8, 8, "rgb", 8);
        let id = s.active().unwrap().doc.id.0;
        s.edit("Empty", |doc, _| {
            doc.selection = Some(Surface::with_default(PixelFormat::GRAY8, &[0.0]));
            Ok(())
        })
        .unwrap();
        assert!(source(&s, 0).is_err());
        assert!(place(&mut s, id, [0, 0, 0, 4], Image { width: 1, height: 1, rgba: &[0; 4] }, None, "x").is_err(), "an empty area");
    }

    fn green(w: u32, h: u32) -> Vec<u8> {
        [0u8, 255, 0, 255].repeat((w * h) as usize)
    }

    #[test]
    fn a_generated_image_lands_above_the_active_layer_masked_in_one_step() {
        let mut s = session(32, 24, "rgb", 8);
        s.execute("layer.new.layer", json!({})).unwrap();
        s.execute("layer.new.layer", json!({})).unwrap();
        let below = s.active().unwrap().doc.layers[1].id;
        s.select_layer(below).unwrap();
        let (doc, layers, steps) = (s.active().unwrap().doc.id.0, s.active().unwrap().doc.layers.len(), s.active().unwrap().history.entries().len());
        s.active_mut().unwrap().saved_revision = s.active().unwrap().revision;
        // The left half of the area is revealed, the right half hidden.
        let mask: Vec<u8> = (0..6).flat_map(|_| (0..10).map(|x| if x < 5 { 255 } else { 0 })).collect();
        let id = place(&mut s, doc, [4, 4, 10, 6], Image { width: 10, height: 6, rgba: &green(10, 6) }, Some(&mask), "a green field").unwrap();
        let st = s.active().unwrap();
        assert_eq!(st.doc.layers.len(), layers + 1);
        assert_eq!(st.doc.layers[2].id, id, "right above the active layer");
        assert_eq!((st.active_layer, st.selected_layers()), (Some(id), vec![id]));
        assert!(st.is_dirty());
        assert_eq!(st.history.undo_label(), Some("Generative Fill"));
        assert_eq!(st.history.entries().len(), steps + 1);
        let l = st.doc.layer(id).unwrap();
        assert_eq!(l.name, "a green field");
        let surf = l.surface().unwrap();
        assert_eq!(surf.content_bounds(), Rect::from_xywh(4, 4, 10, 6));
        assert_eq!(surf.rgba(4, 4), [0.0, 1.0, 0.0, 1.0]);
        let m = l.mask.as_ref().unwrap();
        assert_eq!((m.value(4, 4), m.value(8, 9), m.value(9, 4), m.value(13, 9), m.value(0, 0), m.value(20, 20)), (1.0, 1.0, 0.0, 0.0, 0.0, 0.0));
        // One undo takes it away.
        s.execute("edit.undo", json!({})).unwrap();
        let st = s.active().unwrap();
        assert!(st.doc.layer(id).is_none());
        assert_eq!(st.doc.layers.len(), layers);
    }

    #[test]
    fn without_a_mask_the_layer_has_none_and_an_image_of_another_size_is_scaled() {
        let mut s = session(40, 40, "rgb", 8);
        let doc = s.active().unwrap().doc.id.0;
        let id = place(&mut s, doc, [10, 5, 20, 12], Image { width: 5, height: 3, rgba: &green(5, 3) }, None, "").unwrap();
        let st = s.active().unwrap();
        assert_eq!(st.history.undo_label(), Some("Generate Image"));
        let l = st.doc.layer(id).unwrap();
        assert!(l.mask.is_none());
        assert_eq!(l.name, "Layer 1", "a nameless result gets the next layer name");
        let surf = l.surface().unwrap();
        assert_eq!(surf.content_bounds(), Rect::from_xywh(10, 5, 20, 12));
        for (x, y) in [(10, 5), (29, 16), (20, 10)] {
            let p = surf.rgba(x, y);
            assert!((p[1] - 1.0).abs() < 0.01 && p[0] < 0.01 && (p[3] - 1.0).abs() < 0.01, "({x}, {y}): {p:?}");
        }
    }

    #[test]
    fn the_result_goes_to_its_own_document_even_when_another_is_active() {
        let mut s = session(16, 16, "rgb", 8);
        let first = s.active().unwrap().doc.id.0;
        s.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
        let active = s.active_index();
        let id = place(&mut s, first, [0, 0, 4, 4], Image { width: 4, height: 4, rgba: &green(4, 4) }, None, "g").unwrap();
        assert_eq!(s.active_index(), active, "the active document stays");
        assert!(s.documents()[0].doc.layer(id).is_some());
        assert!(s.documents()[0].is_dirty());
        assert!(s.active().unwrap().doc.layer(id).is_none());
        // Closed: an error, nothing changes.
        s.close(0);
        let err = place(&mut s, first, [0, 0, 4, 4], Image { width: 4, height: 4, rgba: &green(4, 4) }, None, "g").unwrap_err();
        assert!(err.to_string().contains("closed"), "{err}");
        assert!(place(&mut s, u64::MAX, [0, 0, 4, 4], Image { width: 4, height: 4, rgba: &green(4, 4) }, None, "g").is_err());
    }

    #[test]
    fn malformed_pixels_are_errors() {
        let mut s = session(16, 16, "rgb", 8);
        let doc = s.active().unwrap().doc.id.0;
        let layers = s.active().unwrap().doc.layers.len();
        for (rect, w, h, rgba, mask) in [
            ([0, 0, 4, 4], 4, 4, vec![0u8; 15], None),
            ([0, 0, 4, 4], 4, 4, green(4, 4), Some(vec![0u8; 3])),
            ([0, 0, 4, 4], 0, 4, vec![], None),
            ([0, 0, 100_000, 100_000], 4, 4, green(4, 4), None),
            ([0, 0, 4, 4], 70_000, 1, green(70_000, 1), None),
            ([u32::MAX, 0, 4, 4], 4, 4, green(4, 4), None),
        ] {
            assert!(place(&mut s, doc, rect, Image { width: w, height: h, rgba: &rgba }, mask.as_deref(), "x").is_err(), "{rect:?} {w}×{h}");
        }
        assert_eq!(s.active().unwrap().doc.layers.len(), layers);
    }

    #[test]
    fn placed_pixels_take_the_documents_depth_and_colours() {
        // An in-gamut colour for every mode; gray documents show its luminance.
        let tan = [180u8, 130, 90, 255];
        for (mode, depth) in [("rgb", 8), ("rgb", 16), ("rgb", 32), ("grayscale", 8), ("cmyk", 8), ("lab", 16)] {
            let mut s = session(16, 16, mode, depth);
            s.execute("select.all", json!({})).unwrap();
            s.execute("edit.fill", json!({"color": "#000000"})).unwrap();
            s.execute("select.deselect", json!({})).unwrap();
            let doc = s.active().unwrap().doc.id.0;
            let mask: Vec<u8> = (0..16).map(|i| if i < 8 { 255 } else { 128 }).collect();
            let id = place(&mut s, doc, [2, 2, 4, 4], Image { width: 4, height: 4, rgba: &tan.repeat(16) }, Some(&mask), "tan").unwrap();
            let st = s.active().unwrap();
            let l = st.doc.layer(id).unwrap();
            let fmt = st.doc.pixel_format();
            assert_eq!(l.surface().unwrap().format(), PixelFormat::new(fmt.mode, fmt.sample, true), "{mode} {depth}");
            let m = l.mask.as_ref().unwrap();
            assert_eq!(m.surface.format().sample, st.doc.depth, "{mode} {depth}: the mask at the document's depth");
            assert!((m.value(3, 3) - 1.0).abs() < 1e-3 && (m.value(3, 5) - 128.0 / 255.0).abs() < 0.01, "{mode} {depth}: the mask");
            // What the document shows where the mask reveals all is what came in.
            let src = source(&s, 0).unwrap();
            let want = if mode == "grayscale" {
                let shown = px(&src, 3, 3);
                assert!(shown[0] == shown[1] && shown[1] == shown[2], "{mode}: gray {shown:?}");
                shown
            } else {
                tan
            };
            assert!(close(px(&src, 3, 3), want, 4), "{mode} {depth}: {:?}", px(&src, 3, 3));
            assert!(px(&src, 3, 3)[0] > 100, "{mode} {depth}: not the black below");
            assert!(close(px(&src, 0, 0), [0, 0, 0, 255], 8), "{mode} {depth}: outside the area {:?}", px(&src, 0, 0));
        }
    }
}
