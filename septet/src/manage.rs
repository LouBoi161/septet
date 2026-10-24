//! Home › Apps: install and uninstall the apps, and update Septet (`update.rs`).
//!
//! Every app stays built into Septet (one program, see `docs/suite/PLAN.md`): uninstalling takes it
//! out of Home, the + menu, "Open in…", the file types and Claude's tools (`kinds::set_uninstalled`),
//! and installing brings it back at once, with its settings and recent files.

use egui::{Align2, Color32, Id, RichText, Sense, Stroke, Ui, ViewportId, vec2};

use crate::kinds::AppKind;
use crate::shell::{Action, Shell};
use crate::theme::{self, ShellColors};
use crate::update::{Install, Phase};

/// Saved in Septet's storage.
pub const STORAGE_KEY: &str = "septet/apps";

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Saved {
    pub uninstalled: Vec<AppKind>,
}

pub fn page(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui, c: &ShellColors) {
    ui.label(RichText::new("Apps").font(theme::semibold(28.0)).color(c.text));
    ui.add_space(4.0);
    ui.label(
        RichText::new(
            "Install the apps you use. An uninstalled app leaves Home, the + menu, Open in… and Claude's tools; your files and \
             its settings stay for when you install it again.",
        )
        .size(14.0)
        .color(c.text_dim),
    );
    ui.add_space(20.0);
    updates_card(shell, ui, c);
    ui.add_space(28.0);
    let installed: Vec<AppKind> = AppKind::all().collect();
    let available: Vec<AppKind> = AppKind::built().filter(|k| !k.installed()).collect();
    ui.label(RichText::new(format!("Installed ({})", installed.len())).font(theme::semibold(15.0)).color(c.text));
    ui.add_space(10.0);
    if installed.is_empty() {
        ui.label(RichText::new("No apps installed: install one below.").color(c.text_dim));
        ui.add_space(14.0);
    }
    crate::home::app_grid(shell, viewport, ui, c, &installed, crate::home::Cards::Installed);
    if !available.is_empty() {
        ui.add_space(16.0);
        ui.label(RichText::new(format!("Not installed ({})", available.len())).font(theme::semibold(15.0)).color(c.text));
        ui.add_space(10.0);
        crate::home::app_grid(shell, viewport, ui, c, &available, crate::home::Cards::Available);
    }
}

/// Install `kind` again: at once, everything is still there.
pub fn install(shell: &mut Shell, kind: AppKind) {
    let mut list = crate::kinds::uninstalled();
    list.retain(|k| *k != kind);
    crate::kinds::set_uninstalled(&list);
    shell.bridge.notify(format!("{} is installed", kind.name()));
}

/// "Uninstall <app>?", from the app's card.
pub fn uninstall_dialog(shell: &mut Shell, ctx: &egui::Context) {
    let Some((kind, _)) = shell.home.uninstall else { return };
    let c = ShellColors::home();
    let running = shell.apps.contains_key(&kind) || shell.find_app_tab(kind).is_some();
    let (mut go, mut cancel) = (false, false);
    egui::Modal::new(Id::new("septet-uninstall")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
            egui::Image::new(kind.icon()).fit_to_exact_size(r.size()).paint_at(ui, r);
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Uninstall {}?", kind.name())).font(theme::semibold(16.0)));
        });
        ui.add_space(10.0);
        ui.label(format!(
            "{name} leaves Home, the + menu, Open in… and Claude's tools, and Septet opens its files in other apps. Your files \
             and {name}'s settings stay: install it again under Apps any time.",
            name = kind.name()
        ));
        if running {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("{} is open: Septet closes it first and asks about unsaved changes.", kind.name())).color(c.text_dim));
        }
        ui.add_space(16.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new("Uninstall").color(Color32::WHITE)).fill(c.close_hover)).clicked() {
                go = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if go {
        shell.home.uninstall = None;
        shell.uninstall_app(kind);
    } else if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        shell.home.uninstall = None;
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_048_576.0)
}

