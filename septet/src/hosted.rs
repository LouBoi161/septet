//! One running app inside the shell: its `Embedded` (see each app's `apps/<app>/src/embed.rs`) behind a
//! common trait, plus the egui state it keeps to itself.
//!
//! All apps share one `egui::Context`. Each of them stores its theme tokens, panel sizes and widget
//! state in `ctx.data` under fixed ids, and sets its look with `set_visuals` / `global_style_mut`.
//! [`Isolation`] swaps that state in before every call into an app and out again afterwards, so the
//! apps never see each other's (or the shell's) state.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use egui::{Context, FontDefinitions, Pos2, Style, ThemePreference};
use serde_json::Value;

use crate::kinds::AppKind;

/// What the shell needs from an app (implemented for each app's `Embedded`).
pub trait HostedApp {
    fn app(&mut self) -> &mut dyn eframe::App;
    fn document_title(&self) -> Option<String>;
    fn has_unsaved_changes(&self) -> bool;
    fn open_paths(&mut self, paths: &[PathBuf]);
    fn place_paths(&mut self, paths: &[PathBuf], at: Option<Pos2>);
    fn set_visible(&mut self, visible: bool);
    fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>);

    /// Content being dragged out of the app (label for the drag ghost), see `content.rs`.
    fn outgoing_drag(&self) -> Option<String> {
        None
    }
    /// The dragged content as files in the first of `accept` (extensions) the app can make.
    fn take_outgoing_files(&mut self, _accept: &[&str], _dir: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
    /// Another app took the drag: forget it without acting on the release.
    fn cancel_outgoing_drag(&mut self) {}
    /// Links to sibling apps switch to their tab (hook; see `set_open_app` in the apps' embed.rs).
    fn set_open_app(&mut self, _handler: Box<dyn FnMut(&str)>) {}
    /// The active document as one file, for "Send to".
    fn export_active(&mut self, _accept: &[&str], _dir: &Path) -> Option<PathBuf> {
        None
    }

    // Claude drives the app (`docs/embedding/phase5-agent-control.md`). None: the app can't yet.

    /// The app's commands, as its menus and command palette run them.
    fn agent_commands(&mut self, _ctx: &Context) -> Option<Vec<Value>> {
        None
    }
    /// Run a command; the reply (`{"ok", "result"|"error"}`) comes at once or on a later frame of the app.
    fn agent_execute(&mut self, _ctx: &Context, _command: &str, _params: Value) -> Option<Receiver<Value>> {
        None
    }
    /// The document's state as JSON (`what`: document, object, selection…).
    fn agent_inspect(&mut self, _ctx: &Context, _what: &str, _params: &Value) -> Option<Result<Value, String>> {
        None
    }
    /// A caption and a job that renders the document or a part of it (run it off the UI thread).
    fn agent_render(&mut self, _ctx: &Context, _target: &Value) -> Option<Result<(String, AgentRender), String>> {
        None
    }
}

/// A picture the app makes for Claude, on a worker thread (premultiplied, transparent where empty).
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

macro_rules! hosted {
    ($ty:ty) => {
        hosted!($ty, {});
    };
    // Apps that implement the agent methods of phase 5.
    ($ty:ty, agent) => {
        hosted!($ty, {
            fn agent_commands(&mut self, ctx: &Context) -> Option<Vec<Value>> {
                Some(<$ty>::agent_commands(self, ctx))
            }
            fn agent_execute(&mut self, ctx: &Context, command: &str, params: Value) -> Option<Receiver<Value>> {
                Some(<$ty>::agent_execute(self, ctx, command, params))
            }
            fn agent_inspect(&mut self, ctx: &Context, what: &str, params: &Value) -> Option<Result<Value, String>> {
                Some(<$ty>::agent_inspect(self, ctx, what, params))
            }
            fn agent_render(&mut self, ctx: &Context, target: &Value) -> Option<Result<(String, AgentRender), String>> {
                Some(<$ty>::agent_render(self, ctx, target))
            }
        });
    };
    ($ty:ty, { $($agent:tt)* }) => {
        impl HostedApp for $ty {
            fn app(&mut self) -> &mut dyn eframe::App {
                self
            }
            fn document_title(&self) -> Option<String> {
                <$ty>::document_title(self)
            }
            fn has_unsaved_changes(&self) -> bool {
                <$ty>::has_unsaved_changes(self)
            }
            fn open_paths(&mut self, paths: &[PathBuf]) {
                <$ty>::open_paths(self, paths)
            }
            fn place_paths(&mut self, paths: &[PathBuf], at: Option<Pos2>) {
                <$ty>::place_paths(self, paths, at)
            }
            fn set_visible(&mut self, visible: bool) {
                <$ty>::set_visible(self, visible)
            }
            fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
                <$ty>::set_open_externally(self, handler)
            }
            fn outgoing_drag(&self) -> Option<String> {
                <$ty>::outgoing_drag(self)
            }
            fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
                <$ty>::take_outgoing_files(self, accept, dir)
            }
            fn cancel_outgoing_drag(&mut self) {
                <$ty>::cancel_outgoing_drag(self)
            }
            fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
                <$ty>::export_active(self, accept, dir)
            }
            fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
                <$ty>::set_open_app(self, handler)
            }
            $($agent)*
        }
    };
}

