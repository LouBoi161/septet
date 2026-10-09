//! The Claude panel: docked at the right of a window, beside the app.

use egui::{Align, Color32, Context, CursorIcon, Id, Key, Layout, Modifiers, Rect, RichText, Sense, Stroke, Ui, ViewportId, pos2, vec2};
use serde_json::Value;

use super::conversation::Entry;
use super::markdown;
use super::protocol::Part;
use super::{Assistant, Choice, ProbeState};
use crate::theme::{self, ShellColors};

pub const MIN_WIDTH: f32 = 300.0;
const HEADER_H: f32 = 42.0;

pub fn input_id(viewport: ViewportId) -> Id {
    Id::new(("septet-claude-input", viewport))
}

/// The chat field has the keyboard: keys must not reach the app.
pub fn has_keyboard(a: &Assistant, ctx: &Context, viewport: ViewportId) -> bool {
    a.panel == Some(viewport) && ctx.memory(|m| m.has_focus(input_id(viewport)))
}

fn colors() -> (ShellColors, Color32, Color32) {
    let c = ShellColors::home();
    let ok = Color32::from_rgb(0x4c, 0xc3, 0x7a);
    let bad = Color32::from_rgb(0xf0, 0x6c, 0x5c);
    (c, ok, bad)
}

/// A short, readable account of a tool call.
fn label(name: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or_default();
    let file = |k: &str| std::path::Path::new(s(k)).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| s(k).to_owned());
    let app = || {
        let a = s("app");
        let mut c = a.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
    };
    let id = || match &input["id"] {
        Value::Null => String::new(),
        Value::String(v) => format!(" {v}"),
        v => format!(" {v}"),
    };
    let paths = || {
        let names: Vec<String> = input["paths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|p| std::path::Path::new(p).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_owned()))
            .collect();
        names.join(", ")
    };
    match name.strip_prefix("mcp__septet__").unwrap_or(name) {
        "septet_state" => "Looked at the open tabs".into(),
        "septet_open" if s("app").is_empty() => format!("Open {}", paths()),
        "septet_open" => format!("Open {} in {}", paths(), app()),
        "septet_place" => format!("Place {} in {}", paths(), app()),
        "septet_activate" => format!("Show {}", app()),
        "septet_render" => format!("Look at {}", file("path")),
        "septet_fetch" => format!("Download {}", file("path")),
        "app_commands" if s("filter").is_empty() => format!("Looked up {}'s commands", app()),
        "app_commands" => format!("Looked up {}'s commands for “{}”", app(), s("filter")),
        "app_execute" => format!("Ran {} in {}", s("command"), app()),
        "app_inspect" => format!("Looked at the {}{} in {}", s("what"), id(), app()),
        "app_render" => format!("Looked at the {}{} in {}", input["target"].as_str().unwrap_or("document"), id(), app()),
        "app_undo" if input["redo"].as_bool() == Some(true) => format!("Redo in {}", app()),
        "app_undo" => format!("Undo in {}", app()),
        "Skill" => format!("Use the {} skill", s("skill").trim_start_matches("septet:")),
        "WebSearch" => format!("Search the web for “{}”", s("query")),
        "WebFetch" => format!("Read {}", super::conversation::host(s("url")).unwrap_or_else(|| s("url").to_owned())),
        "Write" => format!("Write {}", file("file_path")),
        "Edit" | "MultiEdit" => format!("Edit {}", file("file_path")),
        "Read" => format!("Read {}", file("file_path")),
        "Bash" => {
            let cmd = s("command");
            let short: String = cmd.chars().take(60).collect();
            format!("Run {short}{}", if cmd.chars().count() > 60 { "…" } else { "" })
        }
        "Glob" | "Grep" => format!("Search for {}", s("pattern")),
        other => match other.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
            Some((server, tool)) => format!("{server}: {}", tool.replace('_', " ")),
            None => other.replace('_', " "),
        },
    }
}

