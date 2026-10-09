//! The Claude settings dialog: is Claude Code there and signed in, which model, a connection test.

use egui::{Color32, Context, Id, RichText};

use super::cli::{self, Probe};
use super::extensions::{self, McpServer};
use super::{Assistant, ProbeState};
use crate::theme::{self, ShellColors};

pub fn dialog(a: &mut Assistant, ctx: &Context) {
    let c = ShellColors::home();
    let before = a.settings.clone();
    let mut close = false;
    let resp = egui::Modal::new(Id::new("septet-claude-settings")).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.label(RichText::new("Claude").font(theme::semibold(22.0)));
        ui.label(RichText::new("Create, edit and cut with Claude inside Septet.").color(c.text_dim));
        ui.add_space(12.0);
        ui.checkbox(&mut a.settings.enabled, "Use Claude in Septet");
        ui.add_space(10.0);

        ui.label(RichText::new("Claude Code").font(theme::semibold(13.0)));
        status(a, ui, ctx, &c);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Path");
            let mut path = a.settings.claude_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
            let edit = ui.add(egui::TextEdit::singleline(&mut path).hint_text("found automatically").desired_width(260.0));
            if edit.changed() {
                a.settings.claude_path = (!path.trim().is_empty()).then(|| path.trim().into());
            }
            if ui.button("Browse…").clicked()
                && let Some(p) = rfd::FileDialog::new().set_title("Claude Code executable").pick_file()
            {
                a.settings.claude_path = Some(p);
                a.recheck(ctx);
            }
            if edit.lost_focus() || ui.button("Check again").clicked() {
                a.recheck(ctx);
            }
        });
        ui.add_space(10.0);

        ui.label(RichText::new("Model and effort").font(theme::semibold(13.0)));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("septet-claude-model").selected_text(super::model_name(a.settings.model.as_deref())).show_ui(ui, |ui| {
                ui.selectable_value(&mut a.settings.model, None, "Default");
                for (m, name) in super::MODELS {
                    ui.selectable_value(&mut a.settings.model, Some((*m).to_owned()), *name);
                }
            });
            egui::ComboBox::from_id_salt("septet-claude-effort").selected_text(super::effort_name(a.settings.effort.as_deref())).show_ui(ui, |ui| {
                ui.selectable_value(&mut a.settings.effort, None, "Default");
                for (e, name) in super::EFFORTS {
                    ui.selectable_value(&mut a.settings.effort, Some((*e).to_owned()), *name);
                }
            });
        });
        ui.label(RichText::new("Each model is always its newest version. Also changeable in the chat.").size(11.5).color(c.text_dim));
        ui.add_space(10.0);

        test(a, ui, ctx, &c);
        ui.add_space(8.0);
        egui::CollapsingHeader::new(RichText::new("Skills and extensions").font(theme::semibold(13.0)))
            .id_salt("septet-claude-extensions")
            .show(ui, |ui| extensions(a, ui, &c));
        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Needs Claude Code, signed in with your Claude subscription. Septet starts your own Claude Code; it never \
                 sees or stores your sign-in. Usage counts toward your plan's limits.",
            )
            .size(11.5)
            .color(c.text_dim),
        );
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new("Done").color(Color32::BLACK)).fill(c.accent)).clicked() {
                close = true;
            }
        });
    });
    if a.settings != before {
        a.settings.save();
    }
    if close || resp.should_close() {
        a.dialog = None;
        a.test = None;
    }
}

fn status(a: &mut Assistant, ui: &mut egui::Ui, ctx: &Context, c: &ShellColors) {
    let ok = Color32::from_rgb(0x4c, 0xc3, 0x7a);
    let warn = Color32::from_rgb(0xf0, 0xb4, 0x4c);
    match &a.probe {
        ProbeState::Checking(_) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Looking for Claude Code…");
            });
        }
        ProbeState::Done(Probe::NotInstalled) => {
            ui.label(RichText::new("Claude Code is not installed (or not found).").color(warn));
            ui.horizontal(|ui| {
                ui.hyperlink_to("How to install Claude Code", cli::INSTALL_URL);
                ui.label(RichText::new("· then press Check again").color(c.text_dim));
            });
        }
        ProbeState::Done(Probe::Failed { exe, error }) => {
            ui.label(RichText::new(format!("{} did not answer as expected:", exe.display())).color(warn));
            ui.label(RichText::new(error).size(11.5).color(c.text_dim));
        }
        ProbeState::Done(Probe::Ready { version, auth, .. }) => {
            let (version, auth) = (version.clone(), auth.clone());
            if auth.uses_subscription() {
                let plan = auth.subscription.as_deref().map(|s| format!(" ({})", capitalize(s))).unwrap_or_default();
                ui.label(RichText::new(format!("✔ Signed in with your Claude subscription{plan} · version {version}")).color(ok));
            } else if auth.logged_in {
                ui.label(RichText::new(format!("Signed in with an API account ({}) · version {version}", auth.method)).color(warn));
                ui.label(RichText::new("Usage is billed to that account, not to a Claude subscription.").size(11.5).color(c.text_dim));
            } else {
                ui.label(RichText::new(format!("Not signed in · version {version}")).color(warn));
            }
            if !auth.uses_subscription() {
                ui.horizontal(|ui| {
                    if a.logging_in() {
                        ui.spinner();
                        ui.label("Finish signing in in your browser…");
                        if ui.button("Cancel").clicked() {
                            a.cancel_login();
                        }
                    } else if ui.button("Sign in with Claude…").clicked() {
                        a.login();
                        ctx.request_repaint();
                    }
                });
            }
        }
    }
    if let Some(err) = &a.login_error {
        ui.label(RichText::new(err).size(11.5).color(warn));
        ui.label(RichText::new("You can also run `claude auth login` in a terminal.").size(11.5).color(c.text_dim));
    }
}

