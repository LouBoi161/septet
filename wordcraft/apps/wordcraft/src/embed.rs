//! WordCraft in a tab of a host window (Septet). The host owns the window, its chrome, the fonts,
//! the UI zoom and the wgpu device; [`Embedded`] builds the app the way `main.rs` does for a plain
//! launch and forwards the host's frames to it.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::RwLock;
use std::sync::mpsc::Receiver;

use serde_json::{Value, json};
use wordcraft_engine::Session;
use wordcraft_ui_egui::{Services, UiState, WordApp, hosted};

use crate::{animations, clipboard, default_apps, file_dialogs, print, region};

/// A picture for the host's agent, made on a worker thread ([`Embedded::agent_render`]).
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

/// The folder a host keeps WordCraft's settings in ([`Embedded::set_data_root`]).
static DATA_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// The reply to a request that needs a document while none is open.
const NO_DOCUMENT: &str = "no document is open in WordCraft: make one with app_execute `file.new`, or open one with septet_open";

/// WordCraft for an embedding host: the app, the file dialogs it asked for, and what it last said
/// about unsaved changes.
pub struct Embedded {
    app: WordApp,
    ctx: egui::Context,
    dialogs: file_dialogs::Launcher,
    /// Unsaved changes, comment drafts included (`Services::on_dirty`).
    dirty: Rc<Cell<bool>>,
    open_externally: Option<Box<dyn FnMut(&Path) -> bool>>,
}

impl Embedded {
    /// The app as a plain launch opens it (File › Home over no document, as the Start screen
    /// setting says), with the native file dialogs, printing and the system clipboard, the saved
    /// preferences, the user's fonts and dictionary, and crash recovery. WordCraft keeps its
    /// settings in its settings folder, not in `storage`, and draws on the CPU.
    pub fn new(ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>, storage: Option<&dyn eframe::Storage>) -> Self {
        hosted::set_hosted(true);
        let _ = (render_state, storage);
        load_user_fonts();
        let region = region::system();
        let (file_dialog, dialogs) = file_dialogs::hook(ctx);
        let mut session = Session::new(wordcraft_engine::blank_document(&region));
        session.system_region = region;
        let dirty = Rc::new(Cell::new(false));
        let mut services = services(file_dialog, ctx);
        let report = dirty.clone();
        services.on_dirty = Some(Box::new(move |d| report.set(d)));
        let mut app = WordApp::new(session, services);
        load_prefs(&mut app);
        load_user_dictionary(&mut app);
        // A region chosen in File › Options replaces the system's for the first blank document.
        if app.session.prefs.region.is_some() {
            let doc = app.session.blank_document();
            app.session.set_document(doc);
        }
        app.watch_desktop_theme(ctx);
        if prefs_enabled()
            && let Some(dir) = settings_dir()
        {
            let repaint = ctx.clone();
            if let Err(e) = app.session.enable_recovery(dir.join("recovery"), std::sync::Arc::new(move || repaint.request_repaint())) {
                log::warn!("wordcraft: local recovery unavailable: {e}");
            }
        }
        app.integrated_titlebar = false;
        app.start_screen();
        Embedded { app, ctx: ctx.clone(), dialogs, dirty, open_externally: None }
    }

