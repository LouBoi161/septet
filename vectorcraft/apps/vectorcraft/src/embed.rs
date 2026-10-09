//! VectorCraft as a tab of another app's window (Septet): [`Embedded`] builds the app as the
//! desktop app does, minus everything that belongs to the process or the window, which the host
//! owns (logging, panic hooks, the control server, the macOS menu bar and Apple events, the
//! graphics adapter's choice and its loss, the window's size, title bar, fonts and zoom).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use serde_json::{Value, json};
use vectorcraft_engine::cmd::fileio;
use vectorcraft_engine::doc::{Document, NodeId};
use vectorcraft_engine::{Session, guard};
use vectorcraft_render::{RenderOptions, Renderer};
use vectorcraft_ui_egui::VectorcraftApp;
use vectorcraft_ui_egui::outgoing::Outgoing;
use vectorcraft_ui_egui::place::DropTarget;

use crate::{desktop, prefs};

/// A picture for the host's agent, made on a worker thread ([`Embedded::agent_render`]).
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

/// What the agent hears when there is nothing to work on.
const NO_DOCUMENT: &str = "No document is open in Vectorcraft: open one with septet_open, or make one with app_execute `file.new` {width, height}.";

/// The host's hook for files the app opens in another app ([`Embedded::set_open_externally`]).
type OpenExternally = Box<dyn FnMut(&Path) -> bool>;

/// The host's hook for links to the other ArtCraft apps ([`Embedded::set_open_app`]).
type OpenApp = Box<dyn FnMut(&str)>;

/// The other ArtCraft apps, by the name their page on getartcraft.com has.
const SIBLING_APPS: [&str; 6] = ["photocraft", "lightcraft", "designcraft", "pdfcraft", "filmcraft", "effectcraft"];

/// VectorCraft in a host's tab.
pub struct Embedded {
    app: VectorcraftApp,
    /// The host's context, which egui's drag payloads belong to.
    ctx: egui::Context,
    /// Asked first when Edit Original opens a file in its default app.
    open_externally: Rc<RefCell<Option<OpenExternally>>>,
    /// Asked instead of the browser for another ArtCraft app's page.
    open_app: Rc<RefCell<Option<OpenApp>>>,
    /// The last drag of art out of the app seen while the tab showed: the host asks for its files
    /// once it is dropped on another app, when the app may no longer be drawn.
    outgoing: Option<Outgoing>,
}

impl Embedded {
    /// The app as a plain launch makes it (no files): its preferences, its libraries and Data
    /// Recovery folders next to them and the desktop's services, inside the host's window.
    pub fn new(ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>, _storage: Option<&dyn eframe::Storage>) -> Self {
        vectorcraft_ui_egui::hosted::set_hosted(true);
        vectorcraft_ui_egui::i18n::detect_system_lang_in_background();
        let open_externally: Rc<RefCell<Option<OpenExternally>>> = Rc::default();
        let mut services = desktop::services();
        let hook = open_externally.clone();
        // Edit Original: the host may open the file itself (in another of its tabs). A folder
        // (Show Package) always opens in the file manager.
        services.open_file = Some(Box::new(move |path: &str| {
            let file = Path::new(path);
            let handled = !file.is_dir() && hook.try_borrow_mut().ok().is_some_and(|mut h| h.as_mut().is_some_and(|handler| handler(file)));
            if handled { Ok(()) } else { desktop::open_file(path) }
        }));
        let open_app: Rc<RefCell<Option<OpenApp>>> = Rc::default();
        let apps = open_app.clone();
        // Another ArtCraft app's page: the host may show that app instead (its tab).
        services.open_url = Some(Box::new(move |url: &str| {
            let handled = sibling_app(url)
                .is_some_and(|name| apps.try_borrow_mut().ok().is_some_and(|mut h| h.as_mut().map(|handler| handler(name)).is_some()));
            if !handled {
                desktop::open_url(url);
            }
        }));
        let mut app = VectorcraftApp::new(Session::new(), services);
        prefs::load_prefs(&mut app, prefs::read_prefs());
        desktop::set_user_folders(&mut app);
        // The device is the host's, shared by every app it shows: it handles losing it.
        if let Some(rs) = render_state {
            app.graphics_adapter = Some(desktop::adapter_summary(&rs.adapter.get_info()));
        }
        // The host draws the window's title bar, caption buttons and edges.
        app.integrated_titlebar = false;
        app.custom_titlebar = false;
        Self { app, ctx: ctx.clone(), open_externally, open_app, outgoing: None }
    }