/// What an approval asks, in words.
fn ask(tool: &str, input: &Value) -> (String, Option<String>) {
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_owned);
    match tool {
        "Bash" => ("Run a command?".into(), s("command")),
        "Write" => ("Write a file outside the workspace?".into(), s("file_path")),
        "Edit" | "MultiEdit" => ("Change a file outside the workspace?".into(), s("file_path")),
        "Read" => ("Read a file outside the workspace?".into(), s("file_path")),
        "Glob" | "Grep" => ("Search outside the workspace?".into(), s("path").or_else(|| s("pattern"))),
        "WebSearch" => ("Search the web?".into(), s("query")),
        "WebFetch" => ("Open a web page?".into(), s("url")),
        "app_execute" => {
            let what = s("path").map_or_else(|| input["params"].to_string(), |p| format!("→ {p}"));
            (
                s("question").unwrap_or_else(|| "Let Claude run this?".into()),
                Some(format!("{}: {} {what}", s("app").unwrap_or_default(), s("command").unwrap_or_default())),
            )
        }
        _ => {
            let what = match tool.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
                Some((server, name)) => format!("Use {name} from {server}?"),
                None => format!("Use {tool}?"),
            };
            (what, serde_json::to_string_pretty(input).ok())
        }
    }
}

/// The panel, in `rect` of the window `viewport`.
pub fn show(a: &mut Assistant, ui: &mut Ui, rect: Rect, viewport: ViewportId) {
    let ctx = ui.ctx().clone();
    let (c, _, _) = colors();
    let bg = theme::mix(c.home_bg, Color32::BLACK, 0.18);
    ui.painter().rect_filled(rect, 0.0, bg);
    ui.painter().vline(rect.left(), rect.y_range(), Stroke::new(1.0, c.separator));

    // Resize from the left edge.
    let grip = Rect::from_min_max(pos2(rect.left() - 3.0, rect.top()), pos2(rect.left() + 3.0, rect.bottom()));
    let g = ui.interact(grip, Id::new(("septet-claude-grip", viewport)), Sense::drag()).on_hover_cursor(CursorIcon::ResizeHorizontal);
    if g.dragged() {
        a.panel_width = (a.panel_width - g.drag_delta().x).max(MIN_WIDTH);
    }

    let inner = rect.shrink2(vec2(14.0, 0.0));
    let header = Rect::from_min_size(inner.min, vec2(inner.width(), HEADER_H));
    let mut ui_h = ui.new_child(egui::UiBuilder::new().max_rect(header).layout(Layout::left_to_right(Align::Center)));
    header_ui(a, &mut ui_h, &ctx, viewport);
    ui.painter().hline(rect.x_range(), header.bottom(), Stroke::new(1.0, c.separator));

    let body = Rect::from_min_max(pos2(inner.left(), header.bottom() + 1.0), inner.max);
    let mut ui_b = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Min)));
    ui_b.set_clip_rect(body);
    if !a.settings.enabled {
        intro(&mut ui_b, "Claude is off.", "Claude can create graphics, layouts, PDFs and motion graphics from a description, and work in the apps for you.");
        if ui_b.button("Turn on Claude").clicked() {
            a.settings.enabled = true;
            a.settings.save();
            a.recheck(&ctx);
        }
        return;
    }
    match &a.probe {
        ProbeState::Checking(_) if a.conversation.is_none() => {
            ui_b.add_space(16.0);
            ui_b.horizontal(|ui| {
                ui.spinner();
                ui.label("Looking for Claude Code…");
            });
            return;
        }
        ProbeState::Done(_) if a.ready().is_none() && a.conversation.is_none() => {
            intro(&mut ui_b, "Claude Code isn't ready.", "Septet uses your own Claude Code, signed in with your Claude subscription.");
            if ui_b.button("Open Claude settings").clicked() {
                a.dialog = Some(viewport);
                a.recheck(&ctx);
            }
            return;
        }
        _ => {}
    }
    conversation_ui(a, &mut ui_b, &ctx, viewport);
}

fn intro(ui: &mut Ui, title: &str, text: &str) {
    let (c, _, _) = colors();
    ui.add_space(24.0);
    ui.label(RichText::new(title).font(theme::semibold(15.0)).color(c.text));
    ui.add_space(4.0);
    ui.label(RichText::new(text).color(c.text_dim));
    ui.add_space(12.0);
}