    /// Before [`Embedded::new`]: keep WordCraft's settings folder (preferences, the user's fonts,
    /// dictionary and templates, crash recovery) at `root`, created when missing, instead of the
    /// platform's (`~/.config/wordcraft`); `None` goes back to that.
    pub fn set_data_root(root: Option<PathBuf>) {
        if let Some(dir) = &root
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            log::warn!("wordcraft: data folder {}: {e}", dir.display());
        }
        match DATA_ROOT.write() {
            Ok(mut r) => *r = root,
            Err(poisoned) => *poisoned.into_inner() = root,
        }
    }

    /// The fonts WordCraft installs at startup: Inter (named families `medium` and `semibold`),
    /// JetBrains Mono and the craft-fonts CJK interface faces (Japanese first).
    pub fn font_definitions() -> egui::FontDefinitions {
        wordcraft_ui_egui::theme::font_definitions(false, false)
    }

    /// The document's name (`Document1` before it is saved); `None` while no document is open
    /// (the Start screen).
    pub fn document_title(&self) -> Option<String> {
        self.app.has_document().then(|| self.app.title_stem())
    }

    /// The document has unsaved changes (comment text still being typed counts).
    pub fn has_unsaved_changes(&self) -> bool {
        self.app.has_document() && (self.app.session.dirty || self.dirty.get())
    }

    /// Open files as the command line does, one after the other in this window (WordCraft shows
    /// one document): each asks to save a changed document first. File › Home closes.
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        for path in paths {
            let p = path.to_string_lossy();
            match self.app.run("file.open", json!({"path": p})) {
                Ok(_) => self.app.ui.backstage = false,
                Err(e) => {
                    log::warn!("wordcraft: {p}: {e}");
                    let msg = wordcraft_ui_egui::tl!("Couldn't open {path}: {error}");
                    self.app.status(wordcraft_ui_egui::i18n::fmt(msg, &[("path", &p), ("error", &e)]));
                }
            }
        }
        self.ctx.request_repaint();
    }

    /// Place files as dropping them on the window does, at the caret: pictures go in as pictures,
    /// fonts are added, a spreadsheet becomes the mail merge's recipient list, text documents
    /// (Word, RTF, ODT, HTML, Markdown, text) are inserted as Insert › Text from File does. With no
    /// document open, they open instead. WordCraft can't map `at` to the page from outside a frame.
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        let _ = at;
        if !self.app.has_document() {
            self.open_paths(paths);
            return;
        }
        for path in paths {
            let p = path.to_string_lossy().into_owned();
            let lower = p.to_ascii_lowercase();
            let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
            let result = if PICTURES.contains(&ext.as_str()) {
                self.app.run("insert.picture", json!({"path": p}))
            } else if wordcraft_fonts::is_font_file_name(&lower) {
                self.app.run("font.add", json!({"path": p}))
            } else if wordcraft_engine::cmd::mailings::is_spreadsheet_name(&lower) {
                self.app.run("mailings.recipients", json!({"path": p}))
            } else {
                self.app.run("insert.textFromFile", json!({"path": p}))
            };
            if let Err(e) = result {
                log::warn!("wordcraft: place {p}: {e}");
                self.app.status(e);
            }
        }
        self.ctx.request_repaint();
    }

    /// Hidden: Read Aloud stops. Shown: the next frame draws.
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            self.ctx.request_repaint();
        } else {
            self.app.session.read_aloud.stop();
        }
    }

    /// WordCraft opens no files in other applications (links open in the browser): the handler is
    /// kept, unused.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        self.open_externally = Some(handler);
    }

    /// WordCraft has no links to sibling apps.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        let _ = handler;
    }

    /// Nothing is dragged out of WordCraft into other apps yet.
    pub fn outgoing_drag(&self) -> Option<String> {
        None
    }

    /// See [`Embedded::outgoing_drag`].
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        let _ = (accept, dir);
        Vec::new()
    }

    /// See [`Embedded::outgoing_drag`].
    pub fn cancel_outgoing_drag(&mut self) {}

    /// Send To: the document as one file in the first of `accept` (extensions, best first) that
    /// WordCraft writes (Word, PDF, RTF, ODT, HTML, Markdown, text, or the first page as PNG),
    /// named after the document, in `dir`. The open document keeps its own file.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        if !self.app.has_document() {
            return None;
        }
        let ext = accept.iter().map(|e| e.to_ascii_lowercase()).find(|e| EXPORTS.contains(&e.as_str()))?;
        let path = dir.join(format!("{}.{ext}", safe_name(&self.app.title_stem())));
        let written = if ext == "png" {
            self.app
                .session
                .run("file.exportPng", &json!({"path": path.to_string_lossy(), "page": 1, "scale": 2.0}))
                .map(|_| ())
                .map_err(|e| e.to_string())
        } else {
            wordcraft_engine::io::save_path(&path, &self.app.session.doc)
        };
        match written {
            Ok(()) => Some(path),
            Err(e) => {
                log::warn!("wordcraft: send to {}: {e}", path.display());
                None
            }
        }
    }

    /// WordCraft draws on the CPU: a lost GPU device costs it nothing.
    pub fn gpu_failed(&mut self, lost: bool, message: &str) {
        let _ = (lost, message);
    }

    /// Septet is about to restart: wait for the recovery copy being written → whether the copy
    /// holds the latest changes (WordCraft writes one a moment after typing stops, so the last
    /// edits may not be in it yet).
    pub fn save_recovery(&mut self) -> bool {
        if !self.has_unsaved_changes() {
            return true;
        }
        self.app.session.finish_recovery();
        self.app.session.recovery_ready() && self.app.session.recovery_wait().is_none() && self.app.session.recovery_error().is_none()
    }

    /// Septet's brightness, 0 (darkest) to 3 (lightest): WordCraft's dark theme for the two dark
    /// levels, its light one for the two light levels, and Septet's accent colour, instead of
    /// WordCraft's own until `None`. Neither is saved as WordCraft's.
    pub fn set_appearance(&mut self, level: Option<u8>, accent: Option<egui::Color32>) {
        hosted::set_dark(level.map(|l| l < 2));
        hosted::set_accent(accent);
        self.ctx.request_repaint();
    }

    /// More contrast (secondary text, borders, icons), as Septet's Appearance asks; never saved.
    pub fn set_contrast(&mut self, more: bool) {
        hosted::set_contrast(more);
        self.ctx.request_repaint();
    }

    /// While Septet sets the theme, WordCraft's theme commands open Septet's settings.
    pub fn set_open_appearance(&mut self, handler: Box<dyn FnMut()>) {
        hosted::set_open_appearance(handler);
    }
}

