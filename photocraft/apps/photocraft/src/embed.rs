//! PhotoCraft built for a tab inside a host app (the Septet shell). The host shares one
//! `egui::Context` and one wgpu device between every app it shows, and owns the window, the fonts
//! and the UI zoom (see `photocraft_ui_egui::hosted`).
//!
//! [`Embedded::new`] builds the app as `main.rs` does for a plain launch without arguments, minus
//! everything process-wide or window-owning: no panic hook, control server, macOS menu bar or
//! Apple events, GPU startup marker or relaunch, saved window layout or window sizing.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use photocraft_doc::{Document, LayerId};
use photocraft_engine::Session;
use photocraft_engine::prefs::{GpuBackend, RenderingMode};
use photocraft_ui_egui::layer_transfer::{self, Outgoing};
use photocraft_ui_egui::{ControlRequest, OpenUrlFn, PhotocraftApp, gpu_status, monitor_status, notices};
use serde_json::{Value, json};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::tablet;
use crate::{app_dirs, gpu_startup, monitor_profile, services};

mod content;

/// The host's "open this file in another application" hook ([`Embedded::set_open_externally`]).
type OpenExternally = Rc<RefCell<Option<Box<dyn FnMut(&Path) -> bool>>>>;
/// The host's "show this sibling app" hook ([`Embedded::set_open_app`]).
type OpenApp = Rc<RefCell<Option<Box<dyn FnMut(&str)>>>>;

/// A picture for the host's agent, made on a worker thread ([`Embedded::agent_render`]).
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

/// What the agent hears when there is nothing to work on.
const NO_DOCUMENT: &str = "No document is open in Photocraft: open one with septet_open, or make one with app_execute `file.new` {width, height}.";

/// PhotoCraft as one tab of a host app (see the module docs).
pub struct Embedded {
    app: PhotocraftApp,
    /// The host's context, for files handed over between frames ([`Self::place_paths`]).
    ctx: egui::Context,
    open_externally: OpenExternally,
    open_app: OpenApp,
    /// Where the host showed the app last (its viewport and the area under the host's tabs).
    content: Option<(egui::ViewportId, egui::Rect)>,
    /// Layers dragged out of that area, for the host ([`Self::outgoing_drag`]).
    outgoing: Option<Outgoing>,
    /// Pen tablet samples from AppKit (removed when the tab goes away).
    #[cfg(target_os = "macos")]
    _tablet: Option<photocraft_tablet::macos::Monitor>,
}

