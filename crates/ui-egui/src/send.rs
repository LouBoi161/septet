//! Content leaving EffectCraft for another app (an embedding host's drag-out and Send To): Project
//! items dragged from the Project panel and files from the Media Browser. Footage goes as its
//! source file when the other app takes that kind of file; a composition as its current frame,
//! rendered as an image the other app takes (a layered PSD, a PNG with alpha, a JPEG…).

use std::path::{Path, PathBuf};

use effectcraft_engine::project::{ItemId, ItemKind};
use effectcraft_engine::time::Tick;
use effectcraft_engine::{Event, Session};
use serde_json::json;

use crate::EffectcraftApp;
use crate::panels::DragPayload;

/// What a drag from EffectCraft's panels carries.
#[derive(Clone, Debug, PartialEq)]
pub enum Outgoing {
    /// Project items (compositions and footage with a file): the dragged one, or the selection
    /// it belongs to.
    Items(Vec<ItemId>),
    /// Files from the Media Browser.
    Files(Vec<String>),
}

/// The drag in progress from the Project panel or the Media Browser, if any.
pub fn outgoing(app: &EffectcraftApp, ctx: &egui::Context) -> Option<Outgoing> {
    let payload = egui::DragAndDrop::payload::<DragPayload>(ctx)?;
    match payload.as_ref() {
        DragPayload::Item(id) => {
            let id = ItemId(*id);
            let selection = &app.session.state.project_selection;
            let items = if selection.contains(&id) { selection.clone() } else { vec![id] };
            let items: Vec<ItemId> = items.into_iter().filter(|i| sendable(&app.session, *i)).collect();
            (!items.is_empty()).then_some(Outgoing::Items(items))
        }
        DragPayload::Files(paths) if !paths.is_empty() => Some(Outgoing::Files(paths.clone())),
        _ => None,
    }
}

/// A composition, or footage with a file.
fn sendable(s: &Session, id: ItemId) -> bool {
    match s.project.item(id).map(|i| &i.kind) {
        Some(ItemKind::Comp(_)) => true,
        Some(ItemKind::Footage(f)) => !f.path.is_empty(),
        _ => false,
    }
}

/// The drag ghost's label: `Composition “Main”`, `Footage “clip.mov”`, `3 items`, `2 files`.
pub fn label(app: &EffectcraftApp, out: &Outgoing) -> String {
    match out {
        Outgoing::Items(items) => match items.as_slice() {
            [one] => match app.session.project.item(*one) {
                Some(it) if matches!(it.kind, ItemKind::Comp(_)) => format!("Composition “{}”", it.name),
                Some(it) => format!("Footage “{}”", it.name),
                None => "1 item".into(),
            },
            many => format!("{} items", many.len()),
        },
        Outgoing::Files(paths) => match paths.as_slice() {
            [one] => Path::new(one).file_name().map_or_else(|| one.clone(), |n| n.to_string_lossy().into_owned()),
            many => format!("{} files", many.len()),
        },
    }
}

/// The dragged content as files the other app takes (`accept`: its extensions, best first);
/// rendered frames are written to `dir`.
pub fn files(app: &mut EffectcraftApp, out: &Outgoing, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
    match out {
        Outgoing::Items(items) => items.iter().filter_map(|i| item_file(app, *i, accept, dir)).collect(),
        Outgoing::Files(paths) => paths.iter().map(PathBuf::from).filter(|p| p.is_file() && accepts(accept, p)).collect(),
    }
}

/// Send To: the active composition's current frame, or without one the selected footage's file.
pub fn active_file(app: &mut EffectcraftApp, accept: &[&str], dir: &Path) -> Option<PathBuf> {
    if let Some(comp) = app.session.active_comp_id() {
        return comp_frame_file(app, comp, accept, dir);
    }
    let selection = app.session.state.project_selection.clone();
    selection.into_iter().find_map(|i| item_file(app, i, accept, dir))
}

