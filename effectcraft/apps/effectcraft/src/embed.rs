//! EffectCraft in a tab of a host window (Septet). The host owns the window, its chrome, the
//! fonts, the UI zoom and the wgpu device; [`Embedded`] builds the app the way the desktop window
//! does for a plain launch and forwards the host's frames to it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use effectcraft_engine::project::LayerId;
use effectcraft_engine::time::Tick;
use effectcraft_ui_egui::ControlRequest;
use effectcraft_ui_egui::gpu_failure::GpuFailureBridge;
use effectcraft_ui_egui::panels::DragPayload;
use effectcraft_ui_egui::send::{self, Outgoing};
use effectcraft_ui_egui::{Dialog, EffectcraftApp, hosted, menus, panels, prefs_live, theme};
use serde_json::{Value, json};

use crate::desktop;

/// A picture for the host's agent, made on a worker thread ([`Embedded::agent_render`]).
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

/// Files to open or place. They run in the app's next frame, which has its context (the
/// Composition viewer's mapping, dialogs).
enum Request {
    Open(Vec<String>),
    Place(Vec<String>, Option<egui::Pos2>),
}

/// EffectCraft for an embedding host: the app, the opens and places waiting for its next frame,
/// and the content of the last drag out of its panels.
pub struct Embedded {
    app: EffectcraftApp,
    ctx: egui::Context,
    requests: Vec<Request>,
    /// What the last drag from the Project panel or Media Browser carried, as of the last frame
    /// the app drew: the drag may end in another app after this tab was hidden.
    outgoing: Option<Outgoing>,
}

impl Embedded {
    /// The app as a plain launch opens it (Home as Settings ▸ Startup says, an empty Untitled
    /// Project, crash recovery offered), with the native file dialogs and audio output. The
    /// device handlers go on `render_state`'s device; EffectCraft keeps its settings in its config
    /// directory, not in `storage`.
    pub fn new(ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>, storage: Option<&dyn eframe::Storage>) -> Self {
        hosted::set_hosted(true);
        let _ = storage;
        // The font menus list the installed fonts: read their names in the background.
        effectcraft_engine::text::fonts::scan_system_in_background();
        let gpu_failures = GpuFailureBridge::new(ctx);
        if let Some(rs) = render_state {
            desktop::install_device_handlers(rs, &gpu_failures);
        }
        let mut session = desktop::session();
        let recovery = session.begin_recovery();
        let show_home = session.prefs.startup.show_home_on_launch;
        let mut app = EffectcraftApp::new(session);
        app.set_gpu_failure_bridge(gpu_failures);
        app.ui.start_screen = show_home;
        if let Some(r) = recovery {
            app.offer_recovery(r);
        }
        desktop::install_hooks(&mut app);
        Embedded { app, ctx: ctx.clone(), requests: Vec::new(), outgoing: None }
    }

    /// Before [`Embedded::new`]: keep everything EffectCraft stores per user under `root` (created
    /// when missing) instead of the platform's folders: the config directory (settings, shortcut
    /// and ease presets, crash recovery and untitled projects' auto-saves, plug-ins, templates,
    /// scripts, Roto Brush models, logs, Home thumbnails) is `root` itself, user animation presets
    /// go to `root/Presets`, and the disk, media and conformed-audio caches to `root/Cache`.
    /// `None`: the platform's folders (the default).
    pub fn set_data_root(root: Option<PathBuf>) {
        hosted::set_data_root(root);
    }

    /// The fonts EffectCraft installs at startup: Inter (named families `medium` and
    /// `semibold`), JetBrains Mono and the system's Japanese fallback.
    pub fn font_definitions() -> egui::FontDefinitions {
        theme::font_definitions()
    }

    /// The project's file name (`Untitled Project.ecproj` before it is saved); `None` while Home
    /// shows over an untouched untitled project.
    pub fn document_title(&self) -> Option<String> {
        let s = &self.app.session;
        if self.app.ui.start_screen && s.path.is_none() && !s.is_dirty() {
            return None;
        }
        Some(panels::unsaved::project_name(&self.app))
    }