hosted!(photocraft_embed::Embedded, agent);
hosted!(vectorcraft_embed::Embedded, agent);
hosted!(lightcraft_embed::Embedded);
hosted!(designcraft_embed::Embedded);
hosted!(pdfcraft_embed::Embedded);
hosted!(filmcraft_embed::Embedded);
hosted!(effectcraft_embed::Embedded);

/// Portable mode: keep `kind`'s settings and data under `root` (call before `create`).
pub fn set_data_root(kind: AppKind, root: Option<PathBuf>) {
    match kind {
        AppKind::Photocraft => photocraft_embed::Embedded::set_data_root(root),
        AppKind::Vectorcraft => vectorcraft_embed::Embedded::set_data_root(root),
        AppKind::Lightcraft => lightcraft_embed::Embedded::set_data_root(root),
        AppKind::Designcraft => designcraft_embed::Embedded::set_data_root(root),
        AppKind::Pdfcraft => pdfcraft_embed::Embedded::set_data_root(root),
        AppKind::Filmcraft => filmcraft_embed::Embedded::set_data_root(root),
        AppKind::Effectcraft => effectcraft_embed::Embedded::set_data_root(root),
    }
}

pub fn create(
    kind: AppKind,
    ctx: &Context,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    storage: Option<&dyn eframe::Storage>,
) -> Box<dyn HostedApp> {
    match kind {
        AppKind::Photocraft => Box::new(photocraft_embed::Embedded::new(ctx, render_state, storage)),
        AppKind::Vectorcraft => Box::new(vectorcraft_embed::Embedded::new(ctx, render_state, storage)),
        AppKind::Lightcraft => Box::new(lightcraft_embed::Embedded::new(ctx, render_state, storage)),
        AppKind::Designcraft => Box::new(designcraft_embed::Embedded::new(ctx, render_state, storage)),
        AppKind::Pdfcraft => {
            // Pdfcraft keeps its settings (recent files, signatures, digital IDs, stamps) in eframe
            // storage. Until Septet has saved its own copy, start from the standalone app's.
            // (Not in portable mode: that copy must not pick up this computer's settings.)
            let fallback = if crate::recent::portable_root().is_some() { Default::default() } else { standalone_storage("PdfCraft") };
            let storage = WithFallback { primary: storage, fallback };
            Box::new(pdfcraft_embed::Embedded::new(ctx, render_state, Some(&storage)))
        }
        AppKind::Filmcraft => Box::new(filmcraft_embed::Embedded::new(ctx, render_state, storage)),
        AppKind::Effectcraft => Box::new(effectcraft_embed::Embedded::new(ctx, render_state, storage)),
    }
}

/// Storage that falls back to another app's saved settings for keys Septet hasn't written yet.
struct WithFallback<'a> {
    primary: Option<&'a dyn eframe::Storage>,
    fallback: std::collections::HashMap<String, String>,
}

impl eframe::Storage for WithFallback<'_> {
    fn get_string(&self, key: &str) -> Option<String> {
        self.primary.and_then(|p| p.get_string(key)).or_else(|| self.fallback.get(key).cloned())
    }
    fn set_string(&mut self, _key: &str, _value: String) {}
    fn remove_string(&mut self, _key: &str) {}
    fn flush(&mut self) {}
}

