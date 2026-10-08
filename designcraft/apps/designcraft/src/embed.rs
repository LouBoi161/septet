//! DesignCraft as a tab inside a host eframe app (Septet). The host owns the window, the UI
//! fonts and the UI zoom ([`designcraft_ui_egui::hosted`]) and calls `logic` / `ui` only while the
//! tab is visible.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use designcraft_engine::DocState;
use designcraft_engine::doc::geom::{Affine, Point, Rect};
use designcraft_engine::doc::{Content, Document, ItemId, SpreadRef};
use designcraft_engine::tools::CanvasLayout;
use designcraft_render::{Placed, RenderOptions, Rendered, Renderer};
use designcraft_ui_egui::{DesignApp, UiState, canvas, hosted, panels, theme};
use serde_json::{Value, json};

/// Files dropped together at one point are placed this far apart (points, down and right).
const DROP_STEP: f64 = 18.0;
/// Pages and objects sent to other apps render at this resolution (pixels per inch)…
const EXPORT_PPI: f64 = 300.0;
/// …up to this many pixels on the longer side.
const EXPORT_MAX_SIDE: f64 = 4096.0;

pub struct Embedded {
    app: DesignApp,
    /// (uid, revision) of the dirty documents when recovery data was last written on hiding.
    recovery_written: Vec<(u64, u64)>,
    /// When the tab was hidden: each linked file's modification time (`None`: missing).
    link_times: HashMap<String, Option<SystemTime>>,
    /// Content being dragged out of the app, as of the last frame drawn.
    outgoing: Option<Outgoing>,
    /// A drag went to another app: forget it on the next frame.
    forget_drag: bool,
}

/// Content dragged out of the app.
enum Outgoing {
    /// A page from the Pages panel: document uid, absolute page index.
    Page { doc: u64, page: usize, label: String },
    /// Objects dragged off the canvas, in the document as it was when the drag began.
    Objects { doc: Arc<Document>, ids: Vec<ItemId>, label: String },
}

/// Where a drop at a screen point lands: a spread, the point in its coordinates, and the hit
/// tolerance there (3 screen points).
struct DropAt {
    spread: SpreadRef,
    at: Point,
    tolerance: f64,
}

impl Embedded {
    /// Build the app the way `main.rs` does for a plain launch with no arguments (crash recovery,
    /// desktop services, saved preferences), without the window, control server or native menu.
    pub fn new(ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>, storage: Option<&dyn eframe::Storage>) -> Self {
        hosted::set_hosted(true);
        // The canvas renders on the CPU, and the preferences live in DesignCraft's own config
        // folder, not in eframe storage.
        let _ = (ctx, render_state, storage);
        let mut app = crate::new_app();
        // The host draws the window chrome.
        app.integrated_titlebar = false;
        Embedded { app, recovery_written: Vec::new(), link_times: HashMap::new(), outgoing: None, forget_drag: false }
    }

    /// Before [`Embedded::new`]: keep every per-user file DesignCraft reads or writes under
    /// `root` (created when missing) instead of the per-user OS folders: `ui.json` and
    /// `prefs.json` in `root`, crash recovery in `root/Recovery`. `None` (the default) = the OS
    /// folders. DesignCraft has no other per-user state: libraries, books and exports go where the
    /// user saves them, fonts are read from the system's font folders.
    pub fn set_data_root(root: Option<PathBuf>) {
        hosted::set_data_root(root);
    }

    /// The UI fonts DesignCraft installs at start-up (default interface language), with its
    /// `semibold`, `arabic` and `arabic-semibold` families.
    pub fn font_definitions() -> egui::FontDefinitions {
        theme::ui_font_definitions(&UiState::default().language)
    }

    /// The active document's name (as its document tab shows it, without the `*`).
    pub fn document_title(&self) -> Option<String> {
        self.app.session.active().map(|d| d.title())
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.app.session.documents().iter().any(|d| d.is_dirty())
    }

