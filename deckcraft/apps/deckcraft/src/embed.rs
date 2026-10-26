//! DeckCraft in a tab of a host window (Septet). The host owns the window, its chrome, the fonts,
//! the UI zoom and dropped files; [`Embedded`] builds the app the way `main.rs` does for a plain
//! launch and forwards the host's frames to it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, RwLock};

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Services, SlideApp, hosted};
use serde_json::{Value, json};

use crate::pickers::Pickers;
use crate::{audio, pasteboard};

/// A picture for the host's agent, made on a worker thread ([`Embedded::agent_render`]).
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

/// The folder a host keeps DeckCraft's settings in ([`Embedded::set_data_root`]).
static DATA_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// The reply to a request that needs a presentation while none is open.
const NO_PRESENTATION: &str =
    "no presentation is open in DeckCraft: make one with app_execute `file.new` (or `file.newFromTemplate`), or open one with septet_open";

/// DeckCraft for an embedding host.
pub struct Embedded {
    app: SlideApp,
    ctx: egui::Context,
    open_externally: Option<Box<dyn FnMut(&Path) -> bool>>,
    /// File pickers shown without blocking the host's window.
    pickers: Pickers,
}

impl Embedded {
    /// The app as a plain launch opens it (the start screen, or the presentations crash recovery
    /// kept), with the native file pickers, the system pasteboard and audio output, and the saved
    /// preferences. DeckCraft keeps its settings in its settings folder, not in `storage`, and
    /// draws on the CPU.
    pub fn new(ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>, storage: Option<&dyn eframe::Storage>) -> Self {
        hosted::set_hosted(true);
        let _ = (render_state, storage);
        let pickers = Pickers::default();
        let mut app = SlideApp::new(Session::new(), services(&pickers));
        load_prefs(&mut app);
        // AutoRecover: reopen what a previous run left unsaved, then keep it current.
        if std::env::var_os("DECKCRAFT_NO_RECOVERY").is_none() {
            app.session.recovery_dir = recovery_dir();
            if let Ok(v) = app.session.execute("file.recovery.open", &json!({}))
                && let Some(n) = v.get("opened").and_then(|o| o.as_array()).map(Vec::len).filter(|n| *n > 0)
            {
                app.set_status(format!("Recovered {n} unsaved presentation{} from the last session", if n == 1 { "" } else { "s" }));
            }
        }
        app.integrated_titlebar = false;
        Embedded { app, ctx: ctx.clone(), open_externally: None, pickers }
    }

