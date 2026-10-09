//! FilmCraft as a tab inside a host shell (Septet).
//!
//! The host owns the window, the `egui::Context` and the wgpu device it shares with its other apps,
//! and calls the [`eframe::App`] methods of [`Embedded`] while FilmCraft's tab is shown. The app is
//! built as a plain `filmcraft` launch builds it (settings, auto-save and crash recovery from the
//! usual data folder, Settings ▸ General ▸ At Startup, audio, file dialogs), without what belongs
//! to the process or the window: no panic hook, control server, native menu, App Nap opt-out or
//! window set-up.
//!
//! Content leaves FilmCraft for the host's other apps as files: the media files of Project panel
//! items dragged out of it (or a frame of them when the other app doesn't take their format), and
//! the Program monitor frame for "Send to".

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Once;
use std::sync::mpsc::{Receiver, channel};

use filmcraft_engine::export::{Format, encode_still};
use filmcraft_engine::project::{ClipId, ItemId, ItemKind, TrackKind};
use filmcraft_engine::render::Image;
use filmcraft_engine::time::Tick;
use filmcraft_ui_egui::{FilmcraftApp, hosted};
use serde_json::{Value, json};

use crate::{audio, desktop};

/// The host's "open this file elsewhere" handler (see [`Embedded::set_open_externally`]).
type OpenExternally = Rc<RefCell<Option<Box<dyn FnMut(&Path) -> bool>>>>;

/// FilmCraft (one per process) for a host's tab.
/// A picture for the host's agent, made on a worker thread ([`Embedded::agent_render`]).
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

/// What the agent hears when there is no sequence to work on.
const NO_SEQUENCE: &str =
    "No sequence is open in Filmcraft: open a project with septet_open, or make a sequence with app_execute `sequence.new` (see its params).";

pub struct Embedded {
    app: FilmcraftApp,
    /// The host's context: FilmCraft keeps its drag state in its `data` (swapped in by the host
    /// around every call).
    ctx: egui::Context,
    open_externally: OpenExternally,
    /// The host's "show this sibling app" handler: FilmCraft has no links to its sibling apps (only
    /// to its makers' community and website, and GitHub), so nothing calls it.
    _open_app: Option<Box<dyn FnMut(&str)>>,
    /// An item drag was in progress when the tab was hidden: if the button is up when it shows
    /// again, the drag ended without any app taking it.
    drag_hidden: bool,
}

impl Embedded {
    /// Build FilmCraft as `filmcraft` without arguments does: the settings, auto-save and crash
    /// recovery in its data folder (the [`Self::set_data_root`] root, else `FILMCRAFT_DATA_DIR`, else
    /// the per-user one), the project Settings ▸
    /// General ▸ At Startup asks for (the demo project by default), the offer to recover unsaved
    /// changes a crashed session left, the GPU compositor on the host's wgpu device (unless
    /// `FILMCRAFT_CPU_COMPOSITE` is set), cpal audio in and out and the native file dialogs.
    /// FilmCraft keeps nothing in eframe's storage.
    pub fn new(ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>, _storage: Option<&dyn eframe::Storage>) -> Self {
        filmcraft_ui_egui::hosted::set_hosted(true);
        // the OS hardware decoders are registered for the whole process
        static HARDWARE_DECODERS: Once = Once::new();
        HARDWARE_DECODERS.call_once(|| {
            desktop::register_hardware_decoders();
        });
        let mut session = desktop::session(None);
        desktop::open_at_startup(&mut session, &[], true, false);
        let mut app = FilmcraftApp::new(session);
        // the host draws the window chrome
        app.integrated_titlebar = false;
        if let Some(rs) = render_state.cloned()
            && std::env::var_os("FILMCRAFT_CPU_COMPOSITE").is_none()
        {
            app.set_wgpu(rs);
        }
        // Settings ▸ Audio Hardware is applied on the first frame (`apply_prefs`).
        app.audio = Some(Box::new(audio::CpalOut::new()));
        desktop::install_file_dialogs(&mut app);
        Self::with_app(ctx, app)
    }

    /// Wrap `app`, its OS `open_path` hook going to the host first: Edit Original and the like,
    /// not revealing a file in the file manager.
    fn with_app(ctx: &egui::Context, mut app: FilmcraftApp) -> Self {
        let open_externally = OpenExternally::default();
        let handler = open_externally.clone();
        app.hooks.open_path = Some(Box::new(move |path: &str, reveal: bool| {
            if !reveal
                && let Ok(mut handler) = handler.try_borrow_mut()
                && let Some(handler) = handler.as_mut()
                && handler(Path::new(path))
            {
                return Ok(());
            }
            desktop::open_path(path, reveal)
        }));
        Self { app, ctx: ctx.clone(), open_externally, _open_app: None, drag_hidden: false }
    }

    /// Before [`Self::new`]: keep everything FilmCraft keeps per user (settings, workspaces, auto-save
    /// and crash recovery, shortcuts, presets, logs, the media cache, downloaded speech models) in
    /// `root`, created if missing, instead of its per-user data folder; None = that folder.
    pub fn set_data_root(root: Option<PathBuf>) {
        if let Some(r) = &root
            && let Err(e) = std::fs::create_dir_all(r)
        {
            log::warn!("can't create the data folder {}: {e}", r.display());
        }
        hosted::set_data_root(root);
    }

