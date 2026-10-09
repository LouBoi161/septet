//! LightCraft as a tab of a host application (Septet) that owns the window, the fonts and the
//! UI zoom: the app `main.rs` builds for a plain launch, without what belongs to a process or a
//! window of its own (panic hook, logger, command line, control server, native menu bar, GPU crash
//! sentinel, window options).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, channel};

use lightcraft_engine::catalog::{MediaKind, PhotoId, Source};
use lightcraft_engine::export::{ExportFormat, ExportOptions};
use lightcraft_engine::pipeline::{MaskView, Overlay};
use lightcraft_ui_egui::{LightcraftApp, Services};
use serde_json::{Value, json};

use crate::prefs::PrefsWriter;

/// A picture for the host's agent, made on a worker thread ([`Embedded::agent_render`]).
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

/// The host's hook for files LightCraft opens in another application: true when the host
/// handled the file (nothing is launched then).
type OpenExternally = Box<dyn FnMut(&Path) -> bool>;
/// The host's hook for links to a sibling ArtCraft app (its lowercase name).
type OpenApp = Box<dyn FnMut(&str)>;
/// A host hook shared with the services built before the host sets it.
type HostHook<T> = Rc<RefCell<Option<T>>>;

/// The sibling ArtCraft apps, by their names in ArtCraft URLs (LightCraft's own page stays a link).
const SIBLINGS: [&str; 6] = ["photocraft", "vectorcraft", "designcraft", "pdfcraft", "filmcraft", "effectcraft"];

/// LightCraft running inside a host window.
pub struct Embedded {
    app: LightcraftApp,
    prefs: PrefsWriter,
    /// The host's context, for work started outside a frame (toasts, repaints).
    ctx: egui::Context,
    /// Asked first whenever LightCraft opens a file in another application (shared with the
    /// `open_with` service, which is built before the host sets it).
    open_externally: HostHook<OpenExternally>,
    /// Takes the links to sibling apps (shared with the `open_url` service).
    open_app: HostHook<OpenApp>,
    /// Photos to import once the import in progress is done.
    queued: Vec<String>,
}

impl Embedded {
    /// LightCraft as a plain launch builds it: the settings from `ui.json`, the library they name
    /// (else the default one, a new one seeded with the demo photos), GPU rendering as set there.
    ///
    /// LightCraft draws with egui textures and computes on a GPU device of its own
    /// (`lightcraft_engine::gpu`), and keeps its settings in `ui.json`: `render_state` and
    /// `storage` aren't used, as `main.rs` doesn't use eframe's.
    ///
    /// A library another LightCraft has open (it is locked) or that can't be read leaves the
    /// session empty and in memory, and the tab says so and offers what to do, as the window does.
    pub fn new(ctx: &egui::Context, _render_state: Option<&eframe::egui_wgpu::RenderState>, _storage: Option<&dyn eframe::Storage>) -> Self {
        lightcraft_ui_egui::hosted::set_hosted(true);
        crate::alloc_release::install();
        let (prefs, prefs_warning, keep_prefs_file) = crate::prefs::load_prefs();
        let library_dir = crate::session::library_dir(prefs.as_ref());
        // GPU compute off if the preference says so, before anything can create the device
        lightcraft_engine::gpu::set_enabled(prefs.as_ref().is_none_or(|u| u.settings.gpu));
        let (mut session, problem) = crate::session::open_session(false, library_dir, true);
        crate::session::configure_segmenter(&mut session);
        let (open_externally, open_app): (HostHook<OpenExternally>, HostHook<OpenApp>) = Default::default();
        let services = ask_host_first(crate::services::services(), Rc::clone(&open_externally), Rc::clone(&open_app));
        let mut app = LightcraftApp::new(session, services);
        if let Some(ui) = prefs {
            app.ui = ui;
        }
        lightcraft_ui_egui::i18n::set_language(app.ui.language);
        // the host draws the window chrome
        app.integrated_titlebar = false;
        app.notices.extend(prefs_warning);
        // what's on disk now: only changes are written
        let prefs = PrefsWriter::new(&app, keep_prefs_file);
        app.library_problem = problem;
        Embedded { app, prefs, ctx: ctx.clone(), open_externally, open_app, queued: Vec::new() }
    }

