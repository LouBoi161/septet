//! The Home tab: launch apps, reopen recent files, drop files to open them in the right app.

use egui::{Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Rect, Sense, Stroke, StrokeKind, Ui, ViewportId, pos2, vec2};

use crate::kinds::AppKind;
use crate::recent;
use crate::shell::{Action, OpenRequest, Shell, TabKind};
use crate::theme::{self, ShellColors};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    #[default]
    Home,
    Apps,
    Files,
}

#[derive(Default)]
pub struct HomeState {
    pub page: Page,
    pub filter: String,
}

pub fn show(shell: &mut Shell, wi: usize, ui: &mut Ui) {
    let c = ShellColors::home();
    let viewport = shell.windows[wi].viewport;
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, c.home_bg);
    let sidebar = Rect::from_min_size(rect.min, vec2(228.0, rect.height()));
    ui.painter().rect_filled(sidebar, 0.0, theme::lighten(c.home_bg, 0.025));
    ui.painter().line_segment([sidebar.right_top(), sidebar.right_bottom()], Stroke::new(1.0, c.card_border));

    let mut side = ui.new_child(egui::UiBuilder::new().max_rect(sidebar.shrink2(vec2(16.0, 20.0))));
    sidebar_ui(shell, viewport, &mut side, &c);

    let main = Rect::from_min_max(pos2(sidebar.right(), rect.top()), rect.max);
    let mut main_ui = ui.new_child(egui::UiBuilder::new().max_rect(main));
    egui::ScrollArea::vertical().id_salt(("septet-home-scroll", viewport)).auto_shrink(false).show(&mut main_ui, |ui| {
        let width = (ui.available_width() - 80.0).min(1120.0);
        let margin = ((ui.available_width() - width) / 2.0).max(40.0);
        ui.add_space(36.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                match shell.home.page {
                    Page::Home => home_page(shell, viewport, ui, &c),
                    Page::Apps => apps_page(shell, viewport, ui, &c),
                    Page::Files => files_page(shell, viewport, ui, &c, usize::MAX),
                }
                ui.add_space(40.0);
            });
        });
    });

    // Files dragged over Home: say what will happen.
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    if hovering {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new(("septet-home-drop", viewport))));
        let r = main.shrink(24.0);
        painter.rect_filled(r, 16.0, Color32::from_black_alpha(140));
        painter.rect_stroke(r, 16.0, Stroke::new(2.0, c.accent), StrokeKind::Inside);
        painter.text(r.center(), Align2::CENTER_CENTER, "Drop to open in the right app", theme::semibold(22.0), Color32::WHITE);
    }
}