    /// The fonts FilmCraft installs at startup (Inter, Inter Medium / SemiBold as the "medium" and
    /// "semibold" families, JetBrains Mono, and the Japanese craft-fonts when built with them).
    pub fn font_definitions() -> egui::FontDefinitions {
        filmcraft_ui_egui::theme::font_definitions()
    }

    /// The project's name. FilmCraft always has a project open (an untitled one at least).
    pub fn document_title(&self) -> Option<String> {
        Some(self.app.session.project.name.clone()).filter(|n| !n.trim().is_empty())
    }

    /// The project has changes that are not saved. (FilmCraft itself doesn't ask on quit: they stay
    /// in its crash-recovery journal and are offered at the next start.)
    pub fn has_unsaved_changes(&self) -> bool {
        self.app.session.is_dirty()
    }

    /// As on the command line: open the first project (`.fcproj`), import the other files into the
    /// project.
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        let files: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        if let Some(project) = files.iter().find(|f| desktop::is_project(f))
            && let Err(e) = self.app.session.execute("file.open", json!({"path": project}))
        {
            self.app.ui.status = e.to_string();
        }
        let media: Vec<String> = files.into_iter().filter(|f| !desktop::is_project(f)).collect();
        if !media.is_empty() {
            self.import(media);
        }
    }

    /// Import the files into the project (as dropping them on the window does) and, when `at` is
    /// over a track of the Timeline, put them there one after the other, as dragging them from the
    /// Project panel to that point does. A project file is opened instead ([`Self::open_paths`]).
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        if paths.iter().any(|p| desktop::is_project(&p.to_string_lossy())) {
            self.open_paths(paths);
            return;
        }
        let files: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        if files.is_empty() {
            return;
        }
        // where on the Timeline, as last drawn (before the import changes anything)
        let target = at.and_then(|p| self.timeline_target(p));
        let Some(imported) = self.import(files) else { return };
        let Some((video, audio, mut time)) = target else { return };
        for item in imported.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64) {
            let placed =
                self.app.session.execute("timeline.place", json!({"item": item, "track": video, "audioTrack": audio, "time": time.0, "insert": false}));
            match placed {
                Ok(r) => {
                    // the next file goes after this one
                    let end = self.app.session.active_sequence().and_then(|seq| {
                        r.get("clips")?.as_array()?.iter().filter_map(Value::as_u64).filter_map(|c| seq.find_item(ClipId(c))).map(|(_, it)| it.end()).max()
                    });
                    if let Some(end) = end {
                        time = time.max(end);
                    }
                }
                Err(e) => self.app.ui.status = e.to_string(),
            }
        }
    }

    /// Hidden: stop playback (and its sound, voice-over and mixer recording) and J/K/L trimming,
    /// which run on the UI clock. FilmCraft does nothing in particular when it comes back.
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            return;
        }
        // (a drag to another app's tab hides this one: the drag goes on)
        self.drag_hidden = !hosted::dragged_items(&self.app, &self.ctx).is_empty();
        self.app.stop();
        if self.app.session.trim_play.active()
            && let Err(e) = self.app.session.execute("trim.shuttleStop", json!({}))
        {
            self.app.ui.status = e.to_string();
        }
    }

    /// `handler(path)` is asked first when FilmCraft opens a file in its default application (Edit
    /// ▸ Edit Original); when it returns true FilmCraft launches nothing.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        if let Ok(mut slot) = self.open_externally.try_borrow_mut() {
            *slot = Some(handler);
        }
    }

    /// Project panel items (footage, stills, sequences) dragged out of the Project panel, a bin or
    /// the Media Browser: "Clip “A001.mov”", "3 clips". None when no item is being dragged.
    pub fn outgoing_drag(&self) -> Option<String> {
        let items = hosted::dragged_items(&self.app, &self.ctx);
        let project = &self.app.session.project;
        let sequence = |i: &ItemId| project.item(*i).is_some_and(|it| matches!(it.kind, ItemKind::Sequence(_)));
        match items.as_slice() {
            [] => None,
            [item] => project.item(*item).map(|it| format!("{} “{}”", kind_name(it), it.name)),
            many if many.iter().all(sequence) => Some(format!("{} sequences", many.len())),
            many if !many.iter().any(sequence) => Some(format!("{} clips", many.len())),
            many => Some(format!("{} items", many.len())),
        }
    }

    /// The dragged items as files: each item's media file when `accept` takes its extension (a
    /// subclip's master clip's), otherwise its poster frame (a third in, as on its thumbnail) as
    /// the first still format of `accept` FilmCraft writes (PNG, TIFF, BMP), named after the item in
    /// `dir`. Audio in a format `accept` lacks has no picture and is left out.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for item in hosted::dragged_items(&self.app, &self.ctx) {
            if let Some(f) = self.item_file(item, accept, dir)
                && !files.contains(&f)
            {
                files.push(f);
            }
        }
        files
    }

    /// The drag went to another app: forget it, so no panel acts on it later (a file imported for a
    /// Media Browser drag is taken back).
    pub fn cancel_outgoing_drag(&mut self) {
        self.drag_hidden = false;
        hosted::cancel_drag(&mut self.app, &self.ctx);
    }

    /// "Send to": the Program monitor frame at the playhead (as File ▸ Export Frame renders it), or
    /// with no sequence open the Source monitor's, as the first still format of `accept` FilmCraft
    /// writes (PNG, TIFF, BMP), named after the sequence or clip in `dir`.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        let s = &self.app.session;
        if let Some(seq) = s.state.active_sequence.and_then(|i| s.project.item(i)) {
            return match s.try_render_program_at(1.0, s.playhead()) {
                Ok(img) => write_still(&img, &seq.name, accept, dir),
                Err(e) => {
                    log::warn!("send to: can't render the Program monitor frame: {e}");
                    None
                }
            };
        }
        let item = s.state.source_item?;
        let it = s.project.item(item)?;
        let provider = s.media.provider(s.project.clone(), s.services.clone());
        let img = filmcraft_engine::render::render_item(&s.project, item, s.state.source_playhead, 1.0, &provider)?;
        write_still(&img, &file_stem(it), accept, dir)
    }

    /// When hosted, links to a sibling app would call `handler(name)`; FilmCraft has none (its Help
    /// links go to its makers' community and website, and GitHub), so the handler is kept unused.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        self._open_app = Some(handler);
    }

    /// One dragged item as a file (see [`Self::take_outgoing_files`]).
    fn item_file(&self, item: ItemId, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        let s = &self.app.session;
        if let Some(path) = hosted::source_file(&s.project, item).map(PathBuf::from)
            && path.extension().is_some_and(|e| accept.iter().any(|a| e.eq_ignore_ascii_case(a)))
            && path.is_file()
        {
            return Some(path);
        }
        let it = s.project.item(item)?;
        // the poster frame, else a third in (as the Project panel's thumbnails)
        let t = filmcraft_engine::keyboard::poster_frame(it).unwrap_or(Tick(it.duration().0.saturating_mul(3) / 10));
        let provider = s.media.provider(s.project.clone(), s.services.clone());
        let img = filmcraft_engine::render::render_item(&s.project, item, t, 1.0, &provider)?;
        write_still(&img, &file_stem(it), accept, dir)
    }

    /// `file.import` with the errors shown in the status bar; the command's result.
    fn import(&mut self, paths: Vec<String>) -> Option<Value> {
        match self.app.session.execute("file.import", json!({"paths": paths})) {
            Ok(r) => {
                if let Some(errs) = r.get("errors").and_then(Value::as_array).filter(|e| !e.is_empty()) {
                    self.app.ui.status = errs.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ");
                }
                Some(r)
            }
            Err(e) => {
                self.app.ui.status = e.to_string();
                None
            }
        }
    }

    /// The video track, audio track and time at screen point `p` on the Timeline as it was drawn
    /// in the last frame, the way an item dragged from the Project panel and dropped there is
    /// placed. None when the Timeline isn't showing there.
    fn timeline_target(&self, p: egui::Pos2) -> Option<(Option<u64>, Option<u64>, Tick)> {
        let app = &self.app;
        // the Timeline panel was drawn there (its layout is kept while it is hidden)
        let shown = app.auto.elements.iter().any(|e| {
            let [x, y, w, h] = e.rect;
            e.id == "panel.Timeline" && egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)).contains(p)
        });
        if !shown {
            return None;
        }
        let layout = app.tl.layout.as_ref().filter(|l| l.content.contains(p))?;
        let row = layout.row_at(p.y)?;
        let seq = app.session.active_sequence()?;
        let time = app.session.sequence_rate().snap_nearest(layout.tick_at(p.x).max(Tick::ZERO));
        let (video, audio) = match row.kind {
            TrackKind::Video => (Some(row.track.0), seq.audio_tracks.get(row.index).or(seq.audio_tracks.first()).map(|t| t.id.0)),
            TrackKind::Audio => (seq.video_tracks.get(row.index).or(seq.video_tracks.first()).map(|t| t.id.0), Some(row.track.0)),
        };
        Some((video, audio, time))
    }
}

