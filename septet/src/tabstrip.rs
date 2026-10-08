//! The tab strip at the top of every window, which is also the window's title bar: app tabs, the
//! new-tab menu, window dragging and the caption buttons (Septet draws its own window chrome).

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Rect, ResizeDirection, Sense, Shape, Stroke, StrokeKind, Ui, ViewportCommand, ViewportId, pos2, vec2,
};

use crate::kinds::AppKind;
use crate::shell::{Action, Shell, TabDrag, TabKind};
use crate::theme::{self, ShellColors};

pub const STRIP_H: f32 = 38.0;
const TAB_H: f32 = 31.0;
const TAB_TOP: f32 = 7.0;
const LOGO_W: f32 = 42.0;
const CAPTION_W: f32 = 46.0;
const PLUS_W: f32 = 34.0;
const TAB_MIN: f32 = 92.0;
const TAB_MAX: f32 = 232.0;

/// Colours for a window, from the app its active tab shows.
pub fn colors_for(shell: &Shell, wi: usize) -> ShellColors {
    match shell.windows[wi].active_tab().map(|t| t.kind) {
        Some(TabKind::App(kind)) => match shell.apps.get(&kind).and_then(|s| s.try_borrow().ok()) {
            Some(slot) if slot.frames > 2 => ShellColors::for_app(slot.iso.visuals(), kind.color()),
            _ => ShellColors::home(),
        },
        _ => ShellColors::home(),
    }
}

struct TabView {
    title: String,
    tooltip: String,
    dirty: bool,
    icon: Option<AppKind>,
}

fn tab_view(shell: &Shell, kind: TabKind) -> TabView {
    match kind {
        TabKind::Home => TabView { title: "Home".into(), tooltip: "Septet Home".into(), dirty: false, icon: None },
        TabKind::App(app) => {
            let (doc, dirty) =
                shell.apps.get(&app).and_then(|s| s.try_borrow().ok()).map(|s| (s.tab_title(), s.app.has_unsaved_changes())).unwrap_or((None, false));
            let tooltip = match &doc {
                Some(doc) => format!("{} — {doc}", app.name()),
                None => app.name().to_string(),
            };
            TabView { title: doc.unwrap_or_else(|| app.name().to_string()), tooltip, dirty, icon: Some(app) }
        }
    }
}

/// The strip across the top of `window` (the window's content rect). It lives on a layer above
/// everything the apps draw, so an app's modal dialog never blocks switching tabs, moving or
/// closing the window.
pub fn strip(shell: &mut Shell, wi: usize, ctx: &egui::Context, window: Rect) {
    let viewport = shell.windows[wi].viewport;
    let strip = Rect::from_min_size(window.min, vec2(window.width(), STRIP_H));
    shell.windows[wi].strip = strip;
    egui::Area::new(Id::new(("septet-strip", viewport))).order(egui::Order::Tooltip).fixed_pos(strip.min).constrain(false).show(ctx, |ui| {
        ui.set_clip_rect(strip);
        ui.allocate_rect(strip, Sense::hover());
        strip_ui(shell, wi, ui, strip);
    });
}

