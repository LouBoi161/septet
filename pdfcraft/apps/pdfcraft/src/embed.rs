//! PdfCraft as a tab of a host window (Septet).
//!
//! [`Embedded`] builds the app the way `main.rs` does for a plain launch with no arguments, minus
//! everything that belongs to the process or the window: no logger or panic hook, no GPU
//! selection, no update checks, no Apple events, no UI control channel. The host owns the
//! window, its chrome, the fonts and the UI zoom (`pdfcraft_ui_egui::hosted`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use pdfcraft_automation::{Automation, Content};
use pdfcraft_engine::DocId;
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};
use pdfcraft_ui_egui::{DocView, PdfCraftApp, RecoveryStore, i18n};
use serde_json::{Value, json};

use crate::settings;

/// Asks the host to open a file in another application; `true` when the host handled it.
type OpenExternally = Box<dyn FnMut(&Path) -> bool>;

/// Asks the host to show a sibling ArtCraft app ("photocraft", …).
type OpenApp = Box<dyn FnMut(&str)>;

/// A picture for the host's agent, made on a worker thread ([`Embedded::agent_render`]).
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

/// What the agent hears when there is nothing to work on.
const NO_DOCUMENT: &str = "No PDF is open in Pdfcraft: open one with septet_open, or make one with app_execute `doc_create` (see its params).";

/// PdfCraft running inside a host window.
pub struct Embedded {
    app: PdfCraftApp,
    /// Kept for the host, never called: PdfCraft hands no file to another application (it
    /// opens web addresses only, in the browser).
    _open_externally: Option<OpenExternally>,
    /// Kept for the host, never called: PdfCraft has no links to the other ArtCraft apps (only
    /// Discord, its own web page, GitHub and the ArtCraft website).
    _open_app: Option<OpenApp>,
    /// The automation tools (`pdfcraft-cli mcp`'s), run on the app's own session for the agent.
    automation: Automation,
}

impl Embedded {
    /// Before [`Self::new`]: keep everything PdfCraft stores per user under `root` (created if
    /// missing) instead of the user's data folders: crash recovery in `<root>/Recovery`, the
    /// digital IDs it creates in `<root>/Digital IDs`. `None`: the user's folders (the default).
    /// Its settings are in the host's eframe storage either way.
    pub fn set_data_root(root: Option<PathBuf>) {
        pdfcraft_ui_egui::hosted::set_data_root(root);
    }

    /// Build the app as `main.rs` does without arguments: settings from `storage` (the host's),
    /// autosave and crash recovery in PdfCraft's own recovery folder, OS key-store identities on
    /// macOS and Windows. Fonts are the host's: install [`Self::font_definitions`] among them.
    pub fn new(_ctx: &egui::Context, _render_state: Option<&eframe::egui_wgpu::RenderState>, storage: Option<&dyn eframe::Storage>) -> Self {
        pdfcraft_ui_egui::hosted::set_hosted(true);
        settings::migrate_legacy_folders();
        let mut app = PdfCraftApp::new();
        settings::restore(&mut app, storage);
        // The host draws the window's chrome; there are no traffic lights over our tab strip.
        app.integrated_titlebar = false;
        // No update checks: without `update_source`, Help ▸ Check for updates opens the releases page.
        app.os_key_store_ids = cfg!(any(target_os = "macos", target_os = "windows"));
        // Autosave unsaved changes; offer to recover documents a crashed session left behind.
        if let Some(dir) = RecoveryStore::default_dir() {
            app.enable_recovery(RecoveryStore::new(dir));
        }
        Self { app, _open_externally: None, _open_app: None, automation: Automation::new() }
    }