    /// Before [`Embedded::new`]: keep DeckCraft's settings (`ui.json`, `prefs.json`) and its
    /// AutoRecover copies at `root`, created when missing, instead of the platform's
    /// (`~/.config/deckcraft`, `~/.local/share/deckcraft/recovery`); `None` goes back to those.
    pub fn set_data_root(root: Option<PathBuf>) {
        if let Some(dir) = &root
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            log::warn!("deckcraft: data folder {}: {e}", dir.display());
        }
        match DATA_ROOT.write() {
            Ok(mut r) => *r = root,
            Err(poisoned) => *poisoned.into_inner() = root,
        }
    }

    /// The fonts DeckCraft installs at startup: Inter (and the named family `bold`), and the
    /// craft-fonts Japanese and Arabic interface faces.
    pub fn font_definitions() -> egui::FontDefinitions {
        deckcraft_ui_egui::theme::font_definitions(&[])
    }

    /// The active presentation's name; `None` while none is open (the start screen).
    pub fn document_title(&self) -> Option<String> {
        self.app.session.active().map(|d| d.title())
    }

    /// One of the open presentations has unsaved changes.
    pub fn has_unsaved_changes(&self) -> bool {
        self.app.session.documents().iter().any(|d| d.is_dirty())
    }

    /// Open files as the command line does, each as a presentation of its own (DeckCraft keeps
    /// several open and shows one). A PowerPoint Show (`.ppsx`) given last starts its slide show.
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        for (path, e) in self.app.open_from_system(&paths) {
            log::warn!("deckcraft: {path}: {e}");
            self.app.set_status(format!("Couldn't open {path}: {e}"));
        }
        self.ctx.request_repaint();
    }

    /// Place files as dropping them on the window does: pictures, audio and video go on the
    /// current slide, outlines become slides; presentations open (from their path, so Save keeps
    /// writing to them). With no presentation open, a new one takes the rest. DeckCraft can't map
    /// `at` to the slide from outside a frame.
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        let _ = at;
        let (decks, rest): (Vec<PathBuf>, Vec<PathBuf>) = paths.iter().cloned().partition(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
            PRESENTATIONS.contains(&ext.as_str())
        });
        if !decks.is_empty() {
            self.open_paths(&decks);
        }
        if !rest.is_empty() && self.app.session.active().is_none() {
            let _ = self.app.run("file.new", json!({}));
        }
        for path in &rest {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "dropped file".into());
            match std::fs::read(path) {
                Ok(bytes) => self.app.open_or_insert_bytes(&name, &bytes),
                Err(e) => {
                    log::warn!("deckcraft: place {}: {e}", path.display());
                    self.app.set_status(format!("Couldn't read {}: {e}", path.display()));
                }
            }
        }
        self.ctx.request_repaint();
    }

    /// Hidden: a running slide show ends (the window it made full screen goes back) and audio and
    /// video stop. Shown: the next frame draws.
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            self.ctx.request_repaint();
            return;
        }
        if self.app.show.is_some() {
            let _ = self.app.run("show.end", json!({}));
            self.app.restore_show_window(&self.ctx);
        }
        self.app.media.stop_owned(deckcraft_ui_egui::media::Owner::Editor);
    }

    /// DeckCraft opens no files in other applications (links and printing open in the browser
    /// or the system viewer): the handler is kept, unused.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        self.open_externally = Some(handler);
    }

    /// DeckCraft has no links to sibling apps.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        let _ = handler;
    }

    /// Nothing is dragged out of DeckCraft into other apps yet.
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

    /// Send To: the active presentation as one file in the first of `accept` (extensions, best
    /// first) that DeckCraft writes (PowerPoint, PDF, DeckCraft, the current slide as PNG or
    /// JPEG, an animated GIF, the outline as RTF or text), named after it, in `dir`. The
    /// presentation keeps its own file.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        let title = self.app.session.active()?.title();
        let (ext, format) = accept.iter().map(|e| e.to_ascii_lowercase()).find_map(|e| EXPORTS.iter().find(|(x, _)| *x == e).copied())?;
        let path = dir.join(format!("{}.{ext}", safe_name(&title)));
        match self.app.session.execute("file.export", &json!({"path": path.to_string_lossy(), "format": format})) {
            Ok(_) => Some(path),
            Err(e) => {
                log::warn!("deckcraft: send to {}: {e}", path.display());
                None
            }
        }
    }

    /// DeckCraft draws on the CPU: a lost GPU device costs it nothing.
    pub fn gpu_failed(&mut self, lost: bool, message: &str) {
        let _ = (lost, message);
    }

    /// Septet is about to restart: write the AutoRecover copies of every changed presentation now
    /// → whether they were written.
    pub fn save_recovery(&mut self) -> bool {
        if !self.has_unsaved_changes() {
            return true;
        }
        self.app.session.recovery_dir.is_some() && self.app.session.execute("file.recovery.save", &json!({})).is_ok()
    }

    /// Septet's brightness, 0 (darkest) to 3 (lightest): DeckCraft's dark theme for the two dark
    /// levels, its light one for the two light levels, and Septet's accent colour, instead of
    /// DeckCraft's own until `None`. Neither is saved as DeckCraft's.
    pub fn set_appearance(&mut self, level: Option<u8>, accent: Option<egui::Color32>) {
        hosted::set_dark(level.map(|l| l < 2));
        hosted::set_accent(accent);
        self.ctx.request_repaint();
    }

    /// More contrast (secondary text, borders), as Septet's Appearance asks; never saved.
    pub fn set_contrast(&mut self, more: bool) {
        hosted::set_contrast(more);
        self.ctx.request_repaint();
    }

    /// While Septet sets the theme, DeckCraft's Dark Mode opens Septet's settings.
    pub fn set_open_appearance(&mut self, handler: Box<dyn FnMut()>) {
        hosted::set_open_appearance(handler);
    }

    /// A screen other than the one Septet's window is on (a winit monitor index), for the
    /// Presenter View; `None` without one (see `hosted::set_other_screen`).
    pub fn set_other_screen(&mut self, screen: Option<usize>) {
        hosted::set_other_screen(screen);
    }

    /// A slide show has made Septet's window full screen and draws over all of it: the host
    /// leaves out what it would draw on top (its tab strip).
    pub fn covers_window(&self) -> bool {
        self.app.show_covers_window()
    }
}