impl Embedded {
    /// Build the app the way `main.rs` does for a plain launch with no arguments (preferences,
    /// brush presets and crash recovery from the usual settings directory), minus everything
    /// process-wide or window-owning. `render_state` is the host's wgpu state (the GPU canvas
    /// uses it unless the preferences or the adapter say CPU); `storage` is not used, as the host
    /// keeps egui's memory itself.
    pub fn new(ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>, storage: Option<&dyn eframe::Storage>) -> Self {
        photocraft_ui_egui::hosted::set_hosted(true);
        let _ = storage;
        let open_externally = OpenExternally::default();
        let open_app = OpenApp::default();
        // Read the displays' ICC profiles (macOS) and load the brush presets in the background.
        let monitor = monitor_profile::detect_async();
        let presets = services::presets_dir().map(photocraft_engine::preset_store::open_dir_async);
        let mut services = services::native(None);
        services.preset_store = presets;
        services.open_url = Some(open_url_via_host(services.open_url.take(), open_externally.clone(), open_app.clone()));
        #[cfg(target_os = "linux")]
        let display = display_kind();
        #[cfg(target_os = "linux")]
        {
            services.is_wayland = display == Some(tablet::DisplayKind::Wayland);
        }
        let mut app = PhotocraftApp::new(Session::new(), services);
        // The host draws the window chrome: no integrated title strip, caption buttons or edge
        // resizing.
        app.integrated_titlebar = false;
        app.custom_titlebar = false;
        // Long commands and file opens run as background jobs with progress and Cancel (#210).
        app.background_jobs = std::env::var_os("PHOTOCRAFT_INLINE_JOBS").is_none();
        // Displays and their profiles (#569), as main.rs: the first frames already use the right
        // profile when the reading is quick, a slower one is applied when it arrives.
        if let Some(rx) = monitor {
            app.services.read_displays = Some(std::sync::Arc::new(monitor_profile::detect_async));
            match rx.recv_timeout(std::time::Duration::from_secs(2)) {
                Ok(r) => monitor_status::apply(&mut app, r),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => monitor_status::pending(&mut app, rx),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    monitor_status::apply(&mut app, Err("the display profile reader stopped without an answer".into()));
                }
            }
        }
        attach_gpu(&mut app, render_state);
        // No `services.cursor_pos`: the host's window handle isn't ours to query, so a file the OS
        // drops straight onto the app opens as a document (as on Wayland). The host places files
        // at a position through `place_paths` instead.
        #[cfg(target_os = "macos")]
        let pen_monitor = tablet::install_macos(&app.stylus.feed);
        #[cfg(target_os = "linux")]
        {
            app.stylus.feed = x11_stylus_feed(display);
        }
        // Portable marker found but its data folder isn't writable (#228): say where settings went.
        if let Some(w) = &app_dirs::current().warning {
            notices::post(&mut app, "Portable mode is off", vec![w.clone()], false, None);
        }
        ctx.request_repaint();
        Self {
            app,
            ctx: ctx.clone(),
            open_externally,
            open_app,
            content: None,
            outgoing: None,
            #[cfg(target_os = "macos")]
            _tablet: pen_monitor,
        }
    }

    /// Before [`Self::new`]: keep everything PhotoCraft stores per user (preferences, brush
    /// presets, crash recovery) under `root` instead of the per-user OS folder, as its portable
    /// mode does (`None`: the usual folder). `root` is created; if it can't be written, the app
    /// says so and uses the usual folder.
    pub fn set_data_root(root: Option<PathBuf>) {
        if let Some(dir) = &root
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            log::warn!("couldn't create PhotoCraft's data folder {}: {e}", dir.display());
        }
        photocraft_ui_egui::hosted::set_data_root(root);
        if app_dirs::resolved() {
            log::warn!("PhotoCraft's settings folder was already chosen ({:?}); the new data root applies from the next start", app_dirs::config_dir());
        }
    }

    /// The fonts PhotoCraft installs at startup (Inter, JetBrains Mono and the `medium` /
    /// `semibold` families), for the host to install.
    pub fn font_definitions() -> egui::FontDefinitions {
        photocraft_ui_egui::theme::font_definitions()
    }

    /// The active document's name (the title bar's, without the unsaved marker); `None` on the
    /// start screen.
    pub fn document_title(&self) -> Option<String> {
        self.app.session.active().map(|d| d.doc.name.clone())
    }

    /// Whether any open document has unsaved changes (what quitting would ask about).
    pub fn has_unsaved_changes(&self) -> bool {
        self.app.session.documents().iter().any(|d| d.is_dirty())
    }

    /// Open files as command-line paths do (documents; brushes and gradients go to their
    /// libraries).
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        self.app.open_paths(&paths);
        self.ctx.request_repaint();
    }

    /// Place files into the active document as dropping them on its canvas does: each becomes a
    /// layer (Place Embedded) in Free Transform, one after another. `at` over the document tab
    /// strip opens them at that tab position instead, like a drop there. Without an open
    /// document they open as documents.
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        self.app.place_paths(&self.ctx, paths, at);
    }

    /// The host tab was shown or hidden. Hidden: Timeline playback stops. Shown: the display
    /// profiles are read again, as when the window comes back to the front.
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            monitor_status::came_to_front(&mut self.app);
            self.ctx.request_repaint();
        } else {
            self.app.ui.timeline.playing = false;
        }
    }

    /// Offer files PhotoCraft would open in another application to the host first; `handler`
    /// returns true when it took the file. (Only local paths reach it: PhotoCraft opens nothing
    /// but web links outside itself today.)
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        match self.open_externally.try_borrow_mut() {
            Ok(mut slot) => *slot = Some(handler),
            Err(_) => log::warn!("the open-externally handler is in use; keeping the previous one"),
        }
    }

    /// Links to a sibling ArtCraft app's page (`getartcraft.com/apps/<name>`) call `handler(name)`
    /// instead of opening the page. PhotoCraft shows none today (only its own page, the ArtCraft
    /// site, Discord and GitHub, which stay web links), so this is kept for when it does.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        match self.open_app.try_borrow_mut() {
            Ok(mut slot) => *slot = Some(handler),
            Err(_) => log::warn!("the open-app handler is in use; keeping the previous one"),
        }
    }

    /// Layers dragged out of the app (from the Layers panel, or with the Move tool from the
    /// canvas) once the pointer has left the app's area: "Layer “Sky”" or "3 layers". Inside the
    /// app the drag stays PhotoCraft's own (moving, reordering, copying to another document).
    pub fn outgoing_drag(&self) -> Option<String> {
        self.outgoing.as_ref().map(|o| if o.layers.len() == 1 { format!("Layer “{}”", o.label) } else { o.label.clone() })
    }

    /// The dragged layers alone, trimmed to what they show, as one file in `dir`, in the first
    /// format of `accept` PhotoCraft can write: PSD keeps them as layers, the others are the
    /// flattened image with its transparency (JPEG over white).
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        let Some(o) = self.outgoing.clone().or_else(|| layer_transfer::dragged(&self.app, &self.ctx)) else { return Vec::new() };
        let Some(st) = self.app.session.documents().iter().find(|d| d.doc.id == o.source) else { return Vec::new() };
        let Some(doc) = content::layers_alone(&st.doc, &o.layers) else { return Vec::new() };
        let name = if o.layers.len() == 1 { o.label.clone() } else { format!("{} ({})", content::stem(&st.doc.name), o.label) };
        content::write(&content::trimmed(doc), &name, accept, dir).into_iter().collect()
    }

    /// The drag was dropped in another app: PhotoCraft does nothing with it (no copy into a
    /// document, no reorder, a Move drag moves nothing), even if it never saw the release.
    pub fn cancel_outgoing_drag(&mut self) {
        self.outgoing = None;
        layer_transfer::abandon(&mut self.app, &self.ctx);
    }

    /// Send to: the active document as one file. A saved document whose own file the target
    /// takes is handed over as that file; otherwise it is written to `dir` in the first format of
    /// `accept` PhotoCraft can write (PSD with its layers, else flattened). `None` without a
    /// document.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        let st = self.app.session.active()?;
        if !st.is_dirty()
            && let Some(path) = st.path.as_deref().map(Path::new)
            && content::accepted(path, accept)
            && path.is_file()
        {
            return Some(path.to_path_buf());
        }
        content::write(&st.doc, &content::stem(&st.doc.name), accept, dir)
    }

    /// The engine's commands for the host's agent, with their parameters and whether they can
    /// run now (the menus' dialogs are not among them: the agent passes parameters instead).
    pub fn agent_commands(&mut self, _ctx: &egui::Context) -> Vec<Value> {
        let session = &self.app.session;
        photocraft_engine::commands::command_specs()
            .iter()
            .map(|c| {
                let mut v = json!({ "id": c.id, "label": c.label, "enabled": true });
                if !c.menu.is_empty() {
                    v["menu"] = json!(c.menu.join(" › "));
                }
                if let Some(sc) = c.shortcut {
                    v["shortcut"] = json!(sc);
                }
                if !matches!(c.params.trim(), "" | "{}") {
                    v["params"] = json!(c.params);
                }
                if let Err(reason) = (c.enabled)(session) {
                    v["enabled"] = json!(false);
                    v["disabled_reason"] = json!(reason);
                }
                v
            })
            .collect()
    }

    /// Run an engine command for the host's agent as the control channel's `engine.execute`
    /// does: with its parameters, never a dialog, undoable like the menu item. A command that
    /// runs as a background job replies once the job is done, on a later frame of the app.
    pub fn agent_execute(&mut self, ctx: &egui::Context, command: &str, params: Value) -> Receiver<Value> {
        let params = if params.is_null() { json!({}) } else { params };
        let (req, rx) = ControlRequest::new("engine.execute", json!({ "command": command, "params": params }));
        if photocraft_engine::commands::find(command).is_none() {
            let _ = req.reply.send(json!({ "ok": false, "error": format!("Photocraft has no command `{command}`; app_commands lists them.") }));
            return rx;
        }
        let reply = req.reply.clone();
        let app = &mut self.app;
        if let Err(e) = crate::crash_guard::guard(command, || {
            app.control_now(ctx, req);
            Ok(())
        }) {
            let _ = reply.send(json!({ "ok": false, "error": e }));
        }
        ctx.request_repaint();
        rx
    }

    /// The document's state for the host's agent: `document` (summary and layer tree; `depth`
    /// levels of groups), `layer` (one by `id`, else the active one), `selection` (the selected
    /// layers and the pixel selection's bounds), `history` and `documents` (all open ones).
    pub fn agent_inspect(&mut self, _ctx: &egui::Context, what: &str, p: &Value) -> Result<Value, String> {
        let session = &self.app.session;
        if what == "documents" {
            return Ok(photocraft_engine::inspect::session(session));
        }
        let st = session.active().ok_or(NO_DOCUMENT)?;
        let depth = p.get("depth").and_then(Value::as_u64).unwrap_or(3);
        match what {
            "document" => {
                let mut v = photocraft_engine::inspect::document(st);
                if let Some(layers) = v.get_mut("layers").and_then(Value::as_array_mut) {
                    layers.iter_mut().for_each(|l| prune(l, depth));
                }
                // The undo history can be long: the latest ten steps tell what happened.
                if let Some(list) = v.get_mut("history").and_then(Value::as_array_mut)
                    && list.len() > 10
                {
                    let n = list.len();
                    list.drain(..n - 10);
                    v["historyCount"] = json!(n);
                }
                Ok(v)
            }
            "layer" | "object" | "group" => {
                let id = layer_id(st, p)?;
                let mut v = photocraft_engine::inspect::layer(st.doc.layer(id).ok_or("no layer has that id")?);
                prune(&mut v, depth);
                Ok(v)
            }
            "selection" => {
                let doc = photocraft_engine::inspect::document(st);
                let layers: Vec<Value> = st.selected_layers().iter().filter_map(|id| st.doc.layer(*id)).map(photocraft_engine::inspect::layer).collect();
                Ok(
                    json!({ "layers": layers, "activeLayer": doc["activeLayer"], "hasSelection": doc["hasSelection"], "selectionBounds": doc["selectionBounds"] }),
                )
            }
            "history" => Ok(
                json!({ "history": photocraft_engine::inspect::document(st)["history"], "canUndo": st.history.can_undo(), "canRedo": st.history.can_redo() }),
            ),
            _ => Err(format!("Photocraft has no view “{what}”: use document, layer, selection, history or documents.")),
        }
    }

    /// A picture for the host's agent: `document` (the composite), `layer` (one by `id`, else the
    /// active one, alone and trimmed to what it shows) or `selection` (the selected layers
    /// together), at most `max_side` pixels (never enlarged). Only the document is snapshotted
    /// here; the returned job renders it.
    pub fn agent_render(&mut self, _ctx: &egui::Context, t: &Value) -> Result<(String, AgentRender), String> {
        let st = self.app.session.active().ok_or(NO_DOCUMENT)?;
        let max_side = t.get("max_side").and_then(Value::as_u64).unwrap_or(1024).clamp(16, 4096) as u32;
        let doc: Arc<Document> = st.doc.clone();
        let (caption, layers) = match t.get("target").and_then(Value::as_str).unwrap_or("document") {
            "document" | "page" | "image" => (format!("“{}”, {} × {} px", st.doc.name, st.doc.size.width, st.doc.size.height), None),
            target @ ("layer" | "object" | "group" | "selection") => {
                let ids = if target == "selection" { st.selected_layers() } else { vec![layer_id(st, t)?] };
                let first =
                    ids.first().and_then(|id| st.doc.layer(*id)).ok_or(if target == "selection" { "no layer is selected" } else { "no layer has that id" })?;
                let caption = match &ids[..] {
                    [id] => format!("{} layer “{}” (id {})", first.content.kind_name(), first.name, id.0),
                    _ => format!("{} selected layers", ids.len()),
                };
                (caption, Some(ids))
            }
            other => return Err(format!("Photocraft can't render “{other}”: use document, layer (with `id`) or selection.")),
        };
        let job: AgentRender = Box::new(move || {
            crate::crash_guard::guard("Rendering", || {
                let img = match layers {
                    None => photocraft_compose::thumbnail(&doc, max_side),
                    Some(ids) => {
                        let alone = content::layers_alone(&doc, &ids).ok_or("the layers are gone")?;
                        photocraft_compose::thumbnail(&content::trimmed(alone), max_side)
                    }
                };
                Ok(egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels))
            })
        });
        Ok((caption, job))
    }

    /// Before the app's frame: a layer drag that went out to the host and ended there (released
    /// outside the app's area, or while the app was hidden) does nothing in PhotoCraft.
    fn end_outgoing(&mut self, ctx: &egui::Context) {
        let Some((_, rect)) = self.content.filter(|(v, _)| *v == ctx.viewport_id() && self.outgoing.is_some()) else { return };
        let (pos, down, released) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down(), i.pointer.any_released()));
        let outside = pos.is_none_or(|p| !rect.contains(p));
        if (released && outside) || (!down && !released) {
            layer_transfer::abandon(&mut self.app, ctx);
        }
        if !down && !released {
            self.outgoing = None;
        }
    }

    /// After the app's frame: a layer drag whose pointer left the app's area goes out to the host
    /// (kept through the release, for [`Self::take_outgoing_files`]).
    fn track_outgoing(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let (pos, down, released) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down(), i.pointer.any_released()));
        if down
            && pos.is_some_and(|p| !rect.contains(p))
            && let Some(o) = layer_transfer::dragged(&self.app, ctx)
        {
            self.outgoing = Some(o);
        } else if !down && !released {
            self.outgoing = None;
        }
    }
}