    /// Before [`Embedded::new`]: keep everything LightCraft writes or reads as per-user state under
    /// `root` (created if missing) instead of the per-user OS folders — `ui.json`, camera profiles,
    /// the SAM 3 model and its mirror list, the default library (`<root>/Library`, unless the
    /// settings name another one) and the default export folder (`<root>/Exports`). `None`: the OS
    /// folders (the default). Process-wide.
    pub fn set_data_root(root: Option<PathBuf>) {
        lightcraft_ui_egui::hosted::set_data_root(root);
    }

    /// The fonts LightCraft installs at startup (Inter, egui's defaults, the craft-fonts CJK faces
    /// when built with them; families `Proportional`, `Monospace` and `semibold`), for the
    /// default language.
    pub fn font_definitions() -> egui::FontDefinitions {
        lightcraft_ui_egui::theme::font_definitions_for(lightcraft_engine::CRAFT_FONTS, lightcraft_ui_egui::i18n::default_language())
    }

    /// The open library's name (its folder's); `None` without a library (it couldn't be opened).
    pub fn document_title(&self) -> Option<String> {
        let dir = &self.app.session.library.as_ref()?.dir;
        Some(dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned()))
    }

    /// The library saves itself as it changes, so only changes that couldn't be written to disk
    /// (they are retried) are at risk — and the photos of a temporary session (Continue Without
    /// Saving, when the library couldn't be opened), which never are.
    pub fn has_unsaved_changes(&self) -> bool {
        let temporary = self.app.library_problem.as_ref().is_some_and(|p| p.dismissed) && self.app.session.library.is_none();
        self.app.session.unsaved().is_some() || (temporary && !self.app.session.catalog.is_empty())
    }

    /// Import files and folders into the library and show the grid, as files on the command line
    /// are (presets are imported as presets, as when dropped). While the library couldn't be opened
    /// they wait, as command-line files do, until the user has chosen where to.
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        let paths = to_strings(paths);
        if paths.is_empty() {
            return;
        }
        if let Some(p) = self.app.library_problem.as_mut()
            && !p.dismissed
        {
            p.pending_import.extend(paths);
            return;
        }
        if self.import(paths) {
            self.app.ui.view = lightcraft_ui_egui::state::ViewMode::PhotoGrid;
        }
    }

    /// Import files and folders into the library as dropping them on the window does (in the
    /// background; presets as presets), wherever the drop lands (`at` isn't used). Like
    /// [`Embedded::open_paths`] while the library couldn't be opened.
    pub fn place_paths(&mut self, paths: &[PathBuf], _at: Option<egui::Pos2>) {
        if self.app.library_problem.as_ref().is_some_and(|p| !p.dismissed) {
            return self.open_paths(paths);
        }
        self.import(to_strings(paths));
    }

    /// Shown: what the window does when it regains focus (reload the copies an external editor
    /// saved, see [`Embedded::set_open_externally`]). Hidden: no frames run, so write the library
    /// changes and settings still pending now.
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            let ctx = self.ctx.clone();
            self.app.reload_external_edits(&ctx);
            self.ctx.request_repaint();
        } else {
            self.app.session.persist_if_dirty();
            self.save_prefs();
        }
    }

    /// Edit in External Editor renders an edit copy (`<name>-Edit.tif`, stacked with the original)
    /// and opens it in the editor: `handler` gets that file first, and when it returns true nothing
    /// is launched. Either way the copy is reloaded when it changed, once LightCraft's tab is shown
    /// again ([`Embedded::set_visible`]) or the window regains focus.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        if let Ok(mut h) = self.open_externally.try_borrow_mut() {
            *h = Some(handler);
        }
    }

    /// Links to a sibling ArtCraft app's page call `handler(name)` instead of opening the web page.
    /// LightCraft shows none today (its Help links go to Discord, GitHub, the docs, its own page and
    /// the ArtCraft website, which stay links), so this only takes effect if one is added.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        if let Ok(mut h) = self.open_app.try_borrow_mut() {
            *h = Some(handler);
        }
    }

    /// While photos dragged from the grid are on the move: "Photo “IMG_0001.CR2”" or "3 photos".
    pub fn outgoing_drag(&self) -> Option<String> {
        let ids = self.app.ui.dragging_photos.as_deref().filter(|ids| !ids.is_empty())?;
        Some(match ids {
            [id] => match self.app.session.catalog.photo(PhotoId(*id)) {
                Some(p) => format!("Photo “{}”", p.file_name),
                None => "1 photo".to_string(),
            },
            ids => format!("{} photos", ids.len()),
        })
    }

    /// The dragged photos as files for a target taking `accept` (lowercase extensions, best first):
    /// see [`Embedded::export_active`]. Ends the drag.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        let Some(ids) = self.app.ui.dragging_photos.take() else { return Vec::new() };
        ids.into_iter().filter_map(|id| photo_file(&mut self.app.session, PhotoId(id), accept, dir)).collect()
    }

    /// The photo drag ended in another app: forget it, so no album takes the photos and the drag
    /// badge is gone when LightCraft's tab shows again.
    pub fn cancel_outgoing_drag(&mut self) {
        self.app.ui.dragging_photos = None;
    }

    /// The active photo as one file for a target taking `accept` (lowercase extensions, best
    /// first), written into `dir` named after the photo: a copy of its original when the target
    /// takes that format and the photo is unedited (not a raw: those go developed; a video goes as
    /// its original file), else the photo as LightCraft shows it — rendered at full size with its
    /// edits and crop by the export pipeline, in the first of `accept` it can write (TIFF 16-bit,
    /// JPEG, PNG, WebP, AVIF). `None` without an active photo or a format to send it in.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        let id = self.app.session.active()?;
        photo_file(&mut self.app.session, id, accept, dir)
    }

    /// Import `paths` (presets as presets, photos in the background); true when photos are coming.
    fn import(&mut self, paths: Vec<String>) -> bool {
        let (presets, photos): (Vec<String>, Vec<String>) = paths.into_iter().partition(|p| lightcraft_ui_egui::is_preset_file(p));
        if !presets.is_empty() {
            let _ = self.app.run("file.importPresets", json!({"paths": presets}));
        }
        if photos.is_empty() {
            return false;
        }
        self.queued.extend(photos);
        let ctx = self.ctx.clone();
        self.start_queued(&ctx);
        ctx.request_repaint();
        true
    }

    /// Start importing the queued photos, unless an import is still running (they follow it).
    fn start_queued(&mut self, ctx: &egui::Context) {
        if self.queued.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut self.queued);
        if lightcraft_ui_egui::import::start_paths(&mut self.app, paths.clone()).is_err() {
            self.queued = paths;
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn save_prefs(&mut self) {
        if let Err(e) = self.prefs.save(&self.app) {
            eprintln!("lightcraft: {e}");
        }
    }
}

