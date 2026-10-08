//! Things that cross app boundaries: the clipboard between apps, and notifications.
//!
//! Copy and paste go through the system clipboard, so they work with every other program too. Each
//! app pastes what it understands (Photocraft images, Vectorcraft SVG and images). When an app is
//! asked to paste something it can't read itself — an image in Designcraft, Pdfcraft, Filmcraft or
//! Effectcraft, Vectorcraft's SVG in Photocraft, files copied in the file manager — the bridge turns
//! the clipboard into a file and places it, the way the app places a dropped file.

use std::path::PathBuf;

use egui::{Color32, Context, Event, Id, Key, Rect, ViewportId, pos2, vec2};

use crate::hosted::AppSlot;
use crate::kinds::AppKind;
use crate::theme;

struct Toast {
    text: String,
    until: f64,
}

pub struct Bridge {
    toasts: Vec<Toast>,
    /// The app the last copy/cut happened in (its own clipboard may hold richer data than the system's).
    clip_owner: Option<AppKind>,
    clipboard: Option<arboard::Clipboard>,
}

/// What the system clipboard holds, as far as the bridge cares.
enum Clip {
    Files(Vec<PathBuf>),
    Svg(String),
    Image(arboard::ImageData<'static>),
}

impl Bridge {
    pub fn new(_ctx: &Context) -> Self {
        Bridge { toasts: Vec::new(), clip_owner: None, clipboard: None }
    }

    /// A short message at the bottom of the focused window.
    pub fn notify(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), until: f64::NAN });
    }

    pub fn logic(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        for t in &mut self.toasts {
            if t.until.is_nan() {
                t.until = now + 4.0;
            }
        }
        self.toasts.retain(|t| t.until > now);
        if let Some(next) = self.toasts.iter().map(|t| t.until).reduce(f64::min) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((next - now).max(0.05)));
        }
    }

    /// Before `slot`'s app sees this frame's input (its state swapped in): note copies, and paste
    /// what it can't paste itself.
    pub fn before_app(&mut self, ctx: &Context, slot: &mut AppSlot) {
        let kind = slot.kind;
        let (copy, paste) = ctx.input(|i| {
            let cmd = |m: &egui::Modifiers| m.command && !m.alt;
            let copy = i.events.iter().any(|e| match e {
                Event::Copy | Event::Cut => true,
                Event::Key { key: Key::C | Key::X, pressed: true, modifiers, .. } => cmd(modifiers),
                _ => false,
            });
            let paste = i.events.iter().any(|e| match e {
                Event::Paste(_) => true,
                Event::Key { key: Key::V, pressed: true, modifiers, .. } => cmd(modifiers) && !modifiers.shift,
                _ => false,
            });
            (copy, paste)
        });
        if copy {
            self.clip_owner = Some(kind);
        }
        if !paste || ctx.text_edit_focused() || self.clip_owner == Some(kind) {
            return;
        }
        // Lightcraft pastes develop settings; nothing to translate for it.
        if kind == AppKind::Lightcraft || kind == AppKind::Vectorcraft {
            return;
        }
        let Some(clip) = self.read_clipboard() else { return };
        let file = match (&clip, kind) {
            // Photocraft reads images and copied files itself.
            (Clip::Image(_) | Clip::Files(_), AppKind::Photocraft) => return,
            (Clip::Files(files), _) => {
                take_paste(ctx);
                slot.app.place_paths(files, None);
                return;
            }
            (Clip::Svg(svg), AppKind::Designcraft | AppKind::Effectcraft) => save_pasted("svg", svg.as_bytes()),
            (Clip::Svg(svg), _) => rasterize_svg(svg).and_then(|png| save_pasted("png", &png)),
            (Clip::Image(img), _) => encode_png(img).and_then(|png| save_pasted("png", &png)),
        };
        match file {
            Some(path) => {
                take_paste(ctx);
                slot.app.place_paths(&[path], None);
            }
            None => self.notify("Couldn't paste the clipboard here"),
        }
    }

    fn read_clipboard(&mut self) -> Option<Clip> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let cb = self.clipboard.as_mut()?;
        if let Ok(files) = cb.get().file_list()
            && !files.is_empty()
        {
            return Some(Clip::Files(files));
        }
        if let Ok(text) = cb.get_text() {
            let head: String = text.chars().take(1024).collect();
            if head.contains("<svg") {
                return Some(Clip::Svg(text));
            }
            return None;
        }
        cb.get_image().ok().map(|i| Clip::Image(i.to_owned_img()))
    }

    pub fn window_ui(&mut self, ctx: &Context, viewport: ViewportId) {
        if self.toasts.is_empty() || !ctx.input(|i| i.viewport().focused.unwrap_or(viewport == ViewportId::ROOT)) {
            return;
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("septet-toasts")));
        let mut y = screen.bottom() - 28.0;
        for t in self.toasts.iter().rev() {
            let galley = painter.layout_no_wrap(t.text.clone(), theme::medium(12.5), Color32::WHITE);
            let size = galley.size() + vec2(28.0, 16.0);
            let rect = Rect::from_center_size(pos2(screen.center().x, y - size.y / 2.0), size);
            painter.add(egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(90) }.as_shape(rect, 8.0));
            painter.rect_filled(rect, 8.0, Color32::from_rgb(0x2b, 0x2b, 0x30));
            painter.galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
            y -= size.y + 8.0;
        }
    }
}

/// Keep the paste gesture from the app (the bridge handled it).
fn take_paste(ctx: &Context) {
    let is_paste = |e: &Event| match e {
        Event::Paste(_) => true,
        Event::Key { key: Key::V, modifiers, .. } => modifiers.command,
        _ => false,
    };
    ctx.input_mut(|i| {
        i.events.retain(|e| !is_paste(e));
        i.raw.events.retain(|e| !is_paste(e));
    });
}

/// Pasted content lives on (layout apps link placed files rather than copying them).
fn pasted_dir() -> Option<PathBuf> {
    crate::recent::data_dir("Pasted")
}

fn save_pasted(ext: &str, bytes: &[u8]) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = pasted_dir()?.join(format!("Pasted {stamp}.{ext}"));
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

fn encode_png(img: &arboard::ImageData<'_>) -> Option<Vec<u8>> {
    let buf = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    buf.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// SVG to PNG for apps that only take pixels, at least 1024 px on the long side.
fn rasterize_svg(svg: &str) -> Option<Vec<u8>> {
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &opt).ok()?;
    let size = tree.size();
    let scale = (1024.0 / size.width().max(size.height())).clamp(1.0, 8.0);
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}