fn header_ui(a: &mut Assistant, ui: &mut Ui, ctx: &Context, viewport: ViewportId) {
    let (c, _, _) = colors();
    ui.label(RichText::new("Claude").font(theme::semibold(15.0)).color(c.text));
    if let Some(conv) = &a.conversation
        && !conv.model.is_empty()
    {
        ui.label(RichText::new(conv.model.trim_start_matches("claude-")).size(11.5).color(c.text_dim));
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let small = |t: &str| egui::Button::new(RichText::new(t).size(12.0)).frame(false);
        if ui.add(small("×")).on_hover_text("Close (Ctrl+Shift+K)").clicked() {
            a.panel = None;
        }
        if ui.add(small("Settings")).clicked() {
            a.dialog = Some(viewport);
            a.recheck(ctx);
        }
        let past = ui.add(small("History"));
        egui::Popup::menu(&past).show(|ui| {
            ui.set_min_width(260.0);
            if a.history.items.is_empty() {
                ui.label(RichText::new("No earlier conversations").color(c.text_dim));
            }
            let mut resume = None;
            for p in a.history.items.iter().take(20) {
                let current = a.conversation.as_ref().is_some_and(|conv| conv.session.id == p.id);
                if ui.add(egui::Button::selectable(current, &p.title).right_text(RichText::new(crate::recent::ago(p.updated)).color(c.text_dim))).clicked() {
                    resume = Some(p.clone());
                }
            }
            if let Some(p) = resume {
                a.resume(ctx, &p);
            }
        });
        if ui.add(small("New")).on_hover_text("New conversation").clicked() {
            a.conversation = None;
            a.panel_error = None;
            a.thumbs.clear();
        }
    });
}