impl Embedded {
    /// The commands the host's agent may run: the engine's and the UI's, as the command palette
    /// lists them, with their parameters and whether they can run now.
    pub fn agent_commands(&mut self, _ctx: &egui::Context) -> Vec<Value> {
        let all = lightcraft_ui_egui::control::all_commands(&self.app);
        all.as_array().into_iter().flatten().map(agent_command).collect()
    }

    /// Run a command for the host's agent as its menu item does (undoable like it). A command
    /// that would open a dialog or a file picker instead (it needs parameters), or hand a file to
    /// another program, fails and leaves nothing open: the agent can't answer it and the user
    /// didn't ask for it. The reply is always there at once.
    pub fn agent_execute(&mut self, ctx: &egui::Context, command: &str, params: Value) -> Receiver<Value> {
        let (tx, rx) = channel();
        let params = if params.is_null() { json!({}) } else { params };
        let had_dialog = self.app.ui.dialog.is_some();
        let asked = Rc::new(Cell::new(None::<&'static str>));
        let ask = |what: &'static str| {
            let a = asked.clone();
            move || a.set(Some(what))
        };
        let sv = &mut self.app.services;
        let (files, presets, tracklog, curves, save, save_curves, folder, reveal, open_with) = (
            ask("pick files"),
            ask("pick preset files"),
            ask("pick a track log"),
            ask("pick curve presets"),
            ask("choose where to save"),
            ask("choose where to save"),
            ask("pick a folder"),
            ask("show a file in the file manager"),
            ask("open a file in another program"),
        );
        let saved = (
            sv.pick_files.replace(Box::new(move || {
                files();
                Vec::new()
            })),
            sv.pick_preset_files.replace(Box::new(move || {
                presets();
                Vec::new()
            })),
            sv.pick_tracklog.replace(Box::new(move || {
                tracklog();
                Vec::new()
            })),
            sv.pick_curve_preset_files.replace(Box::new(move || {
                curves();
                Vec::new()
            })),
            sv.save_preset_file.replace(Box::new(move |_| {
                save();
                None
            })),
            sv.save_curve_preset_file.replace(Box::new(move |_| {
                save_curves();
                None
            })),
            sv.pick_folder.replace(Box::new(move || {
                folder();
                None
            })),
            sv.reveal.replace(Box::new(move |_| {
                reveal();
                Err("not for the agent".into())
            })),
            sv.open_with.replace(Box::new(move |_, _| {
                open_with();
                Err("not for the agent".into())
            })),
        );
        let app = &mut self.app;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.run(command, params)))
            .unwrap_or_else(|_| Err("internal error (please report this bug); the library is unchanged".into()));
        let sv = &mut self.app.services;
        (
            sv.pick_files,
            sv.pick_preset_files,
            sv.pick_tracklog,
            sv.pick_curve_preset_files,
            sv.save_preset_file,
            sv.save_curve_preset_file,
            sv.pick_folder,
            sv.reveal,
            sv.open_with,
        ) = saved;
        let opened = !had_dialog && self.app.ui.dialog.is_some();
        if opened {
            self.app.ui.dialog = None;
        }
        let reply = match (r, opened, asked.get()) {
            (_, true, _) => {
                json!({ "ok": false, "error": format!("`{command}` opened a dialog, which waits for the user, so it was closed again: run the command with the parameters it lists instead.") })
            }
            (_, false, Some(what)) => {
                json!({ "ok": false, "error": format!("`{command}` asks the user to {what}: pass the path(s) it lists instead.") })
            }
            (Ok(v), false, None) => json!({ "ok": true, "result": v }),
            (Err(e), false, None) => json!({ "ok": false, "error": e }),
        };
        ctx.request_repaint();
        // The receiver is ours until we return.
        let _ = tx.send(reply);
        rx
    }

    /// The library's state for the host's agent: `document` (the library, its counts and the
    /// current view), `photos` (a page of the photos in view, or matching `params.filter`),
    /// `photo` (one by `id`, else the active one), `develop` (its develop settings and masks),
    /// `controls` (every slider with its range, `params.section`), `selection`, `albums` and
    /// `history` (the photo's develop history).
    pub fn agent_inspect(&mut self, _ctx: &egui::Context, what: &str, p: &Value) -> Result<Value, String> {
        let session = &mut self.app.session;
        let mut query = |id: &str, params: Value| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.execute(id, &params).map_err(|e| e.to_string())))
                .unwrap_or_else(|_| Err("internal error (please report this bug)".into()))
        };
        let get = |k: &str| p.get(k).filter(|v| !v.is_null()).cloned();
        let photo = json!({ "id": get("id") });
        match what {
            "document" | "library" | "selection" => {
                let mut v = query("library.state", json!({}))?;
                v["stats"] = query("catalog.stats", json!({}))?;
                Ok(v)
            }
            "photos" | "find" => {
                let mut q = json!({ "limit": p.get("limit").and_then(Value::as_u64).unwrap_or(50) });
                for k in ["filter", "sort", "offset"] {
                    if let Some(v) = get(k) {
                        q[k] = v;
                    }
                }
                query("catalog.query", q)
            }
            "photo" | "object" => query("photo.inspect", photo),
            "develop" | "masks" | "mask" => query("develop.get", photo),
            "controls" => query("develop.controls", json!({ "section": get("section") })),
            "albums" => query("albums.list", json!({})),
            "history" => query("history.list", photo),
            _ => Err(format!("Lightcraft has no view “{what}”: use document, photos, photo, develop, controls, selection, albums or history.")),
        }
    }

    /// A picture for the host's agent: `photo` (alias `document`, `selection`: the photo `id`, else
    /// the active one, developed and cropped), `before` (the same unedited) or `mask` (the photo's mask `mask`
    /// (its id, else the first), white on black; `view: "color"` tints it over the photo), fitted
    /// into `max_side` pixels. The render runs on a worker thread.
    pub fn agent_render(&mut self, _ctx: &egui::Context, t: &Value) -> Result<(String, AgentRender), String> {
        let s = &mut self.app.session;
        let id = match t.get("id").and_then(Value::as_u64) {
            Some(id) => PhotoId(id),
            None => s.active().ok_or("no photo is active: give the photo's `id` (from app_inspect `photos`)")?,
        };
        let name = s.catalog.photo(id).ok_or("the library has no photo with that id")?.file_name.clone();
        let side = t.get("max_side").and_then(Value::as_u64).unwrap_or(1024).clamp(16, 4096) as usize;
        let target = t.get("target").and_then(Value::as_str).unwrap_or("photo");
        let (caption, overlay) = match target {
            "photo" | "document" | "selection" | "before" => {
                (format!("{}“{name}” (id {})", if target == "before" { "Unedited " } else { "" }, id.0), None)
            }
            "mask" => {
                let masks = s.develop_of(id).map(|d| d.masks.clone()).unwrap_or_default();
                let mask = match t.get("mask").and_then(Value::as_u64) {
                    Some(m) => masks.iter().find(|x| u64::from(x.id) == m).ok_or("the photo has no mask with that id (see app_inspect `develop`)")?,
                    None => masks.first().ok_or("the photo has no masks")?,
                };
                let id16 = u16::try_from(mask.id).map_err(|_| "that mask can't be shown")?;
                let view = if t.get("view").and_then(Value::as_str) == Some("color") { MaskView::Color } else { MaskView::WhiteOnBlack };
                (
                    format!("Mask “{}” (id {}) of “{name}”", mask.name, mask.id),
                    Some(Overlay::Mask { id: id16, view, color: [230, 30, 40], opacity: 60 }),
                )
            }
            other => return Err(format!("Lightcraft can't render “{other}”: use photo, before or mask (with `id`, `mask`).")),
        };
        let mut job = s.render_job(id, side, side, target == "before", true).ok_or("the photo can't be rendered")?;
        if let Some(o) = overlay {
            job = job.with_overlay(o);
        }
        let job: AgentRender = Box::new(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.run())).map_err(|_| "internal error while rendering".to_string())?;
            let img = r.rendered?.image;
            let px: Vec<u8> = img.data.iter().flatten().copied().collect();
            Ok(egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &px))
        });
        Ok((caption, job))
    }
}