/// The layer `p.id` names, else the active one.
fn layer_id(st: &photocraft_engine::DocState, p: &Value) -> Result<LayerId, String> {
    match p.get("id").filter(|v| !v.is_null()) {
        Some(v) => v.as_u64().map(LayerId).ok_or_else(|| "a layer `id` is a number (from app_inspect)".into()),
        None => st.active_layer.ok_or_else(|| "no layer is active: give its `id`".into()),
    }
}

/// Cut a layer tree below `depth` levels of groups, saying how many layers were left out.
fn prune(layer: &mut Value, depth: u64) {
    let Some(children) = layer.get_mut("children").and_then(Value::as_array_mut) else { return };
    if depth == 0 {
        let n = children.len();
        layer["childCount"] = json!(n);
        if let Some(o) = layer.as_object_mut() {
            o.remove("children");
        }
        return;
    }
    children.iter_mut().for_each(|c| prune(c, depth - 1));
}

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.end_outgoing(ctx);
        self.app.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let (ctx, rect) = (ui.ctx().clone(), ui.max_rect());
        self.content = Some((ctx.viewport_id(), rect));
        self.app.ui(ui, frame);
        self.track_outgoing(&ctx, rect);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw_input);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }

    fn on_exit(&mut self) {
        self.app.on_exit();
    }
}