fn test(a: &mut Assistant, ui: &mut egui::Ui, ctx: &Context, c: &ShellColors) {
    ui.horizontal(|ui| {
        let busy = a.test.as_ref().is_some_and(|t| t.outcome.is_none());
        if ui.add_enabled(a.ready().is_some() && !busy, egui::Button::new("Test connection")).clicked() {
            a.start_test(ctx);
        }
        let Some(t) = &a.test else { return };
        match &t.outcome {
            None => {
                ui.spinner();
                ui.label(RichText::new(if t.reply.is_empty() { "Waiting for Claude…".to_owned() } else { t.reply.clone() }).color(c.text_dim));
            }
            Some(Ok(text)) => {
                ui.label(RichText::new(format!("✔ Claude answered “{}” in {:.1} s", text.trim(), t.took)).color(Color32::from_rgb(0x4c, 0xc3, 0x7a)));
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(Color32::from_rgb(0xf0, 0x6c, 0x5c)));
            }
        }
    });
}

fn extensions(a: &mut Assistant, ui: &mut egui::Ui, c: &ShellColors) {
    let warn = Color32::from_rgb(0xf0, 0xb4, 0x4c);
    let small = |t: &str| RichText::new(t).size(11.5).color(c.text_dim);
    ui.label(small(
        "Septet brings skills for graphics, logos, type, colour, print, photos, video and motion. You can add your own; changes apply to new conversations.",
    ));
    let found = a.extensions();

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Your skills").strong());
        if ui.small_button("Open folder").clicked() {
            open_folder(extensions::my_skills_dir().map(|d| d.join("skills")));
        }
    });
    let skills = if found.skills.is_empty() { "None yet. Put each skill in its own folder with a SKILL.md.".to_owned() } else { found.skills.join(", ") };
    ui.label(small(&skills));

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Plugins").strong());
        if ui.small_button("Open folder").clicked() {
            open_folder(extensions::plugins_dir());
        }
    });
    if found.plugins.is_empty() {
        ui.label(small("None. Copy a Claude Code plugin folder here."));
    }
    for p in &found.plugins {
        ui.horizontal(|ui| {
            ui.label(&p.name);
            if p.has_hooks {
                ui.label(RichText::new("has hooks: runs commands without asking").size(11.5).color(warn));
            }
        });
    }

    ui.add_space(6.0);
    ui.label(RichText::new("MCP servers").strong());
    ui.label(small("A command (talks over stdin/stdout) or an http(s) URL. Claude asks before using their tools."));
    let mut remove = None;
    for (i, s) in a.settings.mcp_servers.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.enabled, "");
            ui.add(egui::TextEdit::singleline(&mut s.name).hint_text("name").desired_width(90.0));
            ui.add(egui::TextEdit::singleline(&mut s.target).hint_text("npx -y some-mcp-server  or  https://…").desired_width(250.0));
            if ui.small_button("×").on_hover_text("Remove").clicked() {
                remove = Some(i);
            }
        });
        if s.enabled
            && let Err(e) = s.config()
            && !s.target.trim().is_empty()
        {
            ui.label(RichText::new(e).size(11.5).color(warn));
        }
    }
    if let Some(i) = remove {
        a.settings.mcp_servers.remove(i);
    }
    if ui.small_button("Add server").clicked() {
        a.settings.mcp_servers.push(McpServer { enabled: true, ..Default::default() });
    }

    ui.add_space(8.0);
    ui.checkbox(&mut a.settings.own_setup, "Also use my Claude Code settings, skills and plugins");
    if a.settings.own_setup {
        ui.label(
            RichText::new(
                "Your CLAUDE.md, hooks and permission rules then apply in Septet too, e.g. commands you always allow run \
                 without asking here. MCP servers from Claude Code are not loaded; add them above.",
            )
            .size(11.5)
            .color(warn),
        );
    }
}

fn open_folder(dir: Option<std::path::PathBuf>) {
    let _ = extensions::create_folders();
    if let Some(dir) = dir {
        crate::home::open_in_system(&dir);
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