fn sidebar_ui(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui, c: &ShellColors) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
        crate::icon::paint_mark(ui, r);
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Septet").font(theme::semibold(18.0)).color(c.text));
    });
    ui.add_space(26.0);
    for (page, label, glyph) in [(Page::Home, "Home", "⌂"), (Page::Apps, "Apps", "▦"), (Page::Files, "Files", "🗋")] {
        let selected = shell.home.page == page;
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
        if selected {
            ui.painter().rect_filled(r, 8.0, c.card_hover);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 8.0, c.card);
        }
        ui.painter().text(
            r.left_center() + vec2(12.0, 0.0),
            Align2::LEFT_CENTER,
            glyph,
            FontId::proportional(14.0),
            if selected { c.text } else { c.text_dim },
        );
        ui.painter().text(r.left_center() + vec2(36.0, 0.0), Align2::LEFT_CENTER, label, theme::medium(13.5), if selected { c.text } else { c.text_dim });
        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            shell.home.page = page;
        }
        ui.add_space(2.0);
    }
    ui.add_space(22.0);
    ui.label(egui::RichText::new("RUNNING").font(theme::semibold(10.5)).color(c.text_dim));
    ui.add_space(6.0);
    let running: Vec<AppKind> = AppKind::ALL.into_iter().filter(|k| shell.apps.contains_key(k)).collect();
    if running.is_empty() {
        ui.label(egui::RichText::new("No apps open").font(FontId::proportional(12.0)).color(c.text_dim));
    }
    for kind in running {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 7.0, c.card);
        }
        let icon = Rect::from_center_size(r.left_center() + vec2(18.0, 0.0), vec2(18.0, 18.0));
        egui::Image::new(kind.icon()).fit_to_exact_size(icon.size()).paint_at(ui, icon);
        ui.painter().text(r.left_center() + vec2(36.0, 0.0), Align2::LEFT_CENTER, kind.name(), theme::medium(13.0), c.text);
        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            shell.actions.push(Action::NewTab { window: viewport, kind: TabKind::App(kind) });
        }
    }
    // Claude, About and Open file, pinned to the bottom.
    let bottom = ui.max_rect().bottom();
    let claude = egui::Rect::from_min_max(pos2(ui.max_rect().left(), bottom - 96.0), pos2(ui.max_rect().right(), bottom - 74.0));
    let resp = ui.interact(claude, Id::new(("septet-home-claude", viewport)), Sense::click());
    ui.painter().text(
        claude.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        "Claude",
        theme::medium(12.5),
        if resp.hovered() { c.text } else { c.text_dim },
    );
    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        shell.assistant.toggle_panel(ui.ctx(), viewport);
    }
    let about = egui::Rect::from_min_max(pos2(ui.max_rect().left(), bottom - 70.0), pos2(ui.max_rect().right(), bottom - 48.0));
    let resp = ui.interact(about, Id::new(("septet-home-about", viewport)), Sense::click());
    ui.painter().text(
        about.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        "About Septet",
        theme::medium(12.5),
        if resp.hovered() { c.text } else { c.text_dim },
    );
    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        shell.about = Some(viewport);
    }
    let r = Rect::from_min_max(pos2(ui.max_rect().left(), bottom - 38.0), pos2(ui.max_rect().right(), bottom));
    let resp = ui.interact(r, Id::new(("septet-home-open", viewport)), Sense::click());
    ui.painter().rect_filled(r, 9.0, if resp.hovered() { theme::lighten(c.accent, 0.1) } else { c.accent });
    ui.painter().text(r.center(), Align2::CENTER_CENTER, "Open File…", theme::semibold(13.5), Color32::from_rgb(0x14, 0x14, 0x17));
    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        pick_and_open(shell, Some(viewport), None);
    }
}

fn heading(ui: &mut Ui, text: &str, c: &ShellColors) {
    ui.label(egui::RichText::new(text).font(theme::semibold(15.0)).color(c.text));
    ui.add_space(10.0);
}

fn home_page(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui, c: &ShellColors) {
    ui.label(egui::RichText::new("Welcome to Septet").font(theme::semibold(28.0)).color(c.text));
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Seven creative apps in one place. Drag tabs into their own windows, copy and drop between apps.")
            .font(FontId::proportional(14.0))
            .color(c.text_dim),
    );
    ui.add_space(28.0);
    heading(ui, "Your apps", c);
    app_grid(shell, viewport, ui, c, false);
    ui.add_space(30.0);
    heading(ui, "Recent", c);
    if shell.recent.files.is_empty() {
        drop_hint(ui, c);
    } else {
        files_page(shell, viewport, ui, c, 8);
    }
}

fn apps_page(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui, c: &ShellColors) {
    ui.label(egui::RichText::new("Apps").font(theme::semibold(28.0)).color(c.text));
    ui.add_space(20.0);
    app_grid(shell, viewport, ui, c, true);
}

