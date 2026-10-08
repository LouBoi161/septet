//! The desktop session and the host services the egui frontend uses: native file dialogs, audio
//! output and the shared GPU device's failure handlers.

use std::sync::Arc;

use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use effectcraft_ui_egui::gpu_failure::GpuFailureBridge;
use serde_json::json;

use crate::audio_out;

/// The desktop session: footage checked and auto-saves written in the background, effect
/// plug-ins from `<config>/Plug-ins`, and the settings from the platform config directory.
pub fn session() -> Session {
    let mut session = effectcraft_host::session();
    // Lazy open and non-blocking auto-save: footage is checked and auto-saves are
    // written on background threads (M13.14).
    session.check_footage_on_open = true;
    session.autosave.background = true;
    // Settings, shortcut presets and the crash-recovery sentinel live in the platform
    // config directory.
    if let Some(dir) = effectcraft_host::config_dir() {
        // WebAssembly effect plug-ins in <config>/Plug-ins load before the menus are built.
        let plugins = dir.join("Plug-ins");
        if plugins.is_dir() {
            let _ = session.execute("effect.plugins.load", json!({"folder": plugins.to_string_lossy()}));
        }
        session.config = Some(Arc::new(effectcraft_engine::config::DirConfig::new(dir)));
    }
    session.load_settings();
    session
}

/// Report the device's uncaptured errors and its loss to `bridge` (the device's owner installs
/// these once, before EffectCraft's compositor pipelines are built).
pub fn install_device_handlers(rs: &eframe::egui_wgpu::RenderState, bridge: &GpuFailureBridge) {
    let errors = bridge.clone();
    rs.device.on_uncaptured_error(Arc::new(move |error| {
        let message = format!("uncaptured GPU error: {error}");
        if errors.report(&message, false) {
            log::error!("{message}");
        }
    }));
    let lost = bridge.clone();
    rs.device.set_device_lost_callback(move |reason, message| {
        lost.report(&format!("GPU device lost ({reason:?}): {message}"), true);
    });
}

/// The native file dialogs (rfd) and the audio output (cpal).
pub fn install_hooks(app: &mut EffectcraftApp) {
    app.hooks.pick_files = Some(Box::new(|exts: &[&str]| {
        rfd::FileDialog::new().add_filter("Media", exts).pick_files().unwrap_or_default().into_iter().map(|p| p.to_string_lossy().to_string()).collect()
    }));
    app.hooks.pick_save = Some(Box::new(|name: &str| {
        rfd::FileDialog::new().add_filter("EffectCraft Project", &["ecproj"]).set_file_name(name).save_file().map(|p| p.to_string_lossy().to_string())
    }));
    app.hooks.pick_open_project = Some(Box::new(|| {
        rfd::FileDialog::new().add_filter("EffectCraft Project", &["ecproj", "ecprojx"]).pick_file().map(|p| p.to_string_lossy().to_string())
    }));
    app.hooks.audio_device = Some(Box::new(audio_out::open));
    app.hooks.audio_devices = Some(Box::new(audio_out::devices));
    app.hooks.pick_folder = Some(Box::new(|| rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().to_string())));
    app.hooks.pick_save_file = Some(Box::new(|name: &str, ext: &str| {
        // A default with a folder that exists opens the dialog there.
        let path = std::path::Path::new(name);
        let mut d = rfd::FileDialog::new().add_filter(ext, &[ext]);
        if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        d.set_file_name(file).save_file().map(|p| p.to_string_lossy().to_string())
    }));
}