fn updates_card(shell: &mut Shell, ui: &mut Ui, c: &ShellColors) {
    let ctx = ui.ctx().clone();
    let phase = shell.updater.phase();
    let frame = egui::Frame::new().fill(c.card).stroke(Stroke::new(1.0, c.card_border)).corner_radius(12.0).inner_margin(18.0);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
            crate::icon::paint_mark(ui, r);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(format!("Septet {}", env!("CARGO_PKG_VERSION"))).font(theme::semibold(16.0)).color(c.text));
                let status = match (&shell.updater.install, &phase) {
                    (Install::Unsupported(why), _) => why.clone(),
                    (_, Phase::Idle) if shell.updater.settings.last_check == 0 => "Updates come from the releases on GitHub.".into(),
                    (_, Phase::Idle | Phase::UpToDate) => format!("Up to date · checked {}", crate::recent::ago(shell.updater.settings.last_check)),
                    (_, Phase::Checking) => "Looking for a new version…".into(),
                    (_, Phase::Available(r)) => format!("Version {} is available.", r.version),
                    (_, Phase::Downloading { release, done, total }) => {
                        if *total > 0 {
                            format!("Downloading version {}: {} of {}", release.version, mb(*done), mb(*total))
                        } else {
                            format!("Downloading version {}…", release.version)
                        }
                    }
                    (Install::WindowsSetup, Phase::Ready(r)) => format!("Version {} installs when you quit or restart Septet.", r.version),
                    (_, Phase::Ready(r)) => format!("Version {} is installed and starts with the next start of Septet.", r.version),
                    (_, Phase::Failed { message, .. }) => message.clone(),
                };
                let failed = matches!(phase, Phase::Failed { .. });
                ui.label(RichText::new(status).size(13.0).color(if failed { c.close_hover } else { c.text_dim }));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let accent = |text: &str| egui::Button::new(RichText::new(text).color(c.on_accent).strong()).fill(c.accent).min_size(vec2(0.0, 30.0));
                match &phase {
                    _ if matches!(shell.updater.install, Install::Unsupported(_)) => {
                        ui.hyperlink_to("Releases", format!("{}/releases", crate::about::GITHUB));
                    }
                    Phase::Checking => {
                        ui.spinner();
                    }
                    Phase::Available(r) | Phase::Failed { release: Some(r), .. } => {
                        let label = if matches!(phase, Phase::Failed { .. }) { "Try Again" } else { "Download and Install" };
                        if ui.add(accent(label)).clicked() {
                            shell.updater.download(&ctx, r.clone());
                        }
                    }
                    Phase::Downloading { done, total, .. } => {
                        let fraction = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                        ui.add(egui::ProgressBar::new(fraction).desired_width(180.0).animate(*total == 0));
                    }
                    Phase::Ready(_) => {
                        if ui.add(accent("Restart Now")).clicked() {
                            shell.updater.restart = true;
                            shell.actions.push(Action::Quit);
                        }
                    }
                    Phase::Idle | Phase::UpToDate | Phase::Failed { release: None, .. } => {
                        if ui.add(egui::Button::new("Check for Updates").min_size(vec2(0.0, 30.0))).clicked() {
                            shell.updater.check(&ctx);
                        }
                    }
                }
            });
        });
        let notes = match &phase {
            Phase::Available(r) | Phase::Downloading { release: r, .. } | Phase::Ready(r) | Phase::Failed { release: Some(r), .. } => Some(r),
            _ => None,
        };
        if let Some(r) = notes.filter(|r| !r.notes.trim().is_empty()) {
            ui.add_space(8.0);
            egui::CollapsingHeader::new(RichText::new(format!("What's new in {}", r.version)).color(c.text)).id_salt("septet-update-notes").show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    let style = crate::assistant::markdown::Style { text: c.text, dim: c.text_dim, code_bg: c.card_hover, size: 13.0 };
                    crate::assistant::markdown::show(ui, &r.notes, &style);
                });
                ui.hyperlink_to("Open the release page", &r.page);
            });
        }
        if !matches!(shell.updater.install, Install::Unsupported(_)) {
            ui.add_space(8.0);
            ui.checkbox(&mut shell.updater.settings.auto_check, "Look for a new version once a day")
                .on_hover_text("Septet asks GitHub for the latest release. Nothing downloads until you click Download and Install.");
        }
    });
}

/// The "Uninstall" link on an installed app's card, or what is happening to it.
pub fn card_footer(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui, kind: AppKind, r: egui::Rect, c: &ShellColors) {
    let at = r.right_bottom() + vec2(-18.0, -16.0);
    if shell.pending_uninstall == Some(kind) {
        let text = format!("Closing {} to uninstall it…", kind.name());
        ui.painter().text(at - vec2(60.0, 0.0), Align2::RIGHT_CENTER, text, theme::medium(11.5), c.text_dim);
        let link = egui::Rect::from_min_size(at - vec2(48.0, 9.0), vec2(48.0, 18.0));
        let resp = ui.interact(link, Id::new(("septet-uninstall-cancel", kind)), Sense::click());
        ui.painter().text(link.right_center(), Align2::RIGHT_CENTER, "Cancel", theme::medium(11.5), if resp.hovered() { c.text } else { c.accent });
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            shell.pending_uninstall = None;
        }
        return;
    }
    let galley = ui.painter().layout_no_wrap("Uninstall".into(), theme::medium(11.5), c.text_dim);
    let link = egui::Rect::from_min_size(at - vec2(galley.size().x, galley.size().y / 2.0), galley.size()).expand(4.0);
    let resp = ui.interact(link, Id::new(("septet-uninstall", kind)), Sense::click());
    let color = if resp.hovered() { c.close_hover } else { c.text_dim };
    ui.painter().text(at, Align2::RIGHT_CENTER, "Uninstall", theme::medium(11.5), color);
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        shell.home.uninstall = Some((kind, viewport));
    }
}