    /// The fonts PdfCraft installs at startup in the default (automatic) language: Inter and
    /// JetBrains Mono first, egui's defaults, the CJK and Arabic faces, the installed
    /// "system-fallback" face last, and the "medium" and "semibold" families.
    pub fn font_definitions() -> egui::FontDefinitions {
        let hans = i18n::Lang::from_pref(i18n::AUTO).code() == "zh-hans";
        pdfcraft_ui_egui::theme::installed_font_definitions(hans)
    }

    /// The active document's name (or the title it asks to be shown by); `None` on Home.
    pub fn document_title(&self) -> Option<String> {
        let (_, id) = self.app.active_ids()?;
        self.app.session.get(id).map(|doc| doc.display_name())
    }

    /// Some open document has unsaved changes (including text typed into a form field).
    pub fn has_unsaved_changes(&self) -> bool {
        self.app.first_dirty().is_some()
    }

    /// Open files in their own tabs, as the command line does (images and text become new PDFs).
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        for path in paths {
            self.app.open_path(&path.to_string_lossy());
        }
    }

    /// Place files into the active document: a PDF's pages go in after the page under `at` (or
    /// the current page), an image onto the page under `at`, centred there (or the middle of
    /// the current page). Other files, and all of them on Home, open in their own tabs.
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        self.app.place_paths(&paths, at);
    }

    /// The host tab was shown or hidden: hiding stops middle-button scrolling and autosaves
    /// unsaved changes; showing redraws (PdfCraft doesn't reload files on focus).
    pub fn set_visible(&mut self, visible: bool) {
        self.app.set_visible(visible);
    }

    /// Kept, but never called: PdfCraft opens no file in another application.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        self._open_externally = Some(handler);
    }

    /// Kept, but never called: PdfCraft shows no links to the other ArtCraft apps.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        self._open_app = Some(handler);
    }

    /// While pages are dragged in the Organize grid (or the Pages panel's thumbnails): "Page 4",
    /// "3 pages". Nothing else in PdfCraft drags out.
    pub fn outgoing_drag(&self) -> Option<String> {
        self.app.outgoing_drag()
    }

    /// The dragged pages as files in `dir`: one PDF of them (unsaved edits included) when
    /// `accept` has "pdf" before any image type, else a 150 dpi PNG, JPEG or TIFF of each page,
    /// whichever comes first in `accept`. Empty when none of `accept` can be made.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        self.app.take_outgoing_files(accept, dir)
    }

    /// The drag went to another app: it ends without moving any pages.
    pub fn cancel_outgoing_drag(&mut self) {
        self.app.cancel_outgoing_drag();
    }

    /// Send to: the active document's current page, as a one-page PDF or an image (the first
    /// of `accept` PdfCraft can make). `None` on Home.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        self.app.send_current_page(accept, dir)
    }
}

impl Embedded {
    /// The automation tools for the host's agent (the same as `pdfcraft-cli mcp` offers), with
    /// their parameters as JSON Schema. Pages count from 1; `doc` defaults to the PDF in front.
    pub fn agent_commands(&mut self, _ctx: &egui::Context) -> Vec<Value> {
        pdfcraft_automation::tools()
            .into_iter()
            .map(|t| {
                let mut v = json!({ "id": t.name, "label": t.title, "params": t.description, "schema": t.input_schema, "enabled": true });
                if t.read_only {
                    v["readOnly"] = json!(true);
                }
                if t.destructive {
                    v["destructive"] = json!(true);
                }
                v
            })
            .collect()
    }

    /// Run an automation tool for the host's agent on the open documents (undoable as an edit;
    /// `edit.undo`/`edit.redo` run `edit_undo`/`edit_redo`), then bring the tabs up to date:
    /// new documents get one, closed ones lose theirs, changed ones are drawn again. The reply is
    /// always there at once.
    pub fn agent_execute(&mut self, ctx: &egui::Context, command: &str, params: Value) -> Receiver<Value> {
        let (tx, rx) = channel();
        let name = command.replace('.', "_");
        let reply = match self.tool(&name, params) {
            Ok(v) => json!({ "ok": true, "result": v }),
            Err(e) => json!({ "ok": false, "error": e }),
        };
        ctx.request_repaint();
        // The receiver is ours until we return.
        let _ = tx.send(reply);
        rx
    }