impl Embedded {
    /// The engine's commands and the UI's (`ui: true`) for the host's agent, with their
    /// parameters as text, where the ribbon has them, and whether they can run now.
    pub fn agent_commands(&mut self, _ctx: &egui::Context) -> Vec<Value> {
        let mut out: Vec<Value> = self
            .app
            .session
            .commands()
            .into_iter()
            .map(|c| {
                let mut v = json!({ "id": c.id, "label": c.label, "enabled": c.enabled });
                if !c.menu.is_empty() {
                    v["menu"] = json!(c.menu.join(" › "));
                }
                if let Some(sc) = c.shortcut {
                    v["shortcut"] = json!(sc);
                }
                if !matches!(c.params.trim(), "" | "{}") {
                    v["params"] = json!(c.params);
                }
                if let Some(reason) = c.disabled_reason {
                    v["disabled_reason"] = json!(reason);
                }
                v
            })
            .collect();
        for (id, label, shortcut, params) in deckcraft_ui_egui::UI_COMMANDS {
            if AGENT_BLOCKED.contains(id) {
                continue;
            }
            let mut v = json!({ "id": id, "label": label, "enabled": true, "ui": true });
            if let Some(sc) = shortcut {
                v["shortcut"] = json!(sc);
            }
            if !matches!(params.trim(), "" | "{}") {
                v["params"] = json!(params);
            }
            out.push(v);
        }
        out
    }