fn conversation_ui(a: &mut Assistant, ui: &mut Ui, ctx: &Context, viewport: ViewportId) {
    let (c, ok, bad) = colors();
    let busy = a.conversation.as_ref().is_some_and(|conv| conv.busy);
    let full = ui.max_rect();

    // Bottom up: status line, composer, approvals; the transcript takes what is left.
    let status_h = 22.0;
    let composer_h = a.composer_h;
    let approvals_n = a.conversation.as_ref().map_or(0, |conv| conv.approvals.len().min(1));
    let approvals_h = if approvals_n > 0 { 150.0 } else { 0.0 };
    let bottom = full.bottom() - 10.0;
    let status = Rect::from_min_max(pos2(full.left(), bottom - status_h), pos2(full.right(), bottom));
    let composer = Rect::from_min_max(pos2(full.left(), status.top() - 4.0 - composer_h), pos2(full.right(), status.top() - 4.0));
    let approvals = Rect::from_min_max(pos2(full.left(), composer.top() - approvals_h), pos2(full.right(), composer.top() - 6.0));
    let transcript = Rect::from_min_max(pos2(full.left(), full.top() + 8.0), pos2(full.right(), approvals.top() - 6.0));

    let mut t = ui.new_child(egui::UiBuilder::new().max_rect(transcript).layout(Layout::top_down(Align::Min)));
    t.set_clip_rect(transcript.expand2(vec2(6.0, 0.0)));
    egui::ScrollArea::vertical().id_salt(("septet-claude-scroll", viewport)).stick_to_bottom(true).auto_shrink([false, false]).show(&mut t, |ui| {
        ui.set_width(ui.available_width() - 6.0);
        if a.conversation.as_ref().is_none_or(|conv| conv.entries.is_empty()) {
            ui.add_space(12.0);
            ui.label(RichText::new("What should we make?").font(theme::semibold(15.0)).color(c.text));
            ui.add_space(6.0);
            for example in [
                "Design a logo for a bakery called “Crumb” as SVG and open it in Vectorcraft",
                "Make a one-page event flyer as PDF",
                "Animate a lower third with my name in Effectcraft",
            ] {
                if ui.add(egui::Button::new(RichText::new(example).size(12.5).color(c.text_dim)).wrap().fill(c.card)).clicked() {
                    a.input = example.to_owned();
                    ctx.memory_mut(|m| m.request_focus(input_id(viewport)));
                }
                ui.add_space(4.0);
            }
        }
        if let Some(e) = &a.panel_error {
            ui.label(RichText::new(e).color(bad));
        }
        let Some(conv) = &a.conversation else { return };
        let md = markdown::Style { text: c.text, dim: c.text_dim, code_bg: theme::mix(c.card, Color32::BLACK, 0.25), size: 13.5 };
        let last = conv.entries.len().saturating_sub(1);
        for (i, e) in conv.entries.iter().enumerate() {
            match e {
                Entry::User(text) => {
                    ui.add_space(8.0);
                    ui.with_layout(Layout::top_down(Align::Max), |ui| {
                        egui::Frame::new().fill(c.card).corner_radius(10.0).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
                            ui.set_max_width(ui.available_width() * 0.85);
                            ui.add(egui::Label::new(RichText::new(text).size(13.5).color(c.text)).wrap());
                        });
                    });
                    ui.add_space(4.0);
                }
                Entry::Text(text) if !text.trim().is_empty() => {
                    ui.add_space(4.0);
                    markdown::show(ui, text, &md);
                }
                Entry::Thinking if busy && i == last => {
                    ui.label(RichText::new("Thinking…").italics().color(c.text_dim));
                }
                Entry::Tool { id, name, input, result } => {
                    tool_card(ui, &mut a.thumbs, id, name, input, result.as_ref(), busy, (&c, ok, bad));
                }
                Entry::Note(text) => {
                    ui.add_space(4.0);
                    ui.label(RichText::new(text).size(12.5).italics().color(c.text_dim));
                }
                _ => {}
            }
        }
        if busy {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Working…").color(c.text_dim));
            });
        }
        ui.add_space(8.0);
    });

    if approvals_n > 0 {
        let mut u = ui.new_child(egui::UiBuilder::new().max_rect(approvals).layout(Layout::top_down(Align::Min)));
        approval_card(a, &mut u);
    }

    let mut u = ui.new_child(egui::UiBuilder::new().max_rect(composer).layout(Layout::top_down(Align::Min)));
    let want = composer_ui(a, &mut u, ctx, viewport, busy);
    if (want - a.composer_h).abs() > 0.5 {
        a.composer_h = want;
        ctx.request_repaint();
    }

    let mut s = ui.new_child(egui::UiBuilder::new().max_rect(status).layout(Layout::left_to_right(Align::Center)));
    let mut line = String::new();
    if let Some(r) = a.conversation.as_ref().and_then(|conv| conv.rate_limit.as_ref()) {
        // "allowed", "allowed_warning", … ; "rejected" once the plan's limit is used up.
        if r.status == "rejected" {
            line = "Usage limit reached".into();
        } else if let Some(u) = r.utilization {
            let window = match r.kind.as_str() {
                "five_hour" => "5-hour".to_owned(),
                "seven_day" => "weekly".to_owned(),
                other => other.replace('_', " "),
            };
            line = format!("Plan usage: {:.0}% of the {window} limit", u * 100.0);
        }
    }
    s.label(RichText::new(line).size(11.0).color(c.text_dim));
}

#[allow(clippy::too_many_arguments)]
fn tool_card(
    ui: &mut Ui,
    thumbs: &mut std::collections::HashMap<String, egui::TextureHandle>,
    id: &str,
    name: &str,
    input: &Value,
    result: Option<&(Vec<Part>, bool)>,
    busy: bool,
    (c, ok, bad): (&ShellColors, Color32, Color32),
) {
    ui.add_space(3.0);
    egui::Frame::new().fill(theme::mix(c.card, Color32::BLACK, 0.1)).corner_radius(7.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let glyph = match result {
            None if busy => "…",
            None => "–",
            Some((_, false)) => "✔",
            Some((_, true)) => "✖",
        };
        let color = match result {
            Some((_, false)) => ok,
            Some((_, true)) => bad,
            None => c.text_dim,
        };
        let mut job = egui::text::LayoutJob::default();
        job.append(glyph, 0.0, egui::TextFormat::simple(egui::FontId::proportional(12.5), color));
        job.append(&label(name, input), 8.0, egui::TextFormat::simple(egui::FontId::proportional(12.5), c.text));
        egui::CollapsingHeader::new(job).id_salt(("septet-claude-tool", id)).show(ui, |ui| {
            let input_text = match name {
                "Write" => input["content"].as_str().map(str::to_owned),
                "Bash" => input["command"].as_str().map(str::to_owned),
                _ => None,
            }
            .unwrap_or_else(|| serde_json::to_string_pretty(input).unwrap_or_default());
            ui.add(egui::Label::new(RichText::new(clip(&input_text, 3000)).monospace().size(11.5).color(c.text_dim)).wrap());
            let Some((parts, _)) = result else { return };
            ui.separator();
            for (n, p) in parts.iter().enumerate() {
                match p {
                    Part::Text(t) => {
                        ui.add(egui::Label::new(RichText::new(clip(t, 3000)).monospace().size(11.5).color(c.text_dim)).wrap());
                    }
                    Part::Image { data, .. } => {
                        let key = format!("{id}/{n}");
                        if !thumbs.contains_key(&key)
                            && let Some(tex) = decode(ui.ctx(), &key, data)
                        {
                            thumbs.insert(key.clone(), tex);
                        }
                        if let Some(tex) = thumbs.get(&key) {
                            let size = tex.size_vec2();
                            let w = ui.available_width().min(size.x);
                            ui.image((tex.id(), vec2(w, size.y * w / size.x)));
                        }
                    }
                }
            }
        });
    });
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_owned() } else { s.chars().take(max).collect::<String>() + "\n…" }
}