    /// The documents' state for the host's agent: `document` (the PDF in front, or `id`:
    /// metadata, pages, bookmarks, comments, fields, links…), `documents`, `page` (its text,
    /// `page` counted from 1, else the page in view), `comments`, `fields`, `bookmarks`, `links`
    /// and `history` (the next undo and redo steps).
    pub fn agent_inspect(&mut self, _ctx: &egui::Context, what: &str, p: &Value) -> Result<Value, String> {
        let doc = |p: &Value| p.get("id").filter(|v| !v.is_null()).cloned();
        let (tool, mut args) = match what {
            "documents" => ("doc_list", json!({})),
            "document" | "object" => ("doc_info", json!({})),
            "page" => {
                let page = p.get("page").and_then(Value::as_u64).map_or_else(|| self.current_page() + 1, |n| n as usize);
                ("text_extract", json!({ "pages": [page] }))
            }
            "comments" => ("comment_list", json!({})),
            "fields" | "form" => ("form_fields", json!({})),
            "bookmarks" => ("bookmark_list", json!({})),
            "links" => ("link_list", json!({})),
            // The document's state carries its next undo and redo steps.
            "history" => {
                let mut v = self.tool("doc_info", doc(p).map_or_else(|| json!({}), |d| json!({ "doc": d })))?;
                return Ok(v.get_mut("document").map(Value::take).unwrap_or(Value::Null));
            }
            _ => return Err(format!("Pdfcraft has no view “{what}”: use document, documents, page, comments, fields, bookmarks, links or history.")),
        };
        if let Some(d) = doc(p) {
            args["doc"] = d;
        }
        self.tool(tool, args)
    }