    /// The project has unsaved changes.
    pub fn has_unsaved_changes(&self) -> bool {
        self.app.session.is_dirty()
    }

    /// Open files as the command line does: a project opens (asking to save a modified one
    /// first), media files are imported into it, and Home closes.
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        self.requests.push(Request::Open(strings(paths)));
        self.ctx.request_repaint();
    }

    /// Import files as dropping them at `at` does: in the background, as layers centred there
    /// when `at` is over the Composition viewer, else into the Project panel (a project file
    /// opens). With only Home over an untouched untitled project, the files open instead.
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        if self.document_title().is_none() {
            self.open_paths(paths);
            return;
        }
        if paths.is_empty() {
            return;
        }
        self.requests.push(Request::Place(strings(paths), at));
        self.ctx.request_repaint();
    }

    /// Hidden: the preview stops (with its sound), audio scrubbing closes its output, and queued
    /// prefetch frames are dropped. Shown: the next frame draws.
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            self.ctx.request_repaint();
            return;
        }
        self.app.stop();
        self.app.scrub = None;
        self.app.frames.cancel_prefetch();
    }

    /// Edit Original, Reveal in Finder and Execute File offer their file to `handler` first; it
    /// returns true when the host opened it.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        self.app.hooks.open_externally = Some(handler);
    }

    /// The drag in progress from the Project panel (compositions, footage) or the Media Browser
    /// (files), as the drag ghost's label: `Composition “Main”`, `Footage “clip.mov”`, `3 items`.
    pub fn outgoing_drag(&self) -> Option<String> {
        send::outgoing(&self.app, &self.ctx).map(|out| send::label(&self.app, &out))
    }

    /// The dragged content as files for an app that takes `accept` (extensions, best first):
    /// footage as its source file when its extension is taken (else its first frame as an image),
    /// a composition as its current frame (layered PSD, PNG with alpha, JPEG or EXR) written to
    /// `dir`, Media Browser files whose extension is taken.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        let Some(out) = send::outgoing(&self.app, &self.ctx).or_else(|| self.outgoing.clone()) else { return Vec::new() };
        send::files(&mut self.app, &out, accept, dir)
    }

    /// The drag ended in another app: drop it, so no panel acts on it later.
    pub fn cancel_outgoing_drag(&mut self) {
        self.outgoing = None;
        if egui::DragAndDrop::has_payload_of_type::<DragPayload>(&self.ctx) {
            egui::DragAndDrop::clear_payload(&self.ctx);
        }
    }

    /// Send To: the active composition's current frame (else the selected footage's file) as one
    /// file for an app that takes `accept`; `None` without either.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        send::active_file(&mut self.app, accept, dir)
    }

    /// The Home screen's More apps buttons (and `help.sibling` for an app's page) call
    /// `handler` with the app's lowercase name instead of opening its web page.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        self.app.hooks.open_app = Some(handler);
    }

    /// Run the waiting opens and places, in order. They wait while the unsaved-changes prompt is
    /// up: a project being opened behind it comes before the files that follow it.
    fn run_requests(&mut self, ctx: &egui::Context) {
        while !self.requests.is_empty() && self.app.dialog != Some(Dialog::UnsavedChanges) {
            match self.requests.remove(0) {
                Request::Open(files) => self.open_now(ctx, files),
                Request::Place(files, at) => prefs_live::drop_files(&mut self.app, ctx, files, at),
            }
        }
    }

    fn open_now(&mut self, ctx: &egui::Context, files: Vec<String>) {
        let app = &mut self.app;
        let (projects, media): (Vec<String>, Vec<String>) = files.into_iter().partition(|f| is_project(f));
        if let Some(p) = projects.first()
            && let Err(e) = menus::invoke(app, ctx, "file.open", json!({"path": p}))
        {
            app.ui.status = e;
        }
        app.ui.start_screen = false;
        if media.is_empty() {
            return;
        }
        if app.dialog == Some(Dialog::UnsavedChanges) {
            // The project opens once the prompt is answered; the media go into it then.
            self.requests.insert(0, Request::Open(media));
        } else if let Err(e) = menus::invoke(app, ctx, "file.import", json!({"paths": media, "background": true})) {
            app.ui.status = e;
        }
    }
}