fn decode(ctx: &Context, key: &str, b64: &str) -> Option<egui::TextureHandle> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(key, color, egui::TextureOptions::LINEAR))
}

fn approval_card(a: &mut Assistant, ui: &mut Ui) {
    let (c, _, _) = colors();
    let Some(conv) = a.conversation.as_mut() else { return };
    let Some(first) = conv.approvals.first() else { return };
    let (title, detail) = ask(&first.tool, &first.input);
    let why = first.input.get("description").and_then(Value::as_str).map(str::to_owned);
    let mut choice = None;
    egui::Frame::new().fill(c.card).stroke(Stroke::new(1.0, c.accent)).corner_radius(9.0).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).font(theme::semibold(13.5)).color(c.text));
        if let Some(why) = why {
            ui.label(RichText::new(why).size(12.0).color(c.text_dim));
        }
        if let Some(detail) = detail {
            egui::ScrollArea::vertical().id_salt("septet-claude-approval-detail").max_height(48.0).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(detail).monospace().size(11.5)).wrap());
            });
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Allow").color(Color32::BLACK)).fill(c.accent)).clicked() {
                choice = Some(Choice::Once);
            }
            if ui.button("Always in this chat").clicked() {
                choice = Some(Choice::Always);
            }
            if ui.button("Deny").clicked() {
                choice = Some(Choice::Deny);
            }
        });
    });
    if let Some(choice) = choice {
        conv.answer(0, choice);
    }
}

