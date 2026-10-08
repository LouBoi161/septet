//! VectorCraft as a tab of another app's window (Septet): [`Embedded`] builds the app as the
//! desktop app does, minus everything that belongs to the process or the window, which the host
//! owns (logging, panic hooks, the control server, the macOS menu bar and Apple events, the
//! graphics adapter's choice and its loss, the window's size, title bar, fonts and zoom).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::json;
use vectorcraft_engine::Session;
use vectorcraft_engine::cmd::fileio;
use vectorcraft_ui_egui::VectorcraftApp;
use vectorcraft_ui_egui::outgoing::Outgoing;
use vectorcraft_ui_egui::place::DropTarget;

use crate::{desktop, prefs};

/// The host's hook for files the app opens in another app ([`Embedded::set_open_externally`]).
type OpenExternally = Box<dyn FnMut(&Path) -> bool>;

/// The host's hook for links to the other ArtCraft apps ([`Embedded::set_open_app`]).
type OpenApp = Box<dyn FnMut(&str)>;

/// The other ArtCraft apps, by the name their page on getartcraft.com has.
const SIBLING_APPS: [&str; 6] = ["photocraft", "lightcraft", "designcraft", "pdfcraft", "filmcraft", "effectcraft"];

/// VectorCraft in a host's tab.
pub struct Embedded {
    app: VectorcraftApp,
    /// The host's context, which egui's drag payloads belong to.
    ctx: egui::Context,
    /// Asked first when Edit Original opens a file in its default app.
    open_externally: Rc<RefCell<Option<OpenExternally>>>,
    /// Asked instead of the browser for another ArtCraft app's page.
    open_app: Rc<RefCell<Option<OpenApp>>>,
    /// The last drag of art out of the app seen while the tab showed: the host asks for its files
    /// once it is dropped on another app, when the app may no longer be drawn.
    outgoing: Option<Outgoing>,
}

impl Embedded {
    /// The app as a plain launch makes it (no files): its preferences, its libraries and Data
    /// Recovery folders next to them and the desktop's services, inside the host's window.
    pub fn new(ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>, _storage: Option<&dyn eframe::Storage>) -> Self {
        vectorcraft_ui_egui::hosted::set_hosted(true);
        vectorcraft_ui_egui::i18n::detect_system_lang_in_background();
        let open_externally: Rc<RefCell<Option<OpenExternally>>> = Rc::default();
        let mut services = desktop::services();
        let hook = open_externally.clone();
        // Edit Original: the host may open the file itself (in another of its tabs). A folder
        // (Show Package) always opens in the file manager.
        services.open_file = Some(Box::new(move |path: &str| {
            let file = Path::new(path);
            let handled = !file.is_dir() && hook.try_borrow_mut().ok().is_some_and(|mut h| h.as_mut().is_some_and(|handler| handler(file)));
            if handled { Ok(()) } else { desktop::open_file(path) }
        }));
        let open_app: Rc<RefCell<Option<OpenApp>>> = Rc::default();
        let apps = open_app.clone();
        // Another ArtCraft app's page: the host may show that app instead (its tab).
        services.open_url = Some(Box::new(move |url: &str| {
            let handled = sibling_app(url)
                .is_some_and(|name| apps.try_borrow_mut().ok().is_some_and(|mut h| h.as_mut().map(|handler| handler(name)).is_some()));
            if !handled {
                desktop::open_url(url);
            }
        }));
        let mut app = VectorcraftApp::new(Session::new(), services);
        prefs::load_prefs(&mut app, prefs::read_prefs());
        desktop::set_user_folders(&mut app);
        // The device is the host's, shared by every app it shows: it handles losing it.
        if let Some(rs) = render_state {
            app.graphics_adapter = Some(desktop::adapter_summary(&rs.adapter.get_info()));
        }
        // The host draws the window's title bar, caption buttons and edges.
        app.integrated_titlebar = false;
        app.custom_titlebar = false;
        Self { app, ctx: ctx.clone(), open_externally, open_app, outgoing: None }
    }

    /// Before [`Self::new`]: keep everything VectorCraft stores per user (the UI preferences, the
    /// User Defined swatch and graphic style libraries, Data Recovery copies, the default Templates
    /// folder) under `root`, made if missing, instead of the per-user OS folders. `None` (the
    /// default) uses those.
    pub fn set_data_root(root: Option<PathBuf>) {
        vectorcraft_ui_egui::hosted::set_data_root(root);
    }

    /// The fonts the app installs at startup, with its `ui`, `ui-semibold` and `mono` families.
    pub fn font_definitions() -> egui::FontDefinitions {
        vectorcraft_ui_egui::theme::font_definitions()
    }

    /// The active document's name (none on the Home screen with no document open).
    pub fn document_title(&self) -> Option<String> {
        self.app.session.active().map(|st| st.title())
    }

    /// Is any open document modified?
    pub fn has_unsaved_changes(&self) -> bool {
        vectorcraft_ui_egui::unsaved::any_dirty(&self.app)
    }