/// An EffectCraft project file (`.ecproj`, or its XML form `.ecprojx`).
fn is_project(path: &str) -> bool {
    let l = path.to_ascii_lowercase();
    l.ends_with(".ecproj") || l.ends_with(".ecprojx")
}

fn strings(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

impl Embedded {
    /// The engine's commands for the host's agent, with their parameters as text and as JSON
    /// Schema, and whether they can run now.
    pub fn agent_commands(&mut self, _ctx: &egui::Context) -> Vec<Value> {
        let session = &self.app.session;
        effectcraft_engine::command_specs()
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
                    v["schema"] = effectcraft_engine::commands::params_schema(c);
                }
                if let Err(reason) = (c.enabled)(session) {
                    v["enabled"] = json!(false);
                    v["disabled_reason"] = json!(reason);
                }
                v
            })
            .collect()
    }

    /// Run an engine command for the host's agent as the control channel's `engine.execute` does:
    /// checked against its parameters, never a dialog, undoable like the menu item. A command that
    /// runs as a background job (renders, analysis) replies when the job ends, on a later frame.
    pub fn agent_execute(&mut self, ctx: &egui::Context, command: &str, params: Value) -> Receiver<Value> {
        let params = if params.is_null() { json!({}) } else { params };
        let (req, rx) = ControlRequest::new("engine.execute", json!({ "command": command, "params": params }));
        if effectcraft_engine::find_command(command).is_none() {
            let _ = req.reply.send(json!({ "ok": false, "error": format!("Effectcraft has no command `{command}`; app_commands lists them.") }));
            return rx;
        }
        let reply = req.reply.clone();
        let app = &mut self.app;
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.control_now(ctx, req))).is_err() {
            let _ = reply.send(json!({ "ok": false, "error": "internal error (please report this bug); the project is unchanged" }));
        }
        ctx.request_repaint();
        rx
    }

    /// The project's state for the host's agent: `document` (its items and the active comp),
    /// `comp` (a composition and its layers, by `id` or the active one), `layer` (a layer's
    /// property tree by `id`, `depth` levels), `property` (`layer`, `path`, `time`), `selection`
    /// (the editor's state: selected layers and keyframes, current time) and `history`.
    pub fn agent_inspect(&mut self, _ctx: &egui::Context, what: &str, p: &Value) -> Result<Value, String> {
        let session = &mut self.app.session;
        let mut query = |id: &str, params: Value| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.execute(id, params).map_err(|e| e.to_string())))
                .unwrap_or_else(|_| Err("internal error (please report this bug)".into()))
        };
        let get = |k: &str| p.get(k).filter(|v| !v.is_null()).cloned();
        match what {
            "document" | "project" | "documents" => {
                let mut v = query("project.summary", json!({}))?;
                v["activeComp"] = json!(self.app.session.active_comp_id().map(|c| c.0));
                v["time"] = json!(self.app.session.time().seconds());
                Ok(v)
            }
            "comp" | "composition" => query("comp.info", json!({ "comp": get("id").or_else(|| get("comp")) })),
            "layer" | "object" => {
                let layer = get("id").or_else(|| get("layer")).ok_or("give the layer's `id` (from `comp`)")?;
                query(
                    "layer.tree",
                    json!({ "layer": layer, "comp": get("comp"), "depth": p.get("depth").and_then(Value::as_u64).unwrap_or(2), "time": get("time") }),
                )
            }
            "property" | "prop" => {
                query("prop.get", json!({ "layer": get("layer").or_else(|| get("id")), "path": get("path"), "prop": get("prop"), "time": get("time") }))
            }
            "selection" | "state" => query("editor.state", json!({})),
            "history" => query("edit.history.list", json!({})),
            _ => Err(format!("Effectcraft has no view “{what}”: use document, comp, layer, property, selection or history.")),
        }
    }

    /// A picture for the host's agent: `frame` (alias `document`, `comp`: the composition `comp`,
    /// else the active one, at `time` seconds, else the current time, over its background), `layer`
    /// (one by `id`: number, `#n` or name, alone on transparency) or `selection` (the selected
    /// layers alone), fitted into `max_side` pixels (never enlarged). The project is shared with
    /// the job, which renders on the CPU.
    pub fn agent_render(&mut self, _ctx: &egui::Context, t: &Value) -> Result<(String, AgentRender), String> {
        let s = &self.app.session;
        let comp = s.resolve_comp(t.get("comp")).map_err(|e| e.to_string())?;
        let c = s.project.comp(comp).ok_or("no composition is open: open a project with septet_open, or make one with app_execute `comp.new`")?;
        let name = s.project.item(comp).map(|i| i.name.clone()).unwrap_or_default();
        let time = t.get("time").and_then(Value::as_f64).map(Tick::from_seconds_f64).unwrap_or_else(|| s.time());
        let max_side = t.get("max_side").and_then(Value::as_u64).unwrap_or(1024).clamp(16, 4096) as u32;
        let at = format!("at {:.2} s", time.seconds());
        let (caption, only, background) = match t.get("target").and_then(Value::as_str).unwrap_or("frame") {
            "frame" | "document" | "comp" | "composition" => (format!("Composition “{name}” {at}, {} × {} px", c.width, c.height), vec![], true),
            target @ ("layer" | "object" | "selection") => {
                let ids = if target == "selection" {
                    s.state.selected_layers.clone()
                } else {
                    vec![layer_ref(c, t.get("id").ok_or("give the layer's `id` (from app_inspect `comp`)")?)?]
                };
                let caption = match &ids[..] {
                    [] => return Err("no layer is selected".into()),
                    [id] => {
                        let l = c.layers.iter().find(|l| l.id == *id).ok_or("no layer has that id")?;
                        format!("Layer “{}” (id {}, {}) {at}", l.name, id.0, l.source.type_name())
                    }
                    ids => format!("{} selected layers {at}", ids.len()),
                };
                (caption, ids, false)
            }
            other => return Err(format!("Effectcraft can't render “{other}”: use frame (with `comp`, `time`), layer (with `id`) or selection.")),
        };
        // Layers render on the whole frame, which the host crops to them: big enough that the crop
        // still fills `max_side`.
        let side = if only.is_empty() { max_side } else { max_side.max(2048) };
        let frame = s.frame_job(comp, time, side, background, &only).map_err(|e| e.to_string())?;
        let job: AgentRender = Box::new(move || {
            let (w, h, px) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(frame)).map_err(|_| "internal error while rendering".to_string())?;
            Ok(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px))
        });
        Ok((caption, job))
    }
}

/// The layer `v` names in `c`: its id, `#n` (its index from the top, as in the Timeline) or its name.
fn layer_ref(c: &effectcraft_engine::project::Comp, v: &Value) -> Result<LayerId, String> {
    let found = match v {
        Value::Number(n) => n.as_u64().map(LayerId).filter(|id| c.layers.iter().any(|l| l.id == *id)),
        Value::String(s) => match s.strip_prefix('#').and_then(|n| n.parse::<usize>().ok()) {
            Some(n) => n.checked_sub(1).and_then(|i| c.layers.get(i)).map(|l| l.id),
            None => c.layers.iter().find(|l| l.name == *s).map(|l| l.id),
        },
        _ => None,
    };
    found.ok_or_else(|| format!("the composition has no layer {v}"))
}

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.logic(ctx, frame);
        self.run_requests(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        if let Some(out) = send::outgoing(&self.app, ui.ctx()) {
            self.outgoing = Some(out);
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw_input);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }

    /// A clean exit: no crash recovery next launch (settings are saved as they change).
    fn on_exit(&mut self) {
        self.app.on_exit();
    }
}