fn strip_ui(shell: &mut Shell, wi: usize, ui: &mut Ui, strip: Rect) {
    let ctx = ui.ctx().clone();
    let viewport = shell.windows[wi].viewport;
    let colors = colors_for(shell, wi);
    let painter = ui.painter().clone();
    painter.rect_filled(strip, 0.0, colors.strip);

    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
    // macOS keeps its own traffic lights at the left of the strip.
    let mac = cfg!(target_os = "macos");
    let captions = if fullscreen || mac { 0.0 } else { CAPTION_W * 3.0 };
    let lights = if mac && !fullscreen { 74.0 } else { 0.0 };
    let strip_left = strip.left() + lights;
    let tabs_left = strip_left + LOGO_W;
    let tabs_right = strip.right() - captions - PLUS_W - 48.0;
    let n = shell.windows[wi].tabs.len().max(1);
    let tab_w = ((tabs_right - tabs_left) / n as f32).clamp(TAB_MIN, TAB_MAX);

    logo_button(shell, wi, ui, Rect::from_min_size(pos2(strip_left, strip.top()), vec2(LOGO_W, STRIP_H)), &colors);

    // A tab dragged inside its own strip takes the slot under the pointer; the others slide aside.
    let pointer = ctx.input(|i| i.pointer.latest_pos());
    if let (Some(drag), Some(p)) = (shell.drag.as_mut(), pointer)
        && drag.from == viewport
    {
        drag.local = p;
        let near = p.y > strip.top() - 24.0 && p.y < strip.bottom() + 36.0 && p.x > strip.left() && p.x < strip.right();
        if drag.detached && near {
            drag.detached = false;
        } else if !drag.detached && !near {
            drag.detached = true;
        }
        if !drag.detached
            && let Some(cur) = shell.windows[wi].tabs.iter().position(|t| t.id == drag.tab)
        {
            let center = p.x - drag.grab.x + tab_w / 2.0;
            let want = (((center - tabs_left) / tab_w).floor().max(0.0) as usize).min(shell.windows[wi].tabs.len() - 1);
            if want != cur {
                let w = &mut shell.windows[wi];
                let t = w.tabs.remove(cur);
                w.tabs.insert(want, t);
                w.active = want;
            }
        }
    }

    let tabs: Vec<_> = shell.windows[wi].tabs.clone();
    let active = shell.windows[wi].active;
    let mut rects = Vec::with_capacity(tabs.len());
    let mut x_end = tabs_left;
    let mut dragged_tab = None;
    for (i, tab) in tabs.iter().enumerate() {
        let slot_x = tabs_left + i as f32 * tab_w;
        let being_dragged = shell.drag.as_ref().is_some_and(|d| d.tab == tab.id && d.from == viewport);
        let x = if being_dragged && !shell.drag.as_ref().is_some_and(|d| d.detached) {
            let d = shell.drag.as_ref().map(|d| d.local.x - d.grab.x).unwrap_or(slot_x);
            let x = d.clamp(tabs_left, (tabs_right - tab_w).max(tabs_left));
            // Keep the animation in step so the tab doesn't jump back when released.
            ctx.animate_value_with_time(Id::new(("septet-tab-x", tab.id)), x, 0.0);
            x
        } else {
            ctx.animate_value_with_time(Id::new(("septet-tab-x", tab.id)), slot_x, 0.14)
        };
        let rect = Rect::from_min_size(pos2(x, strip.top() + TAB_TOP), vec2(tab_w, TAB_H));
        rects.push((tab.id, Rect::from_min_size(pos2(slot_x, rect.top()), rect.size())));
        x_end = x_end.max(slot_x + tab_w);
        if being_dragged {
            dragged_tab = Some((i, *tab, rect));
            continue;
        }
        draw_tab(shell, wi, ui, i, *tab, rect, i == active, &colors);
    }
    // The dragged tab on top of the others (hidden while it's out of the strip).
    if let Some((i, tab, rect)) = dragged_tab
        && !shell.drag.as_ref().is_some_and(|d| d.detached)
    {
        draw_tab(shell, wi, ui, i, tab, rect, i == shell.windows[wi].active, &colors);
    }
    shell.windows[wi].tab_rects = rects;

    // New tab.
    let plus = Rect::from_min_size(pos2(x_end + 6.0, strip.top() + TAB_TOP + 2.0), vec2(PLUS_W - 6.0, TAB_H - 4.0));
    let plus_resp = ui.interact(plus, Id::new(("septet-plus", viewport)), Sense::click()).on_hover_text("New tab (Ctrl+Shift+T)");
    if plus_resp.hovered() {
        painter.rect_filled(plus, 6.0, colors.tab_hover);
    }
    let c = plus.center();
    let s = Stroke::new(1.6, colors.text_dim);
    painter.line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], s);
    painter.line_segment([c - vec2(0.0, 6.0), c + vec2(0.0, 6.0)], s);
    egui::Popup::menu(&plus_resp).show(|ui| new_tab_menu(shell, viewport, ui));

    // The rest of the strip is the title bar: drag to move, double-click to maximize.
    let bar = Rect::from_min_max(pos2(plus.right() + 4.0, strip.top()), pos2(strip.right() - captions, strip.bottom()));
    if bar.width() > 0.0 {
        let resp = ui.interact(bar, Id::new(("septet-titlebar", viewport)), Sense::click_and_drag());
        if resp.drag_started_by(egui::PointerButton::Primary) {
            shell.router.send(viewport, ViewportCommand::StartDrag);
        }
        if resp.double_clicked() {
            shell.router.send(viewport, ViewportCommand::Maximized(!maximized));
        }
        resp.context_menu(|ui| window_menu(shell, viewport, ui));
    }
    if captions > 0.0 {
        caption_buttons(shell, viewport, ui, strip, maximized, &colors);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_tab(shell: &mut Shell, wi: usize, ui: &mut Ui, index: usize, tab: crate::shell::Tab, rect: Rect, active: bool, colors: &ShellColors) {
    let ctx = ui.ctx().clone();
    let viewport = shell.windows[wi].viewport;
    let view = tab_view(shell, tab.kind);
    let painter = ui.painter().clone();
    let resp = ui.interact(rect, Id::new(("septet-tab", tab.id)), Sense::click_and_drag());
    let hovered = resp.hovered() || resp.dragged();
    let close_rect = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(18.0, 18.0));
    let narrow = rect.width() < 120.0;
    let show_close = (active || hovered) && !narrow || active;

    if active {
        let r = CornerRadius { nw: 9, ne: 9, sw: 0, se: 0 };
        let body = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() + 1.0));
        painter.rect_filled(body, r, colors.tab_active);
        // A thin line in the app's colour on top, like the suite's app badges.
        let accent = match tab.kind {
            TabKind::App(k) => k.color(),
            TabKind::Home => colors.accent,
        };
        painter.rect_filled(Rect::from_min_max(rect.min + vec2(9.0, 0.0), pos2(rect.right() - 9.0, rect.top() + 2.0)), 1.0, accent);
        // Little flares where the tab meets the app below.
        let flare = |x: f32, dir: f32| {
            let y = body.bottom();
            let pts = vec![pos2(x, y - 6.0), pos2(x, y), pos2(x + dir * 6.0, y)];
            Shape::convex_polygon(pts, colors.tab_active, Stroke::NONE)
        };
        painter.add(flare(rect.left(), -1.0));
        painter.add(flare(rect.right(), 1.0));
        painter.circle_filled(pos2(rect.left() - 6.0, body.bottom() - 6.0), 6.0, colors.strip);
        painter.circle_filled(pos2(rect.right() + 6.0, body.bottom() - 6.0), 6.0, colors.strip);
    } else {
        if hovered {
            painter.rect_filled(rect.shrink2(vec2(2.0, 2.0)), 7.0, colors.tab_hover);
        }
        // Separators between tabs that aren't selected or hovered.
        let next_active = shell.windows[wi].active == index + 1;
        if !hovered && !next_active && index + 1 < shell.windows[wi].tabs.len() {
            let x = rect.right();
            painter.line_segment([pos2(x, rect.top() + 8.0), pos2(x, rect.bottom() - 8.0)], Stroke::new(1.0, colors.separator));
        }
    }

    // Icon.
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0));
    match view.icon {
        Some(kind) => egui::Image::new(kind.icon()).fit_to_exact_size(icon_rect.size()).paint_at(ui, icon_rect),
        None => paint_home_glyph(&painter, icon_rect, if active { colors.text } else { colors.text_dim }),
    }

    // Title.
    let text_left = icon_rect.right() + 8.0;
    let text_right = if show_close { close_rect.left() - 4.0 } else { rect.right() - 10.0 };
    let color = if active { colors.text } else { colors.text_dim };
    let mut job = egui::text::LayoutJob::single_section(view.title.clone(), egui::TextFormat::simple(theme::medium(12.5), color));
    job.wrap = egui::text::TextWrapping { max_width: (text_right - text_left).max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let text_pos = pos2(text_left, rect.center().y - galley.size().y / 2.0);
    painter.with_clip_rect(Rect::from_min_max(pos2(text_left, rect.top()), pos2(text_right, rect.bottom()))).galley(text_pos, galley, color);

    // Unsaved marker in place of the close button, the close button on hover.
    let close_resp = ui.interact(close_rect, Id::new(("septet-tab-close", tab.id)), Sense::click());
    if view.dirty && !close_resp.hovered() {
        painter.circle_filled(close_rect.center(), 3.5, color);
    } else if show_close || close_resp.hovered() {
        if close_resp.hovered() {
            painter.rect_filled(close_rect, 5.0, theme::lighten(if active { colors.tab_active } else { colors.tab_hover }, 0.12));
        }
        let c = close_rect.center();
        let s = Stroke::new(1.3, if close_resp.hovered() { colors.text } else { colors.text_dim });
        painter.line_segment([c - vec2(4.0, 4.0), c + vec2(4.0, 4.0)], s);
        painter.line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], s);
    }

    if std::env::var_os("SEPTET_DEBUG_TABS").is_some() && (resp.is_pointer_button_down_on() || resp.drag_started() || resp.clicked() || resp.hovered()) {
        eprintln!(
            "tab {:?}: hovered={} down_on={} drag_started={} clicked={} dragged={} rect={rect:?}",
            tab.kind,
            resp.hovered(),
            resp.is_pointer_button_down_on(),
            resp.drag_started(),
            resp.clicked(),
            resp.dragged()
        );
    }
    if close_resp.clicked() || resp.middle_clicked() {
        shell.actions.push(Action::CloseTab { tab: tab.id });
    } else if resp.clicked() || resp.drag_started() {
        shell.actions.push(Action::Activate { tab: tab.id });
    }
    if resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(p) = resp.interact_pointer_pos()
    {
        shell.drag =
            Some(TabDrag { tab: tab.id, from: viewport, grab: p - rect.min, size: rect.size(), detached: false, target: None, global: None, local: p });
    }
    let resp = resp.on_hover_text(&view.tooltip);
    resp.context_menu(|ui| tab_menu(shell, viewport, tab, ui));
    let _ = ctx;
}