/// A registry entry from `control::all_commands` in the shape the host's agent reads.
fn agent_command(c: &Value) -> Value {
    let text = |k: &str| c.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty() && *s != "{}");
    let mut out = json!({ "id": c["id"], "label": c["label"], "enabled": c["enabled"].as_bool().unwrap_or(true) });
    let menu: Vec<&str> = c["menu"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|m| !m.is_empty()).collect();
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

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        self.prefs.tick(&mut self.app, ctx);
        self.start_queued(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    /// LightCraft keeps nothing in eframe's storage; its settings file is written here too.
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_prefs();
    }

    fn on_exit(&mut self) {
        self.save_prefs();
        if let Err(e) = self.app.session.close_library() {
            eprintln!("lightcraft: saving the library failed: {e}");
        }
    }
}

/// `services` whose `open_with` (Edit in External Editor) offers the file to the host first, and
/// whose `open_url` hands links to a sibling app's page to the host.
fn ask_host_first(mut services: Services, host: HostHook<OpenExternally>, open_app: HostHook<OpenApp>) -> Services {
    let mut native = services.open_with.take();
    services.open_with = Some(Box::new(move |path: &str, editor: &str| {
        let handled = !path.is_empty() && host.try_borrow_mut().ok().and_then(|mut h| h.as_mut().map(|f| f(Path::new(path)))).unwrap_or(false);
        if handled {
            return Ok(());
        }
        match native.as_mut() {
            Some(open) => open(path, editor),
            None => Err("can't open other applications here".into()),
        }
    }));
    let mut browser = services.open_url.take();
    services.open_url = Some(Box::new(move |url: &str| {
        if let Some(name) = sibling_of(url)
            && let Ok(mut h) = open_app.try_borrow_mut()
            && let Some(open) = h.as_mut()
        {
            open(name);
            return Ok(());
        }
        match browser.as_mut() {
            Some(open) => open(url),
            None => Err("can't open links here".into()),
        }
    }));
    services
}

