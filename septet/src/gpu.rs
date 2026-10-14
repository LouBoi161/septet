//! One set of handlers for the GPU device every app shares. wgpu keeps a single uncaptured-error
//! handler and a single device-lost callback per device, and Photocraft, Filmcraft and Effectcraft
//! each set their own, so the app opened last silenced the others. The shell sets them again after
//! every app starts ([`GpuWatch::install`]) and hands each event to every running app on the next
//! frame (`HostedApp::gpu_failed`), which falls back to its CPU path.
//!
//! A lost device can't be replaced under eframe, and egui-wgpu panics on the next mesh it can't
//! upload. So when the device is lost, the shell has every app write its recovery copies, saves the
//! session and starts Septet again ([`restart`]); the apps offer the copies when they start.

use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui_wgpu::wgpu;
use egui::Context;

pub struct GpuEvent {
    /// The device itself was lost (not only an error reported).
    pub lost: bool,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct GpuWatch {
    events: Arc<Mutex<Vec<GpuEvent>>>,
}

impl GpuWatch {
    /// (Re)install the handlers on `device`. wgpu calls them with its own locks held, possibly off
    /// the UI thread: they only record the event and wake the UI.
    pub fn install(&self, device: &wgpu::Device, ctx: &Context) {
        let (events, wake) = (self.events.clone(), ctx.clone());
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let message = e.to_string();
            eprintln!("uncaptured wgpu error: {message}");
            // Errors about invalid or lost objects come before the device-lost callback, which
            // only runs on the next poll.
            let lower = message.to_ascii_lowercase();
            let lost = lower.contains("device lost") || lower.contains("destroyed");
            // Out of memory: Phase H frees ComfyUI's VRAM here and retries (docs/suite/PLAN.md).
            events.lock().unwrap_or_else(PoisonError::into_inner).push(GpuEvent { lost, message: first_line(&message) });
            wake.request_repaint();
        }));
        let (events, wake) = (self.events.clone(), ctx.clone());
        device.set_device_lost_callback(move |reason, message| {
            eprintln!("wgpu device lost ({reason:?}): {message}");
            let message = if message.is_empty() { format!("{reason:?}") } else { first_line(&message) };
            events.lock().unwrap_or_else(PoisonError::into_inner).push(GpuEvent { lost: true, message });
            wake.request_repaint();
        });
    }

    /// The events since the last call.
    pub fn take(&self) -> Vec<GpuEvent> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(text).to_string()
}

/// Set in the Septet started by [`restart`]: when it started (Unix seconds).
pub const RESTART_ENV: &str = "SEPTET_RESTARTED_AFTER_GPU_LOSS";

/// Start Septet again, as it was started (the session holds the tabs), unless this Septet is
/// itself such a restart from less than a minute ago: a device that keeps getting lost would
/// otherwise restart forever. Whether a new Septet was started.
pub fn restart() -> bool {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let recent = std::env::var(RESTART_ENV).ok().and_then(|v| v.parse::<u64>().ok()).is_some_and(|t| now.saturating_sub(t) < 60);
    if recent {
        eprintln!("the graphics device was lost again right after a restart: not restarting");
        return false;
    }
    if std::env::var_os("SEPTET_AUTOTEST").is_some() {
        eprintln!("autotest: Septet would start again now");
        return true;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    // The files given on the command line are open already and come back with the session.
    let args = std::env::args().skip(1).filter(|a| a.starts_with('-'));
    match std::process::Command::new(exe).args(args).env(RESTART_ENV, now.to_string()).spawn() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("couldn't start Septet again after the graphics device was lost: {e}");
            false
        }
    }
}
