//! Moving content between apps with the mouse, and "Send to".
//!
//! Drag something out of an app — a layer, selected art, photos from Lightcraft's grid, footage from a
//! project panel, a page — and hover another app's tab: after a moment the tab opens (spring-loaded,
//! like Photoshop's document tabs). Release over that app and it places the content where you let go.
//! Across windows this works wherever window positions are known (X11, Windows, macOS).

use std::path::PathBuf;

use egui::{Align2, Color32, Context, Id, Pos2, Rect, Ui, ViewportId, pos2, vec2};

use crate::kinds::AppKind;
use crate::shell::{Action, Shell, TabKind};
use crate::theme;

/// How long the pointer rests on a tab before it opens during a drag.
const SPRING: f64 = 0.55;

#[derive(Clone, Debug)]
pub struct ContentDrag {
    pub source: AppKind,
    pub label: String,
    pub from: ViewportId,
    /// The tab under the pointer and since when.
    pub hover: Option<(crate::shell::TabId, f64)>,
}

/// Formats each app places well, best first.
pub fn accepts(kind: AppKind) -> &'static [&'static str] {
    match kind {
        AppKind::Photocraft => &["psd", "tif", "tiff", "png", "jpg", "jpeg", "webp"],
        AppKind::Vectorcraft => &["svg", "pdf", "ai", "eps", "png", "jpg", "jpeg"],
        AppKind::Lightcraft => &["dng", "tif", "tiff", "jpg", "jpeg", "png", "webp"],
        AppKind::Designcraft => &["pdf", "svg", "psd", "tif", "tiff", "png", "jpg", "jpeg", "txt"],
        AppKind::Pdfcraft => &["pdf", "png", "jpg", "jpeg"],
        AppKind::Filmcraft => &["mp4", "mov", "mkv", "webm", "wav", "mp3", "m4a", "png", "jpg", "jpeg"],
        AppKind::Effectcraft => &["mp4", "mov", "mkv", "webm", "wav", "mp3", "svg", "psd", "pdf", "png", "jpg", "jpeg"],
    }
}

/// Where content handed from one app to another is written (placed files are often linked, so it stays).
pub fn shared_dir() -> Option<PathBuf> {
    crate::recent::data_dir("Shared")
}

/// After an app had its frame in window `viewport`: notice a drag leaving it.
pub fn after_app(shell: &mut Shell, ctx: &Context, viewport: ViewportId, kind: AppKind, label: Option<String>) {
    if shell.content_drag.is_some() {
        return;
    }
    if let Some(label) = label
        && ctx.input(|i| i.pointer.primary_down())
    {
        if std::env::var_os("SEPTET_DEBUG_TABS").is_some() {
            eprintln!("content drag from {} started: {label}", kind.name());
        }
        shell.content_drag = Some(ContentDrag { source: kind, label, from: viewport, hover: None });
    }
}