    /// A picture for the host's agent: `page` (alias `document`: page `page` of the PDF in front
    /// or `id`, counted from 1, else the page in view), fitted into `max_side` pixels;
    /// `comments: false` leaves the comments out. The document's bytes are shared with the job,
    /// which renders them on a worker thread.
    pub fn agent_render(&mut self, _ctx: &egui::Context, t: &Value) -> Result<(String, AgentRender), String> {
        match t.get("target").and_then(Value::as_str).unwrap_or("page") {
            "page" | "document" => {}
            other => return Err(format!("Pdfcraft can't render “{other}”: use page (with `page`).")),
        }
        let id = match t.get("id").and_then(Value::as_u64) {
            Some(id) => DocId(id),
            None => self.app.active_ids().map(|(_, id)| id).ok_or(NO_DOCUMENT)?,
        };
        let doc = self.app.session.get(id).ok_or("no open document has that id (see app_inspect `documents`)")?;
        let page = match t.get("page").and_then(Value::as_u64) {
            Some(n) => (n as usize).checked_sub(1).ok_or("pages count from 1")?,
            None => self.current_page(),
        };
        let info = doc.info.pages.get(page).ok_or_else(|| format!("the document has {} pages", doc.info.pages.len()))?;
        let max_side = t.get("max_side").and_then(Value::as_u64).unwrap_or(1024).clamp(16, 4096) as f32;
        let scale = max_side / info.width.max(info.height).max(1.0);
        let caption = format!("Page {} of {} of “{}”, {:.0} × {:.0} pt", page + 1, doc.info.pages.len(), doc.name, info.width, info.height);
        let hide_comments = t.get("comments").and_then(Value::as_bool) == Some(false);
        let config = RenderConfig { password: doc.password.as_deref().map(Arc::from), hide_comments, ..Default::default() };
        let bytes = doc.bytes.clone();
        let job: AgentRender = Box::new(move || {
            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                PageRenderer::new(bytes, config).render(RenderRequest { page, kind: RequestKind::Pixels, tile: None, scale, tag: 0 })
            }))
            .map_err(|_| "internal error while rendering".to_string())?;
            if let Some(e) = out.error {
                return Err(format!("the page could not be rendered: {e}"));
            }
            Ok(egui::ColorImage::from_rgba_unmultiplied([out.width as usize, out.height as usize], &out.rgba))
        });
        Ok((caption, job))
    }

    /// Run automation tool `name` on the app's session (`doc` defaults to the PDF in front) and
    /// bring the tabs up to date → its JSON (pictures are only described).
    fn tool(&mut self, name: &str, mut args: Value) -> Result<Value, String> {
        let def = pdfcraft_automation::tools()
            .into_iter()
            .find(|t| t.name == name)
            .ok_or_else(|| format!("Pdfcraft has no command `{name}`; app_commands lists them."))?;
        let needs_doc = def.input_schema["required"].as_array().is_some_and(|r| r.iter().any(|k| k == "doc"));
        if args.is_null() {
            args = json!({});
        }
        if needs_doc && args.get("doc").is_none_or(Value::is_null) {
            let (_, id) = self.app.active_ids().ok_or(NO_DOCUMENT)?;
            args["doc"] = json!(id.0);
        }
        let before: Vec<(DocId, Arc<Vec<u8>>)> = self.app.session.docs().iter().map(|d| (d.id, d.bytes.clone())).collect();
        let r = self.automation.on_session(&mut self.app.session, |a| a.call(name, &args));
        self.sync_tabs(&before);
        let content = r.map_err(|_| "internal error (please report this bug)".to_string())?.map_err(|e| e.to_string())?;
        let mut out: Vec<Value> = content
            .into_iter()
            .map(|c| match c {
                Content::Json(v) => v,
                Content::Png { width, height, .. } => json!({ "image": format!("{width} × {height} PNG (look at pages with app_render)") }),
            })
            .collect();
        Ok(if out.len() == 1 { out.remove(0) } else { Value::Array(out) })
    }

    /// After a tool ran: a tab for each new document (the last one in front), none for closed
    /// ones, and the changed ones drawn again.
    fn sync_tabs(&mut self, before: &[(DocId, Arc<Vec<u8>>)]) {
        let app = &mut self.app;
        let open: Vec<DocId> = app.session.docs().iter().map(|d| d.id).collect();
        let front = app.active_ids().map(|(_, id)| id);
        app.views.retain(|v| open.contains(&v.id));
        for doc in app.session.docs() {
            match before.iter().find(|(id, _)| *id == doc.id) {
                Some((_, bytes)) if Arc::ptr_eq(bytes, &doc.bytes) => {}
                Some(_) => {
                    if let Some(v) = app.views.iter_mut().find(|v| v.id == doc.id) {
                        v.document_changed(&doc.info);
                    }
                }
                None => app.views.push(DocView::new(doc.id, &doc.info, app.view_defaults)),
            }
        }
        let new = app.session.docs().iter().rev().find(|d| !before.iter().any(|(id, _)| *id == d.id)).map(|d| d.id);
        let want = new.or(front);
        app.active = want.and_then(|id| app.views.iter().position(|v| v.id == id)).or((!app.views.is_empty()).then(|| app.views.len() - 1));
    }

    /// The page in view in the PDF in front, counted from 0.
    fn current_page(&self) -> usize {
        self.app.active.and_then(|i| self.app.views.get(i)).map_or(0, |v| v.current)
    }
}

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        eframe::App::raw_input_hook(&mut self.app, ctx, raw);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::App::save(&mut self.app, storage);
    }

    fn on_exit(&mut self) {
        eframe::App::on_exit(&mut self.app);
        // As a quit does: documents still unsaved keep an up-to-date recovery entry, and with
        // nothing unsaved the recovery folder is left empty.
        self.app.autosave_now();
        self.app.shutdown_recovery();
    }
}
