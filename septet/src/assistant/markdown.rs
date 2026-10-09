//! Just enough Markdown for Claude's replies: paragraphs, headings, lists, code blocks, `code` and **bold**.

use egui::text::LayoutJob;
use egui::{Color32, FontId, RichText, TextFormat, Ui};

pub struct Style {
    pub text: Color32,
    pub dim: Color32,
    pub code_bg: Color32,
    pub size: f32,
}

pub fn show(ui: &mut Ui, md: &str, s: &Style) {
    let mut lines = md.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(lang) = trimmed.strip_prefix("```") {
            let _ = lang;
            let mut code = String::new();
            for l in lines.by_ref() {
                if l.trim_start().starts_with("```") {
                    break;
                }
                code.push_str(l);
                code.push('\n');
            }
            egui::Frame::new().fill(s.code_bg).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(RichText::new(code.trim_end()).monospace().size(s.size - 1.0).color(s.text)).wrap());
            });
            ui.add_space(4.0);
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        let heading = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=4).contains(&heading) && trimmed[heading..].starts_with(' ') {
            let size = s.size + (5 - heading) as f32 * 1.5;
            ui.add_space(2.0);
            ui.add(egui::Label::new(inline(&trimmed[heading + 1..], s, size, true)).wrap());
            continue;
        }
        let bullet = ["- ", "* ", "+ "].iter().find_map(|b| trimmed.strip_prefix(b));
        let numbered = trimmed.split_once(". ").filter(|(n, _)| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit()));
        if let Some(rest) = bullet.map(|r| ("•".to_owned(), r)).or_else(|| numbered.map(|(n, r)| (format!("{n}."), r))) {
            let indent = (line.len() - trimmed.len()) as f32 * 4.0;
            ui.horizontal_top(|ui| {
                ui.add_space(4.0 + indent);
                ui.label(RichText::new(rest.0).size(s.size).color(s.dim));
                ui.add(egui::Label::new(inline(rest.1, s, s.size, false)).wrap());
            });
            continue;
        }
        // A paragraph: following plain lines join it.
        let mut para = trimmed.to_owned();
        while let Some(next) = lines.peek() {
            let t = next.trim_start();
            if t.is_empty() || t.starts_with("```") || t.starts_with('#') || t.starts_with("- ") || t.starts_with("* ") {
                break;
            }
            para.push(' ');
            para.push_str(t);
            lines.next();
        }
        ui.add(egui::Label::new(inline(&para, s, s.size, false)).wrap());
    }
}

/// `code`, **bold** and *italic* inside a line.
fn inline(text: &str, s: &Style, size: f32, bold: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let plain =
        |bold: bool| TextFormat { font_id: if bold { crate::theme::semibold(size) } else { FontId::proportional(size) }, color: s.text, ..Default::default() };
    let mut rest = text;
    let mut strong = bold;
    let mut italic = false;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('`')
            && let Some(end) = r.find('`')
        {
            let fmt = TextFormat { font_id: FontId::monospace(size - 1.0), color: s.text, background: s.code_bg, ..Default::default() };
            job.append(&r[..end], 0.0, fmt);
            rest = &r[end + 1..];
            continue;
        }
        if let Some(r) = rest.strip_prefix("**") {
            strong = !strong;
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix('*').filter(|r| !r.starts_with(' ')) {
            italic = !italic;
            rest = r;
            continue;
        }
        let end = rest.find(['`', '*']).filter(|&i| i > 0).unwrap_or(rest.len());
        let mut fmt = plain(strong);
        fmt.italics = italic;
        job.append(&rest[..end], 0.0, fmt);
        rest = &rest[end..];
    }
    job
}
