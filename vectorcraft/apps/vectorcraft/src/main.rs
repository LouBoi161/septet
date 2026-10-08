//! VectorCraft desktop app.
//!
//! Usage: `vectorcraft [--control <port>] [--in-window-menus] [files…]`
//!
//! `--in-window-menus` (or `VECTORCRAFT_IN_WINDOW_MENUS=1`) keeps the menus inside the window on
//! macOS instead of the macOS menu bar (`mac_menu`).
//!
//! `--control <port>` (or `VECTORCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `vectorcraft_ui_egui::control` for the methods.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(all(feature = "windows7", any(feature = "wgpu", feature = "accessibility")))]
compile_error!("windows7 requires --no-default-features (wgpu and accessibility must be disabled)");
#[cfg(all(windows, feature = "windows7", not(target_vendor = "win7")))]
compile_error!("windows7 requires --target x86_64-win7-windows-msvc; the ordinary Windows target still imports newer APIs");
#[cfg(all(target_vendor = "win7", not(feature = "windows7")))]
compile_error!("the win7 target requires --no-default-features --features windows7");

mod control_server;
#[cfg(feature = "wgpu")]
mod gpu;
mod logging;
#[cfg(target_os = "macos")]
mod mac_menu;
#[cfg(target_os = "macos")]
mod open_documents;
mod window;

use vectorcraft_embed::desktop::{open_files, services};
use vectorcraft_embed::prefs::{load_prefs, prefs_enabled, read_prefs, save_prefs};
use vectorcraft_engine::Session;
use vectorcraft_ui_egui::VectorcraftApp;
use vectorcraft_ui_egui::graphics::GraphicsLoss;

struct App {
    app: VectorcraftApp,
    /// Reported by wgpu when the window's graphics device is lost (a driver reset).
    graphics_loss: GraphicsLoss,
    /// The graphics device was lost and the unsaved changes are kept for Data Recovery.
    graphics_lost: bool,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(why) = self.graphics_loss.take() {
            // eframe can't give a window a new device: the user saves and starts again.
            self.graphics_lost = self.app.graphics_lost(&why);
            self.app.status(if self.graphics_lost {
                "The graphics device was lost: unsaved changes are kept for Data Recovery. Save your documents and restart VectorCraft"
            } else {
                "The graphics device was lost: save your documents and restart VectorCraft"
            });
        }
        if self.graphics_lost && ctx.input(|i| i.viewport().close_requested()) {
            // The window can't show the Save Changes question: it closes, and the next launch
            // offers the changes back.
            vectorcraft_ui_egui::background::wait_all(&mut self.app);
            return;
        }
        #[cfg(target_os = "macos")]
        open_files(&mut self.app, open_documents::take());
        self.app.logic(ctx);
        window::track(ctx, &mut self.app.ui.window);
        if self.app.ui.status == "quit" {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        #[cfg(target_os = "macos")]
        if self.app.take_ime_discard() {
            vectorcraft_embed::desktop::discard_marked_text();
        }
    }
    #[cfg(not(feature = "windows7"))]
    fn on_exit(&mut self) {
        save_prefs(&self.app);
    }
    #[cfg(feature = "windows7")]
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        save_prefs(&self.app);
    }
}

/// Where the log files live: `logs` in the preferences folder (see `logging`).
fn log_dir() -> Option<std::path::PathBuf> {
    Some(vectorcraft_embed::prefs::data_dir()?.join("logs"))
}

/// The window, Dock, taskbar and app-switcher icon (`assets/app-icon/`, see its README). macOS gets
/// the version with Apple's transparent margin; elsewhere the full-bleed tile. The app ID matches
/// `packaging/linux/ai.storyteller.vectorcraft.desktop` so Wayland docks find the launcher icon.
fn app_icon() -> egui::IconData {
    #[cfg(target_os = "macos")]
    let png: &[u8] = include_bytes!("../../../assets/app-icon/vectorcraft-macos-512.png");
    #[cfg(not(target_os = "macos"))]
    let png: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.vectorcraft.png");
    eframe::icon_data::from_png_bytes(png).unwrap_or_default()
}

