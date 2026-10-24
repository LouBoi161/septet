//! VectorCraft desktop app.
//!
//! Usage: `vectorcraft [--control <port> [--automation-read-root <dir>]... [--automation-write-root <dir>]...]
//! [--in-window-menus] [files…]`, `vectorcraft --version`, `vectorcraft --help` (or `-h`)
//!
//! `--in-window-menus` (or `VECTORCRAFT_IN_WINDOW_MENUS=1`) keeps the menus inside the window on
//! macOS instead of the macOS menu bar (`mac_menu`).
//!
//! `--control <port>` (or `VECTORCRAFT_CONTROL_PORT`; a port from 1 to 65535, or the app doesn't
//! start) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `vectorcraft_ui_egui::control` for the methods. `--automation-read-root` and
//! `--automation-write-root` (or `VECTORCRAFT_AUTOMATION_READ_ROOT` / `_WRITE_ROOT`) confine the
//! files its requests read and write (`vectorcraft_engine::file_access`); the person at the
//! keyboard isn't confined.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(all(feature = "windows7", any(feature = "wgpu", feature = "accessibility")))]
compile_error!("windows7 requires --no-default-features (wgpu and accessibility must be disabled)");
#[cfg(all(windows, feature = "windows7", not(target_vendor = "win7")))]
compile_error!("windows7 requires --target x86_64-win7-windows-msvc; the ordinary Windows target still imports newer APIs");
#[cfg(all(target_vendor = "win7", not(feature = "windows7")))]
compile_error!("the win7 target requires --no-default-features --features windows7");

mod automation_args;
#[cfg(feature = "wgpu")]
mod compositor_gpu;
mod control_server;
#[cfg(feature = "wgpu")]
mod gpu;
#[cfg(target_os = "linux")]
mod linux_stylus;
mod logging;
#[cfg(target_os = "macos")]
mod mac_menu;
#[cfg(target_os = "macos")]
mod mac_window;
#[cfg(target_os = "macos")]
mod open_documents;
mod window;
mod window_frame;
mod xwayland;

use vectorcraft_embed::desktop::{open_files, services};
use vectorcraft_embed::prefs::{load_prefs, prefs_enabled, read_prefs, save_prefs};
use vectorcraft_engine::Session;
use vectorcraft_engine::file_access::{self, AutomationRoots};
use vectorcraft_ui_egui::VectorcraftApp;
use vectorcraft_ui_egui::graphics::GraphicsLoss;

struct App {
    app: VectorcraftApp,
    /// Reported by wgpu when the window's graphics device is lost (a driver reset).
    graphics_loss: GraphicsLoss,
    /// The graphics device was lost and the unsaved changes are kept for Data Recovery.
    graphics_lost: bool,
    /// Frames the UI has run (see [`end_before_teardown`]).
    frames: u64,
    /// Keeps the custom title bar's window borderless (Windows, #1758).
    window_frame: Option<window_frame::WindowFrameFn>,
    /// The main window; used to read and set maximized state the UI layer does not see on Linux (#3635).
    winit_window: Option<std::sync::Arc<winit::window::Window>>,
    /// A saved or fitted geometry asked for maximize once the window is mapped (Linux).
    pending_maximize: bool,
    /// Frames left to retry [`window::apply_pending_maximize`].
    maximize_tries: u8,
}

impl eframe::App for App {
    // Each frame runs unconfined: it is the person at the keyboard's. The app confines what the
    // control channel asks for, and the frames carrying input it injected, to the automation roots
    // (`VectorcraftApp::with_automation_roots`), which are in force elsewhere in the process.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        file_access::unconfined(|| self.logic_frame(ctx, frame));
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // winit gives the window its caption back when it changes the window's flags (maximize,
        // restore, …): take it off again before the frame is laid out (#1758).
        if let Some(window_frame) = self.window_frame.as_mut() {
            window_frame();
        }
        #[cfg(target_os = "macos")]
        if self.app.services.native_menu.is_some() {
            mac_menu::raw_input_hook(raw);
        }
        file_access::unconfined(|| self.app.raw_input_hook(raw));
    }
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            let bar = vectorcraft_ui_egui::chrome::APP_BAR_HEIGHT;
            let inset = mac_window::center_buttons(frame, ui.ctx().zoom_factor(), bar);
            self.app.titlebar_inset = inset.unwrap_or(mac_window::DEFAULT_INSET);
        }
        file_access::unconfined(|| self.app.ui(ui));
        // Preferences are written when confirmed, not only at exit: a session ended any other way
        // (the system shutting down, the process ended or crashing) used to lose them (#1747).
        if self.app.take_prefs_changed() {
            file_access::unconfined(|| save_prefs(&self.app));
        }
        // Not while a file dialog's sheet has the keyboard: its name field's composition would go.
        #[cfg(target_os = "macos")]
        if !self.app.file_dialog_open() && self.app.take_ime_discard() {
            vectorcraft_embed::desktop::discard_marked_text();
        }
    }
    #[cfg(not(feature = "windows7"))]
    fn on_exit(&mut self) {
        file_access::unconfined(|| save_prefs(&self.app));
        end_before_teardown(self.frames);
    }
    #[cfg(feature = "windows7")]
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        file_access::unconfined(|| save_prefs(&self.app));
    }
}