    /// Open files as `designcraft FILE…` does.
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        for path in paths {
            let p = path.to_string_lossy().to_string();
            if let Err(e) = self.app.run("file.open", json!({"path": p})) {
                log::warn!("designcraft: {p}: {e}");
            }
        }
    }

    /// File › Place the files into the active document, as dropping them on the window does
    /// (`.designcraft` and `.idml` open as documents). With `at` over the canvas each file goes
    /// there: into the empty (or same-kind) frame under the point, else into a new frame whose
    /// top-left corner is the point; text gets a frame reaching the page's bottom-right margin.
    /// Without a document the files are opened.
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        // The point means the document shown now; opening a layout makes another one active.
        let shown = self.app.session.active().map(|d| d.uid);
        let drop = at.and_then(|at| drop_at(&mut self.app, at));
        let mut placed = 0;
        for path in paths {
            let p = path.to_string_lossy().to_string();
            let lower = p.to_ascii_lowercase();
            if self.app.session.active().is_none() || lower.ends_with(".designcraft") || lower.ends_with(".idml") {
                self.open_paths(std::slice::from_ref(path));
                continue;
            }
            let params = match &drop {
                Some(drop) if self.app.session.active().map(|d| d.uid) == shown => {
                    let params = place_params(&self.app, &p, drop, placed);
                    // A selected frame (or text insertion point) would take the file instead.
                    if self.app.session.active().is_some_and(|d| !d.selection.is_empty()) {
                        let _ = self.app.run("edit.deselectAll", json!({}));
                    }
                    placed += 1;
                    params
                }
                _ => json!({"path": p}),
            };
            if let Err(e) = self.app.run("file.place", params) {
                log::warn!("designcraft: {p}: {e}");
            }
        }
    }

    /// Nothing in DesignCraft plays or scrubs. Hiding notes when each linked file was last
    /// changed and writes the crash-recovery data of edited documents (the timer in `logic` stops
    /// while the tab is hidden, and the host process runs other apps). Showing again updates the
    /// links whose files changed meanwhile (edited in another app, e.g. through Edit Original).
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            self.update_changed_links();
            return;
        }
        self.link_times = self
            .app
            .session
            .documents()
            .iter()
            .flat_map(|d| d.doc.assets.values().filter_map(|a| a.link.clone()))
            .map(|path| {
                let time = modified(&path);
                (path, time)
            })
            .collect();
        if self.app.session.recovery_dir.is_none() {
            return;
        }
        let dirty: Vec<(u64, u64)> = self.app.session.documents().iter().filter(|d| d.is_dirty()).map(|d| (d.uid, d.revision)).collect();
        if dirty.is_empty() || dirty == self.recovery_written {
            return;
        }
        match self.app.session.execute("file.recovery.save", &json!({})) {
            Ok(_) => self.recovery_written = dirty,
            Err(e) => self.app.status(format!("Couldn't write recovery data: {e}")),
        }
    }

    /// `links.update` for the linked files changed since the tab was hidden, in every document.
    fn update_changed_links(&mut self) {
        let before = std::mem::take(&mut self.link_times);
        if before.is_empty() {
            return;
        }
        let changed: Vec<(usize, Vec<u64>)> = self
            .app
            .session
            .documents()
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let assets = d
                    .doc
                    .assets
                    .values()
                    .filter(|a| a.link.as_deref().is_some_and(|p| before.get(p).is_some_and(|then| modified(p) != *then)))
                    .map(|a| a.id.0)
                    .collect();
                (i, assets)
            })
            .filter(|(_, assets): &(usize, Vec<u64>)| !assets.is_empty())
            .collect();
        let active = self.app.session.active_index();
        for (i, assets) in changed {
            self.app.session.set_active(i);
            for asset in assets {
                if let Err(e) = self.app.run("links.update", json!({"asset": asset})) {
                    log::warn!("designcraft: updating a link: {e}");
                }
            }
        }
        if let Some(i) = active {
            self.app.session.set_active(i);
        }
    }

    /// Links panel › Edit Original opens the linked file through `handler` (the host opens it in
    /// a sibling app); when it returns false the status bar says the file couldn't be opened.
    pub fn set_open_externally(&mut self, mut handler: Box<dyn FnMut(&Path) -> bool>) {
        self.app.services.open_externally = Some(Box::new(move |path: &str| handler(Path::new(path))));
    }

    /// The sibling apps' buttons and links (About › More apps, `help.app`)
    /// call `handler` with the app's lowercase name instead of opening its web page.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        self.app.services.open_app = Some(handler);
    }

    /// A page dragged from the Pages panel ("Page 3"), or objects dragged off the canvas with
    /// the Selection tool ("Graphic “photo.jpg”", "Text frame", "3 objects").
    pub fn outgoing_drag(&self) -> Option<String> {
        self.outgoing.as_ref().map(|o| match o {
            Outgoing::Page { label, .. } | Outgoing::Objects { label, .. } => label.clone(),
        })
    }

    /// The dragged page as a PDF or a PNG / JPEG render; dragged objects as the original file of
    /// a placed graphic (or its embedded copy) when `accept` takes that kind of file, else as a
    /// PNG / JPEG render of the objects alone.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        let Some(drag) = self.outgoing.take() else { return Vec::new() };
        // The drop went to another app: this drag ends without its release.
        self.forget_drag = true;
        let file = match drag {
            Outgoing::Page { doc, page, .. } if self.app.session.active().is_some_and(|d| d.uid == doc) => self.export_page(page, accept, dir),
            Outgoing::Page { .. } => None,
            Outgoing::Objects { doc, ids, .. } => export_objects(&self.app, &doc, &ids, accept, dir),
        };
        file.into_iter().collect()
    }

    /// The drag ended in another app: a canvas move is undone and a Pages panel drag dropped
    /// (on the next frame, when the app's state is in place).
    pub fn cancel_outgoing_drag(&mut self) {
        if self.outgoing.take().is_some() {
            self.forget_drag = true;
        }
    }

    /// Send to another app: the current page (the one in the middle of the view) as a PDF or a
    /// PNG / JPEG render, whichever `accept` lists first.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        self.app.session.active()?;
        let page = canvas::current_page(&self.app).unwrap_or(0);
        self.export_page(page, accept, dir)
    }

    /// Page `abs` of the active document as the first format of `accept` it can write: PDF
    /// (File › Export PDF of that page), PNG or JPEG (a render at [`EXPORT_PPI`]).
    fn export_page(&mut self, abs: usize, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        let st = self.app.session.active()?;
        let name = format!("{} page {}", doc_stem(st), self.app.session.page_label(abs));
        for ext in accept {
            let path = unique_path(dir, &name, ext);
            let written = match *ext {
                "pdf" => self.app.run("file.exportPdf", json!({"path": path.to_string_lossy(), "pages": [abs + 1]})).map(|_| ()),
                "png" | "jpg" | "jpeg" => match render_page(&self.app, abs) {
                    Some(img) => std::fs::write(&path, encode(&img, ext)).map_err(|e| e.to_string()),
                    None => Err("the page can't be rendered".into()),
                },
                _ => continue,
            };
            match written {
                Ok(()) => return Some(path),
                Err(e) => log::warn!("designcraft: {}: {e}", path.display()),
            }
        }
        None
    }
}

