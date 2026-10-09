//! Septet: Photocraft, Vectorcraft, Lightcraft, Designcraft, Pdfcraft, Filmcraft and Effectcraft
//! in one window, as tabs you can drag into windows of their own.
//!
//! Usage: `septet [files…]` — each file opens in the app made for it.
//!
//! On Linux Septet runs through XWayland when it can: winit has no file drag-and-drop on Wayland,
//! and only X11 lets a torn-off tab open where you drop it. `SEPTET_WAYLAND=1` keeps Wayland.
//!
//! `--portable` (or a `portable.txt` next to the executable) keeps all data beside it.

// Release builds on Windows are GUI apps: no console window when started from Explorer.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod about;
mod assistant;
mod autotest;
mod bridge;
mod content;
mod fonts;
mod home;
mod hosted;
mod icon;
mod instance;
mod kinds;
mod pointer;
mod recent;
mod router;
mod shell;
mod tabstrip;
mod theme;

use std::path::PathBuf;

/// Matches the `.desktop` file, so docks pick up the icon.
pub const APP_ID: &str = "septet";

struct Septet {
    shell: shell::Shell,
}

impl eframe::App for Septet {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.shell.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.shell.ui(ui, frame);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.shell.persist(storage);
    }

    fn on_exit(&mut self) {
        self.shell.exit();
    }
}

/// One device for every app: the adapter's texture and buffer sizes (big documents stay on the
/// GPU in Photocraft; Effectcraft wants 16k textures), egui's defaults for everything else.
fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuSetup, wgpu};
    let mut config = eframe::egui_wgpu::WgpuConfiguration::default();
    // Testing on a hidden or throttled display: don't wait for vblank.
    if std::env::var_os("SEPTET_NO_VSYNC").is_some() {
        config.surface.present_mode = wgpu::PresentMode::AutoNoVsync;
    }
    if let WgpuSetup::CreateNew(create) = &mut config.wgpu_setup {
        create.power_preference = wgpu::PowerPreference::HighPerformance;
        create.device_descriptor = std::sync::Arc::new(|adapter| {
            let base = if adapter.get_info().backend == wgpu::Backend::Gl { wgpu::Limits::downlevel_webgl2_defaults() } else { wgpu::Limits::default() };
            let a = adapter.limits();
            let limits = wgpu::Limits {
                max_texture_dimension_2d: a.max_texture_dimension_2d.min(16384),
                max_buffer_size: a.max_buffer_size,
                max_storage_buffer_binding_size: a.max_storage_buffer_binding_size,
                max_storage_buffers_per_shader_stage: a.max_storage_buffers_per_shader_stage.max(base.max_storage_buffers_per_shader_stage),
                ..base
            };
            wgpu::DeviceDescriptor { label: Some("septet wgpu device"), required_limits: limits, ..Default::default() }
        });
    }
    config
}

/// Septet draws its own title bar (the tab strip). On macOS the system's traffic lights stay, over
/// the strip; elsewhere the window has no OS decorations and the strip has its own caption buttons.
pub fn window_chrome(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if cfg!(target_os = "macos") {
        builder.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false)
    } else {
        builder.with_decorations(false)
    }
}

fn main() -> eframe::Result {
    let mut files = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--version" => {
                println!("septet {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--portable" => {}
            _ if arg.starts_with("-psn_") => {}
            _ => files.push(PathBuf::from(arg)),
        }
    }

    // Septet is already running: it opens the files (or comes to the front) instead.
    let Some(primary) = instance::claim(&files) else { return Ok(()) };

    let mut options = eframe::NativeOptions {
        viewport: window_chrome(
            egui::ViewportBuilder::default()
                .with_title("Septet")
                .with_app_id(APP_ID)
                .with_icon(icon::window_icon())
                .with_inner_size([1600.0, 1000.0])
                .with_min_inner_size([900.0, 560.0])
                .with_drag_and_drop(true),
        ),
        centered: true,
        persistence_path: recent::config_dir().map(|d| d.join("shell.ron")),
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("SEPTET_WAYLAND").is_none() {
        // The apps (and their clipboard and tablet code) look at these to tell Wayland from X11.
        // SAFETY: still single-threaded here; nothing reads the environment concurrently.
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::remove_var("WAYLAND_SOCKET");
        }
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }

    eframe::run_native(
        "Septet",
        options,
        Box::new(move |cc| {
            let ctx = &cc.egui_ctx;
            egui_extras::install_image_loaders(ctx);
            ctx.set_fonts(fonts::merged());
            ctx.options_mut(|o| {
                // Ctrl+plus/minus zoom the apps' canvases, not the whole UI.
                o.zoom_with_keyboard = false;
                o.theme_preference = egui::ThemePreference::Dark;
            });
            let router = router::Router::default();
            ctx.add_plugin(router.clone());
            let session = cc.storage.and_then(|s| eframe::get_value::<recent::Session>(s, "septet/session"));
            let mut shell = shell::Shell::new(ctx, router, files, session);
            shell.instance = primary.map(|p| p.serve(ctx.clone()));
            Ok(Box::new(Septet { shell }))
        }),
    )
}
