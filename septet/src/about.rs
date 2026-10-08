//! About Septet: version, licenses, where the apps come from.

use egui::{Color32, Context, Id, RichText, Sense, vec2};

use crate::theme::{self, ShellColors};

pub const GITLAB: &str = "https://gitlab.com/louiswalder6/septet";
pub const GITHUB: &str = "https://github.com/LouBoi161/septet";

/// The third-party license list shipped next to the executable (or in the repository).
fn third_party_licenses() -> String {
    let beside_exe = std::env::current_exe().ok().and_then(|exe| {
        let dir = exe.parent()?.to_path_buf();
        // Next to the binary (zip, installer), or in an AppImage/.app's share/Resources folder.
        [dir.join("THIRD-PARTY-LICENSES.md"), dir.join("../share/septet/THIRD-PARTY-LICENSES.md"), dir.join("../Resources/THIRD-PARTY-LICENSES.md")]
            .into_iter()
            .find(|p| p.is_file())
    });
    match beside_exe {
        Some(path) => path.display().to_string(),
        None => format!("{GITLAB}/-/blob/main/THIRD-PARTY-LICENSES.md"),
    }
}

pub fn dialog(open: &mut bool, ctx: &Context) {
    if !*open {
        return;
    }
    let c = ShellColors::home();
    let resp = egui::Modal::new(Id::new("septet-about")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
            crate::icon::paint_mark(ui, r);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("Septet").font(theme::semibold(22.0)));
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(c.text_dim));
            });
        });
        ui.add_space(12.0);
        ui.label("Seven creative apps in one workspace: Photocraft, Vectorcraft, Lightcraft, Designcraft, Pdfcraft, Filmcraft and Effectcraft.");
        ui.add_space(10.0);
        ui.label(RichText::new("License").font(theme::semibold(13.0)));
        ui.label(
            "Septet's own code is licensed under the PolyForm Noncommercial License 1.0.0: free to use, change and share \
             for any noncommercial purpose.",
        );
        ui.add_space(6.0);
        ui.label(
            "Based on PhotoCraft, VectorCraft, LightCraft, DesignCraft, PdfCraft, FilmCraft and EffectCraft by the \
             ArtCraft team, used under the MIT License or the Apache License 2.0.",
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Septet is an independent project. It is not made, sponsored or endorsed by the ArtCraft Team, and it is \
                 not affiliated with Adobe.",
            )
            .color(c.text_dim),
        );
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to("Source (GitLab)", GITLAB);
            ui.label("·");
            ui.hyperlink_to("Mirror (GitHub)", GITHUB);
            ui.label("·");
            let licenses = third_party_licenses();
            if licenses.starts_with("http") {
                ui.hyperlink_to("Third-party licenses", licenses);
            } else if ui.link("Third-party licenses").clicked() {
                crate::home::open_in_system(std::path::Path::new(&licenses));
            }
        });
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new("Close").color(Color32::BLACK)).fill(c.accent)).clicked() {
                ui.close();
            }
        });
    });
    if resp.should_close() {
        *open = false;
    }
}