/// The content dragged out of the app this frame: a Pages panel page, or objects the Selection
/// tool is moving (or duplicating) with the pointer off the canvas.
fn outgoing(app: &DesignApp, ctx: &egui::Context) -> Option<Outgoing> {
    let st = app.session.active()?;
    if let Some(page) = panels::pages::dragged_page(ctx) {
        return Some(Outgoing::Page { doc: st.uid, page, label: format!("Page {}", app.session.page_label(page)) });
    }
    let moving = st.interaction.as_ref().filter(|i| i.label == "Move" || i.label == "Duplicate")?;
    let (down, at) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
    if !down || at.is_none_or(|at| over_canvas(app, at)) || moving.selection.items.is_empty() {
        return None;
    }
    let ids = moving.selection.items.clone();
    Some(Outgoing::Objects { label: objects_label(&moving.doc, &ids), doc: moving.doc.clone(), ids })
}

/// `at` is over the canvas (either pane of a split window).
fn over_canvas(app: &DesignApp, at: egui::Pos2) -> bool {
    let rot = app.view().map_or(0, |v| v.rotation % 4);
    let other = if app.split { app.other_pane.as_ref().and_then(|(_, r)| *r) } else { None };
    [app.canvas_rect, other].into_iter().flatten().any(|r| canvas::view_rect(r, rot).contains(at))
}

fn objects_label(doc: &Document, ids: &[ItemId]) -> String {
    match ids {
        [id] => match doc.item(*id).map(|it| &it.content) {
            Some(Content::Graphic(g)) => doc.assets.get(&g.asset).map_or_else(|| "Graphic".to_string(), |a| format!("Graphic “{}”", a.name)),
            Some(Content::Text(_)) => "Text frame".into(),
            _ => "1 object".into(),
        },
        ids => format!("{} objects", ids.len()),
    }
}