fn paint_home_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    // A small house.
    let r = rect.shrink(1.5);
    let roof = vec![pos2(r.center().x, r.top()), pos2(r.right(), r.top() + r.height() * 0.45), pos2(r.left(), r.top() + r.height() * 0.45)];
    painter.add(Shape::convex_polygon(roof, color, Stroke::NONE));
    let body = Rect::from_min_max(pos2(r.left() + 2.5, r.top() + r.height() * 0.42), pos2(r.right() - 2.5, r.bottom()));
    painter.rect_filled(body, 1.5, color);
}

fn logo_button(shell: &mut Shell, wi: usize, ui: &mut Ui, rect: Rect, colors: &ShellColors) {
    let viewport = shell.windows[wi].viewport;
    let resp = ui.interact(rect, Id::new(("septet-logo", viewport)), Sense::click()).on_hover_text("Septet Home");
    let painter = ui.painter();
    let mark = Rect::from_center_size(rect.center() + vec2(2.0, 0.0), vec2(22.0, 22.0));
    if resp.hovered() {
        painter.rect_filled(mark.expand(4.0), 7.0, colors.tab_hover);
    }
    crate::icon::paint_mark(ui, mark);
    if resp.clicked() {
        let home = shell.windows[wi].tabs.iter().find(|t| t.kind == TabKind::Home).map(|t| t.id);
        match home {
            Some(tab) => shell.actions.push(Action::Activate { tab }),
            None => shell.actions.push(Action::NewTab { window: viewport, kind: TabKind::Home }),
        }
    }
}