    /// Run a command for the host's agent as the control channel's `engine.execute` does: never a
    /// dialog or a file picker (a command that would open one says so), undoable like the
    /// ribbon's. Closing or reverting a presentation with unsaved changes needs `{"force": true}`.
    pub fn agent_execute(&mut self, ctx: &egui::Context, command: &str, params: Value) -> Receiver<Value> {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut params = if params.is_null() { json!({}) } else { params };
        let force = params.get("force").and_then(Value::as_bool) == Some(true);
        if let Some(o) = params.as_object_mut() {
            o.remove("force");
        }
        let is_ui = deckcraft_ui_egui::UI_COMMANDS.iter().any(|c| c.0 == command);
        let dirty = self.app.session.active().is_some_and(|d| d.is_dirty());
        let reply = if (!is_ui && deckcraft_engine::find_command(command).is_none()) || AGENT_BLOCKED.contains(&command) {
            json!({ "ok": false, "error": format!("DeckCraft has no command `{command}` for Claude; app_commands lists them.") })
        } else if DISCARDS.contains(&command) && dirty && !force {
            // The ribbon asks Save / Don't Save first; the agent's path doesn't, so it is told.
            json!({ "ok": false, "error": format!("the presentation has unsaved changes and `{command}` would discard them: save it first (file.save / file.saveAs with `path`), or pass {{\"force\": true}}") })
        } else {
            let had_dialog = self.app.dialog.is_some();
            let had_palette = self.app.palette.is_some();
            let app = &mut self.app;
            let (ran, asked) = hosted::agent_call(|| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.run(command, params))));
            // A dialog the command opened would wait for the user: closed, and the agent is told.
            let opened = (!had_dialog && self.app.dialog.take().is_some()) | (!had_palette && self.app.palette.take().is_some());
            // An engine command opens its dialog when parameters are missing; a UI command is the dialog.
            let instead = match deckcraft_engine::find_command(command) {
                Some(c) => format!("pass its parameters: {}", c.params),
                None => "run the engine command it stands for with its parameters instead (such as insert.picture {path} or file.saveAs {path}; app_commands lists them)".into(),
            };
            match ran {
                Ok(Ok(_)) if asked || opened => {
                    json!({ "ok": false, "error": format!("`{command}` opens a dialog or a file picker, which Claude can't use; {instead}") })
                }
                Ok(Ok(result)) => json!({ "ok": true, "result": result }),
                Ok(Err(e)) => json!({ "ok": false, "error": e }),
                Err(_) => json!({ "ok": false, "error": "internal error (please report this bug); the presentation is unchanged" }),
            }
        };
        let _ = tx.send(reply);
        ctx.request_repaint();
        rx
    }

    /// The presentation's state for the host's agent: `document` (slides with titles, layouts and
    /// shape counts, the selection, size and theme), `slide` (one slide's shapes with ids, boxes,
    /// text, fills, animations, transition and notes; `index`, the current one by default),
    /// `shape` (one by `id`), `text` (`id`), `selection` (the selection and the formatting at
    /// it), `animations` (`slide`), `comments`, `history` (the undo list), `documents` (the open
    /// presentations) and `show` (the running slide show).
    pub fn agent_inspect(&mut self, _ctx: &egui::Context, what: &str, p: &Value) -> Result<Value, String> {
        let params = if p.is_object() { p.clone() } else { json!({}) };
        if what == "documents" {
            let active = self.app.session.active_index();
            let docs: Vec<Value> = self
                .app
                .session
                .documents()
                .iter()
                .enumerate()
                .map(|(i, d)| json!({"index": i, "title": d.title(), "path": d.path, "dirty": d.is_dirty(), "slides": d.doc.slides.len(), "active": Some(i) == active}))
                .collect();
            return Ok(json!({ "documents": docs }));
        }
        let Some(st) = self.app.session.active() else { return Err(NO_PRESENTATION.into()) };
        match what {
            "history" => {
                let undo: Vec<&str> = st.history.undo.iter().rev().take(50).map(|h| h.label.as_str()).collect();
                let redo: Vec<&str> = st.history.redo.iter().rev().take(50).map(|h| h.label.as_str()).collect();
                return Ok(json!({ "undo": undo, "redo": redo }));
            }
            "show" => {
                return Ok(match &self.app.show {
                    Some(s) => json!({ "running": true, "slide": s.state.slide, "reading": s.reading, "presenter": s.presenter }),
                    None => json!({ "running": false }),
                });
            }
            _ => {}
        }
        let selection = serde_json::to_value(&st.selection).unwrap_or(Value::Null);
        let session = &mut self.app.session;
        let mut query = |id: &str, params: Value| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.execute(id, &params).map_err(|e| e.to_string())))
                .unwrap_or_else(|_| Err("internal error (please report this bug)".into()))
        };
        match what {
            "document" | "presentation" | "deck" => query("document.inspect", params),
            "slide" => query("slide.inspect", params),
            "shape" | "object" => query("shape.inspect", params),
            "text" => query("text.get", params),
            "animations" | "animation" => query("animation.get", params),
            "comments" => query("comment.list", params),
            "selection" | "state" => {
                let format = query("format.state", json!({})).unwrap_or(Value::Null);
                Ok(json!({ "selection": selection, "format": format }))
            }
            _ => Err(format!(
                "DeckCraft has no view “{what}”: use document, slide, shape, text, selection, animations, comments, history, documents or show."
            )),
        }
    }

    /// A picture for the host's agent, drawn as the slide show shows it and fitted into
    /// `max_side` pixels: `slide` (`slide`, 0-based like DeckCraft's commands, the current one by
    /// default) or `deck` (every slide in a grid, up to 24 from `from`, with hidden slides
    /// dimmed). The presentation is shared with the job, which draws on the CPU.
    pub fn agent_render(&mut self, _ctx: &egui::Context, t: &Value) -> Result<(String, AgentRender), String> {
        let Some(st) = self.app.session.active() else { return Err(NO_PRESENTATION.into()) };
        let doc = st.doc.clone();
        let count = doc.slides.len();
        if count == 0 {
            return Err("the presentation has no slides: add one with app_execute `slide.new`".into());
        }
        let max_side = t.get("max_side").and_then(Value::as_u64).unwrap_or(1024).clamp(16, 4096) as f64;
        let (w, h) = (doc.slide_size.width.max(1.0), doc.slide_size.height.max(1.0));
        let index = |key: &str| t.get(key).and_then(Value::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX));
        match t.get("target").and_then(Value::as_str).unwrap_or("slide") {
            "slide" | "page" | "document" => {
                let i = index("slide").or_else(|| index("index")).unwrap_or(st.selection.slide);
                if i >= count {
                    return Err(format!("no slide {i}: the presentation has {count} (0-based)"));
                }
                let scale = (max_side / w.max(h)).clamp(0.01, 16.0);
                let caption = format!("Slide {i} (0-based) of {count}, {w:.0} × {h:.0} pt");
                let job: AgentRender = Box::new(move || {
                    let img = render(&doc, i, scale)?;
                    Ok(egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels))
                });
                Ok((caption, job))
            }
            "deck" | "slides" | "all" => {
                let from = index("from").unwrap_or(0).min(count - 1);
                let n = (count - from).min(24);
                let cols = (n as f64).sqrt().ceil().max(1.0) as usize;
                let rows = n.div_ceil(cols);
                let gap = 8.0;
                // Cells as large as fits `max_side` across and down.
                let cell = ((max_side - gap * (cols as f64 + 1.0)) / cols as f64)
                    .min((max_side - gap * (rows as f64 + 1.0)) * w / h / rows as f64)
                    .max(16.0);
                let scale = (cell / w).clamp(0.005, 16.0);
                let (cw, ch) = ((w * scale).round() as usize, (h * scale).round() as usize);
                let size = [cols * cw + (cols + 1) * gap as usize, rows * ch + (rows + 1) * gap as usize];
                let hidden: Vec<bool> = doc.slides.iter().map(|s| s.hidden).collect();
                let caption = format!("Slides {from}–{} (0-based) of {count}, {cols} per row, left to right, hidden slides dimmed", from + n - 1);
                let job: AgentRender = Box::new(move || {
                    let mut out = egui::ColorImage::filled(size, egui::Color32::from_gray(60));
                    for k in 0..n {
                        let i = from + k;
                        let img = render(&doc, i, scale)?;
                        let (x0, y0) = ((k % cols) * (cw + gap as usize) + gap as usize, (k / cols) * (ch + gap as usize) + gap as usize);
                        let dim = hidden.get(i).copied().unwrap_or(false);
                        for y in 0..(img.height as usize).min(ch) {
                            for x in 0..(img.width as usize).min(cw) {
                                let o = (y * img.width as usize + x) * 4;
                                let Some(px) = img.pixels.get(o..o + 4) else { continue };
                                let c = egui::Color32::from_rgba_premultiplied(px[0], px[1], px[2], px[3]);
                                let c = if dim { c.gamma_multiply(0.35) } else { c };
                                if let Some(slot) = out.pixels.get_mut((y0 + y) * size[0] + x0 + x) {
                                    *slot = c;
                                }
                            }
                        }
                    }
                    Ok(out)
                });
                Ok((caption, job))
            }
            other => Err(format!("DeckCraft can't render “{other}”: use slide (with `slide`, 0-based) or deck (with `from`).")),
        }
    }
}

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // The tab's close: with unsaved changes DeckCraft asks first (its Quit dialog cancels the
        // close) and closes the tab once that is answered, as its window does.
        self.app.logic(ctx);
        // A picker an action asked for, shown off the UI thread; with its answer the action runs again.
        self.pickers.show(frame, ctx);
        if let Some(replay) = self.pickers.answered() {
            self.app.replay_pick(replay);
            self.pickers.forget_answer();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    /// The preferences are saved.
    fn on_exit(&mut self) {
        save_prefs(&self.app);
    }
}