fn app_grid(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui, c: &ShellColors, detailed: bool) {
    let gap = 14.0;
    let card_w = if detailed { 340.0 } else { 252.0 };
    let card_h = if detailed { 150.0 } else { 128.0 };
    let width = ui.available_width();
    let cols = (((width + gap) / (card_w + gap)).floor() as usize).max(1);
    let card_w = (width - gap * (cols as f32 - 1.0)) / cols as f32;
    for row in AppKind::ALL.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &kind in row {
                let (r, resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click());
                let hover = ui.ctx().animate_bool(Id::new(("septet-card", kind, detailed)), resp.hovered());
                let lift = hover * 2.0;
                let r = r.translate(vec2(0.0, -lift));
                let p = ui.painter();
                if hover > 0.0 {
                    p.add(
                        egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha((70.0 * hover) as u8) }.as_shape(r, 12.0),
                    );
                }
                p.rect_filled(r, 12.0, theme::mix(c.card, c.card_hover, hover));
                p.rect_stroke(r, 12.0, Stroke::new(1.0, theme::mix(c.card_border, kind.color(), hover * 0.8)), StrokeKind::Inside);
                let icon = Rect::from_min_size(r.min + vec2(18.0, 18.0), vec2(40.0, 40.0));
                egui::Image::new(kind.icon()).fit_to_exact_size(icon.size()).paint_at(ui, icon);
                let p = ui.painter();
                p.text(pos2(icon.right() + 14.0, icon.center().y - 8.0), Align2::LEFT_CENTER, kind.name(), theme::semibold(16.0), c.text);
                p.text(pos2(icon.right() + 14.0, icon.center().y + 11.0), Align2::LEFT_CENTER, kind.badge(), theme::medium(11.0), kind.color());
                let galley = p.layout(kind.tagline().to_string(), FontId::proportional(12.5), c.text_dim, r.width() - 36.0);
                p.galley(pos2(r.left() + 18.0, icon.bottom() + 14.0), galley, c.text_dim);
                if detailed {
                    let exts = kind.extensions().iter().take(10).map(|e| format!(".{e}")).collect::<Vec<_>>().join("  ");
                    let galley = p.layout(exts, FontId::monospace(10.5), theme::mix(c.text_dim, c.home_bg, 0.3), r.width() - 36.0);
                    p.galley(pos2(r.left() + 18.0, r.bottom() - 18.0 - galley.size().y), galley, c.text_dim);
                }
                let running = shell.apps.contains_key(&kind);
                let pill = Rect::from_min_size(pos2(r.right() - 74.0, r.top() + 22.0), vec2(56.0, 24.0));
                let (fill, text, label) = if running {
                    (Color32::TRANSPARENT, kind.color(), "● Open")
                } else {
                    (theme::mix(c.card_hover, kind.color(), hover * 0.9), if hover > 0.5 { Color32::WHITE } else { c.text }, "Open")
                };
                p.rect_filled(pill, CornerRadius::same(12), fill);
                p.text(pill.center(), Align2::CENTER_CENTER, label, theme::semibold(11.5), text);
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    shell.actions.push(Action::NewTab { window: viewport, kind: TabKind::App(kind) });
                }
            }
        });
        ui.add_space(gap);
    }
}

fn files_page(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui, c: &ShellColors, limit: usize) {
    if limit == usize::MAX {
        ui.label(egui::RichText::new("Files").font(theme::semibold(28.0)).color(c.text));
        ui.add_space(14.0);
        ui.add(egui::TextEdit::singleline(&mut shell.home.filter).hint_text("Filter recent files…").desired_width(320.0));
        ui.add_space(14.0);
        if shell.recent.files.is_empty() {
            drop_hint(ui, c);
            return;
        }
    }
    let filter = shell.home.filter.to_lowercase();
    let files: Vec<recent::RecentFile> = shell
        .recent
        .files
        .iter()
        .filter(|f| limit != usize::MAX || filter.is_empty() || f.path.to_string_lossy().to_lowercase().contains(&filter))
        .take(limit)
        .cloned()
        .collect();
    let frame = egui::Frame::new().fill(c.card).stroke(Stroke::new(1.0, c.card_border)).corner_radius(12.0).inner_margin(6.0);
    frame.show(ui, |ui| {
        for (i, f) in files.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(r, 8.0, c.card_hover);
            }
            let icon = Rect::from_center_size(r.left_center() + vec2(22.0, 0.0), vec2(24.0, 24.0));
            egui::Image::new(f.app.icon()).fit_to_exact_size(icon.size()).paint_at(ui, icon);
            let p = ui.painter();
            let name = f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dir = f.path.parent().map(|d| d.display().to_string()).unwrap_or_default();
            let exists = f.path.exists();
            p.text(pos2(icon.right() + 12.0, r.center().y - 8.0), Align2::LEFT_CENTER, &name, theme::medium(13.0), if exists { c.text } else { c.text_dim });
            let dir_galley = p.layout_no_wrap(if exists { dir } else { format!("{dir} — missing") }, FontId::proportional(11.0), c.text_dim);
            p.with_clip_rect(Rect::from_min_max(r.min, pos2(r.right() - 220.0, r.bottom()))).galley(
                pos2(icon.right() + 12.0, r.center().y + 2.0),
                dir_galley,
                c.text_dim,
            );
            p.text(pos2(r.right() - 14.0, r.center().y), Align2::RIGHT_CENTER, recent::ago(f.opened), FontId::proportional(11.5), c.text_dim);
            p.text(pos2(r.right() - 110.0, r.center().y), Align2::RIGHT_CENTER, f.app.name(), theme::medium(11.5), f.app.color());
            if i + 1 < files.len() {
                p.line_segment([pos2(r.left() + 48.0, r.bottom()), pos2(r.right() - 8.0, r.bottom())], Stroke::new(1.0, c.card_border));
            }
            let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(f.path.display().to_string());
            if resp.clicked() && exists {
                shell.opens.push(OpenRequest { paths: vec![f.path.clone()], app: Some(f.app), window: Some(viewport), place: None });
            }
            resp.context_menu(|ui| {
                for kind in AppKind::all_for_path(&f.path) {
                    if ui.button(format!("Open in {}", kind.name())).clicked() {
                        shell.opens.push(OpenRequest { paths: vec![f.path.clone()], app: Some(kind), window: Some(viewport), place: None });
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Show in Folder").clicked() {
                    if let Some(dir) = f.path.parent() {
                        open_in_system(dir);
                    }
                    ui.close();
                }
                if ui.button("Remove from Recent").clicked() {
                    shell.recent.remove(&f.path);
                    ui.close();
                }
            });
        }
    });
}