fn caption_buttons(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui, strip: Rect, maximized: bool, colors: &ShellColors) {
    let painter = ui.painter().clone();
    let mut x = strip.right() - CAPTION_W * 3.0;
    for (i, what) in ["min", "max", "close"].into_iter().enumerate() {
        let rect = Rect::from_min_size(pos2(x, strip.top()), vec2(CAPTION_W, STRIP_H));
        x += CAPTION_W;
        let resp = ui.interact(rect, Id::new(("septet-caption", viewport, i)), Sense::click());
        let hover = resp.hovered();
        let fg = if hover && what == "close" { Color32::WHITE } else { colors.text_dim };
        if hover {
            painter.rect_filled(rect, 0.0, if what == "close" { colors.close_hover } else { colors.tab_hover });
        }
        let c = rect.center();
        let s = Stroke::new(1.0, fg);
        match what {
            "min" => {
                painter.line_segment([c + vec2(-5.0, 0.5), c + vec2(5.0, 0.5)], s);
                if resp.clicked() {
                    shell.router.send(viewport, ViewportCommand::Minimized(true));
                }
            }
            "max" => {
                if maximized {
                    painter.rect_stroke(Rect::from_center_size(c + vec2(-1.5, 1.5), vec2(8.0, 8.0)), 1.0, s, StrokeKind::Middle);
                    painter.line_segment([c + vec2(-1.5, -4.5), c + vec2(4.5, -4.5)], s);
                    painter.line_segment([c + vec2(4.5, -4.5), c + vec2(4.5, 1.5)], s);
                } else {
                    painter.rect_stroke(Rect::from_center_size(c, vec2(10.0, 10.0)), 1.0, s, StrokeKind::Middle);
                }
                if resp.clicked() {
                    shell.router.send(viewport, ViewportCommand::Maximized(!maximized));
                }
            }
            _ => {
                painter.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], s);
                painter.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], s);
                if resp.clicked() {
                    shell.actions.push(Action::CloseWindow { window: viewport });
                }
            }
        }
    }
}