/// Objects as the first file `accept` takes: one placed graphic's original file (or its
/// embedded bytes), else a PNG / JPEG render of the objects alone (transparent PNG).
fn export_objects(app: &DesignApp, doc: &Document, ids: &[ItemId], accept: &[&str], dir: &Path) -> Option<PathBuf> {
    let graphic = match ids {
        [id] => match doc.item(*id).map(|it| &it.content) {
            Some(Content::Graphic(g)) => doc.assets.get(&g.asset),
            _ => None,
        },
        _ => None,
    };
    if let Some(asset) = graphic {
        if let Some(link) = asset.link.as_deref()
            && Path::new(link).is_file()
            && Path::new(link).extension().is_some_and(|e| accepts(accept, &e.to_string_lossy()))
        {
            return Some(PathBuf::from(link));
        }
        if let Some(ext) = mime_extension(&asset.mime)
            && accepts(accept, ext)
        {
            let stem = Path::new(&asset.name).file_stem().map_or_else(|| asset.name.clone(), |s| s.to_string_lossy().to_string());
            let path = unique_path(dir, &stem, ext);
            match std::fs::write(&path, asset.data.as_slice()) {
                Ok(()) => return Some(path),
                Err(e) => log::warn!("designcraft: {}: {e}", path.display()),
            }
        }
    }
    let name = match graphic {
        Some(a) => Path::new(&a.name).file_stem().map_or_else(|| a.name.clone(), |s| s.to_string_lossy().to_string()),
        None => format!("{} objects", doc.title),
    };
    let ext = accept.iter().find(|e| matches!(**e, "png" | "jpg" | "jpeg"))?;
    let img = render_objects(app, doc, ids)?;
    let path = unique_path(dir, &name, ext);
    match std::fs::write(&path, encode(&img, ext)) {
        Ok(()) => Some(path),
        Err(e) => {
            log::warn!("designcraft: {}: {e}", path.display());
            None
        }
    }
}

/// Render scale (pixels per point) for `w` × `h` points.
fn export_scale(w: f64, h: f64) -> f64 {
    (EXPORT_PPI / 72.0).min(EXPORT_MAX_SIDE / w.max(h).max(1.0))
}

fn render_options(app: &DesignApp) -> RenderOptions {
    RenderOptions { printing_only: true, rich_black: app.session.prefs.rich_black_output, ..Default::default() }
}

/// Page `abs` of the active document on white, as File › Export Page as PNG renders it.
fn render_page(app: &DesignApp, abs: usize) -> Option<Rendered> {
    let st = app.session.active()?;
    let (si, pi) = st.doc.page_loc(abs)?;
    let page = st.doc.spreads.get(si)?.pages.get(pi)?;
    Renderer::new().render_page(&st.doc, &app.session.cache, abs, export_scale(page.width, page.height), false, &render_options(app))
}

/// The objects (their top-level objects, on the first one's spread) alone on a transparent
/// background, cropped to their visible bounds.
fn render_objects(app: &DesignApp, doc: &Document, ids: &[ItemId]) -> Option<Rendered> {
    let mut spread = None;
    let mut tops: Vec<ItemId> = Vec::new();
    let mut bounds: Option<Rect> = None;
    for id in ids {
        let Some(loc) = doc.find(*id) else { continue };
        if spread.is_some_and(|s| s != loc.spread) {
            continue;
        }
        let Some(top) = loc.path.first().and_then(|i| doc.spread(loc.spread)?.items.get(*i)) else { continue };
        spread = Some(loc.spread);
        if !tops.contains(&top.id) {
            tops.push(top.id);
            let b = top.visible_bounds();
            bounds = Some(bounds.map_or(b, |r| r.union(b)));
        }
    }
    let (spread, r) = (spread?, bounds?.inflate(2.0, 2.0));
    let others = doc.spread(spread)?.items.iter().chain(doc.parents.iter().flat_map(|p| p.items.iter()));
    let hidden: Vec<ItemId> = others.map(|it| it.id).filter(|id| !tops.contains(id)).collect();
    let scale = export_scale(r.width(), r.height());
    let (w, h) = ((r.width() * scale).ceil().max(1.0) as u32, (r.height() * scale).ceil().max(1.0) as u32);
    let view = Affine::scale(scale) * Affine::translate(-r.origin().to_vec2());
    let opts = RenderOptions { paper: false, hidden, ..render_options(app) };
    Some(Renderer::new().render(doc, &app.session.cache, &[Placed { spread, xf: Affine::IDENTITY }], w, h, view, &opts))
}

fn encode(img: &Rendered, ext: &str) -> Vec<u8> {
    if ext == "png" { img.to_png() } else { img.to_jpeg(92) }
}

/// The file extension for a placed graphic's MIME type.
fn mime_extension(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/tiff" => "tif",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "image/vnd.adobe.photoshop" => "psd",
        "application/pdf" => "pdf",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        _ => return None,
    })
}