    /// Before [`Self::new`]: keep everything VectorCraft stores per user (the UI preferences, the
    /// User Defined swatch and graphic style libraries, Data Recovery copies, the default Templates
    /// folder) under `root`, made if missing, instead of the per-user OS folders. `None` (the
    /// default) uses those.
    pub fn set_data_root(root: Option<PathBuf>) {
        vectorcraft_ui_egui::hosted::set_data_root(root);
    }

    /// The fonts the app installs at startup, with its `ui`, `ui-semibold` and `mono` families.
    pub fn font_definitions() -> egui::FontDefinitions {
        vectorcraft_ui_egui::theme::font_definitions()
    }

    /// The active document's name (none on the Home screen with no document open).
    pub fn document_title(&self) -> Option<String> {
        self.app.session.active().map(|st| st.title())
    }

    /// Is any open document modified?
    pub fn has_unsaved_changes(&self) -> bool {
        vectorcraft_ui_egui::unsaved::any_dirty(&self.app)
    }

    /// Open `paths` as documents, as the command line does.
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        desktop::open_files(&mut self.app, paths.iter().map(|p| p.to_string_lossy().into_owned()).collect());
    }

    /// File → Place `paths` into the active document, linked, as files dropped on its canvas are:
    /// at `at` (screen points) when that is over the canvas, else in the middle of the view. With
    /// no document open they open as documents.
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        if self.app.session.active().is_none() {
            self.open_paths(paths);
            return;
        }
        let at = at.and_then(|pos| match self.app.drop_target("", Some(pos), false) {
            DropTarget::Place(d) => Some(d.at),
            DropTarget::Open => None,
        });
        for path in paths {
            let path = path.to_string_lossy().into_owned();
            let mut p = json!({ "path": path, "link": true });
            if let Some(at) = at {
                p["at"] = json!([at.x, at.y]);
            }
            if let Err(e) = vectorcraft_ui_egui::place::run(&mut self.app, &p) {
                self.app.status(format!("Couldn't place {}: {e}", fileio::file_name(&path)));
            }
        }
    }

    /// The host showed (`true`) or hid (`false`) the app's tab: as the window regaining or losing
    /// the focus (fonts installed meanwhile are listed).
    pub fn set_visible(&mut self, visible: bool) {
        self.app.set_visible(visible);
    }

    /// Ask `handler` first when the app opens a file in another app (Links panel › Edit Original):
    /// `true` means the host opened it.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        if let Ok(mut hook) = self.open_externally.try_borrow_mut() {
            *hook = Some(handler);
        }
    }

    /// Links to another ArtCraft app's page call `handler` with its name (`photocraft`…) instead
    /// of opening the browser. VectorCraft's own page, the website, Discord and GitHub still open
    /// in the browser.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        if let Ok(mut hook) = self.open_app.try_borrow_mut() {
            *hook = Some(handler);
        }
    }

    /// A short name for the art being dragged out of the app (off the canvas with the Selection
    /// tool, or Layers panel rows) for the drag's ghost: `Layer “Sky”`, `Ellipse`, `3 objects`.
    pub fn outgoing_drag(&self) -> Option<String> {
        self.app.outgoing_drag(&self.ctx).map(|out| self.app.outgoing_label(&out))
    }

    /// The dragged art, cropped to its bounds, as one file of the first format in `accept` the
    /// app writes (SVG, PDF, EPS, PNG, TIFF, PSD…; rasters transparent at whole multiples of
    /// 72 ppi) in `dir`.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        let Some(out) = self.app.outgoing_drag(&self.ctx).or_else(|| self.outgoing.clone()) else { return vec![] };
        match self.app.write_outgoing(&out, accept, dir) {
            Ok(path) => vec![path],
            Err(e) => {
                log::warn!("vectorcraft: dragged art not handed over: {e}");
                self.app.status(format!("Couldn't hand the art over: {e}"));
                vec![]
            }
        }
    }

    /// Another app took the drag: the app forgets it without acting on the release.
    pub fn cancel_outgoing_drag(&mut self) {
        self.app.cancel_outgoing_drag(&self.ctx);
        self.outgoing = None;
    }

    /// Send to another app: the active document as one file of the first format in `accept` the
    /// app writes, in `dir` (its own file when unmodified and of a type `accept` lists; PDF with
    /// every artboard, other formats the artboard in view). None without a document.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        match self.app.write_active(accept, dir)? {
            Ok(path) => Some(path),
            Err(e) => {
                log::warn!("vectorcraft: document not sent: {e}");
                self.app.status(format!("Couldn't send the document: {e}"));
                None
            }
        }
    }

    /// The commands the host's agent may run: the engine's and the UI's, as the command palette
    /// lists them, with their parameters and whether they can run now.
    pub fn agent_commands(&mut self, _ctx: &egui::Context) -> Vec<Value> {
        let all = vectorcraft_ui_egui::control::all_commands(&self.app);
        all.as_array().into_iter().flatten().map(agent_command).collect()
    }

    /// Run a command for the host's agent as its menu item does (undoable like it). A command
    /// that opens a dialog instead (it needs parameters) gets the dialog closed again and fails:
    /// the agent can't answer it and the user didn't ask for it. The reply is always there at once.
    pub fn agent_execute(&mut self, ctx: &egui::Context, command: &str, params: Value) -> Receiver<Value> {
        let (tx, rx) = channel();
        let before = self.app.ui.dialog.as_ref().map(|d| d.kind.clone());
        let params = if params.is_null() { json!({}) } else { params };
        let r =
            guard::catch_panic(|| self.app.run(command, params)).unwrap_or_else(|msg| Err(format!("internal error: {msg} (please report this bug)")));
        let opened = self.app.ui.dialog.as_ref().map(|d| d.kind.clone()).filter(|k| before.as_ref() != Some(k));
        let reply = match (r, opened) {
            (_, Some(kind)) => {
                vectorcraft_ui_egui::dialogs::cancel(&mut self.app);
                let error = format!(
                    "`{command}` opened the “{kind}” dialog, which waits for the user, so it was closed again: run the command with the parameters it lists instead."
                );
                json!({ "ok": false, "error": error })
            }
            (Ok(v), None) => json!({ "ok": true, "result": v }),
            (Err(e), None) => json!({ "ok": false, "error": e }),
        };
        ctx.request_repaint();
        // The receiver is ours until we return.
        let _ = tx.send(reply);
        rx
    }

    /// The document's state for the host's agent: `document` (summary and layer tree; `depth`,
    /// `limit` children per level), `object` (one by `id`), `selection`, `find` (`name`, `kind`,
    /// `text`), `history` and `documents` (all open ones).
    pub fn agent_inspect(&mut self, _ctx: &egui::Context, what: &str, p: &Value) -> Result<Value, String> {
        let count = |key: &str, default: u64| p.get(key).and_then(Value::as_u64).unwrap_or(default);
        let session = &mut self.app.session;
        if what == "documents" {
            let active = session.active_index();
            let docs = session.documents().iter().enumerate();
            return Ok(json!(
                docs.map(|(i, d)| json!({ "index": i, "title": d.title(), "path": d.path, "dirty": d.is_dirty(), "active": Some(i) == active }))
                    .collect::<Vec<_>>()
            ));
        }
        if session.active().is_none() {
            return Err(NO_DOCUMENT.into());
        }
        let mut query = |id: &str, params: Value| {
            guard::catch_panic(|| session.execute(id, &params).map_err(|e| e.to_string())).map_err(|msg| format!("internal error: {msg}"))?
        };
        match what {
            "document" => {
                let mut v = query("document.inspect", json!({ "depth": count("depth", 3), "childLimit": count("limit", 50) }))?;
                // The undo history can be thousands of steps long: the latest ten tell what happened.
                for key in ["history", "redo"] {
                    if let Some(list) = v.get_mut(key).and_then(Value::as_array_mut)
                        && list.len() > 10
                    {
                        let n = list.len();
                        list.drain(..n - 10);
                        v[format!("{key}Count")] = json!(n);
                    }
                }
                Ok(v)
            }
            "object" | "layer" | "node" | "group" => {
                let id = p.get("id").and_then(Value::as_u64).ok_or("give the object's `id` (from `document`, `selection` or `find`)")?;
                query("document.node", json!({ "id": id, "summary": true, "depth": count("depth", 2), "childLimit": count("limit", 50) }))
            }
            "selection" => {
                let doc = query("document.inspect", json!({ "depth": 0 }))?;
                let ids: Vec<u64> = doc["selection"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
                let limit = count("limit", 50) as usize;
                let objects = ids
                    .iter()
                    .take(limit)
                    .map(|id| query("document.node", json!({ "id": id, "summary": true, "depth": 1, "childLimit": 20 })))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(json!({ "count": ids.len(), "bounds": doc["selectionBounds"], "objects": objects }))
            }
            "find" => {
                let mut params = json!({ "limit": count("limit", 100) });
                for key in ["name", "kind", "text"] {
                    if let Some(s) = p.get(key) {
                        params[key] = s.clone();
                    }
                }
                query("document.find", params)
            }
            "history" => {
                let doc = query("document.inspect", json!({ "depth": 0 }))?;
                Ok(json!({ "undo": doc["history"], "redo": doc["redo"] }))
            }
            _ => Err(format!("Vectorcraft has no view “{what}”: use document, object, selection, find, history or documents.")),
        }
    }

    /// A picture for the host's agent: `document` (alias `artboard`, `page`: the artboard `page`,
    /// 1-based, else the one in view), `object`/`layer` (one by `id`, alone and cropped) or
    /// `selection`, fitted into `max_side` pixels. Only the document is snapshotted here; the
    /// returned job renders it.
    pub fn agent_render(&mut self, _ctx: &egui::Context, t: &Value) -> Result<(String, AgentRender), String> {
        let st = self.app.session.active().ok_or(NO_DOCUMENT)?;
        let max_side = t.get("max_side").and_then(Value::as_u64).unwrap_or(1024).clamp(16, 4096) as f64;
        let target = t.get("target").and_then(Value::as_str).unwrap_or("document");
        let (doc, region, caption) = match target {
            "document" | "artboard" | "page" => {
                let index = match t.get("page").and_then(Value::as_u64) {
                    Some(n) => (n as usize).checked_sub(1).ok_or("pages count from 1")?,
                    None => self.app.view().map_or(0, |v| v.artboard),
                };
                let a = st.doc.artboards.get(index).ok_or_else(|| format!("the document has {} artboards", st.doc.artboards.len()))?;
                (st.doc.clone(), a.rect, format!("Artboard {} “{}”", index + 1, a.name))
            }
            "object" | "layer" | "node" | "group" | "selection" => {
                let ids: Vec<NodeId> = if target == "selection" {
                    st.selection.objects.clone()
                } else {
                    vec![NodeId(t.get("id").and_then(Value::as_u64).ok_or("give the object's `id` (from app_inspect)")?)]
                };
                let first = ids.first().and_then(|id| st.doc.node(*id)).ok_or(if target == "selection" {
                    "nothing is selected"
                } else {
                    "no object has that id"
                })?;
                let name = match &ids[..] {
                    [_] => format!("{} “{}” (id {})", first.kind_label(), first.display_name(), first.id.0),
                    _ => format!("{} selected objects", ids.len()),
                };
                let (doc, bounds) =
                    fileio::objects_document(st, &art_of(&st.doc, &ids), &name).ok_or("it draws nothing (empty, hidden or on a template layer)")?;
                (Arc::new(doc), bounds, name)
            }
            other => {
                return Err(format!("Vectorcraft can't render “{other}”: use document (with `page`), object or layer (with `id`) or selection."));
            }
        };
        let scale = max_side / region.width().max(region.height()).max(1e-6);
        vectorcraft_render::raster_size(region, scale)?;
        let caption = format!("{caption}, {:.0} × {:.0} pt", region.width(), region.height());
        let job: AgentRender = Box::new(move || {
            let opts = RenderOptions { skip_templates: true, ..Default::default() };
            let img = guard::catch_panic(|| Renderer::new().render_region_with(&doc, region, scale, &opts))
                .map_err(|msg| format!("internal error: {msg}"))?;
            Ok(egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels))
        });
        Ok((caption, job))
    }
}