pub fn new_tab_menu(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui) {
    ui.set_min_width(250.0);
    for kind in AppKind::ALL {
        let running = shell.find_app_tab(kind).is_some();
        let resp = app_menu_row(ui, kind, running);
        if resp.clicked() {
            shell.actions.push(Action::NewTab { window: viewport, kind: TabKind::App(kind) });
            ui.close();
        }
    }
    ui.separator();
    if ui.button("🏠  Home").clicked() {
        shell.actions.push(Action::NewTab { window: viewport, kind: TabKind::Home });
        ui.close();
    }
    if ui.button("📂  Open File…").clicked() {
        crate::home::pick_and_open(shell, Some(viewport), None);
        ui.close();
    }
}

/// A launcher row: badge, name and what the app is for.
pub fn app_menu_row(ui: &mut Ui, kind: AppKind, running: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(250.0), 40.0), Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(26.0, 26.0));
    egui::Image::new(kind.icon()).fit_to_exact_size(icon.size()).paint_at(ui, icon);
    let text = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    painter.text(pos2(icon.right() + 10.0, rect.center().y - 8.0), Align2::LEFT_CENTER, kind.name(), theme::semibold(13.0), text);
    painter.text(pos2(icon.right() + 10.0, rect.center().y + 8.0), Align2::LEFT_CENTER, kind.tagline(), FontId::proportional(11.0), weak);
    if running {
        painter.circle_filled(pos2(rect.right() - 12.0, rect.center().y), 3.5, kind.color());
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

fn tab_menu(shell: &mut Shell, viewport: ViewportId, tab: crate::shell::Tab, ui: &mut Ui) {
    ui.set_min_width(220.0);
    if ui.button("Move to New Window").clicked() {
        shell.actions.push(Action::TearOff { tab: tab.id, at: None, size: shell.window_size(viewport) });
        ui.close();
    }
    let others: Vec<(ViewportId, String)> =
        shell.windows.iter().filter(|w| w.viewport != viewport && !w.hidden).map(|w| (w.viewport, w.title.trim_end_matches(" — Septet").to_string())).collect();
    if !others.is_empty() {
        ui.menu_button("Move to Window", |ui| {
            for (v, title) in others {
                if ui.button(title).clicked() {
                    let index = shell.window_index(v).map_or(0, |i| shell.windows[i].tabs.len());
                    shell.actions.push(Action::MoveTab { tab: tab.id, to: v, index });
                    ui.close();
                }
            }
        });
    }
    if shell.visible_windows() > 1 && ui.button("Merge All Windows Here").clicked() {
        shell.actions.push(Action::MergeAllWindows { into: viewport });
        ui.close();
    }
    if let TabKind::App(kind) = tab.kind {
        ui.separator();
        if ui.button(format!("Open File in {}…", kind.name())).clicked() {
            crate::home::pick_and_open(shell, Some(viewport), Some(kind));
            ui.close();
        }
        if shell.apps.contains_key(&kind) {
            ui.menu_button("Send to", |ui| {
                for target in AppKind::ALL.into_iter().filter(|k| *k != kind) {
                    let resp = app_menu_row(ui, target, shell.apps.contains_key(&target));
                    if resp.clicked() {
                        shell.actions.push(Action::SendTo { source: kind, target });
                        ui.close();
                    }
                }
            });
        }
    }
    ui.separator();
    if ui.button("Close Tab").clicked() {
        shell.actions.push(Action::CloseTab { tab: tab.id });
        ui.close();
    }
    if ui.button("Close Other Tabs").clicked() {
        shell.actions.push(Action::CloseOtherTabs { tab: tab.id });
        ui.close();
    }
}

fn window_menu(shell: &mut Shell, viewport: ViewportId, ui: &mut Ui) {
    new_tab_menu(shell, viewport, ui);
    if shell.visible_windows() > 1 {
        ui.separator();
        if ui.button("Merge All Windows Here").clicked() {
            shell.actions.push(Action::MergeAllWindows { into: viewport });
            ui.close();
        }
    }
    ui.separator();
    if ui.button("About Septet").clicked() {
        shell.about = Some(viewport);
        ui.close();
    }
    if ui.button("Quit Septet").clicked() {
        shell.actions.push(Action::Quit);
        ui.close();
    }
}

/// Resize handles along the window's edges (no OS decorations).
pub fn window_edges(shell: &mut Shell, wi: usize, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let viewport = shell.windows[wi].viewport;
    let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    // macOS windows keep their system frame (and its resize edges).
    if maximized || fullscreen || cfg!(target_os = "macos") {
        return;
    }
    let r = ctx.content_rect();
    let e = 5.0;
    let c = 12.0;
    let zones = [
        (Rect::from_min_max(r.left_top(), pos2(r.left() + c, r.top() + c)), ResizeDirection::NorthWest, CursorIcon::ResizeNorthWest),
        (Rect::from_min_max(pos2(r.right() - c, r.top()), pos2(r.right(), r.top() + c)), ResizeDirection::NorthEast, CursorIcon::ResizeNorthEast),
        (Rect::from_min_max(pos2(r.left(), r.bottom() - c), pos2(r.left() + c, r.bottom())), ResizeDirection::SouthWest, CursorIcon::ResizeSouthWest),
        (Rect::from_min_max(r.right_bottom() - vec2(c, c), r.right_bottom()), ResizeDirection::SouthEast, CursorIcon::ResizeSouthEast),
        (Rect::from_min_max(pos2(r.left() + c, r.top()), pos2(r.right() - c, r.top() + 3.0)), ResizeDirection::North, CursorIcon::ResizeNorth),
        (Rect::from_min_max(pos2(r.left() + c, r.bottom() - e), pos2(r.right() - c, r.bottom())), ResizeDirection::South, CursorIcon::ResizeSouth),
        (Rect::from_min_max(pos2(r.left(), r.top() + c), pos2(r.left() + e, r.bottom() - c)), ResizeDirection::West, CursorIcon::ResizeWest),
        (Rect::from_min_max(pos2(r.right() - e, r.top() + c), pos2(r.right(), r.bottom() - c)), ResizeDirection::East, CursorIcon::ResizeEast),
    ];
    for (i, (rect, dir, cursor)) in zones.into_iter().enumerate() {
        egui::Area::new(Id::new(("septet-resize", viewport, i))).order(egui::Order::Tooltip).fixed_pos(rect.min).constrain(false).show(&ctx, |ui| {
            let resp = ui.allocate_rect(Rect::from_min_size(rect.min, rect.size()), Sense::drag());
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(cursor);
            }
            if resp.drag_started() {
                shell.router.send(viewport, ViewportCommand::BeginResize(dir));
            }
        });
    }
}