/// `accept` lists extension `ext` (jpg and jpeg, tif and tiff are the same).
fn accepts(accept: &[&str], ext: &str) -> bool {
    let same = |e: &str| match e.to_ascii_lowercase().as_str() {
        "jpeg" => "jpg".to_string(),
        "tiff" => "tif".to_string(),
        e => e.to_string(),
    };
    accept.iter().any(|a| same(a) == same(ext))
}

fn modified(path: &str) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The document's name without the extension.
fn doc_stem(st: &DocState) -> String {
    let title = st.title();
    Path::new(&title).file_stem().map_or(title.clone(), |s| s.to_string_lossy().to_string())
}

/// `dir/name.ext`, numbered (`name 2.ext` …) when taken; the name made safe for a file name.
fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let safe: String = name.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '-' } else { c }).collect();
    let safe = safe.trim().trim_start_matches('.');
    let stem = if safe.is_empty() { "DesignCraft" } else { safe };
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() && n < 10_000 {
        path = dir.join(format!("{stem} {n}.{ext}"));
        n += 1;
    }
    path
}

/// The spread under screen point `at` on the active document's canvas (either pane of a split
/// window; the second window is another viewport), if the canvas has been shown.
fn drop_at(app: &mut DesignApp, at: egui::Pos2) -> Option<DropAt> {
    app.session.active()?;
    let current = app.pane;
    let panes: &[u8] = if app.split { &[0, 1] } else { &[0] };
    let mut found = None;
    for &pane in panes {
        app.switch_pane(pane);
        let (Some(rect), Some(view)) = (app.canvas_rect, app.view().copied()) else { continue };
        let xf = canvas::Xf::new(rect, &view);
        // `fitted` is set when the view is first shown; before that `canvas_rect` belongs to
        // another document.
        if !view.fitted || !canvas::view_rect(rect, xf.rot).contains(at) {
            continue;
        }
        found = app.session.active().and_then(|st| {
            let (spread, at) = CanvasLayout::new(&st.doc, st.editing_parents).spread_at(xf.to_canvas(at))?;
            Some(DropAt { spread, at, tolerance: 3.0 / view.zoom })
        });
        break;
    }
    app.switch_pane(current);
    found
}

