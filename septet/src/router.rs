//! The plumbing between eframe, the shell and the apps, as an egui [`Plugin`]: it sees every window's
//! input before each pass and all window commands after the frame.
//!
//! - Close requests from the OS (the window's × button, Alt+F4) are taken out of the input, so no
//!   app mistakes "close this window" for "quit"; the shell closes the window's tabs one by one.
//! - When the shell closes an app's tab it delivers a close request to just that app, which runs its
//!   own unsaved-changes prompt exactly as it would standalone.
//! - `Close` / `CancelClose` / `Title` commands an app sends to "its" window are taken out of the
//!   output and handed to the shell: `Close` closes the app's tab, not Septet.
//! - The app's `raw_input_hook` runs for whichever window shows it (eframe only calls the hook for
//!   the main window).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

use egui::{Context, FullOutput, RawInput, ViewportCommand, ViewportEvent, ViewportId};

use crate::hosted::AppSlot;

/// What an app asked of the window showing it.
#[derive(Clone, Debug, PartialEq)]
pub enum AppRequest {
    Close,
    CancelClose,
    Title(String),
}

#[derive(Default)]
pub struct Shared {
    /// The shell's windows (the root viewport included); other viewports belong to the apps.
    pub windows: HashSet<ViewportId>,
    /// OS close requests taken out of the input since the shell last looked.
    pub os_close: HashSet<ViewportId>,
    /// Deliver a close request to this app on the next pass of the window showing it.
    pub inject_close: HashMap<ViewportId, crate::kinds::AppKind>,
    /// Windows whose app got an injected close request (taken by the shell next frame).
    pub close_delivered: HashSet<ViewportId>,
    /// What the apps asked for this frame.
    pub app_requests: Vec<(ViewportId, AppRequest)>,
    /// The shell's own window commands, sent after the apps' are filtered.
    pub shell_cmds: Vec<(ViewportId, ViewportCommand)>,
    /// The shell is quitting: let eframe close the root window.
    pub quitting: bool,
    /// Synthetic input for a window's next pass (the autotest's mouse and keyboard).
    pub inject_events: HashMap<ViewportId, Vec<egui::Event>>,
    /// The OS asked to close the root window this frame; eframe closes it unless the output cancels.
    cancel_root_close: bool,
    /// Viewport runs in progress (immediate viewports run inside the root's run).
    stack: Vec<ViewportId>,
}

#[derive(Clone, Default)]
pub struct Router(pub Arc<Mutex<Shared>>);

impl Router {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn send(&self, viewport: ViewportId, cmd: ViewportCommand) {
        self.lock().shell_cmds.push((viewport, cmd));
    }
}

thread_local! {
    /// The app shown in each window. Plugins must be `Send + Sync` and the apps are not, so the
    /// router reaches them through this UI-thread registry.
    static SHOWN: RefCell<HashMap<ViewportId, Weak<RefCell<AppSlot>>>> = RefCell::new(HashMap::new());
}

/// Record which app `viewport` shows (None: the Home screen or nothing).
pub fn set_shown(viewport: ViewportId, slot: Option<&Rc<RefCell<AppSlot>>>) {
    SHOWN.with(|s| {
        let mut s = s.borrow_mut();
        match slot {
            Some(slot) => s.insert(viewport, Rc::downgrade(slot)),
            None => s.remove(&viewport),
        };
    });
}

fn shown(viewport: ViewportId) -> Option<Rc<RefCell<AppSlot>>> {
    SHOWN.with(|s| s.borrow().get(&viewport).and_then(Weak::upgrade))
}

impl egui::Plugin for Router {
    fn debug_name(&self) -> &'static str {
        "septet-router"
    }

    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        let viewport = input.viewport_id;
        {
            let mut sh = self.lock();
            sh.stack.push(viewport);
            if !sh.windows.contains(&viewport) {
                return;
            }
            // Only to the app it is meant for (the window may have switched tabs since).
            let shown_kind = shown(viewport).and_then(|s| s.try_borrow().ok().map(|s| s.kind));
            let inject = sh.inject_close.get(&viewport).is_some_and(|k| Some(*k) == shown_kind);
            if inject {
                sh.inject_close.remove(&viewport);
            }
            if let Some(info) = input.viewports.get_mut(&viewport) {
                let before = info.events.len();
                info.events.retain(|e| *e != ViewportEvent::Close);
                if info.events.len() != before {
                    sh.os_close.insert(viewport);
                    if viewport == ViewportId::ROOT {
                        sh.cancel_root_close = true;
                    }
                }
                if inject {
                    info.events.push(ViewportEvent::Close);
                    sh.close_delivered.insert(viewport);
                }
            }
            if let Some(events) = sh.inject_events.remove(&viewport) {
                input.events.extend(events);
            }
        }
        if let Some(slot) = shown(viewport)
            && let Ok(mut slot) = slot.try_borrow_mut()
        {
            let slot = &mut *slot;
            slot.iso.swap(ctx);
            slot.app.app().raw_input_hook(ctx, input);
            slot.iso.swap(ctx);
        }
    }

    fn output_hook(&mut self, _ctx: &Context, output: &mut FullOutput) {
        let mut sh = self.lock();
        let run = sh.stack.pop();
        // Every window's commands come out of the outermost (root) run; inner runs carry none.
        if run != Some(ViewportId::ROOT) {
            return;
        }
        let sh = &mut *sh;
        for (&id, vo) in output.viewport_output.iter_mut() {
            if !sh.windows.contains(&id) {
                continue;
            }
            vo.commands.retain(|cmd| {
                let request = match cmd {
                    ViewportCommand::Close => AppRequest::Close,
                    ViewportCommand::CancelClose => AppRequest::CancelClose,
                    ViewportCommand::Title(t) => AppRequest::Title(t.clone()),
                    _ => return true,
                };
                sh.app_requests.push((id, request));
                false
            });
        }
        // eframe closes the root window after an OS close request unless the root output cancels it.
        if std::mem::take(&mut sh.cancel_root_close)
            && !sh.quitting
            && let Some(vo) = output.viewport_output.get_mut(&ViewportId::ROOT)
        {
            vo.commands.push(ViewportCommand::CancelClose);
        }
        let mut keep = Vec::new();
        for (id, cmd) in sh.shell_cmds.drain(..) {
            match output.viewport_output.get_mut(&id) {
                Some(vo) => vo.commands.push(cmd),
                // Not shown this frame (a window still opening): try again next frame.
                None => keep.push((id, cmd)),
            }
        }
        sh.shell_cmds = keep;
    }
}