impl Embedded {
    /// The engine's commands for the host's agent, with their parameters and whether they can
    /// run now.
    pub fn agent_commands(&mut self, _ctx: &egui::Context) -> Vec<Value> {
        let s = &self.app.session;
        filmcraft_engine::command_specs()
            .iter()
            .map(|c| {
                let mut v = json!({ "id": c.id, "label": c.label, "enabled": true });
                if !c.menu.is_empty() {
                    v["menu"] = json!(c.menu.join(" › "));
                }
                if let Some(sc) = s.shortcuts.primary(c.id) {
                    v["shortcut"] = json!(sc);
                }
                if !matches!(c.params.trim(), "" | "{}") {
                    v["params"] = json!(c.params);
                }
                if let Err(reason) = (c.enabled)(s) {
                    v["enabled"] = json!(false);
                    v["disabled_reason"] = json!(reason);
                }
                v
            })
            .collect()
    }

    /// Run a command for the host's agent as the control channel's `engine.execute` does (the
    /// menus' dispatcher, so it is undoable like the menu item). A command that would open a
    /// dialog or a file picker instead (it needs parameters), or open a file in another program,
    /// fails and leaves nothing open: the agent can't answer it and the user didn't ask for it.
    /// The reply is always there at once.
    pub fn agent_execute(&mut self, ctx: &egui::Context, command: &str, params: Value) -> Receiver<Value> {
        let (tx, rx) = channel();
        let params = if params.is_null() { json!({}) } else { params };
        let before = open_dialogs(&self.app);
        let asked = Rc::new(Cell::new(None::<&'static str>));
        let ask = |what: &'static str| {
            let a = asked.clone();
            move || a.set(Some(what))
        };
        // No file dialogs and no other programs for the agent: note that the command asked.
        let h = &mut self.app.hooks;
        let (files, save, save_as, project, file, folder, relink, open) = (
            ask("pick files to import"),
            ask("choose where to save"),
            ask("choose where to save"),
            ask("pick a project"),
            ask("pick a file"),
            ask("pick a folder"),
            ask("pick a file"),
            ask("open a file in another program"),
        );
        let saved = (
            h.pick_files.replace(Box::new(move |_| {
                files();
                Vec::new()
            })),
            h.pick_save.replace(Box::new(move |_| {
                save();
                None
            })),
            h.pick_save_as.replace(Box::new(move |_, _, _| {
                save_as();
                None
            })),
            h.pick_open_project.replace(Box::new(move || {
                project();
                None
            })),
            h.pick_open_file.replace(Box::new(move |_, _| {
                file();
                None
            })),
            h.pick_folder.replace(Box::new(move || {
                folder();
                None
            })),
            h.pick_file_for_relink.replace(Box::new(move |_, _| {
                relink();
                None
            })),
            h.open_path.replace(Box::new(move |_, _| {
                open();
                Err("not for the agent".into())
            })),
        );
        let app = &mut self.app;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| filmcraft_ui_egui::menus::invoke(app, ctx, command, params)))
            .unwrap_or_else(|_| Err("internal error (please report this bug); the project is unchanged".into()));
        let h = &mut self.app.hooks;
        (h.pick_files, h.pick_save, h.pick_save_as, h.pick_open_project, h.pick_open_file, h.pick_folder, h.pick_file_for_relink, h.open_path) = saved;
        let opened = close_new_dialogs(&mut self.app, &before);
        let reply = match (r, opened, asked.get()) {
            (_, Some(dialog), _) => {
                json!({ "ok": false, "error": format!("`{command}` opened the {dialog} dialog, which waits for the user, so it was closed again: run the command with the parameters it lists instead.") })
            }
            (_, None, Some(what)) => json!({ "ok": false, "error": format!("`{command}` asks the user to {what}: pass the path(s) it lists instead.") }),
            (Ok(v), None, None) => json!({ "ok": true, "result": v }),
            (Err(e), None, None) => json!({ "ok": false, "error": e }),
        };
        ctx.request_repaint();
        // The receiver is ours until we return.
        let _ = tx.send(reply);
        rx
    }

    /// The project's state for the host's agent: `document` (the project's bins and items),
    /// `sequence` (tracks and clips of the sequence `id`, else the active one), `clip` (one clip
    /// of it by `id`), `selection` (the editor's state: selection, playhead, In/Out) and `history`.
    pub fn agent_inspect(&mut self, _ctx: &egui::Context, what: &str, p: &Value) -> Result<Value, String> {
        let session = &mut self.app.session;
        let mut query = |id: &str, params: Value| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.execute(id, params).map_err(|e| e.to_string())))
                .unwrap_or_else(|_| Err("internal error (please report this bug)".into()))
        };
        let id = p.get("id").filter(|v| !v.is_null()).cloned();
        match what {
            "document" | "project" => query("project.inspect", json!({})),
            "sequence" => {
                let mut v = query("sequence.inspect", json!({ "item": id }))?;
                // Times in the answer are ticks; commands also take seconds, frames or timecode.
                v["ticksPerSecond"] = json!(Tick::from_seconds_f64(1.0).0);
                Ok(v)
            }
            "clip" | "object" | "layer" => {
                let clip = id.and_then(|v| v.as_u64()).ok_or("give the clip's `id` (from `sequence`)")?;
                let seq = query("sequence.inspect", json!({ "item": p.get("sequence") }))?;
                find_id(&seq, clip).cloned().ok_or_else(|| "the sequence has no clip with that id".to_string())
            }
            "selection" | "state" => query("state.inspect", json!({})),
            "history" => query("history.list", json!({})),
            _ => Err(format!("Filmcraft has no view “{what}”: use document, sequence, clip, selection or history.")),
        }
    }

    /// A picture for the host's agent: `frame` (alias `document`, `sequence`: the Program monitor's
    /// picture of the sequence `id`, else the active one, at `time` seconds, else the playhead),
    /// `clip` (one clip by `id` alone, at `time` or the playhead when it is in the clip, else its
    /// middle), `selection` (the first selected clip, so) or `item` (a project item by `id`, at
    /// `time` or its poster frame), fitted into
    /// `max_side` pixels (never enlarged). The project is shared with the job.
    pub fn agent_render(&mut self, _ctx: &egui::Context, t: &Value) -> Result<(String, AgentRender), String> {
        let s = &self.app.session;
        let max_side = t.get("max_side").and_then(Value::as_u64).unwrap_or(1024).clamp(16, 4096) as f32;
        let time = t.get("time").and_then(Value::as_f64).filter(|v| v.is_finite()).map(Tick::from_seconds_f64);
        let at = |t: Tick| format!("at {:.2} s", t.seconds());
        let project = s.project.clone();
        let provider = s.media.provider(s.project.clone(), s.services.clone());
        let id = t.get("id").and_then(Value::as_u64);
        let target = t.get("target").and_then(Value::as_str).unwrap_or("frame");
        let (caption, job): (String, Box<dyn FnOnce() -> Option<Image> + Send>) = match target {
            "frame" | "document" | "sequence" | "clip" | "object" | "layer" | "selection" => {
                let seq_id = match (target, id) {
                    ("sequence", Some(id)) => ItemId(id),
                    _ => t.get("sequence").and_then(Value::as_u64).map(ItemId).or(s.state.active_sequence).ok_or(NO_SEQUENCE)?,
                };
                let seq = s.project.sequence(seq_id).ok_or(NO_SEQUENCE)?;
                let name = s.project.item(seq_id).map(|i| i.name.clone()).unwrap_or_default();
                let (w, h) = (seq.settings.width.max(1) as f32, seq.settings.height.max(1) as f32);
                if matches!(target, "clip" | "object" | "layer" | "selection") {
                    let clip = match target {
                        "selection" => *s.state.selection.first().ok_or("no clip is selected")?,
                        _ => ClipId(id.ok_or("give the clip's `id` (from app_inspect `sequence`)")?),
                    };
                    let (_, item) = seq.find_item(clip).ok_or("the sequence has no clip with that id")?;
                    let inside = |t: Tick| t >= item.start && t < item.end();
                    let t = time.or(Some(s.playhead()).filter(|t| inside(*t))).unwrap_or(Tick(item.start.0.saturating_add(item.duration.0 / 2)));
                    // The clip is drawn where it sits in the frame, which the host crops to it: big
                    // enough that the crop still fills `max_side`.
                    let scale = (max_side.max(2048.0) / w.max(h)).min(1.0);
                    let opts = filmcraft_engine::render::RenderOptions { scale, ..Default::default() };
                    let caption = format!("Clip “{}” (id {}) of “{name}” {}", item.name, clip.0, at(t));
                    (caption, Box::new(move || filmcraft_engine::render::render_clip(&project, seq_id, clip, t, opts, &provider)))
                } else {
                    let t = time.unwrap_or_else(|| s.playhead());
                    let scale = (max_side / w.max(h)).min(1.0);
                    let opts = filmcraft_engine::render::RenderOptions { scale, captions: true, ..Default::default() };
                    let caption = format!("Sequence “{name}” {}, {} × {} px", at(t), seq.settings.width, seq.settings.height);
                    (caption, Box::new(move || Some(filmcraft_engine::render::render_sequence(&project, seq_id, t, opts, &provider))))
                }
            }
            "item" => {
                let item = ItemId(id.ok_or("give the project item's `id` (from app_inspect `document`)")?);
                let it = s.project.item(item).ok_or("the project has no item with that id")?;
                let t = time.unwrap_or_else(|| filmcraft_engine::keyboard::poster_frame(it).unwrap_or(Tick(it.duration().0.saturating_mul(3) / 10)));
                let scale = match it.kind {
                    ItemKind::Sequence(ref q) => (max_side / (q.settings.width.max(q.settings.height).max(1) as f32)).min(1.0),
                    _ => 1.0,
                };
                let caption = format!("{} “{}” (id {}) {}", kind_name(it), it.name, item.0, at(t));
                (caption, Box::new(move || filmcraft_engine::render::render_item(&project, item, t, scale, &provider)))
            }
            other => return Err(format!("Filmcraft can't render “{other}”: use frame (with `time`), clip (with `id`), selection or item (with `id`).")),
        };
        // A frame is what the Program monitor shows: black where no clip is.
        let on_black = matches!(target, "frame" | "document" | "sequence");
        let job: AgentRender = Box::new(move || {
            let img = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                .map_err(|_| "internal error while rendering".to_string())?
                .ok_or("nothing to show there (no picture at that time)")?;
            let mut px = img.to_rgba8();
            if on_black {
                for p in px.chunks_exact_mut(4) {
                    let a = u32::from(p[3]);
                    for c in &mut p[..3] {
                        *c = ((u32::from(*c) * a + 127) / 255) as u8;
                    }
                    p[3] = 255;
                }
            }
            Ok(egui::ColorImage::from_rgba_unmultiplied([img.w, img.h], &px))
        });
        Ok((caption, job))
    }
}