/// Photo `id` as a file for a target taking `accept` (see [`Embedded::export_active`]).
fn photo_file(session: &mut lightcraft_engine::Session, id: PhotoId, accept: &[&str], dir: &Path) -> Option<PathBuf> {
    let p = session.catalog.photo(id)?;
    if let Source::File { path } = &p.source
        && p.kind != MediaKind::Raw
        && (p.kind == MediaKind::Video || !p.is_edited())
        && Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|ext| accept.iter().any(|a| same_extension(a, ext)))
        && Path::new(path).is_file()
    {
        let src = Path::new(path);
        // footage is placed by reference (and can be large)
        if p.kind == MediaKind::Video {
            return Some(src.to_path_buf());
        }
        // a copy, byte for byte: the other app may save over the file it opened, and the library's
        // original stays untouched
        let stem = src.file_stem().map_or_else(|| "Photo".into(), |s| s.to_string_lossy());
        let ext = src.extension().map(|e| e.to_string_lossy()).unwrap_or_default();
        let out = free_path(dir, &stem, &ext);
        match std::fs::copy(src, &out) {
            Ok(_) => return Some(out),
            Err(e) => log::warn!("sending photo {}: copying {path}: {e}; rendering it instead", id.0),
        }
    }
    let (ext, format) = accept.iter().find_map(|a| ExportFormat::parse(a).filter(|f| f.is_rendered()).map(|f| (a.to_ascii_lowercase(), f)))?;
    let opts = ExportOptions { format, quality: 95, ..Default::default() };
    let name = opts.file_name_for(p, 1);
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
    let out = free_path(dir, stem, &ext);
    let written = lightcraft_engine::export::export_photo(session, id, &opts, 1)
        .and_then(|e| lightcraft_engine::catalog::safe_file::write_atomic_nosync(&out, &e.bytes).map_err(|err| format!("{}: {err}", out.display())));
    match written {
        Ok(()) => Some(out),
        Err(e) => {
            log::warn!("sending photo {}: {e}", id.0);
            None
        }
    }
}