fn drop_hint(ui: &mut Ui, c: &ShellColors) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
    let p = ui.painter();
    let dash = egui::Shape::dashed_line(
        &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()].map(|p| p + vec2(0.5, 0.5)),
        Stroke::new(1.2, c.card_border),
        8.0,
        6.0,
    );
    p.extend(dash);
    p.text(r.center() - vec2(0.0, 10.0), Align2::CENTER_CENTER, "Drop files here", theme::semibold(15.0), c.text);
    p.text(r.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, "Septet opens each one in the app made for it", FontId::proportional(12.5), c.text_dim);
}

/// Hand a folder or file to the desktop (file manager, default app).
pub fn open_in_system(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let cmd = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let cmd = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let cmd = std::process::Command::new("xdg-open").arg(path).spawn();
    let _ = cmd;
}

/// File › Open: every file type the apps know, or just `app`'s.
pub fn pick_and_open(shell: &mut Shell, window: Option<ViewportId>, app: Option<AppKind>) {
    let mut dialog = rfd::FileDialog::new().set_title("Open in Septet");
    let all: Vec<&str> = AppKind::ALL.iter().flat_map(|k| k.extensions().iter().copied()).collect();
    match app {
        Some(kind) => {
            dialog = dialog.add_filter(kind.name(), kind.extensions());
        }
        None => {
            dialog = dialog.add_filter("All supported files", &all);
            for kind in AppKind::ALL {
                dialog = dialog.add_filter(kind.name(), kind.extensions());
            }
        }
    }
    dialog = dialog.add_filter("All files", &["*"]);
    if let Some(paths) = dialog.pick_files() {
        shell.opens.push(OpenRequest { paths, app, window, place: None });
    }
}

/// "Close <app>?" for apps without an unsaved-changes prompt of their own.
pub fn confirm_close_dialog(shell: &mut Shell, ctx: &egui::Context) {
    let Some(confirm) = shell.confirm.as_ref() else { return };
    let kind = confirm.kind;
    let doc = shell.apps.get(&kind).and_then(|s| s.try_borrow().ok().and_then(|s| s.tab_title())).unwrap_or_else(|| kind.name().to_string());
    let c = ShellColors::home();
    let mut close = false;
    let mut cancel = false;
    egui::Modal::new(Id::new("septet-confirm-close")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
            egui::Image::new(kind.icon()).fit_to_exact_size(r.size()).paint_at(ui, r);
            ui.add_space(8.0);
            ui.label(egui::RichText::new(format!("Close {}?", kind.name())).font(theme::semibold(16.0)));
        });
        ui.add_space(10.0);
        ui.label(format!(
            "“{doc}” has unsaved changes. {} keeps a recovery copy and offers it the next time it starts, but you may lose recent edits.",
            kind.name()
        ));
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let danger = egui::Button::new(egui::RichText::new("Close Anyway").color(Color32::WHITE)).fill(c.close_hover);
                if ui.add(danger).clicked() {
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
    });
    if close {
        shell.confirm = None;
        shell.force_close_app(kind);
    } else if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        shell.confirm = None;
        for w in &mut shell.windows {
            w.closing = false;
        }
    }
}