/// Windows and Linux: no OS title bar; the app bar is the title bar (`vectorcraft_ui_egui::titlebar`).
/// macOS keeps its traffic lights over the integrated title strip.
const CUSTOM_TITLEBAR: bool = !cfg!(target_os = "macos");

fn main() -> std::process::ExitCode {
    // First, so every start-up warning is recorded (`logging`).
    let logger = logging::install();
    vectorcraft_ui_egui::i18n::detect_system_lang_in_background();
    let mut control_port: Option<u16> = std::env::var("VECTORCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut files = Vec::new();
    let mut in_window_menus = std::env::var_os("VECTORCRAFT_IN_WINDOW_MENUS").is_some_and(|v| !v.is_empty() && v != "0");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = args.next().and_then(|p| p.parse().ok()),
            "--in-window-menus" => in_window_menus = true,
            "--version" => {
                println!("vectorcraft {}", env!("CARGO_PKG_VERSION"));
                return std::process::ExitCode::SUCCESS;
            }
            _ => files.push(a),
        }
    }
    // The log file lives in the settings directory, next to the preferences; opened after the
    // arguments, so `--version` leaves no file behind. Records logged until now are written to it
    // first. Runs without preferences (agents' test runs) log to standard error only, so they
    // don't rotate away the user's own logs.
    if let Some(logger) = logger {
        match log_dir().filter(|_| prefs_enabled()) {
            Some(dir) => match logger.attach_dir(&dir) {
                Ok(path) => log::info!("VectorCraft {}, log file {}", env!("CARGO_PKG_VERSION"), path.display()),
                // Standard error only by now (`attach_dir` gave up on the file); unlike `eprintln!`, never panics.
                Err(e) => log::warn!("no log file: {e}"),
            },
            None => logger.no_file(),
        }
    }
    let saved = read_prefs();
    let saved_window = saved.as_ref().and_then(|ui| ui.window);
    #[cfg(feature = "wgpu")]
    let gpu_pref = saved.as_ref().and_then(|ui| ui.engine_prefs.get("gpuPreference")).and_then(serde_json::Value::as_str);
    #[cfg(feature = "wgpu")]
    let power = gpu::power_preference(gpu_pref, eframe::wgpu::PowerPreference::from_env());
    #[cfg(feature = "wgpu")]
    let startup = std::sync::Arc::new(gpu::Startup::default());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VectorCraft")
            .with_inner_size(window::DEFAULT_SIZE)
            .with_min_inner_size(window::MIN_SIZE)
            .with_drag_and_drop(true)
            .with_decorations(!CUSTOM_TITLEBAR)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_icon(app_icon())
            .with_app_id("ai.storyteller.vectorcraft"),
        #[cfg(feature = "windows7")]
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    #[cfg(feature = "wgpu")]
    let options = {
        let mut options = options;
        // One frame queued, not two: the canvas is rasterized on the CPU and the GPU only
        // composites it, so the window answers the pointer a frame sooner (#444).
        options.wgpu_options.surface = eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY;
        // Only adapters that can show the window, in the order `gpu` gives (#306, #502).
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
            create.native_adapter_selector = Some(gpu::selector(power, startup.clone()));
        }
        options
    };
    #[cfg(feature = "wgpu")]
    gpu::watch_panics();
    // Files opened from Finder and the Dock arrive as events, not arguments.
    #[cfg(target_os = "macos")]
    open_documents::install();
    #[cfg(feature = "wgpu")]
    let created = startup.clone();
    // A panic that ends the window (egui-wgpu's own, #502) is caught here: never a crash.
    let outcome = vectorcraft_engine::guard::catch_panic(|| {
        eframe::run_native(
            "VectorCraft",
            options,
            Box::new(move |cc| {
                let mut app = VectorcraftApp::new(Session::new(), services());
                load_prefs(&mut app, saved);
                // Fit the window to its monitor, or put it back where it was (still hidden).
                if let Some(w) = cc.winit_window() {
                    app.ui.window = Some(window::restore(w, saved_window));
                }
                // The libraries and Data Recovery folders next to the preferences.
                vectorcraft_embed::desktop::set_user_folders(&mut app);
                let graphics_loss = GraphicsLoss::default();
                #[cfg(feature = "wgpu")]
                if let Some(rs) = &cc.wgpu_render_state {
                    let summary = vectorcraft_embed::desktop::adapter_summary(&rs.adapter.get_info());
                    log::info!("rendering with {summary} (power preference {power:?})");
                    created.created(&cc.egui_ctx);
                    if !gpu::skipped().is_empty() {
                        app.status(format!("The graphics processor tried first couldn't show the window, so VectorCraft started again on {summary}"));
                    }
                    app.graphics_adapter = Some(summary);
                    let (loss, ctx) = (graphics_loss.clone(), cc.egui_ctx.clone());
                    rs.device.set_device_lost_callback(move |reason, msg| loss.report(&ctx, format!("{reason:?}: {msg}")));
                }
                #[cfg(feature = "windows7")]
                {
                    app.graphics_adapter = Some("OpenGL (Windows 7 compatibility)".into());
                }
                app.integrated_titlebar = cfg!(target_os = "macos");
                app.custom_titlebar = CUSTOM_TITLEBAR;
                if let Some(port) = control_port {
                    let rx = control_server::start(port, cc.egui_ctx.clone());
                    app = app.with_control(rx);
                }
                #[cfg(target_os = "macos")]
                {
                    open_documents::set_ui(&cc.egui_ctx);
                    // The macOS menu bar, installed now so winit's default menu doesn't stay up.
                    if !in_window_menus {
                        app.services.native_menu = mac_menu::install(&cc.egui_ctx, &app);
                    }
                }
                #[cfg(not(target_os = "macos"))]
                let _ = in_window_menus;
                open_files(&mut app, files);
                Ok(Box::new(App { app, graphics_loss, graphics_lost: false }))
            }),
        )
    });
    #[cfg(feature = "wgpu")]
    let outcome = gpu::finish(outcome, &startup);
    #[cfg(not(feature = "wgpu"))]
    let outcome = outcome.and_then(|r| r.map_err(|e| e.to_string()));
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            log::error!("VectorCraft stopped: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(all(test, feature = "wgpu"))]
mod tests {
    use vectorcraft_engine::cmd::fileio;

    /// The file extensions the macOS bundle declares: its document types and its own exported type.
    fn plist_extensions(plist: &str) -> Vec<&str> {
        ["<key>CFBundleTypeExtensions</key>", "<key>public.filename-extension</key>"]
            .iter()
            .flat_map(|key| plist.split(key).skip(1))
            .filter_map(|rest| rest.split("</array>").next())
            .flat_map(|array| array.split("<string>").skip(1))
            .filter_map(|s| s.split("</string>").next())
            .collect()
    }

    /// Finder offers the app for every file File › Open reads (#295, #354), takes over no other
    /// app's files, and hands them to the app rather than to AppKit's document machinery.
    #[test]
    fn the_macos_bundle_opens_every_readable_format() {
        let plist = include_str!("../../../packaging/macos/Info.plist.in");
        let declared = plist_extensions(plist);
        for e in fileio::OPEN_EXTS {
            assert!(declared.contains(e), "Info.plist.in doesn't declare .{e}");
        }
        for e in &declared {
            assert!(fileio::OPEN_EXTS.contains(e), "Info.plist.in declares .{e}, which the app doesn't open");
        }
        assert!(!plist.contains("<key>NSDocumentClass</key>"), "not an NSDocument app: AppKit would refuse the files");
        let types = plist.matches("<key>CFBundleTypeName</key>").count();
        assert!(types > 1 && plist.matches("<key>LSHandlerRank</key>").count() == types, "every document type has a rank");
        assert_eq!(plist.matches("<string>Owner</string>").count(), 2, "only VectorCraft documents and templates are owned");
    }
}