impl Embedded {
    /// The engine's commands for the host's agent, with their parameters as text, where the
    /// ribbon has them, and whether they can run now.
    pub fn agent_commands(&mut self, _ctx: &egui::Context) -> Vec<Value> {
        let session = &self.app.session;
        session
            .registry
            .all()
            .iter()
            .map(|c| {
                let mut v = json!({ "id": c.id, "label": c.label, "enabled": true });
                if !c.location.is_empty() {
                    v["menu"] = json!(c.location);
                }
                if !c.shortcut.is_empty() {
                    v["shortcut"] = json!(c.shortcut);
                }
                if !matches!(c.params.trim(), "" | "{}") {
                    v["params"] = json!(c.params);
                }
                if let Some(reason) = session.why_disabled(c.id) {
                    v["enabled"] = json!(false);
                    v["disabled_reason"] = json!(reason);
                }
                v
            })
            .collect()
    }

    /// Run a command for the host's agent as the control channel's `engine.execute` does: never a
    /// dialog, undoable like the ribbon's. Replacing or closing a document with unsaved changes
    /// (`file.new`, `file.open`, `file.close`…) needs `{"force": true}`.
    pub fn agent_execute(&mut self, ctx: &egui::Context, command: &str, params: Value) -> Receiver<Value> {
        let (tx, rx) = std::sync::mpsc::channel();
        let params = if params.is_null() { json!({}) } else { params };
        let force = params.get("force").and_then(Value::as_bool) == Some(true);
        let reply = if !command.starts_with("ui.") && self.app.session.registry.get(command).is_none() {
            json!({ "ok": false, "error": format!("WordCraft has no command `{command}`; app_commands lists them.") })
        } else if wordcraft_ui_egui::discards_document(command, &params) && self.has_unsaved_changes() && !force {
            // The ribbon asks Save / Don't Save first; the agent's path doesn't, so it is told.
            json!({ "ok": false, "error": format!("the document has unsaved changes and `{command}` would replace or close it: save it first (file.save / file.saveAs), or pass {{\"force\": true}} to discard them") })
        } else {
            let had_document = self.app.has_document();
            let app = &mut self.app;
            let (ran, asked) = hosted::agent_call(|| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.execute(command, params))));
            // A document made or opened for the agent shows at once, not behind File › Home.
            if matches!(ran, Ok(Ok(_))) && (matches!(command, "file.new" | "file.open") || !had_document && self.app.has_document()) {
                self.app.ui.backstage = false;
            }
            let needs = self.app.session.registry.get(command).map(|c| c.params).unwrap_or("{}");
            match ran {
                // It wanted to ask the user (a dialog): what to pass instead.
                Ok(Ok(_)) if asked => {
                    json!({ "ok": false, "error": format!("`{command}` opens a dialog without its parameters; pass them: {needs}") })
                }
                Ok(Ok(result)) => json!({ "ok": true, "result": result }),
                Ok(Err(e)) => json!({ "ok": false, "error": e }),
                Err(_) => json!({ "ok": false, "error": "internal error (please report this bug); the document is unchanged" }),
            }
        };
        let _ = tx.send(reply);
        ctx.request_repaint();
        rx
    }

    /// The document's state for the host's agent: `document` (its paragraphs and tables with
    /// styles, runs and text; `text: false` leaves the text out), `text` (the plain text of a
    /// `story`, the body by default), `paragraph` (one by `path`, as `document` numbers them),
    /// `selection` (the caret or selection, its text and formatting), `pages` (how many, and
    /// their sizes in points) and `history` (the undo list).
    pub fn agent_inspect(&mut self, _ctx: &egui::Context, what: &str, p: &Value) -> Result<Value, String> {
        if !self.app.has_document() {
            return Err(NO_DOCUMENT.into());
        }
        let session = &mut self.app.session;
        let mut query = |id: &str, params: Value| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.run(id, &params).map_err(|e| e.to_string())))
                .unwrap_or_else(|_| Err("internal error (please report this bug)".into()))
        };
        let params = if p.is_object() { p.clone() } else { json!({}) };
        match what {
            "document" | "documents" => query("document.inspect", params),
            "text" | "story" => query("document.text", params),
            "paragraph" | "object" => query("document.paragraph", params),
            "selection" | "state" => query("document.selection", params),
            "history" => query("edit.undoHistory", params),
            "pages" | "page" => {
                let layout = self.app.session.export_layout();
                let sizes: Vec<Value> = layout.pages.iter().take(200).map(|pg| json!([pg.w.round(), pg.h.round()])).collect();
                Ok(json!({ "pages": layout.pages.len(), "sizes_pt": sizes }))
            }
            _ => Err(format!("WordCraft has no view “{what}”: use document, text, paragraph, selection, pages or history.")),
        }
    }

    /// A picture of one page for the host's agent (`target` `page` or `document`, `page` 1-based,
    /// the first by default), laid out as it prints and fitted into `max_side` pixels. The
    /// document and its layout are shared with the job, which draws on the CPU.
    pub fn agent_render(&mut self, _ctx: &egui::Context, t: &Value) -> Result<(String, AgentRender), String> {
        if !self.app.has_document() {
            return Err(NO_DOCUMENT.into());
        }
        match t.get("target").and_then(Value::as_str).unwrap_or("page") {
            "page" | "document" => {}
            other => return Err(format!("WordCraft can't render “{other}”: use page (with `page`, 1-based).")),
        }
        let layout = self.app.session.export_layout();
        let count = layout.pages.len();
        let n = t.get("page").and_then(Value::as_u64).unwrap_or(1).max(1);
        let index = usize::try_from(n - 1).map_err(|_| format!("no page {n}"))?;
        let page = layout.pages.get(index).ok_or_else(|| format!("no page {n}: the document has {count}"))?;
        let max_side = t.get("max_side").and_then(Value::as_u64).unwrap_or(1024).clamp(16, 4096) as f32;
        let scale = (max_side / page.w.max(page.h).max(1.0)).clamp(0.05, 8.0);
        let caption = format!("Page {n} of {count}, {:.0} × {:.0} pt", page.w, page.h);
        let doc = self.app.session.doc.clone();
        let job: AgentRender = Box::new(move || {
            let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                layout.pages.get(index).map(|page| wordcraft_render::render_page(&doc, page, scale, &Default::default()))
            }));
            let img = drawn.map_err(|_| "internal error while rendering".to_string())?.ok_or("the page is gone")?;
            Ok(egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels))
        });
        Ok((caption, job))
    }
}

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        self.dialogs.show(frame);
        // The tab's close: with unsaved changes WordCraft asks first (`WordApp::close_requested`)
        // and closes the tab once that is answered, as its window does.
        let closing = ctx.input(|i| i.viewport().close_requested());
        if closing && !self.app.close_requested() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else if !closing && self.app.quit_requested && !self.app.printing.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui);
        self.dialogs.show(frame);
        // Ctrl+Q is Word's Reset Paragraph Formatting off the Mac, and the app has read it: egui
        // must not take it as "quit" (the host's window keeps its quit shortcut for other apps).
        if !cfg!(target_os = "macos") {
            ui.ctx().input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Q)));
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    /// Print jobs finish, recovery data is cleared on a clean exit, and the preferences are saved.
    fn on_exit(&mut self) {
        print::finish_pending_jobs();
        self.app.session.finish_recovery();
        save_prefs(&self.app);
    }
}

