//! The shell: windows, their tabs, and the apps behind them.
//!
//! Every window has a tab strip; a tab shows either the Home screen or one of the apps. Each app runs
//! at most once (like the apps themselves, which keep several documents in their own tabs), so an
//! app's tab lives in exactly one window. Tabs can be reordered, dragged into another window or torn
//! off into a new one. The first window is eframe's root viewport; the others are immediate
//! viewports rendered from the root's frame.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;

use egui::{Context, Pos2, Rect, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId};

use crate::bridge::Bridge;
use crate::home::HomeState;
use crate::hosted::{AppSlot, CloseState};
use crate::kinds::AppKind;
use crate::recent::Recent;
use crate::router::{self, AppRequest, Router};

pub type TabId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TabKind {
    Home,
    App(AppKind),
}

#[derive(Clone, Copy, Debug)]
pub struct Tab {
    pub id: TabId,
    pub kind: TabKind,
}

pub struct Window {
    pub viewport: ViewportId,
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Closing this window: its tabs are being closed one after the other.
    pub closing: bool,
    /// The root window, hidden because all its tabs moved away while other windows remain.
    pub hidden: bool,
    /// How a torn-off window opens (kept constant: egui sends only changes of it to the window).
    pub builder: ViewportBuilder,
    /// The tab strip, last frame (window coordinates).
    pub strip: Rect,
    /// Where each tab was drawn, last frame.
    pub tab_rects: Vec<(TabId, Rect)>,
    /// The title last sent to the OS.
    pub title: String,
    /// Outer position and inner size last frame, for restoring the window next time.
    pub geometry: Option<([f32; 2], [f32; 2])>,
}

impl Window {
    pub fn active_tab(&self) -> Option<Tab> {
        self.tabs.get(self.active).copied()
    }
}

/// A tab being dragged with the mouse.
#[derive(Clone, Debug)]
pub struct TabDrag {
    pub tab: TabId,
    pub from: ViewportId,
    /// Pointer minus the tab's top-left corner when the drag started.
    pub grab: Vec2,
    pub size: Vec2,
    /// Pulled out of the strip: dropping it outside any strip opens a new window.
    pub detached: bool,
    /// Another window's strip under the pointer, and the slot the tab would take there.
    pub target: Option<(ViewportId, usize)>,
    /// Pointer in screen coordinates, when the platform reports window positions (X11, Windows, macOS).
    pub global: Option<Pos2>,
    /// Pointer in the source window's coordinates.
    pub local: Pos2,
}

/// A tab dropped outside its window where window positions are unknown (Wayland): if another of our
/// windows sees the pointer on its tab strip right after the release, the tab goes there; otherwise
/// it opens in a new window.
#[derive(Clone, Debug)]
pub struct PendingTear {
    pub tab: TabId,
    pub from: ViewportId,
    pub since: f64,
    pub size: Vec2,
}

/// Changes to windows and tabs, applied between frames so rendering never sees half of one.
#[derive(Clone, Debug)]
pub enum Action {
    Activate { tab: TabId },
    NewTab { window: ViewportId, kind: TabKind },
    CloseTab { tab: TabId },
    CloseOtherTabs { tab: TabId },
    MoveTab { tab: TabId, to: ViewportId, index: usize },
    Reorder { tab: TabId, index: usize },
    TearOff { tab: TabId, at: Option<Pos2>, size: Vec2 },
    CloseWindow { window: ViewportId },
    MergeAllWindows { into: ViewportId },
    SendTo { source: AppKind, target: AppKind },
    Quit,
}

/// Asking before closing an app that has no unsaved-changes prompt of its own.
pub struct ConfirmClose {
    pub tab: TabId,
    pub kind: AppKind,
}

pub struct Shell {
    pub router: Router,
    pub windows: Vec<Window>,
    pub apps: BTreeMap<AppKind, Rc<RefCell<AppSlot>>>,
    next_tab: TabId,
    next_window: u64,
    pub frame_no: u64,
    pub drag: Option<TabDrag>,
    pub pending_tear: Option<PendingTear>,
    /// Content being dragged from one app to another (see `content.rs`).
    pub content_drag: Option<crate::content::ContentDrag>,
    pub autotest: Option<crate::autotest::Autotest>,
    pub actions: Vec<Action>,
    /// Files waiting to be opened (creating an app needs the eframe frame, so this waits for `ui`).
    pub opens: Vec<OpenRequest>,
    pub confirm: Option<ConfirmClose>,
    /// The About dialog, in this window.
    pub about: Option<ViewportId>,
    pub home: HomeState,
    pub assistant: crate::assistant::Assistant,
    pub recent: Recent,
    pub bridge: Bridge,
    /// Files an app wanted to open in another program (e.g. Lightcraft's "Edit in"), shared with the
    /// handler each app got.
    external: Rc<RefCell<Vec<PathBuf>>>,
    /// The app each window showed last frame (requests in the router refer to it).
    last_shown: HashMap<ViewportId, AppKind>,
    /// Apps cleared to close (no prompt needed); their tabs go before the next frame is drawn.
    pending_finish: Vec<AppKind>,
    /// Windows whose app got a close request delivered last frame.
    delivered: Vec<ViewportId>,
    /// The layout when quitting began (closing tabs one by one empties the windows).
    quit_session: Option<crate::recent::Session>,
    /// Sibling apps an app asked to switch to (Effectcraft's "More ArtCraft apps").
    open_app: Rc<RefCell<Vec<AppKind>>>,
    quit_requested: bool,
}