/// A tab being dragged: the floating tab in its own window, the drop marker in another one.
pub fn drag_overlay(shell: &mut Shell, wi: usize, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let viewport = shell.windows[wi].viewport;

    // Wayland tear-off: this window sees the pointer on its strip right after the release.
    if let Some(p) = shell.pending_tear.clone()
        && p.from != viewport
        && let Some(pos) = ctx.input(|i| i.pointer.hover_pos())
        && shell.windows[wi].strip.expand2(vec2(0.0, 16.0)).contains(pos)
    {
        let index = slot_at(shell, wi, pos.x);
        shell.pending_tear = None;
        shell.actions.push(Action::MoveTab { tab: p.tab, to: viewport, index });
    }

    let Some(drag) = shell.drag.clone() else { return };
    if drag.from == viewport {
        // Where is the pointer on screen, and is another window's strip under it?
        let inner = ctx.input(|i| i.viewport().inner_rect);
        let global = inner.map(|r| r.min + drag.local.to_vec2());
        let mut target = None;
        if drag.detached
            && let Some(g) = global
        {
            for (i, w) in shell.windows.iter().enumerate() {
                if w.viewport == viewport || w.hidden {
                    continue;
                }
                let Some(r) = ctx.input_for(w.viewport, |inp| inp.viewport().inner_rect) else { continue };
                let strip = w.strip.translate(r.min.to_vec2()).expand2(vec2(0.0, 18.0));
                if strip.contains(g) {
                    target = Some((w.viewport, slot_at(shell, i, g.x - r.min.x)));
                    break;
                }
            }
        }
        if let Some(d) = shell.drag.as_mut() {
            d.global = global;
            d.target = target;
        }
        if drag.detached && shell.windows[wi].tabs.len() == 1 && global.is_none() {
            // Only tab, and we can't place windows (Wayland): move the whole window instead.
            shell.drag = None;
            shell.router.send(viewport, ViewportCommand::StartDrag);
            return;
        }
        // Where we know window positions, the floating tab is a small window of its own (see
        // `ghost_window`), so it can leave this window; otherwise it's drawn in here.
        if drag.detached && global.is_none() {
            ghost(shell, &ctx, &drag, target.is_none());
        }
        ctx.request_repaint();
    } else if drag.target.is_some_and(|(v, _)| v == viewport) {
        let (_, index) = drag.target.unwrap_or((viewport, 0));
        let w = &shell.windows[wi];
        let x = w.tab_rects.get(index).map(|(_, r)| r.left()).or_else(|| w.tab_rects.last().map(|(_, r)| r.right())).unwrap_or(w.strip.left() + LOGO_W);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new(("septet-drop", viewport))));
        let accent = match shell.find_tab(drag.tab).map(|(a, b)| shell.windows[a].tabs[b].kind) {
            Some(TabKind::App(k)) => k.color(),
            _ => Color32::from_rgb(0xff, 0x45, 0x3a),
        };
        painter.rect_filled(Rect::from_center_size(pos2(x, w.strip.top() + TAB_TOP + TAB_H / 2.0), vec2(3.0, TAB_H - 4.0)), 1.5, accent);
        painter.rect_stroke(w.strip.shrink(1.0), 0.0, Stroke::new(1.0, accent.gamma_multiply(0.6)), StrokeKind::Inside);
        ctx.request_repaint();
    }
}