/// Preferences › Performance as main.rs applies it, on the host's wgpu state: the GPU canvas
/// unless the preferences, `PHOTOCRAFT_CPU_CANVAS` or a software adapter say CPU, with the
/// fallback notice main.rs shows. There is no startup marker here, so no crash fallback.
fn attach_gpu(app: &mut PhotocraftApp, render_state: Option<&eframe::egui_wgpu::RenderState>) {
    let (pref, mode) = gpu_startup::read_rendering_prefs(services::prefs_file().as_deref());
    let env_backend = std::env::var("WGPU_BACKEND").ok();
    let plan = gpu_startup::plan_with_mode(pref, mode, None, env_backend.as_deref(), false, gpu_startup::Os::current());
    let info = &mut app.perf.gpu_info;
    info.preference = pref.name().to_string();
    info.selected = if plan.env.is_some() { "env".into() } else { plan.backend.name().to_string() };
    info.fallback = plan.reason.clone();
    info.canvas = "cpu".into();
    if let Some(rs) = render_state {
        app.perf.gpu_info.set_adapter(&rs.adapter.get_info());
        let software_window = rs.adapter.get_info().device_type == eframe::wgpu::DeviceType::Cpu;
        if software_window && mode != RenderingMode::Cpu {
            app.perf.gpu_info.fallback = Some("No compatible hardware graphics adapter; using software graphics.".into());
        }
        if !software_window
            && plan.backend != GpuBackend::Cpu
            && std::env::var_os("PHOTOCRAFT_CPU_CANVAS").is_none()
            && app.session.prefs().performance.effective_rendering_mode() != RenderingMode::Cpu
        {
            app.set_wgpu(rs.clone());
        } else {
            // The window still draws with wgpu: record its errors instead of panicking.
            let _ = photocraft_ui_egui::gpu_canvas::DeviceHealth::watch(&rs.device);
        }
    }
    let fallback = (app.perf.gpu_info.canvas == "cpu" && mode != RenderingMode::Cpu).then(|| app.perf.gpu_info.fallback.clone()).flatten();
    if let Some(reason) = fallback {
        gpu_status::queue_fallback_notice(app, &reason);
    }
}

