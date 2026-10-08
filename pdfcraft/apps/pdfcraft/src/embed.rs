//! PdfCraft as a tab of a host window (Septet).
//!
//! [`Embedded`] builds the app the way `main.rs` does for a plain launch with no arguments, minus
//! everything that belongs to the process or the window: no logger or panic hook, no GPU
//! selection, no update checks, no Apple events, no UI control channel. The host owns the
//! window, its chrome, the fonts and the UI zoom (`pdfcraft_ui_egui::hosted`).

use std::path::{Path, PathBuf};

use pdfcraft_ui_egui::{PdfCraftApp, RecoveryStore, i18n};

use crate::settings;

/// Asks the host to open a file in another application; `true` when the host handled it.
type OpenExternally = Box<dyn FnMut(&Path) -> bool>;

/// Asks the host to show a sibling ArtCraft app ("photocraft", …).
type OpenApp = Box<dyn FnMut(&str)>;

/// PdfCraft running inside a host window.
pub struct Embedded {
    app: PdfCraftApp,
    /// Kept for the host, never called: PdfCraft hands no file to another application (it
    /// opens web addresses only, in the browser).
    _open_externally: Option<OpenExternally>,
    /// Kept for the host, never called: PdfCraft has no links to the other ArtCraft apps (only
    /// Discord, its own web page, GitHub and the ArtCraft website).
    _open_app: Option<OpenApp>,
}

impl Embedded {
    /// Before [`Self::new`]: keep everything PdfCraft stores per user under `root` (created if
    /// missing) instead of the user's data folders: crash recovery in `<root>/Recovery`, the
    /// digital IDs it creates in `<root>/Digital IDs`. `None`: the user's folders (the default).
    /// Its settings are in the host's eframe storage either way.
    pub fn set_data_root(root: Option<PathBuf>) {
        pdfcraft_ui_egui::hosted::set_data_root(root);
    }

    /// Build the app as `main.rs` does without arguments: settings from `storage` (the host's),
    /// autosave and crash recovery in PdfCraft's own recovery folder, OS key-store identities on
    /// macOS and Windows. Fonts are the host's: install [`Self::font_definitions`] among them.
    pub fn new(_ctx: &egui::Context, _render_state: Option<&eframe::egui_wgpu::RenderState>, storage: Option<&dyn eframe::Storage>) -> Self {
        pdfcraft_ui_egui::hosted::set_hosted(true);
        settings::migrate_legacy_folders();
        let mut app = PdfCraftApp::new();
        settings::restore(&mut app, storage);
        // The host draws the window's chrome; there are no traffic lights over our tab strip.
        app.integrated_titlebar = false;
        // No update checks: without `update_source`, Help ▸ Check for updates opens the releases page.
        app.os_key_store_ids = cfg!(any(target_os = "macos", target_os = "windows"));
        // Autosave unsaved changes; offer to recover documents a crashed session left behind.
        if let Some(dir) = RecoveryStore::default_dir() {
            app.enable_recovery(RecoveryStore::new(dir));
        }
        Self { app, _open_externally: None, _open_app: None }
    }

    /// The fonts PdfCraft installs at startup in the default (automatic) language: Inter and
    /// JetBrains Mono first, egui's defaults, the CJK and Arabic faces, the installed
    /// "system-fallback" face last, and the "medium" and "semibold" families.
    pub fn font_definitions() -> egui::FontDefinitions {
        let hans = i18n::Lang::from_pref(i18n::AUTO).code() == "zh-hans";
        pdfcraft_ui_egui::theme::installed_font_definitions(hans)
    }

    /// The active document's name (or the title it asks to be shown by); `None` on Home.
    pub fn document_title(&self) -> Option<String> {
        let (_, id) = self.app.active_ids()?;
        self.app.session.get(id).map(|doc| doc.display_name())
    }

    /// Some open document has unsaved changes (including text typed into a form field).
    pub fn has_unsaved_changes(&self) -> bool {
        self.app.first_dirty().is_some()
    }

    /// Open files in their own tabs, as the command line does (images and text become new PDFs).
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        for path in paths {
            self.app.open_path(&path.to_string_lossy());
        }
    }

    /// Place files into the active document: a PDF's pages go in after the page under `at` (or
    /// the current page), an image onto the page under `at`, centred there (or the middle of
    /// the current page). Other files, and all of them on Home, open in their own tabs.
    pub fn place_paths(&mut self, paths: &[PathBuf], at: Option<egui::Pos2>) {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        self.app.place_paths(&paths, at);
    }

    /// The host tab was shown or hidden: hiding stops middle-button scrolling and autosaves
    /// unsaved changes; showing redraws (PdfCraft doesn't reload files on focus).
    pub fn set_visible(&mut self, visible: bool) {
        self.app.set_visible(visible);
    }

    /// Kept, but never called: PdfCraft opens no file in another application.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&Path) -> bool>) {
        self._open_externally = Some(handler);
    }

    /// Kept, but never called: PdfCraft shows no links to the other ArtCraft apps.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>) {
        self._open_app = Some(handler);
    }

    /// While pages are dragged in the Organize grid (or the Pages panel's thumbnails): "Page 4",
    /// "3 pages". Nothing else in PdfCraft drags out.
    pub fn outgoing_drag(&self) -> Option<String> {
        self.app.outgoing_drag()
    }

    /// The dragged pages as files in `dir`: one PDF of them (unsaved edits included) when
    /// `accept` has "pdf" before any image type, else a 150 dpi PNG, JPEG or TIFF of each page,
    /// whichever comes first in `accept`. Empty when none of `accept` can be made.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &Path) -> Vec<PathBuf> {
        self.app.take_outgoing_files(accept, dir)
    }

    /// The drag went to another app: it ends without moving any pages.
    pub fn cancel_outgoing_drag(&mut self) {
        self.app.cancel_outgoing_drag();
    }

    /// Send to: the active document's current page, as a one-page PDF or an image (the first
    /// of `accept` PdfCraft can make). `None` on Home.
    pub fn export_active(&mut self, accept: &[&str], dir: &Path) -> Option<PathBuf> {
        self.app.send_current_page(accept, dir)
    }
}

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        eframe::App::raw_input_hook(&mut self.app, ctx, raw);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::App::save(&mut self.app, storage);
    }

    fn on_exit(&mut self) {
        eframe::App::on_exit(&mut self.app);
        // As a quit does: documents still unsaved keep an up-to-date recovery entry, and with
        // nothing unsaved the recovery folder is left empty.
        self.app.autosave_now();
        self.app.shutdown_recovery();
    }
}