impl App {
    fn logic_frame(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames = ctx.cumulative_frame_nr();
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
        // Files from the Finder and the Dock wait while a file dialog is open: what asked for it runs
        // again on its document when it answers.
        #[cfg(target_os = "macos")]
        if !self.app.file_dialog_open() {
            open_files(&mut self.app, open_documents::take());
        }
        self.app.logic(ctx);
        if self.pending_maximize {
            if let Some(w) = &self.winit_window {
                self.pending_maximize = window::apply_pending_maximize(w, true, &mut self.maximize_tries);
            } else {
                self.pending_maximize = false;
            }
        }
        let host_maximized = self.winit_window.as_ref().map(|w| w.is_maximized());
        window::track(ctx, &mut self.app.ui.window, host_maximized);
        if self.app.ui.status == "quit" {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Where the log files live: `logs` in the preferences folder (see `logging`).
fn log_dir() -> Option<std::path::PathBuf> {
    Some(vectorcraft_embed::prefs::data_dir()?.join("logs"))
}

/// On macOS with accessibility, end the process once the app has saved what it keeps on quitting,
/// before eframe tears the window down (#661). AccessKit then gives the window's content view back
/// its own class, which AppKit's Touch Bar support was observing it under, and AppKit aborts the
/// app as it quits on Macs with a Touch Bar. Nothing after the window is needed on a normal quit.
/// Not while the window is still starting up (`frames` below [`gpu::STARTUP_FRAMES`]): a graphics
/// failure then starts the app again on another adapter (#651).
#[allow(unused_variables)]
fn end_before_teardown(frames: u64) {
    #[cfg(all(target_os = "macos", feature = "accessibility", feature = "wgpu"))]
    if frames >= gpu::STARTUP_FRAMES {
        log::info!("quitting");
        log::logger().flush();
        std::process::exit(0);
    }
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

/// Windows shows a window's big icon (`ICON_BIG`) in the taskbar and Alt+Tab; eframe sets only the
/// small one (the title bar's), so the taskbar showed a generic icon. Set the big one too.
#[cfg(windows)]
fn set_taskbar_icon(w: &winit::window::Window) {
    use winit::platform::windows::WindowExtWindows as _;
    let icon = app_icon();
    // An icon that can't be made leaves the generic one: nothing else depends on it.
    if let Ok(i) = winit::window::Icon::from_rgba(icon.rgba, icon.width, icon.height) {
        w.set_taskbar_icon(Some(i));
    }
}

/// Windows and Linux: no OS title bar; the app bar is the title bar (`vectorcraft_ui_egui::titlebar`).
/// macOS keeps its traffic lights over the integrated title strip.
///
/// Used on macOS and Windows (where VectorCraft keeps today's frame regardless of the
/// `craft-window` probe) and by the tests; Linux and the BSDs take the decision from the probe.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
const CUSTOM_TITLEBAR: bool = !cfg!(target_os = "macos");

/// Whether the user asked for the system title bar in Preferences › User Interface › System Title
/// Bar. Read from the saved preferences before the window opens; a missing or unreadable value is
/// `false` (the default). This preference forces the desktop/native frame back on.
fn opted_into_system_title_bar(saved: Option<&vectorcraft_ui_egui::UiState>) -> bool {
    saved.map(|ui| ui.engine_prefs.get("systemTitleBar").and_then(serde_json::Value::as_bool).unwrap_or(false)).unwrap_or(false)
}

/// Whether this start draws its own title bar, given the frame the shared `craft-window` crate
/// decided and the user's System Title Bar preference.
///
/// The suite-wide rule (`craft_window::Choice`) is: the desktop draws the frame
/// ([`craft_window::Frame::Desktop`]) on macOS, Windows, X11 and any Wayland compositor with
/// server-side decorations (KDE, Sway, Hyprland, COSMIC); the app draws it
/// ([`craft_window::Frame::App`]) only where the compositor leaves it to the app (GNOME's Mutter),
/// or when `VECTORCRAFT_TITLEBAR` overrides the probe. VectorCraft draws its own title bar exactly
/// when `craft-window` says the app owns the frame **and** the user has not opted into the system
/// title bar; the preference always forces the native frame back on.
///
/// macOS and Windows keep today's behavior and are decided by [`CUSTOM_TITLEBAR`] rather than the
/// probe: `craft-window` returns `Frame::Desktop` for both (the OS always draws a frame), but
/// VectorCraft insets its content under the macOS traffic lights and draws its own caption buttons
/// on Windows — an overdraw on an OS-decorated window that is VectorCraft's choice, not
/// `craft-window`'s. Only Linux and the BSDs take the decision from the probe.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn custom_titlebar_from(frame: &craft_window::Frame, opted_into_system: bool) -> bool {
    matches!(frame, craft_window::Frame::App(_)) && !opted_into_system
}

/// macOS and Windows: today's behavior, independent of the `craft-window` probe (see
/// [`custom_titlebar_from`]). macOS never draws its own frame (`CUSTOM_TITLEBAR` is `false`);
/// Windows draws its own caption unless the user opted into the system title bar.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn custom_titlebar_from(_frame: &craft_window::Frame, opted_into_system: bool) -> bool {
    CUSTOM_TITLEBAR && !opted_into_system
}

/// Whether this start draws its own title bar. Probes the shared `craft-window` crate once (its
/// override variable is `VECTORCRAFT_TITLEBAR`) and combines its decision with the saved System
/// Title Bar preference. Call once at startup, before [`native_options`].
///
/// On Linux, Preferences › Performance › Linux Display Server › X11 opens the window through
/// XWayland (below). A GNOME/Wayland probe would say `Frame::App` there, but the window really runs
/// on X11, whose window manager draws the frame — so when XWayland is being forced, keep the native
/// frame, the same as a real X11 session. An explicit `VECTORCRAFT_TITLEBAR` override still wins.
fn custom_titlebar(saved: Option<&vectorcraft_ui_egui::UiState>) -> bool {
    let choice = craft_window::Choice::probe(craft_window::Prefix::new("VECTORCRAFT"));
    let opted_into_system = opted_into_system_title_bar(saved);
    #[cfg(target_os = "linux")]
    {
        let pref = saved.and_then(|ui| ui.engine_prefs.get("linuxDisplayServer")).and_then(serde_json::Value::as_str);
        // `overridden() == None` means the probe decided (no VECTORCRAFT_TITLEBAR). When the window
        // will actually open on X11 (XWayland) — the user asked for it *and* the X11 libraries are
        // present, the same two conditions the launch path checks below — that run is effectively
        // X11, so keep the native frame. If the libraries are missing, startup stays on Wayland, so
        // the probe's own decision (app-drawn on GNOME) must stand.
        let forced_to_x11 = xwayland::wanted(pref, |name| std::env::var_os(name), xwayland::pen_attached) && xwayland::x11_libraries_found();
        if choice.overridden().is_none() && forced_to_x11 {
            return custom_titlebar_from(&craft_window::Frame::Desktop, opted_into_system);
        }
    }
    custom_titlebar_from(choice.frame(), opted_into_system)
}

fn native_options(custom_titlebar: bool) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VectorCraft")
            .with_inner_size(window::DEFAULT_SIZE)
            .with_min_inner_size(window::MIN_SIZE)
            .with_drag_and_drop(true)
            .with_decorations(!custom_titlebar)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_icon(app_icon())
            .with_app_id("ai.storyteller.vectorcraft"),
        #[cfg(feature = "windows7")]
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    }
}