/// The floating tab under the pointer while it is out of the strip.
fn ghost(shell: &Shell, ctx: &egui::Context, drag: &TabDrag, new_window: bool) {
    let Some((wi, ti)) = shell.find_tab(drag.tab) else { return };
    let kind = shell.windows[wi].tabs[ti].kind;
    let view = tab_view(shell, kind);
    let colors = colors_for(shell, wi);
    let rect = Rect::from_min_size(drag.local - drag.grab, drag.size);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("septet-ghost")));
    painter.add(egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(110) }.as_shape(rect, 9.0));
    painter.rect_filled(rect, 9.0, colors.tab_active);
    if let TabKind::App(k) = kind {
        painter.rect_filled(Rect::from_min_max(rect.min + vec2(9.0, 0.0), pos2(rect.right() - 9.0, rect.top() + 2.0)), 1.0, k.color());
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0));
    if let Some(k) = view.icon {
        let ui = egui::Ui::new(ctx.clone(), Id::new("septet-ghost-ui"), egui::UiBuilder::new().layer_id(painter.layer_id()).max_rect(rect));
        egui::Image::new(k.icon()).fit_to_exact_size(icon.size()).paint_at(&ui, icon);
    }
    painter.text(pos2(icon.right() + 8.0, rect.center().y), Align2::LEFT_CENTER, &view.title, theme::medium(12.5), colors.text);
    if new_window {
        let hint = Rect::from_min_size(rect.left_bottom() + vec2(0.0, 6.0), vec2(rect.width(), 20.0));
        painter.text(hint.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, "Release for a new window", FontId::proportional(11.0), colors.text_dim);
    }
}