/// The sibling ArtCraft app whose page on the ArtCraft website `url` is (`…/apps/<name>`).
fn sibling_of(url: &str) -> Option<&'static str> {
    let rest = url.strip_prefix(lightcraft_ui_egui::links::WEBSITE)?.strip_prefix("/apps/")?;
    let name = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    SIBLINGS.into_iter().find(|s| *s == name)
}

/// Whether two file extensions name the same format (`jpg` = `jpeg`, `tif` = `tiff`).
fn same_extension(a: &str, b: &str) -> bool {
    let norm = |e: &str| match e.to_ascii_lowercase().as_str() {
        "jpeg" => "jpg".to_string(),
        "tiff" => "tif".to_string(),
        e => e.to_string(),
    };
    norm(a) == norm(b)
}

/// `<stem>.<ext>` in `dir`, or `<stem>-2.<ext>`… when that is taken (an earlier drag's file may
/// still be open in the other app).
fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..10_000).map(|n| dir.join(format!("{stem}-{n}.{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

fn to_strings(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.to_string_lossy().into_owned()).filter(|p| !p.is_empty()).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The host gets a file LightCraft opens elsewhere first; only what it declines is launched.
    #[test]
    fn open_with_asks_the_host_first() {
        let launched: Rc<RefCell<Vec<String>>> = Rc::default();
        let l = Rc::clone(&launched);
        let services = Services {
            open_with: Some(Box::new(move |path: &str, _editor: &str| {
                l.borrow_mut().push(path.to_string());
                Ok(())
            })),
            ..Default::default()
        };
        let host: HostHook<OpenExternally> = Rc::default();
        let mut services = ask_host_first(services, Rc::clone(&host), Rc::default());
        let mut open = |path: &str| (services.open_with.as_mut().unwrap())(path, "");
        open("/a/no-host-yet-Edit.tif").unwrap();
        *host.borrow_mut() = Some(Box::new(|p: &Path| p.extension().is_some_and(|e| e == "tif")));
        open("/a/IMG_1-Edit.tif").unwrap();
        open("/a/IMG_2.jpg").unwrap();
        assert_eq!(*launched.borrow(), ["/a/no-host-yet-Edit.tif", "/a/IMG_2.jpg"]);
    }

    /// Links to a sibling app's page go to the host once it takes them; everything else (and
    /// LightCraft's own page) stays a link.
    #[test]
    fn sibling_app_links_go_to_the_host() {
        let browsed: Rc<RefCell<Vec<String>>> = Rc::default();
        let b = Rc::clone(&browsed);
        let services = Services {
            open_url: Some(Box::new(move |url: &str| {
                b.borrow_mut().push(url.to_string());
                Ok(())
            })),
            ..Default::default()
        };
        let apps: HostHook<OpenApp> = Rc::default();
        let mut services = ask_host_first(services, Rc::default(), Rc::clone(&apps));
        let opened: Rc<RefCell<Vec<String>>> = Rc::default();
        let o = Rc::clone(&opened);
        let mut open = |url: &str| (services.open_url.as_mut().unwrap())(url);
        open("https://getartcraft.com/apps/photocraft").unwrap();
        *apps.borrow_mut() = Some(Box::new(move |name: &str| o.borrow_mut().push(name.to_string())));
        for url in [
            "https://getartcraft.com/apps/vectorcraft",
            "https://getartcraft.com/apps/FilmCraft/",
            lightcraft_ui_egui::links::APP_PAGE,
            lightcraft_ui_egui::links::DISCORD,
            lightcraft_ui_egui::links::WEBSITE,
        ] {
            open(url).unwrap();
        }
        assert_eq!(*opened.borrow(), ["vectorcraft", "filmcraft"]);
        assert_eq!(browsed.borrow().len(), 4, "{:?}", browsed.borrow());
    }

    /// Unedited originals go as they are (copied) when the target takes their format; everything else is
    /// rendered in the first format LightCraft can write, named after the photo.
    #[test]
    fn photos_go_out_as_originals_or_renders() {
        let dir = std::env::temp_dir().join(format!("lc-embed-send-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(same_extension("JPEG", "jpg") && same_extension("tif", "TIFF") && !same_extension("png", "jpg"));
        assert_eq!(free_path(&dir, "IMG_1", "tif"), dir.join("IMG_1.tif"));
        std::fs::write(dir.join("IMG_1.tif"), b"x").unwrap();
        assert_eq!(free_path(&dir, "IMG_1", "tif"), dir.join("IMG_1-2.tif"));
        // an unedited PNG on disk goes as it is to a target taking PNG…
        std::fs::create_dir_all(dir.join("originals")).unwrap();
        let png = dir.join("originals").join("Beach.png");
        let mut img = lightcraft_raster::Rgba8::new(64, 48);
        img.data.iter_mut().for_each(|px| *px = [200, 120, 60, 255]);
        let bytes = lightcraft_codecs::encode_png(&lightcraft_codecs::EncodeImage::rgba8(&img), &Default::default()).unwrap();
        std::fs::write(&png, bytes).unwrap();
        let mut s = lightcraft_engine::Session::new().with_fs();
        s.execute("library.import", &json!({"paths": [png.to_string_lossy()]})).unwrap();
        let id = s.visible_cloned().first().copied().unwrap();
        // (a copy: the library's original stays out of the other app's reach)
        let copy = photo_file(&mut s, id, &["psd", "tif", "png"], &dir).unwrap();
        assert_eq!(copy, dir.join("Beach.png"));
        assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&png).unwrap());
        // …and rendered for one that doesn't (TIFF first for PhotoCraft), named after it
        let tif = photo_file(&mut s, id, &["psd", "tif"], &dir).unwrap();
        assert_eq!(tif, dir.join("Beach.tif"));
        assert!(std::fs::read(&tif).unwrap().starts_with(b"II") || std::fs::read(&tif).unwrap().starts_with(b"MM"));
        // nothing LightCraft can write
        assert_eq!(photo_file(&mut s, id, &["svg", "pdf"], &dir), None);
        // edited: rendered, even where the original's format is taken
        s.execute("library.select", &json!({"ids": [id.0]})).unwrap();
        s.execute("develop.set", &json!({"control": "light.exposure", "value": 1.0})).unwrap();
        assert_eq!(photo_file(&mut s, id, &["png"], &dir), Some(dir.join("Beach-2.png")));
        // a generated demo photo has no original: rendered
        let mut demo = lightcraft_engine::Session::with_demo();
        let first = demo.visible_cloned()[0];
        let jpg = photo_file(&mut demo, first, &["jpeg"], &dir).unwrap();
        assert!(jpg.extension().is_some_and(|e| e == "jpeg") && std::fs::read(&jpg).unwrap().starts_with(&[0xff, 0xd8]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Hosted, LightCraft's per-user files live under the data root, and nothing advertises
    /// ArtCraft: no Discord or ArtCraft-website items (Help, GitHub and Send Feedback stay).
    #[test]
    fn hosted_builds_keep_their_files_in_the_data_root_and_show_no_artcraft_promotions() {
        let root = std::env::temp_dir().join(format!("lc-embed-root-{}", std::process::id()));
        Embedded::set_data_root(Some(root.clone()));
        assert!(root.is_dir(), "created");
        assert_eq!(crate::prefs::config_dir(), Some(root.clone()));
        if std::env::var_os("LIGHTCRAFT_LIBRARY").is_none() {
            assert_eq!(crate::session::library_dir(None), Some(root.join("Library")));
        }
        assert_eq!(lightcraft_ui_egui::control::default_export_dir(), root.join("Exports").to_string_lossy());
        Embedded::set_data_root(None);
        assert_ne!(crate::prefs::config_dir(), Some(root.clone()));
        let _ = std::fs::remove_dir_all(&root);

        lightcraft_ui_egui::hosted::set_hosted(true);
        let app = LightcraftApp::new(lightcraft_engine::Session::new(), Services::default());
        fn ids(nodes: &[lightcraft_ui_egui::menubar::MenuNode], out: &mut Vec<String>) {
            for n in nodes {
                match n {
                    lightcraft_ui_egui::menubar::MenuNode::Item { id, .. } => out.push(id.clone()),
                    lightcraft_ui_egui::menubar::MenuNode::Submenu { children, .. } => ids(children, out),
                    _ => {}
                }
            }
        }
        let mut all = Vec::new();
        for (_, items) in lightcraft_ui_egui::menubar::menu_bar(&app) {
            ids(&items, &mut all);
        }
        for brand in lightcraft_ui_egui::links::ARTCRAFT {
            assert!(!all.iter().any(|id| id == brand), "{brand} in the hosted menus");
        }
        assert!(all.iter().any(|id| id == "app.github") && all.iter().any(|id| id == "app.about"), "{all:?}");
    }

    /// The fonts the host installs carry the family the UI draws its headings with.
    #[test]
    fn font_definitions_have_the_semibold_family() {
        let fonts = Embedded::font_definitions();
        let family = egui::FontFamily::Name(lightcraft_ui_egui::theme::FONT_SEMIBOLD.into());
        assert!(fonts.families.get(&family).is_some_and(|f| f.first().is_some_and(|n| n == "Inter-SemiBold")));
        assert!(fonts.font_data.contains_key("Inter"));
    }
}
