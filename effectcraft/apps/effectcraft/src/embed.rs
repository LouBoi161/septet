//! EffectCraft in a tab of a host window (Septet). The host owns the window, its chrome, the
//! fonts, the UI zoom and the wgpu device; [`Embedded`] builds the app the way the desktop window
//! does for a plain launch and forwards the host's frames to it.

use std::path::{Path, PathBuf};

use effectcraft_ui_egui::gpu_failure::GpuFailureBridge;
use effectcraft_ui_egui::panels::DragPayload;
use effectcraft_ui_egui::send::{self, Outgoing};
use effectcraft_ui_egui::{Dialog, EffectcraftApp, hosted, menus, panels, prefs_live, theme};
use serde_json::json;

use crate::desktop;

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