/// A small menu button: text and a chevron.
fn chip(ui: &mut Ui, text: &str, color: Color32, hover_bg: Color32) -> egui::Response {
    let font = egui::FontId::proportional(12.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = vec2(galley.size().x + 26.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    if resp.hovered() || resp.has_focus() {
        painter.rect_filled(rect, 6.0, hover_bg);
    }
    painter.galley(pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0), galley, color);
    let c = pos2(rect.right() - 10.0, rect.center().y);
    let s = Stroke::new(1.4, color);
    painter.line_segment([c + vec2(-3.5, -1.8), c + vec2(0.0, 1.8)], s);
    painter.line_segment([c + vec2(0.0, 1.8), c + vec2(3.5, -1.8)], s);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// The round button at the bottom right of the composer: send (arrow) or stop (square).
fn send_button(ui: &mut Ui, busy: bool, can_send: bool, c: &ShellColors) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), if busy || can_send { Sense::click() } else { Sense::hover() });
    let painter = ui.painter();
    let center = rect.center();
    if busy {
        painter.circle_filled(center, 14.0, if resp.hovered() { Color32::WHITE } else { c.text });
        painter.rect_filled(Rect::from_center_size(center, vec2(9.0, 9.0)), 2.0, c.home_bg);
        return resp.on_hover_text("Stop").on_hover_cursor(CursorIcon::PointingHand);
    }
    let (bg, fg) = if can_send {
        (if resp.hovered() { theme::lighten(c.accent, 0.12) } else { c.accent }, Color32::from_rgb(0x14, 0x14, 0x17))
    } else {
        (theme::mix(c.card, c.text_dim, 0.18), theme::mix(c.card, c.text_dim, 0.7))
    };
    painter.circle_filled(center, 14.0, bg);
    let s = Stroke::new(2.0, fg);
    painter.line_segment([center + vec2(0.0, 6.0), center + vec2(0.0, -6.0)], s);
    painter.line_segment([center + vec2(-5.0, -1.0), center + vec2(0.0, -6.0)], s);
    painter.line_segment([center + vec2(5.0, -1.0), center + vec2(0.0, -6.0)], s);
    if can_send { resp.on_hover_text("Send (Enter)").on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

/// The message field with model, effort and send at its bottom edge. Returns the height it would like.
fn composer_ui(a: &mut Assistant, ui: &mut Ui, ctx: &Context, viewport: ViewportId, busy: bool) -> f32 {
    let (c, _, _) = colors();
    let id = input_id(viewport);
    // Enter sends, Shift+Enter makes a new line.
    let focused = ctx.memory(|m| m.has_focus(id));
    let enter = focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    let rect = ui.max_rect();

    // A click anywhere on the box puts the cursor in the text.
    if ui.interact(rect, id.with("box"), Sense::click()).on_hover_cursor(CursorIcon::Text).clicked() {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let border = if focused { theme::mix(c.card_border, c.accent, 0.55) } else { c.card_border };
    ui.painter().rect(rect, 12.0, c.card, Stroke::new(1.0, border), egui::StrokeKind::Inside);

    let inner = rect.shrink2(vec2(12.0, 10.0));
    let bar = Rect::from_min_max(pos2(inner.left() - 4.0, inner.bottom() - 28.0), inner.max);
    let text = Rect::from_min_max(inner.min, pos2(inner.right(), bar.top() - 4.0));

    let mut tu = ui.new_child(egui::UiBuilder::new().max_rect(text).layout(Layout::top_down(Align::Min)));
    tu.set_clip_rect(text.expand(2.0));
    let out = egui::ScrollArea::vertical().id_salt(("septet-claude-input-scroll", viewport)).max_height(text.height()).auto_shrink([false, true]).show(
        &mut tu,
        |ui| {
            egui::TextEdit::multiline(&mut a.input)
                .id(id)
                .hint_text(RichText::new("Describe what to make or change…").color(theme::mix(c.card, c.text_dim, 0.75)))
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .desired_rows(1)
                .desired_width(text.width())
                .font(egui::FontId::proportional(13.5))
                .show(ui)
        },
    );
    let text_h = out.inner.galley.rect.height().max(18.0);

    let mut bu = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(Layout::left_to_right(Align::Center)));
    let hover_bg = theme::mix(c.card, Color32::WHITE, 0.06);
    let model = chip(&mut bu, super::model_name(a.settings.model.as_deref()), c.text_dim, hover_bg).on_hover_text("Model (always the newest of each)");
    egui::Popup::menu(&model).show(|ui| {
        for (m, name) in super::MODELS {
            if ui.selectable_label(a.settings.model.as_deref() == Some(*m), *name).clicked() {
                a.settings.model = Some((*m).to_owned());
                a.settings.save();
            }
        }
    });
    let effort =
        chip(&mut bu, &format!("{} effort", super::effort_name(a.settings.effort.as_deref())), c.text_dim, hover_bg).on_hover_text("How hard Claude thinks");
    egui::Popup::menu(&effort).show(|ui| {
        for (e, name) in super::EFFORTS {
            if ui.selectable_label(a.settings.effort.as_deref() == Some(*e), *name).clicked() {
                a.settings.effort = Some((*e).to_owned());
                a.settings.save();
            }
        }
    });
    bu.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let can = !a.input.trim().is_empty();
        let b = send_button(ui, busy, can, &c);
        if busy {
            if b.clicked()
                && let Some(conv) = a.conversation.as_mut()
            {
                conv.interrupt();
            }
        } else if (b.clicked() || enter) && can {
            let text = std::mem::take(&mut a.input);
            a.ask(ctx, text.trim());
            ctx.memory_mut(|m| m.request_focus(id));
        }
    });
    // Text (1 to 8 lines), the bar, the padding.
    text_h.min(8.0 * 18.0) + 4.0 + 28.0 + 20.0
}