#[derive(Clone, Debug)]
pub struct OpenRequest {
    pub paths: Vec<PathBuf>,
    /// None: pick by file type.
    pub app: Option<AppKind>,
    /// Window to open the tab in (if the app has none yet).
    pub window: Option<ViewportId>,
    pub place: Option<Option<Pos2>>,
}

/// Apps that ask about unsaved work themselves when their window closes.
fn has_own_close_prompt(kind: AppKind) -> bool {
    !matches!(kind, AppKind::Designcraft | AppKind::Filmcraft)
}

impl Shell {
    pub fn new(ctx: &Context, router: Router, files: Vec<PathBuf>, restore: Option<crate::recent::Session>) -> Self {
        router.lock().windows.insert(ViewportId::ROOT);
        let mut shell = Shell {
            router,
            windows: Vec::new(),
            apps: BTreeMap::new(),
            next_tab: 1,
            next_window: 1,
            frame_no: 0,
            drag: None,
            pending_tear: None,
            content_drag: None,
            autotest: crate::autotest::Autotest::from_env(),
            actions: Vec::new(),
            opens: Vec::new(),
            confirm: None,
            about: None,
            home: HomeState::default(),
            assistant: crate::assistant::Assistant::new(ctx),
            recent: Recent::load(),
            bridge: Bridge::new(ctx),
            external: Rc::new(RefCell::new(Vec::new())),
            last_shown: HashMap::new(),
            pending_finish: Vec::new(),
            delivered: Vec::new(),
            quit_session: None,
            open_app: Rc::new(RefCell::new(Vec::new())),
            quit_requested: false,
        };
        let root = shell.new_window(ViewportId::ROOT, ViewportBuilder::default());
        shell.windows.push(root);
        let restored = restore.map(|s| shell.restore_session(s)).unwrap_or(false);
        if !restored {
            let id = shell.new_tab_id();
            shell.windows[0].tabs.push(Tab { id, kind: TabKind::Home });
        }
        if !files.is_empty() {
            shell.opens.push(OpenRequest { paths: files, app: None, window: Some(ViewportId::ROOT), place: None });
        }
        shell
    }

    fn new_window(&mut self, viewport: ViewportId, builder: ViewportBuilder) -> Window {
        Window {
            viewport,
            tabs: Vec::new(),
            active: 0,
            closing: false,
            hidden: false,
            builder,
            strip: Rect::NOTHING,
            tab_rects: Vec::new(),
            title: String::new(),
            geometry: None,
        }
    }

    pub fn new_tab_id(&mut self) -> TabId {
        let id = self.next_tab;
        self.next_tab += 1;
        id
    }

    pub fn window_index(&self, viewport: ViewportId) -> Option<usize> {
        self.windows.iter().position(|w| w.viewport == viewport)
    }

    pub fn find_tab(&self, tab: TabId) -> Option<(usize, usize)> {
        self.windows.iter().enumerate().find_map(|(wi, w)| w.tabs.iter().position(|t| t.id == tab).map(|ti| (wi, ti)))
    }

    pub fn find_app_tab(&self, kind: AppKind) -> Option<(usize, usize)> {
        self.windows.iter().enumerate().find_map(|(wi, w)| w.tabs.iter().position(|t| t.kind == TabKind::App(kind)).map(|ti| (wi, ti)))
    }

    pub fn visible_windows(&self) -> usize {
        self.windows.iter().filter(|w| !w.hidden).count()
    }

    // ---------------------------------------------------------------------------------------------
    // Session

    pub fn session(&self) -> crate::recent::Session {
        crate::recent::Session {
            windows: self
                .windows
                .iter()
                .filter(|w| !w.hidden)
                .map(|w| crate::recent::SavedWindow { tabs: w.tabs.iter().map(|t| t.kind).collect(), active: w.active, geometry: w.geometry })
                .collect(),
        }
    }

    /// Reopen last session's tabs (each app restores its own documents as it would standalone).
    fn restore_session(&mut self, s: crate::recent::Session) -> bool {
        let mut any = false;
        for (wi, saved) in s.windows.into_iter().enumerate() {
            let active = saved.active;
            let kinds: Vec<TabKind> = saved
                .tabs
                .into_iter()
                .filter(|k| match k {
                    TabKind::App(kind) => self.find_app_tab(*kind).is_none(),
                    TabKind::Home => true,
                })
                .collect();
            let tabs: Vec<Tab> = kinds.into_iter().map(|kind| Tab { id: self.new_tab_id(), kind }).collect();
            if tabs.is_empty() {
                continue;
            }
            any = true;
            let active = active.min(tabs.len() - 1);
            if wi == 0 || self.windows.len() == 1 && self.windows[0].tabs.is_empty() {
                self.windows[0].tabs = tabs;
                self.windows[0].active = active;
            } else {
                let viewport = self.next_viewport_id();
                let (at, size) = match saved.geometry {
                    Some((pos, size)) => (Some(egui::pos2(pos[0], pos[1])), Vec2::new(size[0], size[1])),
                    None => (None, Vec2::new(1280.0, 820.0)),
                };
                let mut w = self.new_window(viewport, window_builder(at, size));
                w.tabs = tabs;
                w.active = active;
                self.router.lock().windows.insert(viewport);
                self.windows.push(w);
            }
        }
        any
    }

    fn next_viewport_id(&mut self) -> ViewportId {
        self.next_window += 1;
        ViewportId::from_hash_of(("septet-window", self.next_window))
    }

    // ---------------------------------------------------------------------------------------------
    // Frame