/// `file.place` params for the `index`th file placed at `drop` (each one [`DROP_STEP`] further
/// down and right; only the first goes into the frame under the point).
fn place_params(app: &DesignApp, path: &str, drop: &DropAt, index: u32) -> Value {
    let offset = f64::from(index) * DROP_STEP;
    let pt = Point::new(drop.at.x + offset, drop.at.y + offset);
    let mut params = json!({"path": path, "spread": drop.spread, "x": pt.x, "y": pt.y});
    let (Some(st), SpreadRef::Doc(si)) = (app.session.active(), drop.spread) else { return params };
    let text = designcraft_textimport::is_text_file(path);
    // Into the frame under the point when it is empty or holds the same kind of content.
    if index == 0
        && let Some(id) = st.doc.hit_item(si, pt, drop.tolerance)
        && let Some(item) = st.doc.item(id)
        && match item.content {
            Content::Unassigned => true,
            Content::Text(_) => text,
            Content::Graphic(_) => !text,
            _ => false,
        }
    {
        params["frame"] = json!(id.0);
    }
    // Text without a frame: a new one from the point to the page's bottom-right margin corner.
    if text
        && params.get("frame").is_none()
        && let Some(sp) = st.doc.spreads.get(si)
        && let Some(pi) = sp.pages.iter().position(|pg| pg.bounds().contains(pt)).or_else(|| sp.page_at_x(pt.x))
        && let Some(page) = sp.pages.get(pi)
    {
        let m = page.margin_rect();
        params["page"] = json!(st.doc.first_page_of_spread(si) + pi + 1);
        params["rect"] = json!([pt.x, pt.y, m.x1.max(pt.x + 72.0), m.y1.max(pt.y + 72.0)]);
    }
    params
}

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.forget_drag) {
            self.app.forget_drag(ctx);
        }
        self.app.logic(ctx);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        self.outgoing = outgoing(&self.app, ui.ctx());
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }
    /// DesignCraft keeps its state in its own config folder ([`crate::prefs`]), not in eframe
    /// storage.
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
    fn on_exit(&mut self) {
        crate::prefs::save_prefs(&self.app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_engine::tools::{PointerEvent, PointerKind};

    /// A new document on an 800 × 600 canvas at (100, 100), fitted as the canvas shows it.
    fn shown() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), designcraft_ui_egui::Services::default());
        app.run("file.new", json!({})).unwrap();
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(800.0, 600.0));
        app.canvas_rect = Some(rect);
        canvas::fit(&mut app, rect, "spread");
        app
    }

    /// The screen point of point (x, y) of the first spread.
    fn screen(app: &DesignApp, x: f64, y: f64) -> egui::Pos2 {
        let st = app.session.active().unwrap();
        let xf = canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        xf.to_screen(CanvasLayout::new(&st.doc, false).to_canvas(SpreadRef::Doc(0), Point::new(x, y)))
    }

    /// A drop at point (x, y) of the first spread.
    fn drop_on(app: &mut DesignApp, x: f64, y: f64) -> DropAt {
        let at = screen(app, x, y);
        drop_at(app, at).unwrap()
    }

    #[test]
    fn a_drop_on_the_canvas_places_at_that_point() {
        let mut app = shown();
        let drop = drop_on(&mut app, 100.0, 120.0);
        assert_eq!(drop.spread, SpreadRef::Doc(0));
        let p = place_params(&app, "/photos/photo.png", &drop, 0);
        assert!((p["x"].as_f64().unwrap() - 100.0).abs() < 0.01 && (p["y"].as_f64().unwrap() - 120.0).abs() < 0.01, "{p}");
        assert!(p.get("frame").is_none(), "{p}");
        // Each further file of the drop lands a step down and right.
        let next = place_params(&app, "/photos/next.png", &drop, 1);
        assert!((next["x"].as_f64().unwrap() - 100.0 - DROP_STEP).abs() < 0.01, "{next}");
        // Off the canvas there is no point.
        assert!(drop_at(&mut app, egui::pos2(50.0, 50.0)).is_none());
    }

    #[test]
    fn a_drop_on_an_empty_frame_fills_it() {
        let mut app = shown();
        let id = app.run("frame.create", json!({"rect": [72, 72, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        let drop = drop_on(&mut app, 150.0, 150.0);
        assert_eq!(place_params(&app, "a.png", &drop, 0)["frame"], json!(id));
        assert_eq!(place_params(&app, "notes.txt", &drop, 0)["frame"], json!(id));
        // The second file of the drop gets a frame of its own.
        assert!(place_params(&app, "b.png", &drop, 1).get("frame").is_none());
    }

    #[test]
    fn dropped_text_gets_a_frame_from_the_point_to_the_margins() {
        let mut app = shown();
        let id = app.run("frame.create", json!({"rect": [300, 300, 400, 400], "content": "text", "caret": false})).unwrap()["id"].as_u64().unwrap();
        let drop = drop_on(&mut app, 100.0, 120.0);
        let p = place_params(&app, "notes.txt", &drop, 0);
        assert_eq!(p["page"], json!(1), "{p}");
        let margins = app.session.active().unwrap().doc.spreads[0].pages[0].margin_rect();
        let rect: Vec<f64> = p["rect"].as_array().unwrap().iter().filter_map(Value::as_f64).collect();
        for (got, want) in rect.iter().zip([100.0, 120.0, margins.x1, margins.y1]) {
            assert!((got - want).abs() < 0.01, "{p}");
        }
        // An image dropped on a text frame doesn't replace its text.
        let on_text = drop_on(&mut app, 350.0, 350.0);
        assert!(place_params(&app, "a.png", &on_text, 0).get("frame").is_none());
        assert_eq!(place_params(&app, "more.txt", &on_text, 0)["frame"], json!(id));
    }

    fn embedded(app: DesignApp) -> Embedded {
        Embedded { app, recovery_written: Vec::new(), link_times: HashMap::new(), outgoing: None, forget_drag: false }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("designcraft-embed-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        Rendered { width: w, height: h, pixels: vec![200; (w * h * 4) as usize] }.to_png()
    }

    /// The canvas point of point (x, y) of the first spread.
    fn on_canvas(app: &DesignApp, x: f64, y: f64) -> Point {
        CanvasLayout::new(&app.session.active().unwrap().doc, false).to_canvas(SpreadRef::Doc(0), Point::new(x, y))
    }

    #[test]
    fn sibling_app_pages_name_the_app() {
        use designcraft_ui_egui::about::sibling_app;
        assert_eq!(sibling_app("https://getartcraft.com/apps/photocraft"), Some("photocraft"));
        assert_eq!(sibling_app("https://getartcraft.com/apps/drawcraft"), Some("vectorcraft"));
        for (slug, _, _) in designcraft_ui_egui::about::SIBLINGS {
            assert!(sibling_app(&format!("https://getartcraft.com/apps/{slug}")).is_some(), "{slug}");
        }
        // DesignCraft's own page and the community links stay web links.
        assert_eq!(sibling_app(designcraft_engine::links::APP_PAGE), None);
        assert_eq!(sibling_app(designcraft_engine::links::DISCORD), None);
        assert_eq!(sibling_app(designcraft_engine::links::GITHUB), None);
    }

    #[test]
    fn the_current_page_is_sent_as_pdf_or_a_render() {
        let dir = temp_dir("page");
        let mut e = embedded(shown());
        let pdf = e.export_active(&["svg", "pdf", "png"], &dir).unwrap();
        assert_eq!(pdf.extension().unwrap(), "pdf");
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        let png = e.export_active(&["psd", "tif", "png"], &dir).unwrap();
        assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
        // A second send doesn't overwrite the first.
        let again = e.export_active(&["png"], &dir).unwrap();
        assert_ne!(again, png);
        assert!(e.export_active(&["mp4"], &dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dragged_graphics_go_as_their_files_and_other_objects_as_renders() {
        let dir = temp_dir("objects");
        let mut app = shown();
        let bytes = png(10, 10);
        let b64 = designcraft_engine::cmd::base64_encode(&bytes);
        let embedded_id =
            ItemId(app.run("file.place", json!({"base64": b64, "name": "dot.png", "x": 100, "y": 100})).unwrap()["id"].as_u64().unwrap());
        let original = dir.join("photo.png");
        std::fs::write(&original, png(12, 8)).unwrap();
        // (A selected graphic frame would take the next placed file.)
        app.run("edit.deselectAll", json!({})).unwrap();
        let linked_id =
            ItemId(app.run("file.place", json!({"path": original.to_string_lossy(), "x": 300, "y": 100})).unwrap()["id"].as_u64().unwrap());
        let text_id = ItemId(
            app.run("frame.create", json!({"rect": [72, 400, 300, 450], "content": "text", "text": "Hello", "caret": false})).unwrap()["id"]
                .as_u64()
                .unwrap(),
        );
        let doc = app.session.active().unwrap().doc.clone();
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        // The embedded image's own bytes; the linked image's original file.
        let file = export_objects(&app, &doc, &[embedded_id], &["png"], &out).unwrap();
        assert_eq!(file.file_name().unwrap(), "dot.png");
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        assert_eq!(export_objects(&app, &doc, &[linked_id], &["psd", "png"], &out).unwrap(), original);
        // A target that doesn't take PNG files gets a render.
        let jpg = export_objects(&app, &doc, &[linked_id], &["jpeg"], &out).unwrap();
        assert!(std::fs::read(&jpg).unwrap().starts_with(&[0xff, 0xd8]));
        let text = export_objects(&app, &doc, &[text_id], &["png"], &out).unwrap();
        assert!(std::fs::read(&text).unwrap().starts_with(b"\x89PNG"));
        assert!(export_objects(&app, &doc, &[text_id], &["pdf"], &out).is_none());
        assert_eq!(objects_label(&doc, &[embedded_id]), "Graphic “dot.png”");
        assert_eq!(objects_label(&doc, &[embedded_id, text_id]), "2 objects");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn links_edited_while_hidden_update_when_shown() {
        let dir = temp_dir("links");
        let original = dir.join("photo.png");
        std::fs::write(&original, png(12, 8)).unwrap();
        let mut e = embedded(shown());
        e.app.run("file.place", json!({"path": original.to_string_lossy(), "x": 100, "y": 100})).unwrap();
        let pixels = |e: &Embedded| e.app.session.active().unwrap().doc.assets.values().next().unwrap().pixels;
        e.set_visible(false);
        // Edited in another app.
        std::fs::write(&original, png(20, 10)).unwrap();
        let later = SystemTime::now() + std::time::Duration::from_secs(10);
        std::fs::File::options().write(true).open(&original).unwrap().set_modified(later).unwrap();
        assert_eq!(pixels(&e), Some((12, 8)));
        e.set_visible(true);
        assert_eq!(pixels(&e), Some((20, 10)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn objects_dragged_off_the_canvas_go_out_and_come_back_when_dropped_elsewhere() {
        let mut app = shown();
        let id = ItemId(app.run("frame.create", json!({"rect": [72, 72, 172, 172]})).unwrap()["id"].as_u64().unwrap());
        let bounds = |app: &DesignApp| app.session.active().unwrap().doc.item(id).unwrap().bounds();
        let before = bounds(&app);
        let vi = app.view_info();
        for (kind, x, y) in [(PointerKind::Down, 100.0, 100.0), (PointerKind::Drag, 130.0, 100.0), (PointerKind::Drag, 400.0, 300.0)] {
            app.session.pointer(&PointerEvent { kind, pos: on_canvas(&app, x, y), mods: Default::default() }, vi).unwrap();
        }
        assert_ne!(bounds(&app), before);
        let ctx = egui::Context::default();
        let pressed_at = |ctx: &egui::Context, at: egui::Pos2| {
            let events = vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            ];
            ctx.begin_pass(egui::RawInput { events, ..Default::default() });
        };
        // Over the canvas the move is the app's own.
        pressed_at(&ctx, egui::pos2(500.0, 400.0));
        assert!(outgoing(&app, &ctx).is_none());
        ctx.end_pass().textures_delta.clear();
        // Off the canvas it goes out.
        pressed_at(&ctx, egui::pos2(20.0, 20.0));
        let mut e = embedded(app);
        e.outgoing = outgoing(&e.app, &ctx);
        ctx.end_pass().textures_delta.clear();
        assert_eq!(e.outgoing_drag().as_deref(), Some("1 object"));
        // Dropped in another app: the objects stay where they were.
        e.cancel_outgoing_drag();
        assert!(e.outgoing_drag().is_none());
        ctx.begin_pass(egui::RawInput::default());
        if std::mem::take(&mut e.forget_drag) {
            e.app.forget_drag(&ctx);
        }
        ctx.end_pass().textures_delta.clear();
        assert!(e.app.session.active().unwrap().interaction.is_none());
        assert_eq!(bounds(&e.app), before);
    }

    #[test]
    fn hosted_menus_leave_out_the_community_links() {
        use designcraft_ui_egui::menus::{Item, menu_tree};
        fn check(items: &[Item], menu: &str) {
            assert!(!matches!(items.first(), Some(Item::Sep)) && !matches!(items.last(), Some(Item::Sep)), "{menu}: separator at an end");
            for (i, it) in items.iter().enumerate() {
                match it {
                    Item::Cmd { id, .. } => assert!(!hosted::is_community_command(id), "{menu}: {id}"),
                    Item::Sub(name, children) => check(children, name),
                    Item::Sep => assert!(!matches!(items.get(i + 1), Some(Item::Sep)), "{menu}: doubled separator"),
                }
            }
        }
        hosted::set_hosted(true);
        let tree = menu_tree();
        for (menu, items) in &tree {
            check(items, menu);
        }
        let help = &tree.iter().find(|(m, _)| *m == "Help").unwrap().1;
        assert!(help.iter().any(|it| matches!(it, Item::Cmd { id, .. } if id == "help.github")), "the source repository stays");
    }

    #[test]
    fn the_about_box_shows_the_app_icon() {
        hosted::set_hosted(true);
        let ctx = egui::Context::default();
        ctx.set_fonts(Embedded::font_definitions());
        let mut app = DesignApp::new(designcraft_engine::Session::new(), designcraft_ui_egui::Services::default());
        app.ui.about = true;
        for _ in 0..4 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            out.textures_delta.clear();
        }
        let names: Vec<String> = ctx.tex_manager().read().allocated().map(|(_, meta)| meta.name.clone()).collect();
        assert!(names.iter().any(|n| n == "designcraft_app_icon"), "{names:?}");
    }

    #[test]
    fn hosted_app_leaves_fonts_and_zoom_to_the_host() {
        let fonts = Embedded::font_definitions();
        for family in ["semibold", "arabic", "arabic-semibold"] {
            assert!(fonts.families.contains_key(&egui::FontFamily::Name(family.into())), "{family}");
        }
        assert_eq!(fonts.families[&egui::FontFamily::Proportional].first().map(String::as_str), Some("ui"));
        // The host installs its fonts (DesignCraft's among them) and owns the zoom.
        hosted::set_hosted(true);
        let ctx = egui::Context::default();
        let mut host = fonts;
        host.families.insert(egui::FontFamily::Name("host".into()), vec!["ui".into()]);
        ctx.set_fonts(host);
        let mut app = DesignApp::new(designcraft_engine::Session::new(), designcraft_ui_egui::Services::default());
        app.ui.ui_scale = 2.0;
        for frame in 0..4 {
            if frame == 2 {
                // A language switch reinstalls the fonts standalone.
                app.ui.language = "zh".into();
            }
            ctx.begin_pass(egui::RawInput::default());
            app.logic(&ctx);
            if frame == 3 {
                assert!(ctx.fonts(|f| f.definitions().families.contains_key(&egui::FontFamily::Name("host".into()))));
            }
            ctx.end_pass().textures_delta.clear();
        }
        assert_eq!(ctx.zoom_factor(), 1.0);
    }
}
