//! Settings › Image generation: where ComfyUI is, whether Septet starts it, how much graphics
//! memory it leaves to the apps, and when it unloads and ends.

use std::sync::{Arc, Mutex, PoisonError};

use egui::{Context, Id, RichText};

use super::{Comfy, Settings, client::Client, launch};
use crate::theme::{self, ShellColors};

/// The last connection test's answer (from a thread: ComfyUI may take a moment).
type Check = Arc<Mutex<Option<Result<String, String>>>>;

pub fn settings(open: &mut bool, ctx: &Context, comfy: &mut Comfy) {
    let c = ShellColors::home();
    let check: Check = ctx.data_mut(|d| d.get_temp_mut_or_default::<Check>(Id::new("septet-comfy-check")).clone());
    let before = comfy.settings.clone();
    let s = &mut comfy.settings;
    let resp = egui::Modal::new(Id::new("septet-comfy-settings")).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.label(RichText::new("Image generation").font(theme::semibold(18.0)));
        ui.label(
            RichText::new("Generative Fill and Generate Image use ComfyUI with Qwen Image 2.1 on this computer. Septet does not ship them: install ComfyUI and the models yourself.")
                .color(c.text_dim),
        );
        ui.add_space(12.0);
        ui.label(RichText::new("ComfyUI").font(theme::semibold(13.0)));
        ui.horizontal(|ui| {
            ui.label("Address");
            ui.add(egui::TextEdit::singleline(&mut s.address).desired_width(220.0));
            if ui.button("Test").clicked() {
                *check.lock().unwrap_or_else(PoisonError::into_inner) = None;
                let (address, check, ctx) = (s.address.clone(), check.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let answer = Client::new(&address).version().map(|v| format!("ComfyUI {v} answers."));
                    *check.lock().unwrap_or_else(PoisonError::into_inner) = Some(answer);
                    ctx.request_repaint();
                });
            }
        });
        match check.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            Some(Ok(v)) => ui.label(RichText::new(v).color(c.text_dim)),
            Some(Err(e)) => ui.label(RichText::new(e).color(ui.visuals().error_fg_color)),
            None => ui.label(""),
        };
        ui.checkbox(&mut s.autostart, "Start ComfyUI when it is not running");
        ui.add_enabled_ui(s.autostart, |ui| {
            ui.horizontal(|ui| {
                ui.label("main.py");
                let mut path = s.main.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                // Looked up once per dialog: it reads Comfy Desktop's settings.
                let found = ctx.data_mut(|d| {
                    d.get_temp_mut_or_insert_with(Id::new("septet-comfy-found"), || launch::find(None).map(|i| i.main.display().to_string())).clone()
                });
                let hint = found.unwrap_or_else(|| "not found: choose it".into());
                if ui.add(egui::TextEdit::singleline(&mut path).hint_text(hint).desired_width(260.0)).changed() {
                    s.main = (!path.trim().is_empty()).then(|| path.trim().into());
                }
                if ui.button("Browse…").clicked()
                    && let Some(p) = rfd::FileDialog::new().set_title("ComfyUI's main.py").add_filter("main.py", &["py"]).pick_file()
                {
                    s.main = Some(p);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Keep for Septet's apps");
                let mut auto = s.reserve_gb.is_none();
                if ui.checkbox(&mut auto, "as they need").changed() {
                    s.reserve_gb = if auto { None } else { Some(4.0) };
                }
                if let Some(gb) = &mut s.reserve_gb {
                    ui.add(egui::Slider::new(gb, 0.5..=16.0).step_by(0.5).suffix(" GB"));
                }
            });
            ui.label(RichText::new("Graphics memory ComfyUI leaves free when Septet starts it. Less for ComfyUI is slower, never a crash.").size(11.5).color(c.text_dim));
        });
        ui.checkbox(&mut s.unload_ollama, "Unload Ollama's models when the graphics memory is short")
            .on_hover_text("Ollama loads them again on its next request.");
        ui.add_space(10.0);
        ui.label(RichText::new("When idle").font(theme::semibold(13.0)));
        ui.horizontal(|ui| {
            ui.label("Unload the models after");
            ui.add(egui::DragValue::new(&mut s.unload_after_min).range(1..=120).suffix(" min"));
        });
        ui.horizontal(|ui| {
            ui.label("End the ComfyUI Septet started after");
            ui.add(egui::DragValue::new(&mut s.stop_after_min).range(1..=600).suffix(" min"));
        });
        ui.label(RichText::new("A ComfyUI you started yourself is never ended; Septet unloads its models after each image.").size(11.5).color(c.text_dim));
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.button("Defaults").clicked() {
                *s = Settings::default();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("Done").color(c.on_accent)).fill(c.accent)).clicked() {
                    ui.close();
                }
            });
        });
    });
    if comfy.settings != before {
        comfy.apply_settings();
    }
    if resp.should_close() {
        *open = false;
        ctx.data_mut(|d| d.remove::<Option<String>>(Id::new("septet-comfy-found")));
    }
}