/// The picture types dropping inserts as pictures.
const PICTURES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "wmf", "emf"];

/// What Send To writes, by extension.
const EXPORTS: &[&str] = &["docx", "pdf", "rtf", "odt", "html", "htm", "md", "txt", "png"];

/// `name` without the characters file names can't have.
fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    if s.trim().is_empty() { "Document".into() } else { s }
}

/// The settings folder: the host's data root, else the platform's.
fn settings_dir() -> Option<PathBuf> {
    let root = match DATA_ROOT.read() {
        Ok(r) => r.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    root.or_else(wordcraft_control_key::settings_dir)
}

/// Runs without preferences (`WORDCRAFT_NO_PREFS`, agents' test runs) neither read nor write them.
fn prefs_enabled() -> bool {
    std::env::var_os("WORDCRAFT_NO_PREFS").is_none()
}

/// `ui.json` in the settings folder.
fn prefs_path() -> Option<PathBuf> {
    settings_dir().map(|d| d.join("ui.json"))
}

fn load_prefs(app: &mut WordApp) {
    if !prefs_enabled() {
        return;
    }
    if let Some(p) = prefs_path()
        && let Ok(bytes) = std::fs::read(&p)
        && let Some(ui) = UiState::from_json(&bytes)
    {
        app.apply_prefs(ui);
    }
}

fn save_prefs(app: &WordApp) {
    if !prefs_enabled() {
        return;
    }
    let Some(path) = prefs_path() else { return };
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        log::warn!("wordcraft: preferences: cannot create {}: {e}", dir.display());
        return;
    }
    let bytes = match serde_json::to_vec_pretty(&app.prefs()) {
        Ok(bytes) => bytes,
        Err(e) => return log::warn!("wordcraft: preferences: cannot serialize: {e}"),
    };
    // Through a sibling temporary file, so a failed write leaves the previous preferences.
    let tmp = path.with_file_name(format!(".ui.json.{}.tmp", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &path)) {
        let _ = std::fs::remove_file(&tmp);
        log::warn!("wordcraft: preferences: cannot save {}: {e}", path.display());
    }
}