/// The dialogs open in `app`: (name, open).
fn open_dialogs(app: &FilmcraftApp) -> Vec<(&'static str, bool)> {
    let u = &app.ui;
    vec![
        ("app", app.dialog.is_some()),
        ("Settings", u.settings.is_some()),
        ("Link Media", u.link_media.is_some()),
        ("Create Proxies", u.create_proxies.is_some()),
        ("Project Manager", u.project_manager.is_some()),
        ("Make Offline", u.make_offline.is_some()),
        ("colour", u.color_dialog.is_some()),
        ("Save Preset", u.save_preset.is_some()),
        ("Synchronize", u.sync_dialog.is_some()),
        ("Edit Cameras", u.edit_cameras.is_some()),
        ("guide", u.guide_dialog.is_some()),
        ("Text Properties", u.text_props_dialog.is_some()),
        ("Workspaces", u.workspace_dialog.is_some()),
        ("clip", u.clip_dialog.is_some()),
        ("menu", u.extras.dialog.is_some()),
    ]
}

/// Close the dialogs that opened since `before` → the name of one of them.
fn close_new_dialogs(app: &mut FilmcraftApp, before: &[(&'static str, bool)]) -> Option<String> {
    let now = open_dialogs(app);
    let opened: Vec<&str> = now.iter().zip(before).filter(|((_, n), (_, b))| *n && !*b).map(|((name, _), _)| *name).collect();
    for name in &opened {
        let u = &mut app.ui;
        match *name {
            "app" => app.dialog = None,
            "Settings" => u.settings = None,
            "Link Media" => u.link_media = None,
            "Create Proxies" => u.create_proxies = None,
            "Project Manager" => u.project_manager = None,
            "Make Offline" => u.make_offline = None,
            "colour" => u.color_dialog = None,
            "Save Preset" => u.save_preset = None,
            "Synchronize" => u.sync_dialog = None,
            "Edit Cameras" => u.edit_cameras = None,
            "guide" => u.guide_dialog = None,
            "Text Properties" => u.text_props_dialog = None,
            "Workspaces" => u.workspace_dialog = None,
            "clip" => u.clip_dialog = None,
            _ => u.extras.dialog = None,
        }
    }
    opened.first().map(|n| format!("“{n}”"))
}

/// The clip `id` in `sequence.inspect`'s answer `v`.
fn find_id(v: &Value, id: u64) -> Option<&Value> {
    match v {
        Value::Object(o) if o.get("clip").and_then(Value::as_u64) == Some(id) && o.contains_key("start") => Some(v),
        Value::Object(o) => o.values().find_map(|c| find_id(c, id)),
        Value::Array(a) => a.iter().find_map(|c| find_id(c, id)),
        _ => None,
    }
}

/// What an item is, for the drag label.
fn kind_name(it: &filmcraft_engine::project::ProjectItem) -> &'static str {
    match &it.kind {
        ItemKind::Sequence(_) => "Sequence",
        ItemKind::Media(m) if !m.info.has_video() => "Audio",
        _ => "Clip",
    }
}

/// A file name for an item: its name without a file extension (sequences keep theirs whole).
fn file_stem(it: &filmcraft_engine::project::ProjectItem) -> String {
    let p = Path::new(&it.name);
    let ext = p.extension().and_then(|e| e.to_str()).filter(|e| (2..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()));
    match (ext, p.file_stem()) {
        (Some(_), Some(stem)) if !matches!(it.kind, ItemKind::Sequence(_)) => stem.to_string_lossy().into_owned(),
        _ => it.name.clone(),
    }
}

/// Encode `img` as the first of `accept` FilmCraft writes (PNG, TIFF, BMP, as Export Frame does)
/// into a new file `<name>.<ext>` in `dir` (`<name> 2.<ext>`… when taken: files handed to other apps
/// stay linked there).
fn write_still(img: &Image, name: &str, accept: &[&str], dir: &Path) -> Option<PathBuf> {
    let (ext, format) = accept.iter().find_map(|a| match a.to_ascii_lowercase().as_str() {
        "png" => Some(("png", Format::PngSequence)),
        "tif" => Some(("tif", Format::TiffSequence)),
        "tiff" => Some(("tiff", Format::TiffSequence)),
        "bmp" => Some(("bmp", Format::BmpSequence)),
        _ => None,
    })?;
    let (w, h) = (u32::try_from(img.w).ok()?, u32::try_from(img.h).ok()?);
    let bytes = match encode_still(format, img.to_rgba8(), w, h) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("can't encode {name}.{ext}: {e}");
            return None;
        }
    };
    let base: String =
        name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    let base = match base.trim().trim_start_matches('.') {
        "" => "Frame".to_string(),
        b => b.to_string(),
    };
    let path = (1..10_000).map(|n| if n == 1 { dir.join(format!("{base}.{ext}")) } else { dir.join(format!("{base} {n}.{ext}")) }).find(|p| !p.exists())?;
    match std::fs::write(&path, bytes) {
        Ok(()) => Some(path),
        Err(e) => {
            log::warn!("can't write {}: {e}", path.display());
            None
        }
    }
}