/// The windowing system the host's winit uses, from the environment (it picks Wayland when
/// `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set, else X11 on `DISPLAY`).
#[cfg(target_os = "linux")]
fn display_kind() -> Option<tablet::DisplayKind> {
    let set = |key: &str| std::env::var_os(key).is_some_and(|v| !v.is_empty());
    if set("WAYLAND_DISPLAY") || set("WAYLAND_SOCKET") {
        Some(tablet::DisplayKind::Wayland)
    } else if set("DISPLAY") {
        Some(tablet::DisplayKind::X11)
    } else {
        None
    }
}

/// The X11 tablet reader's feed. The reader is started once per process (its thread runs until
/// the X connection closes), so a PhotoCraft tab opened again reuses it.
#[cfg(target_os = "linux")]
fn x11_stylus_feed(display: Option<tablet::DisplayKind>) -> photocraft_ui_egui::stylus::StylusFeed {
    static FEED: std::sync::OnceLock<photocraft_ui_egui::stylus::StylusFeed> = std::sync::OnceLock::new();
    FEED.get_or_init(|| {
        let feed = photocraft_ui_egui::stylus::StylusFeed::default();
        tablet::spawn_x11(&feed, display);
        feed
    })
    .clone()
}

/// `open_url` that first offers local files and sibling apps' pages to the host's handlers (see
/// [`Embedded::set_open_externally`], [`Embedded::set_open_app`]); web links and files it
/// declines go to `open_url`.
fn open_url_via_host(open_url: Option<OpenUrlFn>, host: OpenExternally, apps: OpenApp) -> OpenUrlFn {
    Box::new(move |url: &str| {
        if let Some(path) = local_path(url)
            && let Ok(mut slot) = host.try_borrow_mut()
            && let Some(handler) = slot.as_mut()
            && handler(&path)
        {
            return Ok(());
        }
        if let Some(app) = sibling_app(url)
            && let Ok(mut slot) = apps.try_borrow_mut()
            && let Some(handler) = slot.as_mut()
        {
            handler(app);
            return Ok(());
        }
        match &open_url {
            Some(open) => open(url),
            None => Err(format!("can't open {url}")),
        }
    })
}