    /// Open `paths` as documents, as the command line does.
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        desktop::open_files(&mut self.app, paths.iter().map(|p| p.to_string_lossy().into_owned()).collect());
    }

    /// File → Place `paths` into the active document, linked, as files dropped on its canvas are:
    /// at `at` (screen points) when that is over the canvas, else in the middle of the view. With
    /// no document open they open as documents.
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        if self.app.session.active().is_none() {
            self.open_paths(paths);
            return;
        }
        let at = at.and_then(|pos| match self.app.drop_target("", Some(pos), false) {
            DropTarget::Place(d) => Some(d.at),
            DropTarget::Open => None,
        });
        for path in paths {
            let path = path.to_string_lossy().into_owned();
            let mut p = json!({ "path": path, "link": true });
            if let Some(at) = at {
                p["at"] = json!([at.x, at.y]);
            }
            if let Err(e) = vectorcraft_ui_egui::place::run(&mut self.app, &p) {
                self.app.status(format!("Couldn't place {}: {e}", fileio::file_name(&path)));
            }
        }
    }

    /// The host showed (`true`) or hid (`false`) the app's tab: as the window regaining or losing
    /// the focus (fonts installed meanwhile are listed).
    pub fn set_visible(&mut self, visible: bool) {
        self.app.set_visible(visible);
    }

    /// Ask `handler` first when the app opens a file in another app (Links panel › Edit Original):
    /// `true` means the host opened it.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        if let Ok(mut hook) = self.open_externally.try_borrow_mut() {
            *hook = Some(handler);
        }
    }

    /// Links to another ArtCraft app's page call `handler` with its name (`photocraft`…) instead
    /// of opening the browser. VectorCraft's own page, the website, Discord and GitHub still open
    /// in the browser.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        if let Ok(mut hook) = self.open_app.try_borrow_mut() {
            *hook = Some(handler);
        }
    }

    /// A short name for the art being dragged out of the app (off the canvas with the Selection
    /// tool, or Layers panel rows) for the drag's ghost: `Layer “Sky”`, `Ellipse`, `3 objects`.
    pub fn outgoing_drag(&self) -> Option<String> {
        self.app.outgoing_drag(&self.ctx).map(|out| self.app.outgoing_label(&out))
    }

    /// The dragged art, cropped to its bounds, as one file of the first format in `accept` the
    /// app writes (SVG, PDF, EPS, PNG, TIFF, PSD…; rasters transparent at whole multiples of
    /// 72 ppi) in `dir`.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        let Some(out) = self.app.outgoing_drag(&self.ctx).or_else(|| self.outgoing.clone()) else { return vec![] };
        match self.app.write_outgoing(&out, accept, dir) {
            Ok(path) => vec![path],
            Err(e) => {
                log::warn!("vectorcraft: dragged art not handed over: {e}");
                self.app.status(format!("Couldn't hand the art over: {e}"));
                vec![]
            }
        }
    }

    /// Another app took the drag: the app forgets it without acting on the release.
    pub fn cancel_outgoing_drag(&mut self) {
        self.app.cancel_outgoing_drag(&self.ctx);
        self.outgoing = None;
    }

    /// Send to another app: the active document as one file of the first format in `accept` the
    /// app writes, in `dir` (its own file when unmodified and of a type `accept` lists; PDF with
    /// every artboard, other formats the artboard in view). None without a document.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        match self.app.write_active(accept, dir)? {
            Ok(path) => Some(path),
            Err(e) => {
                log::warn!("vectorcraft: document not sent: {e}");
                self.app.status(format!("Couldn't send the document: {e}"));
                None
            }
        }
    }
}

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        // The host keeps the window's geometry (no `window::track`).
        if self.app.ui.status == "quit" {
            // Quit (nothing left unsaved): the host closes the tab. Once is enough, and the app
            // doesn't close again at once if the host shows it again.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            self.app.ui.status.clear();
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        if let Some(out) = self.app.outgoing_drag(ui.ctx()) {
            self.outgoing = Some(out);
        }
        #[cfg(target_os = "macos")]
        if self.app.take_ime_discard() {
            desktop::discard_marked_text();
        }
    }

    fn on_exit(&mut self) {
        // Saves and exports still running in the background finish first (the host may quit
        // while this tab is hidden, without asking it to close).
        vectorcraft_ui_egui::background::wait_all(&mut self.app);
        prefs::save_prefs(&self.app);
    }
}

/// The ArtCraft app whose page `url` is (another than VectorCraft): `https://getartcraft.com/apps/<name>`.
fn sibling_app(url: &str) -> Option<&'static str> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let page = rest.strip_prefix("www.").unwrap_or(rest).strip_prefix("getartcraft.com/apps/")?;
    let name = page.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    SIBLING_APPS.into_iter().find(|app| *app == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_other_apps_pages_are_sibling_links() {
        assert_eq!(sibling_app("https://getartcraft.com/apps/photocraft"), Some("photocraft"));
        assert_eq!(sibling_app("https://www.getartcraft.com/apps/PdfCraft/?ref=vectorcraft#top"), Some("pdfcraft"));
        for url in [
            "https://getartcraft.com/apps/vectorcraft",
            "https://getartcraft.com",
            "https://discord.gg/artcraft",
            "https://github.com/storytold/photocraft",
        ] {
            assert_eq!(sibling_app(url), None, "{url}");
        }
    }
}