/// UI commands the host's agent doesn't get: window and dialog plumbing it can't use (the
/// command palette, the File tab's start screen) and New Window, a second OS window.
const AGENT_BLOCKED: &[&str] = &["app.palette", "app.openDialog", "app.dialog", "view.newWindow", "app.links"];

/// Commands that throw away the active presentation's unsaved changes.
const DISCARDS: &[&str] = &["file.close", "file.revert", "app.closePresentation"];

/// The presentations DeckCraft opens, by extension.
const PRESENTATIONS: &[&str] = &["deckcraft", "slidecraft", "pptx", "potx", "ppsx"];

/// What Send To writes: extension, `file.export` format.
const EXPORTS: &[(&str, &str)] = &[
    ("pptx", "pptx"),
    ("pdf", "pdf"),
    ("deckcraft", "deckcraft"),
    ("png", "png"),
    ("jpg", "jpeg"),
    ("jpeg", "jpeg"),
    ("gif", "gif"),
    ("rtf", "rtf"),
    ("txt", "outline"),
];

/// Slide `i` of `doc` at `scale` pixels per point, as the show draws it (premultiplied RGBA).
fn render(doc: &Arc<deckcraft_model::Presentation>, i: usize, scale: f64) -> Result<deckcraft_render::Image, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        deckcraft_render::render_slide(doc, i, &deckcraft_render::RenderOpts { scale, threads: 2, ..Default::default() })
    }))
    .map_err(|_| "internal error while rendering".to_string())
}

