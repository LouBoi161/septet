//! DesignCraft desktop app.
//!
//! Usage: `designcraft [--control <port>] [--sample] [files…]`
//!
//! `--control <port>` (or `DESIGNCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `designcraft_ui_egui::control` for the methods.
//!
//! The graphics backend (DirectX 12, Vulkan, Metal or OpenGL) is chosen in `gpu` before the window
//! exists, with a fallback for a driver that crashes at start-up; `WGPU_BACKEND` overrides it.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod control_server;
mod gpu;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;

use designcraft_embed::prefs::{no_prefs, save_prefs};
use designcraft_embed::profile;
use designcraft_ui_egui::DesignApp;

struct App {
    app: DesignApp,
    #[cfg(target_os = "macos")]
    menu: Option<native_menu::NativeMenu>,
    /// The graphics start in progress (`gpu.json`) until a frame has been presented; see `gpu`.
    startup: Option<gpu::Startup>,
    /// Frames begun so far.
    frames: u32,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The third call: two frames went through `ui`, so the first was presented and the driver
        // survived it (a fault at the first present is what the fallback in `gpu` is for).
        if self.frames >= 2
            && let Some(s) = self.startup.as_mut()
        {
            s.presented();
            self.startup = None;
        }
        self.frames = self.frames.saturating_add(1);
        #[cfg(target_os = "macos")]
        {
            if self.menu.is_none() && std::env::var_os("DESIGNCRAFT_NO_NATIVE_MENU").is_none() {
                self.menu = Some(native_menu::NativeMenu::install(&mut self.app));
            }
            if let Some(m) = &mut self.menu {
                m.poll(&mut self.app, ctx);
            }
        }
        self.app.logic(ctx);
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
    fn on_exit(&mut self) {
        // A normal exit before the third frame is not a crash.
        if let Some(s) = self.startup.as_mut() {
            s.presented();
        }
        save_prefs(&self.app);
    }
}

/// Desktop app ID: the Linux `.desktop` file name and hicolor icon name (Wayland matches windows by it).
const APP_ID: &str = "ai.storyteller.designcraft";

/// The window, Dock, taskbar and Alt-Tab icon. macOS gets Apple's icon grid (transparent margin);
/// elsewhere the full tile. Regenerate with `packaging/icons.sh`.
fn app_icon() -> Option<egui::IconData> {
    #[cfg(target_os = "macos")]
    let png: &[u8] = include_bytes!("../../../assets/app-icon/designcraft-macos-512.png");
    #[cfg(not(target_os = "macos"))]
    let png: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.designcraft.png");
    eframe::icon_data::from_png_bytes(png).map_err(|e| log::warn!("app icon: {e}")).ok()
}

/// The control port from `--control <port>` or `DESIGNCRAFT_CONTROL_PORT` (`source`); a value that
/// is not a port is logged and ignored instead of silently starting no control channel.
fn control_port_from(source: &str, value: Option<String>) -> Option<u16> {
    let value = value?;
    let port = value.trim().parse().ok();
    if port.is_none() {
        log::warn!("{source}: {value:?} is not a port number; the control channel is off");
    }
    port
}

fn main() -> eframe::Result {
    // First, so every start-up record (and the engine's panic hook, installed with the first
    // Session) is captured; see `logging`.
    let logger = logging::install();
    let mut control_port = control_port_from("DESIGNCRAFT_CONTROL_PORT", std::env::var("DESIGNCRAFT_CONTROL_PORT").ok());
    let mut files = Vec::new();
    let mut sample = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = control_port_from("--control", args.next()),
            "--sample" => sample = true,
            "--version" => {
                println!("designcraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => files.push(a),
        }
    }
    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // leaves no file behind. Records logged until now are written to it first.
    if let (Some(logger), Some(dir)) = (logger, profile::config_dir()) {
        match logger.attach_dir(&dir.join("logs")) {
            Ok(path) => log::info!("DesignCraft {}, log file {}", env!("CARGO_PKG_VERSION"), path.display()),
            // Standard error only by now (`attach_dir` gave up on the file); unlike `eprintln!`, never panics.
            Err(e) => log::warn!("no log file: {e}"),
        }
    }
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DesignCraft")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        ..Default::default()
    };
    options.viewport = options.viewport.with_app_id(APP_ID);
    if let Some(icon) = app_icon() {
        options.viewport = options.viewport.with_icon(icon);
    }
    // The graphics backend, decided before the window exists: a driver that faults takes the
    // process down before any Rust code can catch it (see `gpu`).
    let gpu = gpu::Startup::begin(if no_prefs() { None } else { profile::config_dir().map(|d| d.join(gpu::FILE)) }, env!("CARGO_PKG_VERSION"));
    if let Some(s) = &gpu
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
    {
        create.instance_descriptor.backends = s.backends();
    }
    // winit has no file drag-and-drop on Wayland (only on X11), so dropping images from the file
    // manager showed a "no" cursor. Run through XWayland when it's there; DESIGNCRAFT_WAYLAND=1
    // keeps the native Wayland backend.
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("DESIGNCRAFT_WAYLAND").is_none() {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    eframe::run_native(
        "DesignCraft",
        options,
        Box::new(move |cc| {
            let mut gpu = gpu;
            if let Some(rs) = &cc.wgpu_render_state {
                // What a bug report needs to know about the graphics driver.
                let info = rs.adapter.get_info();
                log::info!("graphics: {} on {:?} ({:?}; driver {} {})", info.name, info.backend, info.device_type, info.driver, info.driver_info);
                if let (Some(s), Some(backend)) = (gpu.as_mut(), gpu::Backend::of(info.backend)) {
                    s.started_with(backend);
                }
            }
            // Crash recovery, desktop services and saved preferences; the graphics note shows
            // unless recovered documents are reported.
            let mut app = designcraft_embed::new_app(gpu.as_ref().and_then(gpu::Startup::status_line));
            app.integrated_titlebar = cfg!(target_os = "macos");
            if let Some(port) = control_port {
                let rx = control_server::start(port, cc.egui_ctx.clone());
                app = app.with_control(rx);
            }
            if sample {
                let _ = app.run("file.newSample", serde_json::json!({}));
            }
            for f in files {
                if let Err(e) = app.open_file("file.open", serde_json::json!({"path": f})) {
                    eprintln!("designcraft: {f}: {e}");
                }
            }
            Ok(Box::new(App {
                app,
                #[cfg(target_os = "macos")]
                menu: None,
                startup: gpu,
                frames: 0,
            }))
        }),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn control_port_values_that_are_not_ports_are_ignored() {
        assert_eq!(super::control_port_from("--control", Some("7979".into())), Some(7979));
        assert_eq!(super::control_port_from("--control", Some(" 7979 ".into())), Some(7979));
        assert_eq!(super::control_port_from("--control", None), None);
        for bad in ["", "abc", "-1", "65536", "99999999999999999999"] {
            assert_eq!(super::control_port_from("--control", Some(bad.into())), None, "{bad:?}");
        }
    }
}