/// The ArtCraft apps besides PhotoCraft, by the lowercase name the host uses.
const SIBLING_APPS: [&str; 6] = ["vectorcraft", "lightcraft", "designcraft", "pdfcraft", "filmcraft", "effectcraft"];

/// The sibling app whose product page `url` is (`https://getartcraft.com/apps/<name>`).
fn sibling_app(url: &str) -> Option<&'static str> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let name = rest.strip_prefix("getartcraft.com/apps/")?;
    let name = name.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    SIBLING_APPS.into_iter().find(|app| *app == name)
}

/// The file `url` names when it is a local one: a `file://` URL or an absolute path.
fn local_path(url: &str) -> Option<PathBuf> {
    let path = match url.strip_prefix("file://") {
        Some(rest) => PathBuf::from(percent_decode(rest)?),
        None => PathBuf::from(url),
    };
    path.is_absolute().then_some(path)
}

/// `%XX` escapes decoded (`None` for a malformed escape or a result that isn't UTF-8).
fn percent_decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn only_local_files_are_offered_to_the_host() {
        assert_eq!(local_path("/tmp/a.psd"), Some(PathBuf::from("/tmp/a.psd")));
        assert_eq!(local_path("file:///tmp/a%20b.png"), Some(PathBuf::from("/tmp/a b.png")));
        assert_eq!(local_path("https://discord.gg/artcraft"), None);
        assert_eq!(local_path("file://localhost/tmp/a.png"), None);
        assert_eq!(local_path("file:///tmp/bad%2"), None);
        assert_eq!(local_path("relative.png"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_the_host_takes_is_not_opened_here() {
        let host = OpenExternally::default();
        let seen: Rc<RefCell<Vec<String>>> = Rc::default();
        let log = seen.clone();
        let open = open_url_via_host(
            Some(Box::new(move |url: &str| {
                log.borrow_mut().push(url.to_string());
                Ok(())
            })),
            host.clone(),
            OpenApp::default(),
        );
        let taken: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
        let took = taken.clone();
        *host.borrow_mut() = Some(Box::new(move |p: &Path| {
            took.borrow_mut().push(p.to_path_buf());
            p.extension().is_some_and(|e| e == "psd")
        }));
        open("/tmp/a.psd").unwrap();
        open("/tmp/b.txt").unwrap();
        open("https://example.com").unwrap();
        assert_eq!(*taken.borrow(), [PathBuf::from("/tmp/a.psd"), PathBuf::from("/tmp/b.txt")]);
        assert_eq!(*seen.borrow(), ["/tmp/b.txt", "https://example.com"]);
    }

    #[test]
    fn sibling_app_pages_open_their_tabs_and_other_links_stay_web_links() {
        assert_eq!(sibling_app("https://getartcraft.com/apps/vectorcraft"), Some("vectorcraft"));
        assert_eq!(sibling_app("https://www.getartcraft.com/apps/FilmCraft/?ref=photocraft"), Some("filmcraft"));
        assert_eq!(sibling_app("https://getartcraft.com/apps/photocraft"), None, "PhotoCraft's own page");
        assert_eq!(sibling_app("https://getartcraft.com"), None);
        assert_eq!(sibling_app("https://github.com/storytold/vectorcraft"), None);
        let apps = OpenApp::default();
        let seen: Rc<RefCell<Vec<String>>> = Rc::default();
        let log = seen.clone();
        let open = open_url_via_host(
            Some(Box::new(move |url: &str| {
                log.borrow_mut().push(url.to_string());
                Ok(())
            })),
            OpenExternally::default(),
            apps.clone(),
        );
        open("https://getartcraft.com/apps/lightcraft").unwrap();
        let shown: Rc<RefCell<Vec<String>>> = Rc::default();
        let show = shown.clone();
        *apps.borrow_mut() = Some(Box::new(move |name: &str| show.borrow_mut().push(name.to_string())));
        open("https://getartcraft.com/apps/lightcraft").unwrap();
        open("https://discord.gg/artcraft").unwrap();
        assert_eq!(*shown.borrow(), ["lightcraft"]);
        assert_eq!(*seen.borrow(), ["https://getartcraft.com/apps/lightcraft", "https://discord.gg/artcraft"], "without a handler the page opens");
    }
}