/// `name` without the characters file names can't have.
fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    if s.trim().is_empty() { "Presentation".into() } else { s }
}

fn data_root() -> Option<PathBuf> {
    match DATA_ROOT.read() {
        Ok(r) => r.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// The settings folder: the host's data root, else the platform's (as `main.rs`).
fn settings_dir() -> Option<PathBuf> {
    data_root().or_else(|| {
        if cfg!(target_os = "macos") {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/DeckCraft"))
        } else if cfg!(windows) {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DeckCraft"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
                .map(|c| c.join("deckcraft"))
        }
    })
}

/// AutoRecover copies: `recovery` in the host's data root, else the platform's folder.
fn recovery_dir() -> Option<PathBuf> {
    match data_root() {
        Some(root) => Some(root.join("recovery")),
        None => deckcraft_engine::recovery::default_dir(),
    }
}

fn load_prefs(app: &mut SlideApp) {
    if std::env::var_os("DECKCRAFT_NO_PREFS").is_some() {
        return;
    }
    let Some(dir) = settings_dir() else { return };
    let (ui, prefs) = deckcraft_ui_egui::settings::load(&dir);
    if let Some(ui) = ui {
        app.ui = ui;
    }
    if let Some(prefs) = prefs {
        app.session.prefs = prefs;
    }
}

fn save_prefs(app: &SlideApp) {
    if std::env::var_os("DECKCRAFT_NO_PREFS").is_some() {
        return;
    }
    if let Some(dir) = settings_dir() {
        deckcraft_ui_egui::settings::save(&dir, &app.ui, &app.session.prefs);
    }
}

/// The desktop services `main.rs` gives the app, with file pickers that don't block the host's
/// window (`pickers`) and stay closed for the host's agent ([`hosted::agent_call`]).
fn services(pickers: &Pickers) -> Services {
    let (open, save) = pickers.hooks();
    Services {
        pick_open: Some(open),
        pick_save: Some(save),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
        clipboard_image: Some(Box::new(pasteboard::image_png)),
        clipboard_html: Some(Box::new(pasteboard::html)),
        clipboard_set_html: Some(Box::new(pasteboard::set_html)),
        // Media is mixed and clocked by the output device; `DECKCRAFT_NO_AUDIO` plays silently.
        audio_out: std::env::var_os("DECKCRAFT_NO_AUDIO")
            .is_none()
            .then(|| Box::new(audio::CpalOut::default()) as Box<dyn deckcraft_media::AudioOut>),
        ..Default::default()
    }
}