/// The tab slot at window x coordinate `x` in window `wi`'s strip.
fn slot_at(shell: &Shell, wi: usize, x: f32) -> usize {
    let w = &shell.windows[wi];
    w.tab_rects.iter().position(|(_, r)| x < r.center().x).unwrap_or(w.tab_rects.len())
}

/// The tab being dragged out of its window, as a small borderless window under the pointer, like a
/// browser's: you see where the new window will open. Only where window positions are known.
pub fn ghost_window(shell: &mut Shell, ctx: &egui::Context) {
    let Some(drag) = shell.drag.clone() else { return };
    let (true, Some(global), None) = (drag.detached, drag.global, drag.target) else { return };
    let Some((wi, ti)) = shell.find_tab(drag.tab) else { return };
    let kind = shell.windows[wi].tabs[ti].kind;
    let view = tab_view(shell, kind);
    let colors = colors_for(shell, wi);
    let size = drag.size + vec2(24.0, 44.0);
    let builder = egui::ViewportBuilder::default()
        .with_title("Septet")
        .with_app_id(crate::APP_ID)
        .with_decorations(false)
        .with_transparent(true)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_mouse_passthrough(true)
        .with_taskbar(false)
        .with_active(false)
        .with_resizable(false)
        .with_inner_size(size)
        .with_position(global - drag.grab - vec2(12.0, 8.0));
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("septet-tab-ghost"), builder, |ui, _class| {
        let full = ui.ctx().content_rect();
        let painter = ui.painter();
        painter.rect_filled(full, 0.0, Color32::TRANSPARENT);
        let chip = Rect::from_min_size(full.min + vec2(12.0, 8.0), drag.size);
        painter.add(egui::epaint::Shadow { offset: [0, 6], blur: 16, spread: 0, color: Color32::from_black_alpha(120) }.as_shape(chip, 9.0));
        painter.rect_filled(chip, 9.0, colors.tab_active);
        if let TabKind::App(k) = kind {
            painter.rect_filled(Rect::from_min_max(chip.min + vec2(9.0, 0.0), pos2(chip.right() - 9.0, chip.top() + 2.0)), 1.0, k.color());
        }
        let icon = Rect::from_center_size(pos2(chip.left() + 18.0, chip.center().y), vec2(16.0, 16.0));
        if let Some(k) = view.icon {
            egui::Image::new(k.icon()).fit_to_exact_size(icon.size()).paint_at(ui, icon);
        }
        ui.painter().text(pos2(icon.right() + 8.0, chip.center().y), Align2::LEFT_CENTER, &view.title, theme::medium(12.5), colors.text);
    });
}