/// `vectorcraft --help`. Headless work (convert, run, MCP) is `vectorcraft-cli`'s.
const USAGE: &str = "\
Usage: vectorcraft [options] [files…]

Opens the VectorCraft desktop app with the given files.

Options:
  --control <port>               Start the localhost JSON-lines control server on <port>
                                 (or VECTORCRAFT_CONTROL_PORT)
  --automation-read-root <dir>   Confine the files control requests read to <dir>
                                 (may be given more than once)
  --automation-write-root <dir>  Confine the files control requests write to <dir>
                                 (may be given more than once)
  --in-window-menus              Keep the menus inside the window on macOS
                                 (or VECTORCRAFT_IN_WINDOW_MENUS=1)
  --version                      Print the version and exit
  -h, --help                     Print this help and exit

Without a window (converting, scripting, the MCP server), use vectorcraft-cli:
  vectorcraft-cli --help
";

fn main() -> std::process::ExitCode {
    // Before any thread starts: the pointer must find a cursor theme on Wayland (#2450).
    #[cfg(target_os = "linux")]
    vectorcraft_embed::cursor_theme::apply();
    // Validate roots before logging, font scans, preferences or the window: a missing value
    // must not clear an environment root and start the control channel without confinement.
    let mut control = automation_args::ControlArg::default();
    let root_env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    let mut roots_args =
        automation_args::AutomationArgs::new(root_env("VECTORCRAFT_AUTOMATION_READ_ROOT"), root_env("VECTORCRAFT_AUTOMATION_WRITE_ROOT"));
    let mut files = Vec::new();
    let mut in_window_menus = std::env::var_os("VECTORCRAFT_IN_WINDOW_MENUS").is_some_and(|v| !v.is_empty() && v != "0");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match roots_args.consume(&a, &mut args).and_then(|root| if root { Ok(true) } else { control.consume(&a, &mut args) }) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => {
                eprintln!("vectorcraft: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
        match a.as_str() {
            "--in-window-menus" => in_window_menus = true,
            "--version" => {
                println!("vectorcraft {}", env!("CARGO_PKG_VERSION"));
                return std::process::ExitCode::SUCCESS;
            }
            // Before anything starts, like `--version`: from a terminal or an agent's shell the app
            // used to open (or hang without a display) instead (#2110).
            "--help" | "-h" => {
                // Not `print!`, which panics when standard output is closed (`| head -0`):
                // there is nothing left to tell then.
                let _ = std::io::Write::write_all(&mut std::io::stdout(), USAGE.as_bytes());
                return std::process::ExitCode::SUCCESS;
            }
            _ => files.push(a),
        }
    }
    let automation_args::AutomationArgs { read_roots, write_roots, .. } = roots_args;
    // After the arguments: `--control` and `--version` win over a bad environment value.
    let control_port = match control.port {
        Some(port) => Some(port),
        None => match automation_args::env_control_port(std::env::var_os("VECTORCRAFT_CONTROL_PORT")) {
            Ok(port) => port,
            Err(e) => {
                eprintln!("vectorcraft: {e}");
                return std::process::ExitCode::FAILURE;
            }
        },
    };
    // Before desktop services, so every start-up warning is recorded (`logging`).
    let logger = logging::install();
    vectorcraft_ui_egui::i18n::detect_system_lang_in_background();
    // Before the first font scan (the app's start): the fonts font services load (#579).
    vectorcraft_embed::desktop::install_platform_fonts();
    // VectorCraft's own Fonts folder, which text.addFontFiles copies fonts into.
    vectorcraft_embed::prefs::install_fonts();
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
    // The roots confine the control channel: a bad one keeps the app from starting rather than
    // leaving agents unconfined. Without a control port there is nothing to confine.
    let roots = if control_port.is_none() {
        if !read_roots.is_empty() || !write_roots.is_empty() {
            log::warn!("--automation-read-root and --automation-write-root confine the control channel: ignored without --control");
        }
        None
    } else {
        let read: Vec<&std::path::Path> = read_roots.iter().map(std::path::PathBuf::as_path).collect();
        let write: Vec<&std::path::Path> = write_roots.iter().map(std::path::PathBuf::as_path).collect();
        match AutomationRoots::new(&read, &write) {
            Ok(roots) => roots,
            Err(e) => {
                log::error!("{e}");
                eprintln!("vectorcraft: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    };
    let saved = read_prefs();
    let saved_window = saved.as_ref().and_then(|ui| ui.window);
    #[cfg(feature = "wgpu")]
    let gpu_pref = saved.as_ref().and_then(|ui| ui.engine_prefs.get("gpuPreference")).and_then(serde_json::Value::as_str);
    #[cfg(feature = "wgpu")]
    let power_env = eframe::wgpu::PowerPreference::from_env();
    #[cfg(feature = "wgpu")]
    let power = gpu::power_preference(gpu_pref, power_env);
    // Automatic draws on the GPU that drives the (primary) display: a GPU without a monitor reset
    // its driver and took every monitor down (pdfcraft#378). Logged first: the first question in
    // every black-window report.
    #[cfg(feature = "wgpu")]
    let displays = gpu::preferred_displays(gpu_pref, power_env);
    // Under Wayland, the GPU the compositor draws with comes even before those (src/compositor_gpu.rs).
    #[cfg(feature = "wgpu")]
    let compositor = gpu::preferred_compositor(gpu_pref, power_env);
    #[cfg(feature = "wgpu")]
    let chosen_gpu = !gpu::automatic(gpu_pref, power_env);
    #[cfg(feature = "wgpu")]
    if !chosen_gpu {
        let listed: Vec<String> = displays.iter().map(ToString::to_string).collect();
        log::info!("display GPUs (PCI vendor:device): {}", if listed.is_empty() { "unknown".to_string() } else { listed.join(", ") });
    } else {
        log::info!("graphics processor chosen by the user ({power:?}): which GPU drives the display isn't considered");
    }
    #[cfg(feature = "wgpu")]
    let startup = std::sync::Arc::new(gpu::Startup::default());
    // A driver that crashes the process while the window starts leaves this start recorded in the
    // log folder; the next start leaves that adapter out (#2243). Runs without preferences, which
    // write no log file, don't record their starts.
    #[cfg(feature = "wgpu")]
    let left_out = match log_dir().filter(|_| prefs_enabled()) {
        Some(dir) => startup.track(&dir, gpu_pref, gpu::explicit_choice()),
        None => gpu::LeftOut::default(),
    };
    #[cfg(feature = "wgpu")]
    let crash = left_out.crash.clone();
    let custom_titlebar = custom_titlebar(saved.as_ref());
    let options = native_options(custom_titlebar);
    // winit picks Wayland whenever `WAYLAND_DISPLAY` is set; Preferences › Performance › Linux
    // Display Server › X11 opens the window through XWayland instead, where file drops work (#1151),
    // and X11 (XWayland) When a Pen Is Attached does so when a pen is attached, where the pen draws (#2028).
    #[cfg(target_os = "linux")]
    let options = {
        let mut options = options;
        let pref = saved.as_ref().and_then(|ui| ui.engine_prefs.get("linuxDisplayServer")).and_then(serde_json::Value::as_str);
        if xwayland::wanted(pref, |name| std::env::var_os(name), xwayland::pen_attached) {
            if xwayland::x11_libraries_found() {
                if pref == Some("x11WithPen") {
                    log::info!("pen found: opening the window through XWayland (Preferences › Performance › Linux Display Server)");
                } else {
                    log::info!("opening the window through XWayland (Preferences › Performance › Linux Display Server)");
                }
                use winit::platform::x11::EventLoopBuilderExtX11 as _;
                options.event_loop_builder = Some(Box::new(|builder| {
                    builder.with_x11();
                }));
            } else {
                log::warn!("Linux Display Server is X11, but the X11 libraries aren't all installed: opening the window on Wayland");
            }
        }
        options
    };
    #[cfg(feature = "wgpu")]
    let options = {
        let mut options = options;
        // One frame queued, not two: the canvas is rasterized on the CPU and the GPU only
        // composites it, so the window answers the pointer a frame sooner (#444).
        options.wgpu_options.surface = eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY;
        // Only adapters that can show the window, in the order `gpu` gives (#306, #502).
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
            create.native_adapter_selector = Some(gpu::selector(power, displays, compositor, left_out.adapters.clone(), startup.clone()));
            // Nothing draws or dispatches indirectly, so wgpu's check of indirect arguments only
            // costs a compute shader at start-up, one some drivers can't compile (OCLP-patched
            // Metal on an Iris Pro, #651). `WGPU_VALIDATION_INDIRECT_CALL=1` turns it back on.
            create.instance_descriptor.flags =
                (eframe::wgpu::InstanceFlags::from_build_config() - eframe::wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL).with_env();
            create.instance_descriptor.backends =
                left_out.backends(gpu::backends(create.instance_descriptor.backends, std::env::var_os("WGPU_BACKEND").is_some()));
            // DX12 shaders compile with FXC, part of Windows: wgpu's default loads whichever
            // dxcompiler.dll the search path finds, and one without its dxil.dll fails every shader,
            // losing the device before the first frame (#1194). `WGPU_DX12_COMPILER` picks another.
            create.instance_descriptor.backend_options.dx12.shader_compiler =
                eframe::wgpu::Dx12Compiler::from_env().unwrap_or(eframe::wgpu::Dx12Compiler::Fxc);
            // The window's surface may be as large as the adapter allows, not eframe's fixed
            // 8192 px: a window stretched over several monitors, or grown by a display scale
            // change, otherwise fails to configure its surface and the app closes (#1459).
            let device = create.device_descriptor.clone();
            create.device_descriptor = std::sync::Arc::new(move |adapter: &eframe::wgpu::Adapter| {
                let mut descriptor = device(adapter);
                let limit = &mut descriptor.required_limits.max_texture_dimension_2d;
                *limit = gpu::surface_limit(*limit, adapter.limits().max_texture_dimension_2d);
                descriptor
            });
        }
        options
    };
    #[cfg(feature = "wgpu")]
    gpu::watch_panics();
    #[cfg(feature = "wgpu")]
    gpu::watch_first_frame(startup.clone());
    // Files opened from Finder and the Dock arrive as events, not arguments.
    #[cfg(target_os = "macos")]
    open_documents::install();
    #[cfg(feature = "wgpu")]
    let created = startup.clone();
    // A panic that ends the window (egui-wgpu's own, #502) is caught here: never a crash.
    let run = |options: eframe::NativeOptions, app_creator: eframe::AppCreator<'_>| {
        #[cfg(target_os = "linux")]
        {
            linux_stylus::run_native("VectorCraft", options, app_creator)
        }
        #[cfg(not(target_os = "linux"))]
        {
            eframe::run_native("VectorCraft", options, app_creator)
        }
    };
    let outcome = vectorcraft_engine::guard::catch_panic(|| {
        run(
            options,
            Box::new(move |cc| {
                let mut session = Session::new();
                // The profiles loaded before come back before the preferences name any of them.
                vectorcraft_embed::prefs::attach_profile_store(&mut session);
                vectorcraft_embed::prefs::attach_plugin_store(&mut session);
                let mut app = VectorcraftApp::new(session, services(cc.winit_window().cloned(), cc.egui_ctx.clone()));
                load_prefs(&mut app, saved);
                // The saved PDF presets live in their own folder (also read by vectorcraft-cli),
                // which takes over from the copy the preferences file used to hold.
                vectorcraft_embed::prefs::attach_pdf_preset_store(&mut app.session);
                // Fit the window to its monitor, or put it back where it was (still hidden).
                let winit_window = cc.winit_window().cloned();
                if let Some(w) = &winit_window {
                    let g = window::restore(w, saved_window);
                    app.ui.window = Some(g);
                    #[cfg(windows)]
                    set_taskbar_icon(w);
                }
                // The libraries and Data Recovery folders next to the preferences.
                vectorcraft_embed::desktop::set_user_folders(&mut app);
                // Linux may not map the window before the first frames, so a saved maximize is retried then (#3635).
                let pending_maximize = cfg!(target_os = "linux") && app.ui.window.as_ref().is_some_and(|g| g.maximized);
                let graphics_loss = GraphicsLoss::default();
                #[cfg(feature = "wgpu")]
                if let Some(rs) = &cc.wgpu_render_state {
                    let summary = vectorcraft_embed::desktop::adapter_summary(&rs.adapter.get_info());
                    log::info!("rendering with {summary} (power preference {power:?})");
                    created.created(&cc.egui_ctx);
                    if !gpu::skipped().is_empty() {
                        // A processor chosen in Settings can be the one that can't show the window.
                        let hint = if chosen_gpu {
                            " (Settings › Performance › Graphics Processor › Automatic uses the one that drives your display)"
                        } else {
                            ""
                        };
                        app.status(format!(
                            "The graphics processor tried first couldn't show the window, so VectorCraft started again on {summary}{hint}"
                        ));
                    } else if let Some(crash) = &crash {
                        app.status(match crash {
                            gpu::Crash::Adapter { name, .. } => format!(
                                "The last start of VectorCraft ended on {name} before its window appeared, so it started on {summary}. To try that graphics processor again, change Preferences › Performance › Graphics Processor or start with WGPU_BACKEND set"
                            ),
                            gpu::Crash::Listing => format!(
                                "The last start of VectorCraft ended while it listed the graphics processors, so it started on {summary} with OpenGL alone. To try the other graphics backends again, change Preferences › Performance › Graphics Processor or start with WGPU_BACKEND set"
                            ),
                        });
                    }
                    app.graphics_adapter = Some(summary);
                    let (loss, ctx) = (graphics_loss.clone(), cc.egui_ctx.clone());
                    rs.device.set_device_lost_callback(move |reason, msg| loss.report(&ctx, format!("{reason:?}: {msg}")));
                }
                #[cfg(feature = "windows7")]
                {
                    app.graphics_adapter = Some("OpenGL (Windows 7 compatibility)".into());
                }
                app.custom_titlebar = custom_titlebar;
                if let Some(port) = control_port {
                    let rx = control_server::start(port, cc.egui_ctx.clone());
                    app = app.with_control(rx).with_automation_roots(roots.clone());
                    if let Some(roots) = &roots {
                        // Everywhere the app doesn't say whose work it is (worker threads), the
                        // roots hold: unconfined is only what the person at the keyboard does.
                        file_access::confine_process(roots.clone());
                        log::info!("control channel confined to the automation roots: {}", roots.describe());
                    }
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
                file_access::unconfined(|| open_files(&mut app, files));
                // The first frame is due from now (`gpu::watch_first_frame`).
                #[cfg(feature = "wgpu")]
                created.ready();
                // The content and the pointer stay aligned on Windows (#1758).
                let window_frame = window_frame::service(cc, custom_titlebar);
                Ok(Box::new(App {
                    app,
                    graphics_loss,
                    graphics_lost: false,
                    frames: 0,
                    window_frame,
                    winit_window,
                    pending_maximize,
                    maximize_tries: 8,
                }))
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

    fn ui_state_with_prefs(engine_prefs: serde_json::Value) -> vectorcraft_ui_egui::UiState {
        vectorcraft_ui_egui::UiState { engine_prefs, ..Default::default() }
    }

    #[test]
    fn the_system_title_bar_preference_keeps_the_window_decorations() {
        use craft_window::{Frame, Layout};

        // The System Title Bar preference (Preferences › User Interface) is read the same way on
        // every platform; a missing or unreadable value is `false`.
        assert!(!super::opted_into_system_title_bar(None), "no preferences: the default");
        let off = ui_state_with_prefs(serde_json::json!({"systemTitleBar": false}));
        assert!(!super::opted_into_system_title_bar(Some(&off)));
        let on = ui_state_with_prefs(serde_json::json!({"systemTitleBar": true}));
        assert!(super::opted_into_system_title_bar(Some(&on)));

        // The suite-wide mapping (Frame + preference -> whether VectorCraft draws its own title
        // bar). The desktop frame (macOS, Windows, X11, KDE/Wayland, …) always keeps native
        // decorations; the app frame (GNOME/Wayland, or a `VECTORCRAFT_TITLEBAR` override) draws
        // our own title bar — unless the user opted into the system title bar, which forces the
        // native frame back on.
        let app = Frame::App(Layout::default());
        let desktop = Frame::Desktop;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            assert!(super::custom_titlebar_from(&app, false), "GNOME/Wayland: VectorCraft draws its own");
            assert!(!super::custom_titlebar_from(&app, true), "System Title Bar preference forces native");
            assert!(!super::custom_titlebar_from(&desktop, false), "KDE/X11/…: keep the compositor's frame");
            assert!(!super::custom_titlebar_from(&desktop, true));
        }
        // macOS and Windows keep today's behavior, independent of the probed frame: the pref still
        // forces the native frame back on where a custom one would otherwise be drawn.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            assert_eq!(super::custom_titlebar_from(&desktop, false), super::CUSTOM_TITLEBAR);
            assert_eq!(super::custom_titlebar_from(&app, false), super::CUSTOM_TITLEBAR, "the probe does not change macOS/Windows");
            assert!(!super::custom_titlebar_from(&desktop, true));
        }

        assert_eq!(super::native_options(false).viewport.decorations, Some(true));
        assert_eq!(super::native_options(true).viewport.decorations, Some(false));
    }

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

    /// Finder offers the app for every file File › Open reads (#295, #354) but those with an
    /// extension in `UNASSOCIATED_EXTS`, takes over no other app's files, and hands them to the app
    /// rather than to AppKit's document machinery.
    #[test]
    fn the_macos_bundle_opens_every_readable_format() {
        let plist = include_str!("../../../packaging/macos/Info.plist.in");
        let declared = plist_extensions(plist);
        for e in fileio::OPEN_EXTS.iter().filter(|e| !fileio::UNASSOCIATED_EXTS.contains(e)) {
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

    /// The MIME types a Linux desktop gives each extension in `OPEN_EXTS` but `UNASSOCIATED_EXTS`
    /// (one row per type): shared-mime-info's, or one that `ai.storyteller.vectorcraft.mime.xml`
    /// declares.
    const LINUX_TYPES: &[(&str, &str)] = &[
        ("vectorcraft", "application/x-vectorcraft"),
        ("drawcraft", "application/x-vectorcraft"),
        ("svg", "image/svg+xml"),
        ("svgz", "image/svg+xml-compressed"),
        ("pdf", "application/pdf"),
        ("ai", "application/illustrator"), // brand-ok: shared-mime-info's type
        ("ait", "application/x-ai-template"),
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("gif", "image/gif"),
        ("webp", "image/webp"),
        ("tif", "image/tiff"),
        ("tiff", "image/tiff"),
        ("bmp", "image/bmp"),
        ("vctemplate", "application/x-vectorcraft-template"),
        ("dxf", "image/vnd.dxf"),
        ("emf", "image/emf"),
        ("wmf", "image/wmf"),
        ("eps", "image/x-eps"),
        ("af", "application/x-affinity"),
        ("afdesign", "application/x-affinity"),
        ("afpub", "application/x-affinity"),
    ];

    /// The types in `LINUX_TYPES` that shared-mime-info defines. The MIME package declares every
    /// other one.
    const LINUX_SHARED_TYPES: &[&str] = &[
        "image/svg+xml",
        "image/svg+xml-compressed",
        "application/pdf",
        "application/illustrator", // brand-ok: shared-mime-info's type
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/tiff",
        "image/bmp",
        "image/vnd.dxf",
        "image/emf",
        "image/wmf",
        "image/x-eps",
    ];

    /// Extensions in `LINUX_TYPES` whose type the MIME package matches by file content and not by a
    /// glob, with the package's `<magic>` rule for that type (its lines joined, indentation removed).
    const LINUX_BY_CONTENT: &[(&str, &str)] = &[(
        "af",
        // `MAGIC` at offset 0 and `DOCUMENT_KIND` at offset 8 (crates/affinity/src/container.rs).
        r#"<magic><match type="string" offset="0" value="\x00\xff\x4b\x41"><match type="string" offset="8" value="nsrP"/></match></magic>"#,
    )];

    /// The desktop entry lists a MIME type for every extension in `OPEN_EXTS` but
    /// `UNASSOCIATED_EXTS`, and the app's MIME package declares the types shared-mime-info doesn't
    /// have.
    #[test]
    fn the_linux_desktop_entry_opens_every_readable_format() {
        let desktop = include_str!("../../../packaging/linux/ai.storyteller.vectorcraft.desktop");
        let mime = include_str!("../../../packaging/linux/ai.storyteller.vectorcraft.mime.xml");
        let listed: Vec<&str> =
            desktop.lines().find_map(|l| l.strip_prefix("MimeType=")).unwrap_or_default().split(';').filter(|t| !t.is_empty()).collect();
        // Each type the MIME package declares, with the extensions of its globs and its `<magic>`
        // rule (empty when it has none) in the form `LINUX_BY_CONTENT` gives.
        let declared: Vec<(&str, Vec<&str>, String)> = mime
            .split("<mime-type type=\"")
            .skip(1)
            .map(|block| {
                let body = block.split("</mime-type>").next().unwrap_or_default();
                let globs = body.split("<glob pattern=\"*.").skip(1).filter_map(|g| g.split('"').next()).collect();
                let magic = match (body.find("<magic"), body.find("</magic>")) {
                    (Some(start), Some(end)) => body[start..end + "</magic>".len()].lines().map(str::trim).collect(),
                    _ => String::new(),
                };
                (block.split('"').next().unwrap_or_default(), globs, magic)
            })
            .collect();
        let mut associated: Vec<&str> = fileio::OPEN_EXTS.iter().copied().filter(|e| !fileio::UNASSOCIATED_EXTS.contains(e)).collect();
        let mut typed: Vec<&str> = LINUX_TYPES.iter().map(|(e, _)| *e).collect();
        associated.sort_unstable();
        typed.sort_unstable();
        typed.dedup();
        assert_eq!(typed, associated, "a MIME type for each extension the packaging associates with the app");
        for (e, t) in LINUX_TYPES {
            assert!(listed.contains(t), "the desktop entry doesn't list {t} (.{e})");
            let package = declared.iter().find(|(name, ..)| name == t);
            if LINUX_SHARED_TYPES.contains(t) {
                assert!(package.is_none(), "{t} is shared-mime-info's, and the MIME package declares it again");
                continue;
            }
            let Some((_, globs, magic)) = package else { panic!("the MIME package doesn't declare {t} (.{e})") };
            if let Some((_, rule)) = LINUX_BY_CONTENT.iter().find(|(content, _)| content == e) {
                assert!(!globs.contains(e), "{t} matches .{e} files by content only");
                assert_eq!(magic, rule, "{t}'s magic rule for .{e} files");
            } else {
                assert!(globs.contains(e), "{t} has no *.{e} glob");
            }
        }
        for t in &listed {
            assert!(LINUX_TYPES.iter().any(|(_, known)| known == t), "the desktop entry lists {t}, which the app doesn't open");
        }
        for (t, globs, _) in &declared {
            assert!(listed.contains(t), "the desktop entry doesn't list the declared {t}");
            for g in globs {
                assert!(LINUX_TYPES.contains(&(*g, *t)), "{t}'s *.{g} isn't a file the app opens as {t}");
            }
        }
    }
}