/// During a content drag, in each window's pass: spring-loaded tabs and the ghost.
pub fn window_ui(shell: &mut Shell, wi: usize, ui: &mut Ui) {
    let Some(drag) = shell.content_drag.clone() else { return };
    let ctx = ui.ctx().clone();
    let viewport = shell.windows[wi].viewport;
    let Some(local) = pointer_in(shell, &ctx, &drag, wi) else { return };
    let now = ctx.input(|i| i.time);
    // Still inside the app it came from (reordering layers, moving art): that app's business.
    let in_source = shell.windows[wi].active_tab().is_some_and(|t| t.kind == TabKind::App(drag.source)) && !shell.windows[wi].strip.contains(local);
    if in_source {
        if let Some(d) = shell.content_drag.as_mut() {
            d.hover = None;
        }
        return;
    }

    // Spring-loaded tabs.
    let hovered_tab = shell.windows[wi].tab_rects.iter().find(|(_, r)| r.contains(local)).map(|(id, _)| *id);
    let mut hover = drag.hover;
    match hovered_tab {
        Some(tab) if hover.map(|(t, _)| t) != Some(tab) => hover = Some((tab, now)),
        Some(tab) => {
            if let Some((_, since)) = hover
                && now - since > SPRING
                && shell.windows[wi].active_tab().map(|t| t.id) != Some(tab)
            {
                shell.actions.push(Action::Activate { tab });
            }
        }
        None => hover = None,
    }
    if let Some(d) = shell.content_drag.as_mut() {
        d.hover = hover;
    }

    // The ghost, with what will happen on release.
    let target = match shell.windows[wi].active_tab().map(|t| t.kind) {
        Some(TabKind::App(k)) if k != drag.source && !shell.windows[wi].strip.contains(local) => Some(k),
        _ => None,
    };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new(("septet-content-ghost", viewport))));
    let text = match target {
        Some(k) => format!("{}  →  {}", drag.label, k.name()),
        None => drag.label.clone(),
    };
    let galley = painter.layout_no_wrap(text, theme::medium(12.5), Color32::WHITE);
    let rect = Rect::from_min_size(local + vec2(16.0, 14.0), galley.size() + vec2(38.0, 14.0));
    painter.add(egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(100) }.as_shape(rect, 7.0));
    let fill = target.map_or(Color32::from_rgb(0x2b, 0x2b, 0x30), |k| theme::darken(k.color(), 0.25));
    painter.rect_filled(rect, 7.0, fill);
    let icon = Rect::from_center_size(pos2(rect.left() + 15.0, rect.center().y), vec2(14.0, 14.0));
    let icon_ui = Ui::new(ctx.clone(), Id::new(("septet-content-ghost-ui", viewport)), egui::UiBuilder::new().layer_id(painter.layer_id()).max_rect(rect));
    egui::Image::new(drag.source.icon()).fit_to_exact_size(icon.size()).paint_at(&icon_ui, icon);
    painter.galley(pos2(icon.right() + 8.0, rect.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
    let _ = Align2::LEFT_CENTER;
    ctx.request_repaint();
}

/// The pointer in window `wi`'s coordinates during `drag`, if it is over that window.
fn pointer_in(shell: &Shell, ctx: &Context, drag: &ContentDrag, wi: usize) -> Option<Pos2> {
    let viewport = shell.windows[wi].viewport;
    let from_local = ctx.input_for(drag.from, |i| i.pointer.latest_pos())?;
    if viewport == drag.from {
        return ctx.input(|i| i.viewport().inner_rect).map_or(Some(from_local), |r| r.contains(r.min + from_local.to_vec2()).then_some(from_local));
    }
    // Another window: only where window positions are known.
    let from_rect = ctx.input_for(drag.from, |i| i.viewport().inner_rect)?;
    let this_rect = ctx.input(|i| i.viewport().inner_rect)?;
    let global = from_rect.min + from_local.to_vec2();
    this_rect.contains(global).then(|| pos2(global.x - this_rect.min.x, global.y - this_rect.min.y))
}

/// After every window's pass: a released content drag lands in the app under the pointer.
pub fn finish(shell: &mut Shell, ctx: &Context) {
    let Some(drag) = shell.content_drag.clone() else { return };
    if ctx.input_for(drag.from, |i| i.pointer.primary_down()) {
        return;
    }
    shell.content_drag = None;
    // Which window, and where in it?
    let hit = (0..shell.windows.len()).filter(|&wi| !shell.windows[wi].hidden).find_map(|wi| {
        let p = if shell.windows[wi].viewport == drag.from {
            ctx.input_for(drag.from, |i| i.pointer.latest_pos())
                .filter(|p| ctx.input_for(drag.from, |i| i.viewport().inner_rect).is_none_or(|r| r.contains(r.min + p.to_vec2())))
        } else {
            let from_rect = ctx.input_for(drag.from, |i| i.viewport().inner_rect)?;
            let local = ctx.input_for(drag.from, |i| i.pointer.latest_pos())?;
            let this = ctx.input_for(shell.windows[wi].viewport, |i| i.viewport().inner_rect)?;
            let g = from_rect.min + local.to_vec2();
            this.contains(g).then(|| pos2(g.x - this.min.x, g.y - this.min.y))
        };
        p.map(|p| (wi, p))
    });
    if std::env::var_os("SEPTET_DEBUG_TABS").is_some() {
        eprintln!("content drag from {} released: hit={hit:?}", drag.source.name());
    }
    let Some((wi, at)) = hit else { return };
    let w = &shell.windows[wi];
    // Over a tab: that app (opening it); over the content: the app shown there.
    let (target, at) = match w.tab_rects.iter().find(|(_, r)| r.contains(at)).and_then(|(id, _)| w.tabs.iter().find(|t| t.id == *id)) {
        Some(t) => (t.kind, None),
        None if w.strip.contains(at) => return,
        None => match w.active_tab() {
            Some(t) => (t.kind, Some(at)),
            None => return,
        },
    };
    let TabKind::App(target) = target else { return };
    if target == drag.source {
        return;
    }
    let (Some(source), Some(dir)) = (shell.apps.get(&drag.source).cloned(), shared_dir()) else { return };
    let files = {
        let mut s = source.borrow_mut();
        let files = s.with_app(ctx, |a| a.take_outgoing_files(accepts(target), &dir));
        s.with_app(ctx, |a| a.cancel_outgoing_drag());
        files
    };
    if files.is_empty() {
        shell.bridge.notify(format!("{} can't hand this to {}", drag.source.name(), target.name()));
        return;
    }
    let viewport = shell.windows[wi].viewport;
    shell.opens.push(crate::shell::OpenRequest { paths: files, app: Some(target), window: Some(viewport), place: Some(at) });
}

/// Tab menu › Send to: the active document of `source` placed into `target`.
pub fn send_to(shell: &mut Shell, ctx: &Context, source: AppKind, target: AppKind) {
    let (Some(slot), Some(dir)) = (shell.apps.get(&source).cloned(), shared_dir()) else { return };
    let file = slot.borrow_mut().with_app(ctx, |a| a.export_active(accepts(target), &dir));
    match file {
        Some(file) => {
            let window = shell.find_app_tab(target).map(|(wi, _)| shell.windows[wi].viewport);
            shell.opens.push(crate::shell::OpenRequest { paths: vec![file], app: Some(target), window, place: Some(None) });
        }
        None => shell.bridge.notify(format!("Nothing to send from {}", source.name())),
    }
}
