# Septet embedding contract (applies to every *craft app)

Context: <workspace> contains seven cloned egui 0.36 apps (photocraft, vectorcraft,
lightcraft, designcraft, pdfcraft, filmcraft, effectcraft) and a new host app `septet/` that shows each app
as a TAB inside one eframe window (and torn-off extra windows = egui immediate viewports). All apps share ONE
egui::Context and ONE wgpu device (the host's eframe RenderState). The host:
- creates at most ONE instance of each app per process (no two Photocraft instances at once);
- calls `logic()` then `ui()` on the app every frame while its tab is visible, passing a child `Ui` whose
  max_rect is the area below the host's tab strip; while the tab is hidden it calls neither;
- swaps `ctx.memory.data` (IdTypeMap) and the dark/light styles + theme preference in/out around every call
  into the app, so fixed egui Ids, Tokens in ctx.data, panel ids and `set_visuals`/`global_style_mut` do NOT
  leak between apps — you do NOT need to salt ids or scope styles;
- owns window management through an egui Plugin: it strips OS close requests from the input, may inject a
  close request (`ViewportEvent::Close`) for the active app when the user closes its tab, and intercepts
  `ViewportCommand::Close` / `CancelClose` / `Title` sent by the app (Close = "app is done, close my tab",
  CancelClose = "app vetoed the close", Title = tab title hint). So keep the app's own close guard /
  quit commands as they are. StartDrag/Maximized/Minimized/Fullscreen from the app are passed through.
- installs ONE merged `FontDefinitions` (union of all apps' fonts and named families) at startup.

## What to build in YOUR app repo (you are on git branch `freedobe`; do NOT commit)

1. A library target in the desktop package `apps/<app>/` with lib name `<app>_embed`:
   `[lib] name = "<app>_embed"  path = "src/lib.rs"` (keep the existing binary working unchanged).
   Prefer moving the binary-only modules the embed needs (services, prefs I/O, audio out, file dialogs …)
   into the lib (lib.rs declares them, main.rs then uses `<app>_embed::module`) so nothing is compiled twice.
   Keep main.rs behaviour identical.

2. `apps/<app>/src/embed.rs` (re-exported from lib.rs as `pub use embed::Embedded;`) with EXACTLY this API:

```rust
pub struct Embedded { /* the app + whatever wrapper state main.rs keeps */ }

impl Embedded {
    /// Build the app the way main.rs does for a plain launch with no CLI arguments, minus everything
    /// process-wide or window-owning: NO panic hooks, NO log::set_logger, NO control/TCP/MCP servers, NO macOS
    /// native menus / Apple events, NO GPU-startup sentinels or self-relaunch, NO update checks, NO forced
    /// X11, NO NativeOptions. Use `render_state` where main.rs uses `cc.wgpu_render_state`, and `storage`
    /// where main.rs uses `cc.storage`. Load the app's preferences from its normal config dir like main.rs.
    /// Must call `<ui crate>::hosted::set_hosted(true)` first.
    pub fn new(ctx: &egui::Context,
               render_state: Option<&eframe::egui_wgpu::RenderState>,
               storage: Option<&dyn eframe::Storage>) -> Self;

    /// Exactly the FontDefinitions the app would pass to `ctx.set_fonts` at startup (default language),
    /// including its named families. Pure function, no Context needed.
    pub fn font_definitions() -> egui::FontDefinitions;

    /// Name of the active document / project / library for the host tab label: no app name, no dirty
    /// marker. None when nothing is open (start screen).
    pub fn document_title(&self) -> Option<String>;

    /// true if quitting now would lose unsaved work (any open document dirty).
    pub fn has_unsaved_changes(&self) -> bool;

    /// Open files exactly like passing them on the command line does.
    pub fn open_paths(&mut self, paths: &[std::path::PathBuf]);

    /// "Place"/insert/import files INTO the current document (as layer / placed object / footage / page /
    /// library import, whatever is natural for this app), at screen position `at` (egui points, same
    /// coordinate space as pointer positions) when the app can map it, else wherever its normal drop/place
    /// puts things. Falls back to open_paths when there is no document to place into.
    pub fn place_paths(&mut self, paths: &[std::path::PathBuf], at: Option<egui::Pos2>);

    /// The host tab became visible (true) / hidden (false). Hidden: stop playback/audio/scrubbing and
    /// anything that should not run unseen. Visible: do what the app normally does when its window
    /// regains focus (e.g. reload externally edited files).
    pub fn set_visible(&mut self, visible: bool);

    /// Optional host hook: when the app is about to open a file in ANOTHER application (e.g. "Edit in
    /// external editor", "Open with default app"), call `handler(path)` first; if it returns true the host
    /// handled it (e.g. opened it in a sibling Septet tab) and the app must not launch anything.
    /// Implement as a no-op store if the app never opens files externally.
    pub fn set_open_externally(&mut self, handler: Box<dyn FnMut(&std::path::Path) -> bool>);
}

impl eframe::App for Embedded {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame);   // forward (+ what main.rs's wrapper does)
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame);       // forward
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput); // forward
    fn save(&mut self, storage: &mut dyn eframe::Storage);  // forward if the app uses eframe storage
    fn on_exit(&mut self);  // save prefs / shutdown / flush like main.rs does on exit
}
```

3. A tiny `hosted` module in the app's UI crate (`crates/ui-egui/src/hosted.rs`, `pub mod hosted;`):
   `static HOSTED: AtomicBool; pub fn set_hosted(bool); pub fn is_hosted() -> bool`. When hosted:
   - NEVER call `ctx.set_fonts` (startup, language switches, UI font-size changes, plugins' output hooks …).
     Additive `ctx.add_font` is fine. `font_definitions()` above must return what would have been set.
     Keep the app's `fonts_ready`/`styled` gating working (it will see the host-installed fonts).
   - NEVER call `ctx.set_zoom_factor` (the host owns UI zoom).
   - Do not resize / maximize / position the window on startup (fit_window, work-area fitting …).
   - Disable the app's own custom title bar caption buttons / resize zones if it has a flag for it
     (e.g. custom_titlebar = false, integrated_titlebar = false) — the host draws the window chrome.
   Standalone behaviour (not hosted) must stay byte-for-byte the same.

4. Make every `bytes://…` URI the app registers with egui's image loaders unique to the app (e.g.
   `bytes://icons/x.svg` → `bytes://<app>/icons/x.svg`), because egui's bytes loader is shared and keeps the
   first bytes for a URI. (`include_image!` URIs already contain the file path and are fine.)

## Rules
- Only edit files inside your app's directory. Do not touch <workspace>/septet or
  other apps. Do not commit. Match the surrounding code style and the workspace lints (many apps deny
  unwrap/expect/panic and forbid unsafe).
- Build from the workspace root (shared target dir; other agents build concurrently, waiting for the cargo
  lock is normal):
    cd <workspace> && cargo build -p <app> --lib 2>&1 | grep -E '^(warning|error)' -A5 | head -80
    cd <workspace> && cargo build -p <app> --bins 2>&1 | grep -E '^(warning|error)' -A5 | head -80
  Both must succeed with no new warnings from your code. Don't run the full test suites (too slow); you may
  run a focused `cargo test -p <app>-ui-egui <filter>` if you touched logic that has tests.
- Finish with a short report: the final public API (signatures), what new() sets up, what place_paths does
  in this app, set_visible behaviour, files changed, and any caveats the host must know.