/// A standalone eframe app's saved settings (`<storage dir>/app.ron`).
fn standalone_storage(app_name: &str) -> std::collections::HashMap<String, String> {
    eframe::storage_dir(app_name)
        .and_then(|dir| std::fs::read_to_string(dir.join("app.ron")).ok())
        .and_then(|text| ron::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn font_definitions(kind: AppKind) -> FontDefinitions {
    match kind {
        AppKind::Photocraft => photocraft_embed::Embedded::font_definitions(),
        AppKind::Vectorcraft => vectorcraft_embed::Embedded::font_definitions(),
        AppKind::Lightcraft => lightcraft_embed::Embedded::font_definitions(),
        AppKind::Designcraft => designcraft_embed::Embedded::font_definitions(),
        AppKind::Pdfcraft => pdfcraft_embed::Embedded::font_definitions(),
        AppKind::Filmcraft => filmcraft_embed::Embedded::font_definitions(),
        AppKind::Effectcraft => effectcraft_embed::Embedded::font_definitions(),
    }
}

/// The parts of the shared `egui::Context` an app treats as its own.
pub struct Isolation {
    pub data: egui::util::IdTypeMap,
    dark: Arc<Style>,
    light: Arc<Style>,
    theme: ThemePreference,
}

impl Isolation {
    /// What a freshly started app would find in a context of its own: egui's default styles. The
    /// apps are designed dark and most never set a theme preference, so a light desktop must not
    /// flip them to egui's light style.
    pub fn fresh() -> Self {
        Isolation {
            data: Default::default(),
            dark: Arc::new(egui::Theme::Dark.default_style()),
            light: Arc::new(egui::Theme::Light.default_style()),
            theme: ThemePreference::Dark,
        }
    }

    /// Exchange the context's state with ours. Called in pairs around every call into the app; the
    /// second call puts back what the first one took out.
    pub fn swap(&mut self, ctx: &Context) {
        ctx.memory_mut(|m| {
            std::mem::swap(&mut m.data, &mut self.data);
            std::mem::swap(&mut m.options.dark_style, &mut self.dark);
            std::mem::swap(&mut m.options.light_style, &mut self.light);
            std::mem::swap(&mut m.options.theme_preference, &mut self.theme);
        });
    }

    /// The app's own panel colours (while swapped out), for the tab strip above it.
    pub fn visuals(&self) -> &egui::Visuals {
        match self.theme {
            ThemePreference::Light => &self.light.visuals,
            _ => &self.dark.visuals,
        }
    }
}

/// A running app and everything the shell tracks about it.
pub struct AppSlot {
    pub kind: AppKind,
    pub app: Box<dyn HostedApp>,
    pub iso: Isolation,
    /// Last `ViewportCommand::Title` the app sent (some apps name their document only that way).
    pub title_hint: Option<String>,
    pub visible: bool,
    /// Frames shown so far (an app needs a couple of passes before it draws anything).
    pub frames: u64,
    pub close: CloseState,
}

/// Where closing this app's tab stands. The app's own unsaved-changes prompt decides: the shell
/// delivers a close request to it, and the tab goes unless the app answers with `CancelClose`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CloseState {
    #[default]
    Open,
    /// Deliver a close request to the app on its next frame.
    Requested,
    /// Delivered; no veto during that frame means "close".
    Delivered,
    /// The app vetoed (it is showing its own save prompt). When it is done it sends `Close`, which
    /// starts another round — it lets that one through.
    Vetoed,
}

impl AppSlot {
    pub fn new(kind: AppKind, app: Box<dyn HostedApp>) -> Self {
        AppSlot { kind, app, iso: Isolation::fresh(), title_hint: None, visible: false, frames: 0, close: CloseState::Open }
    }

    /// Run one of the app's methods with its isolated state in place.
    pub fn with_app<R>(&mut self, ctx: &Context, f: impl FnOnce(&mut dyn HostedApp) -> R) -> R {
        self.iso.swap(ctx);
        let r = f(self.app.as_mut());
        self.iso.swap(ctx);
        r
    }

    /// The title for the tab: the document name, else the app's own title hint.
    pub fn tab_title(&self) -> Option<String> {
        self.app.document_title().filter(|t| !t.trim().is_empty()).or_else(|| {
            // "<doc> — App" / "App - <doc> *": keep the document part.
            let name = self.kind.name().to_lowercase();
            let mut s = self.title_hint.as_deref()?.trim();
            for sep in [" — ", " – ", " - "] {
                if let Some((a, b)) = s.split_once(sep) {
                    s = if a.to_lowercase().contains(&name) { b } else { a };
                }
            }
            let s = s.trim_end_matches(['*', ' ']);
            (!s.is_empty() && s.to_lowercase() != name).then(|| s.to_string())
        })
    }
}