/// A registry entry from `control::all_commands` in the shape the host's agent reads.
fn agent_command(c: &Value) -> Value {
    let text = |k: &str| c.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty() && *s != "{}");
    let mut out = json!({ "id": c["id"], "label": c["label"], "enabled": c["enabled"].as_bool().unwrap_or(true) });
    let menu: Vec<&str> = c["menu"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !menu.is_empty() {
        out["menu"] = json!(menu.join(" › "));
    }
    for key in ["params", "shortcut", "disabled_reason"] {
        if let Some(s) = text(key) {
            out[key] = json!(s);
        }
    }
    out
}

/// The objects `ids` stand for in a picture of them: a layer is its art (and its shown sublayers').
fn art_of(doc: &Document, ids: &[NodeId]) -> Vec<NodeId> {
    fn add(n: &vectorcraft_engine::doc::Node, top: bool, out: &mut Vec<NodeId>) {
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

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        // The host keeps the window's geometry (no `window::track`).
        if self.app.ui.status == "quit" {
            // Quit (nothing left unsaved): the host closes the tab. Once is enough, and the app
            // doesn't close again at once if the host shows it again.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            self.app.ui.status.clear();
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        if let Some(out) = self.app.outgoing_drag(ui.ctx()) {
            self.outgoing = Some(out);
        }
        #[cfg(target_os = "macos")]
        if self.app.take_ime_discard() {
            desktop::discard_marked_text();
        }
    }

    fn on_exit(&mut self) {
        // Saves and exports still running in the background finish first (the host may quit
        // while this tab is hidden, without asking it to close).
        vectorcraft_ui_egui::background::wait_all(&mut self.app);
        prefs::save_prefs(&self.app);
    }
}

/// The ArtCraft app whose page `url` is (another than VectorCraft): `https://getartcraft.com/apps/<name>`.
fn sibling_app(url: &str) -> Option<&'static str> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let page = rest.strip_prefix("www.").unwrap_or(rest).strip_prefix("getartcraft.com/apps/")?;
    let name = page.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    SIBLING_APPS.into_iter().find(|app| *app == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_entries_read_short_for_the_agent() {
        let engine = json!({"id": "object.arrange.front", "label": "Bring to Front", "menu": ["Object", "Arrange"], "shortcut": "Cmd+Shift+]", "params": "{}", "enabled": false, "disabled_reason": "nothing selected"});
        assert_eq!(
            agent_command(&engine),
            json!({"id": "object.arrange.front", "label": "Bring to Front", "menu": "Object › Arrange", "shortcut": "Cmd+Shift+]", "enabled": false, "disabled_reason": "nothing selected"})
        );
        let ui = json!({"id": "view.zoomIn", "label": "Zoom In", "shortcut": "", "params": "", "enabled": true, "ui": true});
        assert_eq!(agent_command(&ui), json!({"id": "view.zoomIn", "label": "Zoom In", "enabled": true}));
    }

    #[test]
    fn only_the_other_apps_pages_are_sibling_links() {
        assert_eq!(sibling_app("https://getartcraft.com/apps/photocraft"), Some("photocraft"));
        assert_eq!(sibling_app("https://www.getartcraft.com/apps/PdfCraft/?ref=vectorcraft#top"), Some("pdfcraft"));
        for url in [
            "https://getartcraft.com/apps/vectorcraft",
            "https://getartcraft.com",
            "https://discord.gg/artcraft",
            "https://github.com/storytold/photocraft",
        ] {
            assert_eq!(sibling_app(url), None, "{url}");
        }
    }
}