    /// Router traffic and closing, once per frame before any window is drawn.
    pub fn logic(&mut self, ctx: &Context) {
        self.frame_no += 1;
        let (os_close, delivered, requests) = {
            let mut sh = self.router.lock();
            (std::mem::take(&mut sh.os_close), std::mem::take(&mut sh.close_delivered), std::mem::take(&mut sh.app_requests))
        };
        for viewport in os_close {
            self.actions.push(Action::CloseWindow { window: viewport });
        }
        // Deliveries reported last frame: the app has had its frame with the request since, and
        // any veto from it is in `requests` (handled first).
        let settled = std::mem::replace(&mut self.delivered, delivered.into_iter().collect());
        for (viewport, request) in requests {
            let Some(kind) = self.last_shown.get(&viewport).copied() else { continue };
            let Some(slot) = self.apps.get(&kind).cloned() else { continue };
            let mut slot = slot.borrow_mut();
            match request {
                AppRequest::Title(t) => slot.title_hint = Some(t),
                AppRequest::CancelClose => {
                    if matches!(slot.close, CloseState::Requested | CloseState::Delivered) {
                        slot.close = CloseState::Vetoed;
                    }
                }
                // The app's Quit/Close command, or the end of its save prompt: ask it once more
                // through a close request, which its prompt lets through once it is done.
                AppRequest::Close => {
                    drop(slot);
                    self.begin_close_app(kind);
                }
            }
        }
        for viewport in settled {
            let Some(kind) = self.last_shown.get(&viewport).copied() else { continue };
            if self.apps.get(&kind).is_some_and(|s| s.borrow().close == CloseState::Delivered) {
                self.finish_close_app(ctx, kind);
            }
        }
        if !self.delivered.is_empty() {
            ctx.request_repaint();
        }
        // Deliver pending close requests to apps that are on screen.
        for (kind, slot) in self.apps.clone() {
            let mut s = slot.borrow_mut();
            if s.close != CloseState::Requested {
                continue;
            }
            match self.find_app_tab(kind) {
                Some((wi, ti)) => {
                    let w = &mut self.windows[wi];
                    w.active = ti;
                    s.close = CloseState::Delivered;
                    self.router.lock().inject_close.insert(w.viewport, kind);
                    ctx.request_repaint_of(w.viewport);
                }
                None => s.close = CloseState::Open,
            }
        }
        // Windows being closed: close their tabs one at a time.
        let closing: Vec<ViewportId> = self.windows.iter().filter(|w| w.closing).map(|w| w.viewport).collect();
        for viewport in closing {
            self.continue_close_window(ctx, viewport);
        }
        if let Some(p) = self.pending_tear.clone() {
            let now = ctx.input(|i| i.time);
            if now - p.since > 0.35 {
                self.pending_tear = None;
                self.actions.push(Action::TearOff { tab: p.tab, at: None, size: p.size });
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
            }
        }
        let wanted: Vec<AppKind> = self.open_app.borrow_mut().drain(..).collect();
        for kind in wanted {
            let window = self.focused_window(ctx);
            self.actions.push(Action::NewTab { window, kind: TabKind::App(kind) });
        }
        // Files apps wanted opened elsewhere: open them here when one of our apps can.
        let external: Vec<PathBuf> = self.external.borrow_mut().drain(..).collect();
        if !external.is_empty() {
            self.opens.push(OpenRequest { paths: external, app: None, window: None, place: None });
        }
        self.bridge.logic(ctx);
        self.assistant.logic(ctx);
        crate::assistant::tools::drain(self, ctx);
        // Claude is waiting for an answer: make sure the panel that asks is on screen.
        if self.assistant.panel.is_none_or(|v| self.window_index(v).is_none_or(|wi| self.windows[wi].hidden))
            && self.assistant.conversation.as_ref().is_some_and(|c| !c.approvals.is_empty())
        {
            self.assistant.panel = Some(self.focused_window(ctx));
        }
        crate::autotest::Autotest::tick(self, ctx);
    }

    /// Draw every window. The root window draws into `ui`; the others are immediate viewports.
    pub fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_actions(&ctx);
        self.process_opens(&ctx, frame);
        self.apply_actions(&ctx);
        self.sync_visibility(&ctx);