/// The words Add to Dictionary kept, in the settings folder.
fn load_user_dictionary(app: &mut WordApp) {
    if !prefs_enabled() {
        return;
    }
    if let Some(p) = settings_dir().map(|d| d.join(Session::USER_DICT_FILE)) {
        wordcraft_proof::load_user_dictionary(&p);
        app.session.user_dict_path = Some(p);
    }
}

/// Fonts the user added live in `fonts` in the settings folder.
fn load_user_fonts() {
    if !prefs_enabled() {
        return;
    }
    let Some(dir) = settings_dir().map(|d| d.join("fonts")) else { return };
    let (loaded, skipped) = wordcraft_fonts::FontDb::global().load_user_fonts(&dir);
    if !loaded.is_empty() || !skipped.is_empty() {
        log::info!("wordcraft: user fonts: {} loaded, {} skipped from {}", loaded.len(), skipped.len(), dir.display());
    }
}

/// The desktop services `main.rs` gives the app, with the templates in the settings folder.
fn services(file_dialog: file_dialogs::Hook, ctx: &egui::Context) -> Services {
    Services {
        file_dialog: Some(file_dialog),
        printers: Some(print::discover()),
        print_native: Some(print::submit()),
        clipboard_picture: Some(Box::new(clipboard::picture)),
        make_default_app: default_apps::hook(),
        templates_dir: settings_dir().map(|d| d.join("Templates")),
        animations_enabled: Some(animations::hook(ctx)),
        clipboard_text: Some(Box::new(clipboard::text)),
        clipboard_html: Some(Box::new(clipboard::html)),
        set_clipboard_html: Some(Box::new(clipboard::set_html)),
        clipboard_rtf: Some(Box::new(clipboard::rtf)),
        clipboard_formats: Some(Box::new(clipboard::formats)),
        ..Default::default()
    }
}