impl eframe::App for Embedded {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw_input);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.drag_hidden) && !ctx.input(|i| i.pointer.any_down()) {
            // an item drag ended while the tab was hidden and no app took it
            self.cancel_outgoing_drag();
        }
        self.app.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }

    fn on_exit(&mut self) {
        // flush the recovery journal and stop the auto-save worker, as the standalone app does
        self.app.on_exit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, pos2};
    use filmcraft_engine::Session;
    use filmcraft_ui_egui::automation::Element;
    use filmcraft_ui_egui::panels::timeline::{Layout, Row};

    /// The demo project, without the data folder, audio devices or file dialogs of `new`.
    fn embedded() -> Embedded {
        let mut session = Session::default();
        session.execute("file.openDemoProject", json!({})).unwrap();
        Embedded::with_app(&egui::Context::default(), FilmcraftApp::new(session))
    }

    /// A silent 48 kHz mono WAV `seconds` long.
    fn wav(path: &Path, seconds: u32) {
        let (rate, n) = (48_000_u32, 48_000 * seconds);
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + n * 2).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16_u32.to_le_bytes());
        b.extend(1_u16.to_le_bytes());
        b.extend(1_u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2).to_le_bytes());
        b.extend(2_u16.to_le_bytes());
        b.extend(16_u16.to_le_bytes());
        b.extend(b"data");
        b.extend((n * 2).to_le_bytes());
        b.resize(b.len() + n as usize * 2, 0);
        std::fs::write(path, b).unwrap();
    }

    fn files(name: &str) -> (PathBuf, Vec<PathBuf>) {
        let dir = std::env::temp_dir().join(format!("filmcraft-embed-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<PathBuf> = ["one.wav", "two.wav"].iter().map(|f| dir.join(f)).collect();
        for p in &paths {
            wav(p, 1);
        }
        (dir, paths)
    }

    /// The Timeline as if drawn with A1 at y 600–700, 10 points a second from x 200.
    fn draw_timeline(e: &mut Embedded) {
        let seq = e.app.session.active_sequence().unwrap();
        let (v1, a1) = (seq.video_tracks[0].id, seq.audio_tracks[0].id);
        let row = |track, kind, y: f32| Row { track, kind, index: 0, rect: Rect::from_min_max(pos2(200.0, y), pos2(1600.0, y + 100.0)) };
        e.app.tl.layout = Some(Layout {
            content: Rect::from_min_max(pos2(200.0, 500.0), pos2(1600.0, 900.0)),
            ruler: Rect::from_min_max(pos2(200.0, 470.0), pos2(1600.0, 500.0)),
            rows: vec![row(v1, TrackKind::Video, 500.0), row(a1, TrackKind::Audio, 600.0)],
            pps: 10.0,
            scroll: 0.0,
            split_y: 600.0,
        });
        e.app.auto.elements.push(Element { id: "panel.Timeline".into(), label: "Timeline".into(), rect: [0.0, 440.0, 1600.0, 460.0] });
    }

    fn clips_on_a1(e: &Embedded) -> Vec<(Tick, Tick)> {
        let seq = e.app.session.active_sequence().unwrap();
        let mut c: Vec<_> = seq.audio_tracks[0].items.iter().map(|it| (it.start, it.end())).collect();
        c.sort();
        c
    }

    #[test]
    fn files_placed_over_the_timeline_go_one_after_the_other_from_the_track_and_frame_under_the_point() {
        let (dir, paths) = files("place");
        let mut e = embedded();
        draw_timeline(&mut e);
        let before = clips_on_a1(&e);
        let items = e.app.session.project.items.len();
        // frame 2400 at 23.976 fps is 100.1 s: x = 200 + 1001 points
        e.place_paths(&paths, Some(pos2(1201.0, 650.0)));
        assert_eq!(e.app.session.project.items.len(), items + 2, "both imported");
        let start = filmcraft_engine::time::FrameRate::FPS_23_976.tick_of(2400);
        let added: Vec<_> = clips_on_a1(&e).into_iter().filter(|c| !before.contains(c)).collect();
        assert_eq!(added.len(), 2, "{added:?}");
        assert_eq!(added[0].0, start);
        assert_eq!(added[1].0, added[0].1, "the second file follows the first");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn files_placed_elsewhere_are_only_imported() {
        let (dir, paths) = files("import");
        let mut e = embedded();
        draw_timeline(&mut e);
        let before = clips_on_a1(&e);
        let items = e.app.session.project.items.len();
        // over the track headers, then with the Timeline not drawn in the last frame
        e.place_paths(&paths[..1], Some(pos2(100.0, 650.0)));
        e.app.auto.elements.clear();
        e.place_paths(&paths[1..], Some(pos2(1201.0, 650.0)));
        e.place_paths(&paths[1..], None);
        assert_eq!(e.app.session.project.items.len(), items + 3);
        assert_eq!(clips_on_a1(&e), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn opening_a_file_elsewhere_asks_the_host_first() {
        let mut e = embedded();
        let asked = Rc::new(RefCell::new(Vec::new()));
        let seen = asked.clone();
        e.set_open_externally(Box::new(move |p: &Path| {
            seen.borrow_mut().push(p.to_path_buf());
            true
        }));
        let open = e.app.hooks.open_path.as_mut().unwrap();
        assert_eq!(open("/media/clip.mov", false), Ok(()));
        assert_eq!(*asked.borrow(), [PathBuf::from("/media/clip.mov")]);
    }

    #[test]
    fn hiding_stops_playback_and_the_title_is_the_project_name() {
        let mut e = embedded();
        e.app.play(1.0);
        assert!(e.app.playback.playing);
        e.set_visible(false);
        assert!(!e.app.playback.playing);
        assert_eq!(e.document_title(), Some(e.app.session.project.name.clone()));
        assert!(!e.has_unsaved_changes());
    }

    #[test]
    fn the_fonts_are_the_ones_filmcraft_installs() {
        let fonts = Embedded::font_definitions();
        for family in filmcraft_ui_egui::theme::font_families() {
            assert!(fonts.families.contains_key(&family), "{family:?}");
        }
        assert_eq!(fonts.families[&egui::FontFamily::Proportional][0], "inter");
        assert_eq!(fonts.families[&egui::FontFamily::Monospace][0], "jbmono");
    }

    /// Start dragging `item` out of the Project panel.
    fn drag(e: &Embedded, item: ItemId) {
        let mut out = e.ctx.run_ui(egui::RawInput::default(), |ui| filmcraft_ui_egui::panels::start_drag_item(ui, item));
        out.textures_delta.clear();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filmcraft-embed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_dragged_clip_leaves_as_its_file_or_as_a_still_of_it() {
        let (src, out) = (scratch("drag-src"), scratch("drag-out"));
        let png = src.join("still.png");
        std::fs::write(&png, encode_still(Format::PngSequence, Image::new(32, 18).to_rgba8(), 32, 18).unwrap()).unwrap();
        let mut e = embedded();
        let r = e.app.session.execute("file.import", json!({"paths": [png.to_string_lossy()]})).unwrap();
        let item = ItemId(r["items"][0].as_u64().unwrap());
        assert_eq!(e.outgoing_drag(), None);
        drag(&e, item);
        assert_eq!(e.outgoing_drag().as_deref(), Some("Clip “still.png”"));
        // the original when the other app takes PNG, else a still in the first format FilmCraft writes
        assert_eq!(e.take_outgoing_files(&["psd", "png"], &out), [png.clone()]);
        assert_eq!(e.take_outgoing_files(&["psd", "tif", "png"], &out), [png.clone()], "the original wherever the other app lists it");
        assert_eq!(e.take_outgoing_files(&["psd", "tif"], &out), [out.join("still.tif")]);
        assert_eq!(e.take_outgoing_files(&["tif"], &out), [out.join("still 2.tif")], "earlier files stay");
        assert!(e.take_outgoing_files(&["svg", "pdf"], &out).is_empty());
        // a selection drags along
        e.app.session.state.project_selection = vec![item, item];
        assert_eq!(e.outgoing_drag().as_deref(), Some("2 clips"));
        e.cancel_outgoing_drag();
        assert_eq!(e.outgoing_drag(), None);
        std::fs::remove_dir_all(src).unwrap();
        std::fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn a_dragged_sequence_leaves_as_a_frame() {
        let out = scratch("drag-seq");
        let mut e = embedded();
        let seq = e.app.session.state.active_sequence.unwrap();
        let name = e.app.session.project.item(seq).unwrap().name.clone();
        drag(&e, seq);
        assert_eq!(e.outgoing_drag(), Some(format!("Sequence “{name}”")));
        let files = e.take_outgoing_files(&["mp4", "png"], &out);
        assert_eq!(files, [out.join(format!("{name}.png"))]);
        assert!(std::fs::read(&files[0]).unwrap().starts_with(b"\x89PNG"));
        std::fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn send_to_hands_over_the_program_frame() {
        let out = scratch("send");
        let mut e = embedded();
        let seq = e.app.session.state.active_sequence.unwrap();
        let name = e.app.session.project.item(seq).unwrap().name.clone();
        let file = e.export_active(&["psd", "png"], &out).unwrap();
        assert_eq!(file, out.join(format!("{name}.png")));
        assert!(std::fs::read(&file).unwrap().starts_with(b"\x89PNG"));
        // an empty project has nothing to send
        let mut empty = Embedded::with_app(&egui::Context::default(), FilmcraftApp::new(Session::default()));
        assert_eq!(empty.export_active(&["png"], &out), None);
        std::fs::remove_dir_all(out).unwrap();
    }

    /// A portable install keeps the settings, auto-save, recovery and models under its data root.
    #[test]
    fn a_data_root_holds_all_per_user_data() {
        let root = scratch("root").join("Data").join("Filmcraft");
        Embedded::set_data_root(Some(root.clone()));
        assert!(root.is_dir(), "created");
        assert_eq!(filmcraft_engine::autosave::default_data_dir(), Some(root.clone()));
        assert_eq!(filmcraft_engine::transcript::models_dir(), Some(root.join("models")));
        let mut session = desktop::session(None);
        assert_eq!(session.prefs_path.as_deref(), Some(root.join("preferences.json").as_path()));
        session.shutdown();
        Embedded::set_data_root(None);
        assert_ne!(filmcraft_engine::autosave::default_data_dir(), Some(root.clone()));
        let _ = std::fs::remove_dir_all(root.parent().and_then(Path::parent).unwrap());
    }
}