        if !self.windows[0].hidden {
            self.window_ui(0, ui, frame);
        } else {
            ui.painter().rect_filled(ui.max_rect(), 0.0, crate::theme::ShellColors::home().home_bg);
        }
        let mut wi = 1;
        while wi < self.windows.len() {
            let viewport = self.windows[wi].viewport;
            let builder = self.windows[wi].builder.clone();
            ctx.show_viewport_immediate(viewport, builder, |ui, _class| {
                self.window_ui(wi, ui, frame);
            });
            wi += 1;
        }
        crate::tabstrip::ghost_window(self, &ctx);
        self.finish_drag(&ctx);
        crate::content::finish(self, &ctx);
        self.sync_titles();
        self.apply_actions(&ctx);
    }

    fn window_ui(&mut self, wi: usize, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let viewport = self.windows[wi].viewport;
        crate::autotest::Autotest::window(self, &ctx, viewport);
        if std::env::var_os("SEPTET_FPS").is_some() {
            fps_probe(&ctx, viewport);
        }
        self.shortcuts(&ctx, wi);
        self.windows[wi].geometry = ctx.input(|i| {
            let v = i.viewport();
            Some(([v.outer_rect?.min.x, v.outer_rect?.min.y], [v.inner_rect?.width(), v.inner_rect?.height()]))
        });
        let full = ui.max_rect();
        crate::tabstrip::strip(self, wi, &ctx, full);
        self.strip_drops(&ctx, wi);
        let mut content = Rect::from_min_max(egui::pos2(full.left(), self.windows[wi].strip.bottom()), full.max);
        // The Claude panel takes the right side; the app keeps at least 420 points.
        let panel = (self.assistant.panel == Some(viewport)).then(|| {
            let w = self.assistant.panel_width.clamp(crate::assistant::panel::MIN_WIDTH, (content.width() - 420.0).max(crate::assistant::panel::MIN_WIDTH));
            let r = Rect::from_min_max(egui::pos2(content.right() - w, content.top()), content.max);
            content.max.x = r.left();
            r
        });
        // While the chat field has the keyboard, the app must not see the keys.
        let held: Vec<egui::Event> = if crate::assistant::panel::has_keyboard(&self.assistant, &ctx, viewport) {
            ctx.input_mut(|i| {
                let (keys, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut i.events).into_iter().partition(|e| {
                    matches!(
                        e,
                        egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Ime(_)
                    )
                });
                i.events = rest;
                keys
            })
        } else {
            Vec::new()
        };
        let tab = self.windows[wi].active_tab();
        match tab.map(|t| t.kind) {
            Some(TabKind::App(kind)) if self.apps.contains_key(&kind) => {
                let slot = self.apps[&kind].clone();
                router::set_shown(viewport, Some(&slot));
                self.last_shown.insert(viewport, kind);
                let mut guard = slot.borrow_mut();
                let s = &mut *guard;
                s.iso.swap(&ctx);
                self.bridge.before_app(&ctx, s);
                self.route_drops(&ctx, s);
                let app = s.app.app();
                app.logic(&ctx, frame);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content).layout(egui::Layout::top_down(egui::Align::Min)));
                child.set_clip_rect(content);
                app.ui(&mut child, frame);
                let outgoing = s.app.outgoing_drag();
                s.iso.swap(&ctx);
                s.frames += 1;
                drop(guard);
                crate::content::after_app(self, &ctx, viewport, kind, outgoing);
            }
            Some(TabKind::App(_)) => {
                // Starting up (created in `process_opens` next frame).
                router::set_shown(viewport, None);
                ui.painter().rect_filled(content, 0.0, crate::theme::ShellColors::home().home_bg);
                ctx.request_repaint();
            }
            _ => {
                router::set_shown(viewport, None);
                self.last_shown.remove(&viewport);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content).layout(egui::Layout::top_down(egui::Align::Min)));
                child.set_clip_rect(content);
                crate::home::show(self, wi, &mut child);
            }
        }
        if !held.is_empty() {
            ctx.input_mut(|i| i.events.extend(held));
        }
        if let Some(r) = panel {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)));
            child.set_clip_rect(r);
            crate::assistant::panel::show(&mut self.assistant, &mut child, r, viewport);
        }
        // Home and empty windows take OS file drops; apps take their own.
        if !matches!(tab.map(|t| t.kind), Some(TabKind::App(_))) {
            let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
            if !dropped.is_empty() {
                self.opens.push(OpenRequest { paths: dropped, app: None, window: Some(viewport), place: None });
            }
        }
        crate::tabstrip::window_edges(self, wi, ui);
        crate::tabstrip::drag_overlay(self, wi, ui);
        crate::content::window_ui(self, wi, ui);
        if let Some(confirm) = self.confirm.as_ref()
            && self.find_tab(confirm.tab).is_some_and(|(cwi, _)| cwi == wi)
        {
            crate::home::confirm_close_dialog(self, &ctx);
        }
        if self.about == Some(viewport) {
            let mut open = true;
            crate::about::dialog(&mut open, &ctx);
            if !open {
                self.about = None;
            }
        }
        if self.assistant.dialog == Some(viewport) {
            crate::assistant::settings::dialog(&mut self.assistant, &ctx);
        }
        self.bridge.window_ui(&ctx, viewport);
    }

    /// Files dropped from the file manager onto the tab strip: onto a tab, that app opens them;
    /// elsewhere on the strip, the app made for each file. Needs the pointer position (X11).
    fn strip_drops(&mut self, ctx: &Context, wi: usize) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if dropped.is_empty() {
            return;
        }
        let Some(at) = crate::pointer::in_viewport(ctx) else { return };
        let w = &self.windows[wi];
        if !w.strip.contains(at) {
            return;
        }
        let app = w.tab_rects.iter().find(|(_, r)| r.contains(at)).and_then(|(id, _)| w.tabs.iter().find(|t| t.id == *id)).and_then(|t| match t.kind {
            TabKind::App(k) => Some(k),
            TabKind::Home => None,
        });
        let viewport = w.viewport;
        ctx.input_mut(|i| i.raw.dropped_files.clear());
        self.opens.push(OpenRequest { paths: dropped, app, window: Some(viewport), place: None });
    }

    /// Files dropped from the file manager onto `slot`'s app: placed where they landed, for apps whose
    /// own drop handling can't see the pointer (winit drops carry no position). Otherwise, or when
    /// the pointer can't be read, the app handles the drop itself.
    fn route_drops(&mut self, ctx: &Context, slot: &mut AppSlot) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if dropped.is_empty() {
            return;
        }
        let Some(at) = crate::pointer::in_viewport(ctx) else { return };
        let take = || ctx.input_mut(|i| i.raw.dropped_files.clear());
        if matches!(slot.kind, AppKind::Photocraft | AppKind::Effectcraft | AppKind::Designcraft | AppKind::Filmcraft) {
            take();
            for p in &dropped {
                self.recent.add(p, slot.kind);
            }
            slot.app.place_paths(&dropped, Some(at));
        }
    }

    fn shortcuts(&mut self, ctx: &Context, wi: usize) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let viewport = self.windows[wi].viewport;
        let n = self.windows[wi].tabs.len();
        let active = self.windows[wi].active;
        let ctrl_alt = Modifiers::CTRL | Modifiers::ALT;
        let next = ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::PageDown)));
        let prev = ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::PageUp)));
        if n > 0 && (next || prev) {
            let i = if next { (active + 1) % n } else { (active + n - 1) % n };
            let tab = self.windows[wi].tabs[i].id;
            self.actions.push(Action::Activate { tab });
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, Key::K))) {
            self.assistant.toggle_panel(ctx, viewport);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, Key::T))) {
            self.actions.push(Action::NewTab { window: viewport, kind: TabKind::Home });
        }
        // Ctrl+Alt+1…7: the apps, in Home's order.
        let keys = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7];
        for (key, kind) in keys.into_iter().zip(AppKind::ALL) {
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl_alt, key))) {
                self.actions.push(Action::NewTab { window: viewport, kind: TabKind::App(kind) });
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl_alt, Key::W)))
            && let Some(tab) = self.windows[wi].active_tab()
        {
            self.actions.push(Action::CloseTab { tab: tab.id });
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Apps

    /// Start an app (needs the eframe frame for the GPU and storage).
    fn ensure_app(&mut self, ctx: &Context, frame: &mut eframe::Frame, kind: AppKind) -> Rc<RefCell<AppSlot>> {
        if let Some(slot) = self.apps.get(&kind) {
            return slot.clone();
        }
        let mut iso = crate::hosted::Isolation::fresh();
        if let Some(data) = frame.storage().and_then(|s| eframe::get_value(s, &format!("septet/egui/{}", kind.name()))) {
            iso.data = data;
        }
        iso.swap(ctx);
        let render_state = frame.wgpu_render_state().cloned();
        crate::hosted::set_data_root(kind, crate::recent::portable_root().map(|root| root.join(kind.name())));
        let mut app = crate::hosted::create(kind, ctx, render_state.as_ref(), frame.storage());
        let external = self.external.clone();
        let open_app = self.open_app.clone();
        app.set_open_app(Box::new(move |name| {
            if let Some(kind) = AppKind::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(name)) {
                open_app.borrow_mut().push(kind);
            }
        }));
        app.set_open_externally(Box::new(move |path| {
            if AppKind::for_path(path).is_some() {
                external.borrow_mut().push(path.to_path_buf());
                true
            } else {
                false
            }
        }));
        iso.swap(ctx);
        let mut slot = AppSlot::new(kind, app);
        slot.iso = iso;
        let slot = Rc::new(RefCell::new(slot));
        self.apps.insert(kind, slot.clone());
        slot
    }

    fn process_opens(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        // Tabs for apps that are not running yet start them here.
        let wanted: Vec<AppKind> = self
            .windows
            .iter()
            .filter(|w| !w.hidden)
            .filter_map(|w| match w.active_tab()?.kind {
                TabKind::App(k) => Some(k),
                TabKind::Home => None,
            })
            .collect();
        for kind in wanted {
            self.ensure_app(ctx, frame, kind);
        }
        for req in std::mem::take(&mut self.opens) {
            let mut by_app: BTreeMap<AppKind, Vec<PathBuf>> = BTreeMap::new();
            for path in req.paths {
                match req.app.or_else(|| AppKind::for_path(&path)) {
                    Some(kind) => by_app.entry(kind).or_default().push(path),
                    None => self.bridge.notify(format!("Septet can't open {}", path.display())),
                }
            }
            for (kind, paths) in by_app {
                let slot = self.ensure_app(ctx, frame, kind);
                if self.find_app_tab(kind).is_none() {
                    let window = req.window.filter(|v| self.window_index(*v).is_some()).unwrap_or_else(|| self.focused_window(ctx));
                    let id = self.new_tab_id();
                    let wi = self.window_index(window).unwrap_or(0);
                    let w = &mut self.windows[wi];
                    let at = (w.active + 1).min(w.tabs.len());
                    // An empty Home tab gives way to the app.
                    if w.active_tab().is_some_and(|t| t.kind == TabKind::Home) && w.tabs.len() == 1 {
                        w.tabs[0] = Tab { id, kind: TabKind::App(kind) };
                    } else {
                        w.tabs.insert(at, Tab { id, kind: TabKind::App(kind) });
                    }
                }
                if let Some((wi, ti)) = self.find_app_tab(kind) {
                    self.windows[wi].active = ti;
                    if self.windows[wi].hidden {
                        self.show_root();
                    }
                    let viewport = self.windows[wi].viewport;
                    self.router.send(viewport, ViewportCommand::Focus);
                }
                for p in &paths {
                    self.recent.add(p, kind);
                }
                let mut s = slot.borrow_mut();
                match req.place {
                    Some(at) => s.with_app(ctx, |a| a.place_paths(&paths, at)),
                    None => s.with_app(ctx, |a| a.open_paths(&paths)),
                }
            }
        }
    }

    /// The window the user is working in.
    pub fn focused_window(&self, ctx: &Context) -> ViewportId {
        self.windows
            .iter()
            .filter(|w| !w.hidden)
            .find(|w| ctx.input_for(w.viewport, |i| i.viewport().focused).unwrap_or(false))
            .or_else(|| self.windows.iter().find(|w| !w.hidden))
            .map_or(ViewportId::ROOT, |w| w.viewport)
    }

    /// Tell each app whether its tab is on screen.
    fn sync_visibility(&mut self, ctx: &Context) {
        for (kind, slot) in &self.apps {
            let visible = self.windows.iter().any(|w| !w.hidden && w.active_tab().is_some_and(|t| t.kind == TabKind::App(*kind)));
            let mut s = slot.borrow_mut();
            if s.visible != visible {
                s.visible = visible;
                s.with_app(ctx, |a| a.set_visible(visible));
            }
        }
    }

    fn sync_titles(&mut self) {
        let mut cmds = Vec::new();
        for w in &mut self.windows {
            let title = match w.active_tab().map(|t| t.kind) {
                Some(TabKind::App(kind)) => {
                    let doc = self.apps.get(&kind).and_then(|s| s.try_borrow().ok().and_then(|s| s.tab_title()));
                    match doc {
                        Some(doc) => format!("{doc} — {} — Septet", kind.name()),
                        None => format!("{} — Septet", kind.name()),
                    }
                }
                _ => "Septet".to_string(),
            };
            if w.title != title {
                w.title = title.clone();
                cmds.push((w.viewport, ViewportCommand::Title(title)));
            }
        }
        for (v, c) in cmds {
            self.router.send(v, c);
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Closing

    /// Close an app's tab: through its own prompt if it has one, else after asking about unsaved work.
    pub fn begin_close_app(&mut self, kind: AppKind) {
        let Some(slot) = self.apps.get(&kind).cloned() else {
            // Never started: just drop the tab.
            if let Some((wi, ti)) = self.find_app_tab(kind) {
                self.remove_tab(wi, ti);
            }
            return;
        };
        let mut s = slot.borrow_mut();
        if matches!(s.close, CloseState::Requested | CloseState::Delivered) {
            return;
        }
        if has_own_close_prompt(kind) {
            s.close = CloseState::Requested;
        } else if s.app.has_unsaved_changes() {
            if let Some((wi, ti)) = self.find_app_tab(kind) {
                let tab = self.windows[wi].tabs[ti].id;
                self.windows[wi].active = ti;
                self.confirm = Some(ConfirmClose { tab, kind });
            }
        } else {
            s.close = CloseState::Delivered;
            drop(s);
            self.pending_finish.push(kind);
        }
    }

    /// "Close Anyway" in the shell's own prompt.
    pub fn force_close_app(&mut self, kind: AppKind) {
        if let Some(slot) = self.apps.get(&kind) {
            slot.borrow_mut().close = CloseState::Delivered;
        }
        self.pending_finish.push(kind);
    }

    pub fn finish_close_app(&mut self, ctx: &Context, kind: AppKind) {
        if let Some(slot) = self.apps.remove(&kind) {
            let mut s = slot.borrow_mut();
            s.with_app(ctx, |a| a.app().on_exit());
        }
        for (_, k) in self.last_shown.clone() {
            if k == kind {
                self.last_shown.retain(|_, v| *v != kind);
            }
        }
        for w in &self.windows {
            router::set_shown(w.viewport, None);
        }
        if let Some((wi, ti)) = self.find_app_tab(kind) {
            self.remove_tab(wi, ti);
        }
        if self.confirm.as_ref().is_some_and(|c| c.kind == kind) {
            self.confirm = None;
        }
    }

    /// Take a tab out of its window; an emptied window closes (or, the main window, shows Home).
    fn remove_tab(&mut self, wi: usize, ti: usize) {
        let w = &mut self.windows[wi];
        w.tabs.remove(ti);
        if w.active > ti || w.active >= w.tabs.len() {
            w.active = w.active.saturating_sub(1);
        }
        if w.tabs.is_empty() {
            self.window_emptied(wi);
        }
    }

    fn window_emptied(&mut self, wi: usize) {
        let others = self.windows.iter().enumerate().any(|(i, w)| i != wi && !w.hidden);
        let closing = self.windows[wi].closing;
        if wi == 0 {
            if others {
                self.windows[0].hidden = true;
                self.windows[0].closing = false;
                self.router.send(ViewportId::ROOT, ViewportCommand::Visible(false));
            } else if closing || self.quit_requested {
                self.quit();
            } else {
                let id = self.new_tab_id();
                self.windows[0].tabs.push(Tab { id, kind: TabKind::Home });
                self.windows[0].active = 0;
            }
        } else {
            let viewport = self.windows[wi].viewport;
            self.windows.remove(wi);
            self.router.lock().windows.remove(&viewport);
            router::set_shown(viewport, None);
            self.last_shown.remove(&viewport);
            if self.windows.iter().all(|w| w.hidden) {
                self.quit();
            }
        }
    }

    fn continue_close_window(&mut self, ctx: &Context, viewport: ViewportId) {
        let Some(wi) = self.window_index(viewport) else { return };
        let Some(tab) = self.windows[wi].active_tab().or_else(|| self.windows[wi].tabs.first().copied()) else {
            self.window_emptied(wi);
            return;
        };
        match tab.kind {
            TabKind::Home => {
                let ti = self.windows[wi].tabs.iter().position(|t| t.id == tab.id).unwrap_or(0);
                self.remove_tab(wi, ti);
            }
            TabKind::App(kind) => {
                let state = self.apps.get(&kind).map(|s| s.borrow().close);
                match state {
                    None => {
                        let ti = self.windows[wi].tabs.iter().position(|t| t.id == tab.id).unwrap_or(0);
                        self.remove_tab(wi, ti);
                    }
                    Some(CloseState::Open) if self.confirm.is_none() => self.begin_close_app(kind),
                    _ => {}
                }
            }
        }
        let _ = ctx;
    }

    fn quit(&mut self) {
        self.quit_requested = true;
        let mut sh = self.router.lock();
        sh.quitting = true;
        sh.shell_cmds.push((ViewportId::ROOT, ViewportCommand::Close));
    }

    fn show_root(&mut self) {
        self.windows[0].hidden = false;
        self.router.send(ViewportId::ROOT, ViewportCommand::Visible(true));
        self.router.send(ViewportId::ROOT, ViewportCommand::Focus);
    }

    // ---------------------------------------------------------------------------------------------
    // Actions

    fn apply_actions(&mut self, ctx: &Context) {
        for kind in std::mem::take(&mut self.pending_finish) {
            self.finish_close_app(ctx, kind);
        }
        for action in std::mem::take(&mut self.actions) {
            self.apply(ctx, action);
        }
    }

    fn apply(&mut self, ctx: &Context, action: Action) {
        match action {
            Action::Activate { tab } => {
                if let Some((wi, ti)) = self.find_tab(tab) {
                    let w = &mut self.windows[wi];
                    w.active = ti;
                    w.closing = false;
                    let viewport = w.viewport;
                    self.router.send(viewport, ViewportCommand::Focus);
                    if !self.quit_requested {
                        self.quit_session = None;
                    }
                }
            }
            Action::NewTab { window, kind } => {
                if let TabKind::App(app) = kind
                    && let Some((wi, ti)) = self.find_app_tab(app)
                {
                    // One instance per app: bring its tab forward wherever it is.
                    self.windows[wi].active = ti;
                    let viewport = self.windows[wi].viewport;
                    if self.windows[wi].hidden {
                        self.show_root();
                    }
                    self.router.send(viewport, ViewportCommand::Focus);
                    return;
                }
                let Some(wi) = self.window_index(window) else { return };
                let id = self.new_tab_id();
                let w = &mut self.windows[wi];
                // The Home tab you launched from turns into the app.
                if let (TabKind::App(_), Some(t)) = (kind, w.active_tab())
                    && t.kind == TabKind::Home
                {
                    w.tabs[w.active] = Tab { id, kind };
                } else {
                    let at = (w.active + 1).min(w.tabs.len());
                    w.tabs.insert(at, Tab { id, kind });
                    w.active = at;
                }
                w.closing = false;
            }
            Action::CloseTab { tab } => {
                let Some((wi, ti)) = self.find_tab(tab) else { return };
                match self.windows[wi].tabs[ti].kind {
                    TabKind::Home => self.remove_tab(wi, ti),
                    TabKind::App(kind) => self.begin_close_app(kind),
                }
            }
            Action::CloseOtherTabs { tab } => {
                let Some((wi, _)) = self.find_tab(tab) else { return };
                let others: Vec<Tab> = self.windows[wi].tabs.iter().filter(|t| t.id != tab).copied().collect();
                for t in others {
                    self.apply(ctx, Action::CloseTab { tab: t.id });
                }
            }
            Action::Reorder { tab, index } => {
                if let Some((wi, ti)) = self.find_tab(tab) {
                    let w = &mut self.windows[wi];
                    let t = w.tabs.remove(ti);
                    let index = index.min(w.tabs.len());
                    w.tabs.insert(index, t);
                    w.active = index;
                }
            }
            Action::MoveTab { tab, to, index } => {
                let (Some((wi, ti)), Some(_)) = (self.find_tab(tab), self.window_index(to)) else { return };
                if self.windows[wi].viewport == to {
                    self.apply(ctx, Action::Reorder { tab, index });
                    return;
                }
                let t = self.windows[wi].tabs[ti];
                self.remove_tab(wi, ti);
                let Some(tw) = self.window_index(to) else { return };
                if self.windows[tw].hidden {
                    self.show_root();
                }
                let w = &mut self.windows[tw];
                let index = index.min(w.tabs.len());
                w.tabs.insert(index, t);
                w.active = index;
                w.closing = false;
                self.router.send(to, ViewportCommand::Focus);
            }
            Action::TearOff { tab, at, size } => {
                if std::env::var_os("SEPTET_DEBUG_TABS").is_some() {
                    eprintln!("tear off {tab} at {at:?} size {size:?}");
                }
                let Some((wi, ti)) = self.find_tab(tab) else { return };
                let t = self.windows[wi].tabs[ti];
                // The hidden main window comes back for it rather than opening another one.
                if self.windows[0].hidden && wi != 0 {
                    self.remove_tab(wi, ti);
                    if self.window_index(ViewportId::ROOT).is_some() {
                        self.windows[0].tabs.push(t);
                        self.windows[0].active = self.windows[0].tabs.len() - 1;
                        if let Some(at) = at {
                            self.router.send(ViewportId::ROOT, ViewportCommand::OuterPosition(at));
                        }
                        self.show_root();
                    }
                    return;
                }
                if self.windows[wi].tabs.len() == 1 {
                    // Tearing the only tab off just moves its window.
                    if let Some(at) = at {
                        let viewport = self.windows[wi].viewport;
                        self.router.send(viewport, ViewportCommand::OuterPosition(at));
                    }
                    return;
                }
                self.remove_tab(wi, ti);
                let viewport = self.next_viewport_id();
                let mut w = self.new_window(viewport, window_builder(at, size));
                w.tabs.push(t);
                self.router.lock().windows.insert(viewport);
                self.windows.push(w);
            }
            Action::CloseWindow { window } => {
                if let Some(wi) = self.window_index(window) {
                    if self.visible_windows() == 1 && self.quit_session.is_none() {
                        self.quit_session = Some(self.session());
                    }
                    self.windows[wi].closing = true;
                    // Apps that vetoed an earlier attempt get asked again.
                    for t in self.windows[wi].tabs.clone() {
                        if let TabKind::App(kind) = t.kind
                            && let Some(slot) = self.apps.get(&kind)
                            && slot.borrow().close == CloseState::Vetoed
                        {
                            slot.borrow_mut().close = CloseState::Open;
                        }
                    }
                }
            }
            Action::MergeAllWindows { into } => {
                let Some(target) = self.window_index(into) else { return };
                let mut moved = Vec::new();
                for (wi, w) in self.windows.iter_mut().enumerate() {
                    if wi != target {
                        moved.append(&mut w.tabs);
                    }
                }
                self.windows[target].tabs.extend(moved);
                let others: Vec<usize> = (0..self.windows.len()).filter(|&i| i != target).rev().collect();
                for wi in others {
                    self.window_emptied(wi);
                }
            }
            Action::SendTo { source, target } => crate::content::send_to(self, ctx, source, target),
            Action::Quit => {
                if self.quit_session.is_none() {
                    self.quit_session = Some(self.session());
                }
                self.quit_requested = true;
                for w in &mut self.windows {
                    w.closing = true;
                }
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Tab dragging between windows

    /// After all windows had their pass: settle a tab drag whose button was released.
    fn finish_drag(&mut self, ctx: &Context) {
        let Some(drag) = self.drag.clone() else { return };
        let released = ctx.input_for(drag.from, |i| !i.pointer.primary_down());
        if std::env::var_os("SEPTET_DEBUG_TABS").is_some() {
            eprintln!("finish_drag: released={released} detached={} target={:?} global={:?} local={:?}", drag.detached, drag.target, drag.global, drag.local);
        }
        if !released {
            ctx.request_repaint();
            return;
        }
        self.drag = None;
        if !drag.detached {
            return;
        }
        if let Some((to, index)) = drag.target {
            self.actions.push(Action::MoveTab { tab: drag.tab, to, index });
        } else if let Some(global) = drag.global {
            self.actions.push(Action::TearOff { tab: drag.tab, at: Some(global - drag.grab - Vec2::new(8.0, 4.0)), size: self.window_size(drag.from) });
        } else {
            // Wayland: we can't tell where the pointer went; see if a window reports it next.
            let since = ctx.input(|i| i.time);
            self.pending_tear = Some(PendingTear { tab: drag.tab, from: drag.from, since, size: self.window_size(drag.from) });
            ctx.request_repaint();
        }
    }

    pub fn window_size(&self, viewport: ViewportId) -> Vec2 {
        let ctx_size = self.windows.iter().find(|w| w.viewport == viewport).map(|w| w.strip.width()).unwrap_or(1280.0);
        Vec2::new(ctx_size.max(900.0), (ctx_size * 0.62).max(600.0))
    }

    pub fn persist(&self, storage: &mut dyn eframe::Storage) {
        let session = self.quit_session.clone().unwrap_or_else(|| self.session());
        eframe::set_value(storage, "septet/session", &session);
        for (kind, slot) in &self.apps {
            if let Ok(mut s) = slot.try_borrow_mut() {
                s.app.app().save(storage);
                // The app's panel sizes and other remembered widget state (its own egui memory).
                eframe::set_value(storage, &format!("septet/egui/{}", kind.name()), &s.iso.data);
            }
        }
        self.recent.save();
    }

    pub fn exit(&mut self) {
        self.assistant.exit();
        for slot in self.apps.values() {
            if let Ok(mut s) = slot.try_borrow_mut() {
                s.app.app().on_exit();
            }
        }
    }
}

/// A torn-off window: like the main one, at `at` (screen points) when the platform allows.
pub fn window_builder(at: Option<Pos2>, size: Vec2) -> ViewportBuilder {
    let mut b = crate::window_chrome(ViewportBuilder::default())
        .with_title("Septet")
        .with_app_id(crate::APP_ID)
        .with_icon(crate::icon::window_icon())
        .with_inner_size(size)
        .with_min_inner_size([760.0, 480.0])
        .with_drag_and_drop(true);
    if let Some(at) = at {
        b = b.with_position(at);
    }
    b
}

/// `SEPTET_FPS=1`: once a second, how many frames a window drew and what asked for them.
fn fps_probe(ctx: &Context, viewport: ViewportId) {
    use std::collections::BTreeMap;
    /// Since when, how many frames, and what asked for them.
    type Stats = (f64, u32, BTreeMap<String, u32>);
    thread_local! {
        static STATS: RefCell<HashMap<ViewportId, Stats>> = RefCell::new(HashMap::new());
    }
    let now = ctx.input(|i| i.time);
    let causes: Vec<String> = ctx.repaint_causes().iter().map(|c| format!("{}:{}", c.file.rsplit("/src/").next().unwrap_or(c.file), c.line)).collect();
    STATS.with(|s| {
        let mut s = s.borrow_mut();
        let e = s.entry(viewport).or_insert((now, 0, BTreeMap::new()));
        e.1 += 1;
        for c in causes {
            *e.2.entry(c).or_default() += 1;
        }
        if now - e.0 >= 1.0 {
            let mut top: Vec<_> = e.2.iter().collect();
            top.sort_by(|a, b| b.1.cmp(a.1));
            let top: Vec<String> = top.into_iter().take(6).map(|(k, v)| format!("{k}×{v}")).collect();
            eprintln!("fps {viewport:?}: {} frames; repaint causes: {}", e.1, top.join(", "));
            *e = (now, 0, BTreeMap::new());
        }
    });
}