/// A composition's current frame, or footage's source file (else its first frame as an image).
pub fn item_file(app: &mut EffectcraftApp, item: ItemId, accept: &[&str], dir: &Path) -> Option<PathBuf> {
    let it = app.session.project.item(item)?;
    if matches!(it.kind, ItemKind::Comp(_)) {
        return comp_frame_file(app, item, accept, dir);
    }
    let ItemKind::Footage(footage) = &it.kind else { return None };
    let (name, footage) = (it.name.clone(), footage.clone());
    let src = PathBuf::from(&footage.path);
    if !footage.path.is_empty() && src.is_file() && accepts(accept, &src) {
        return Some(src);
    }
    if !footage.has_video {
        return None;
    }
    let img = app.session.footage.frame(item, &footage, Tick::ZERO)?;
    let rgba = img.to_rgba8();
    let stem = file_stem(Path::new(&name).file_stem().map_or(name.clone(), |s| s.to_string_lossy().into_owned()).as_str());
    accept.iter().find_map(|ext| {
        let bytes = match *ext {
            "png" => encode_png(&rgba, img.width, img.height),
            "jpg" | "jpeg" => encode_jpeg(&rgba, img.width, img.height),
            _ => None,
        }?;
        write(dir, &stem, ext, &bytes)
    })
}

/// A composition's frame at its current time, full size, in the first of `accept` EffectCraft
/// writes: a layered PSD, a PNG with the frame's alpha, an opaque JPEG or a multi-layer EXR.
pub fn comp_frame_file(app: &mut EffectcraftApp, comp: ItemId, accept: &[&str], dir: &Path) -> Option<PathBuf> {
    let stem = file_stem(&app.session.project.item(comp)?.name);
    let t = app.session.time_of(comp);
    for ext in accept {
        let path = match *ext {
            "png" => {
                app.session.render_rgba8_alpha(comp, t, 0, true).ok().and_then(|(w, h, px)| encode_png(&px, w, h)).and_then(|b| write(dir, &stem, ext, &b))
            }
            "jpg" | "jpeg" => app.session.render_rgba8(comp, t, 0).ok().and_then(|(w, h, px)| encode_jpeg(&px, w, h)).and_then(|b| write(dir, &stem, ext, &b)),
            "psd" => save_frame(&mut app.session, "comp.saveFrameAsPsd", comp, t, &unique_path(dir, &stem, ext)),
            "exr" => save_frame(&mut app.session, "comp.saveFrameAsExr", comp, t, &unique_path(dir, &stem, ext)),
            _ => None,
        };
        if path.is_some() {
            return path;
        }
    }
    None
}

/// Composition ▸ Save Frame As ▸ Photoshop Layers / ProEXR to `path`, without its toast.
fn save_frame(s: &mut Session, cmd: &str, comp: ItemId, t: Tick, path: &Path) -> Option<PathBuf> {
    let before = s.events.len();
    let r = s.execute(cmd, json!({"path": path.to_string_lossy(), "comp": comp.0, "time": t.seconds()}));
    let added = s.events.split_off(before.min(s.events.len()));
    s.events.extend(added.into_iter().filter(|e| !matches!(e, Event::Toast { .. })));
    match r {
        Ok(_) => Some(path.to_path_buf()),
        Err(e) => {
            log::warn!("{cmd}: {e}");
            None
        }
    }
}

/// Whether `path`'s extension is one of `accept` (jpg/jpeg and tif/tiff alike).
fn accepts(accept: &[&str], path: &Path) -> bool {
    let Some(ext) = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()) else { return false };
    let norm = |e: &str| match e {
        "jpeg" => "jpg".to_string(),
        "tiff" => "tif".to_string(),
        e => e.to_string(),
    };
    accept.iter().any(|a| norm(a) == norm(&ext))
}

/// A name usable as a file name (no path separators or characters Windows refuses).
fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').trim();
    if s.is_empty() { "Frame".into() } else { s.to_string() }
}

/// `dir/stem.ext`, numbered (`stem 2.ext`…) when that file exists.
fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() && n < 10_000 {
        path = dir.join(format!("{stem} {n}.{ext}"));
        n += 1;
    }
    path
}

fn write(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> Option<PathBuf> {
    let path = unique_path(dir, stem, ext);
    match std::fs::write(&path, bytes) {
        Ok(()) => Some(path),
        Err(e) => {
            log::warn!("cannot write {}: {e}", path.display());
            None
        }
    }
}

/// Straight 8-bit RGBA as PNG.
fn encode_png(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(rgba, w, h, image::ExtendedColorType::Rgba8).ok()?;
    Some(out)
}

/// Straight 8-bit RGBA as JPEG (transparent pixels over black).
fn encode_jpeg(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]].map(|c| (c as u16 * p[3] as u16 / 255) as u8)).collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92).write_image(&rgb, w, h, image::ExtendedColorType::Rgb8).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_comp() -> (EffectcraftApp, ItemId) {
        let mut s = Session::default();
        let comp = ItemId(s.execute("comp.new", json!({"name": "Main/Shot", "width": 8, "height": 4, "duration": 1})).unwrap()["comp"].as_u64().unwrap());
        s.execute("layer.newSolid", json!({"comp": comp.0, "color": "#ff0000", "width": 4, "height": 4})).unwrap();
        (EffectcraftApp::new(s), comp)
    }

    fn out_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("effectcraft-send-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A dragged composition becomes its current frame: a PNG with the frame's alpha (the solid
    /// covers half of it), a JPEG when that is what the other app takes, named after the comp.
    #[test]
    fn a_comp_goes_out_as_its_frame() {
        let (mut app, comp) = app_with_comp();
        let ctx = egui::Context::default();
        egui::DragAndDrop::set_payload(&ctx, DragPayload::Item(comp.0));
        let out = outgoing(&app, &ctx).unwrap();
        assert_eq!(label(&app, &out), "Composition “Main/Shot”");
        let dir = out_dir("comp");
        let files = files(&mut app, &out, &["svg", "png"], &dir);
        assert_eq!(files, vec![dir.join("Main_Shot.png")]);
        let img = image::open(&files[0]).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (8, 4));
        assert_eq!(img.get_pixel(0, 0)[3], 0, "transparent outside the solid");
        assert_eq!(img.get_pixel(4, 2).0, [255, 0, 0, 255]);
        let jpg = active_file(&mut app, &["jpg"], &dir).unwrap();
        assert_eq!(jpg, dir.join("Main_Shot.jpg"));
        assert_eq!(image::open(&jpg).unwrap().to_rgb8().dimensions(), (8, 4));
        // Photoshop takes the layered frame; a second send doesn't overwrite the first.
        assert_eq!(comp_frame_file(&mut app, comp, &["psd", "png"], &dir), Some(dir.join("Main_Shot.psd")));
        assert_eq!(comp_frame_file(&mut app, comp, &["psd"], &dir), Some(dir.join("Main_Shot 2.psd")));
        assert!(app.session.drain_events().iter().all(|e| !matches!(e, Event::Toast { .. })));
        assert_eq!(super::files(&mut app, &out, &["svg"], &dir), Vec::<PathBuf>::new());
    }

    /// Only Project items and Media Browser files go out; other drags (effects, properties)
    /// don't.
    #[test]
    fn only_project_items_and_files_drag_out() {
        let (app, _) = app_with_comp();
        let ctx = egui::Context::default();
        assert_eq!(outgoing(&app, &ctx), None);
        egui::DragAndDrop::set_payload(&ctx, DragPayload::Effect("ec.blur.gaussian".into()));
        assert_eq!(outgoing(&app, &ctx), None);
        egui::DragAndDrop::set_payload(&ctx, DragPayload::Files(vec!["/a/b.mov".into(), "/a/c.png".into()]));
        let out = outgoing(&app, &ctx).unwrap();
        assert_eq!(label(&app, &out), "2 files");
        assert!(accepts(&["jpeg"], Path::new("/x/A.JPG")) && !accepts(&["png"], Path::new("/x/a.mov")));
        assert_eq!(file_stem(" ..a:b. "), "a_b");
    }
}
