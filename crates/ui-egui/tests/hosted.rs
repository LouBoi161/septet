//! PdfCraft as a tab of a host window (`pdfcraft_ui_egui::hosted`): pages dragged out to another
//! app, and Send to. A test binary of its own, because being hosted is process-wide.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_render::{PageRenderer, RenderRequest, RequestKind};
use pdfcraft_ui_egui::PdfCraftApp;

/// An `n`-page document; page `i` shows "Page i+1".
fn fixture(n: usize) -> Vec<u8> {
    let mut objs: Vec<String> = vec!["<< /Type /Catalog /Pages 2 0 R >>".into()];
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 200 300] >>", kids.join(" ")));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    for i in 0..n {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>", 5 + 2 * i));
        let body = format!("BT /F1 24 Tf 20 150 Td (Page {}) Tj ET", i + 1);
        objs.push(format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

fn harness(options: &'static [(&'static str, &'static str)]) -> Harness<'static, PdfCraftApp> {
    pdfcraft_ui_egui::hosted::set_hosted(true);
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |cc| {
        // The host installs the fonts, PdfCraft's among them.
        cc.egui_ctx.set_fonts(pdfcraft_ui_egui::theme::installed_font_definitions(false));
        let mut app = PdfCraftApp::new();
        app.open_bytes("doc.pdf", None, fixture(4)).expect("fixture opens");
        for (k, v) in options {
            app.set_option(k, v).unwrap();
        }
        app
    });
    h.run_steps(4);
    h
}

fn texts(bytes: Vec<u8>) -> Vec<String> {
    let mut r = PageRenderer::new(std::sync::Arc::new(bytes), Default::default());
    (0..r.page_count())
        .map(|p| {
            r.render(RenderRequest { page: p, kind: RequestKind::Text, scale: 1.0, ..Default::default() })
                .text
                .unwrap()
                .plain_text()
                .trim()
                .to_string()
        })
        .collect()
}

fn doc_texts(h: &Harness<'static, PdfCraftApp>) -> Vec<String> {
    let s = h.state();
    texts(s.session.get(s.views[0].id).unwrap().bytes.to_vec())
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-hosted-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Press on `from`, drag a little, then on to `to` (one frame per step).
fn drag(h: &mut Harness<'static, PdfCraftApp>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run_steps(1);
    }
}

#[test]
fn pages_dragged_out_of_the_grid_go_to_the_other_app_and_stay_put() {
    let mut h = harness(&[("organize", "on")]);
    let dir = temp_dir("grid");
    let before = doc_texts(&h);
    let from = h.get_by_label("Page 2").rect().center();
    // Above the grid: the host's tab strip would be up there.
    let outside = egui::pos2(from.x, 4.0);
    drag(&mut h, from, outside);
    assert_eq!(h.state().outgoing_drag().as_deref(), Some("Page 2"));
    h.drop_at(outside);
    h.run_steps(1);
    // The host takes the pages on the frame they are let go, as a PDF for an app that places PDFs.
    let files = h.state_mut().take_outgoing_files(&["svg", "pdf", "png"], &dir);
    h.state_mut().cancel_outgoing_drag();
    assert_eq!(files, [dir.join("doc (page 2).pdf")]);
    assert_eq!(texts(std::fs::read(&files[0]).unwrap()), ["Page 2"]);
    h.run_steps(3);
    assert_eq!(doc_texts(&h), before, "nothing moved");
    assert!(h.state().session.get(h.state().views[0].id).unwrap().can_undo().is_none());
    assert_eq!(h.state().outgoing_drag(), None);
    // Several selected pages, to an app that only takes images: one PNG each. Taken while the
    // drag is still on (the host may have shown the other app's tab meanwhile), then let go
    // over the grid after the host cancelled it: no move either.
    h.state_mut().views[0].select_pages(&[0, 2]);
    h.run_steps(1);
    let from = h.get_by_label("Page 1").rect().center();
    let to = h.get_by_label("Page 4").rect().center();
    drag(&mut h, from, to);
    assert_eq!(h.state().outgoing_drag().as_deref(), Some("2 pages"));
    let files = h.state_mut().take_outgoing_files(&["psd", "png", "jpg"], &dir);
    h.state_mut().cancel_outgoing_drag();
    assert_eq!(files, [dir.join("doc (page 1).png"), dir.join("doc (page 3).png")]);
    assert!(std::fs::read(&files[0]).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
    h.drop_at(to);
    h.run_steps(3);
    assert_eq!(doc_texts(&h), before, "the cancelled drag moved nothing");
    // Dragging within the grid still reorders, and is no drag out.
    h.state_mut().views[0].select_pages(&[]);
    h.run_steps(1);
    let from = h.get_by_label("Page 1").rect().center();
    let to = h.get_by_label("Page 3").rect().right_center() - egui::vec2(10.0, 0.0);
    drag(&mut h, from, to);
    h.drop_at(to);
    h.run_steps(1);
    assert!(h.state_mut().take_outgoing_files(&["pdf"], &dir).is_empty());
    h.run_steps(3);
    assert_eq!(doc_texts(&h), [before[1].clone(), before[2].clone(), before[0].clone(), before[3].clone()]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn send_to_writes_the_current_page() {
    let mut h = harness(&[("page", "3")]);
    let dir = temp_dir("send");
    let pdf = h.state_mut().send_current_page(&["svg", "pdf"], &dir).unwrap();
    assert_eq!(pdf, dir.join("doc (page 3).pdf"));
    assert_eq!(texts(std::fs::read(&pdf).unwrap()), ["Page 3"]);
    // Again: a new name, the first file stays as it was.
    assert_eq!(h.state_mut().send_current_page(&["pdf"], &dir), Some(dir.join("doc (page 3) 2.pdf")));
    let jpg = h.state_mut().send_current_page(&["mp4", "jpeg", "png"], &dir).unwrap();
    assert_eq!(jpg, dir.join("doc (page 3).jpeg"));
    assert!(std::fs::read(&jpg).unwrap().starts_with(&[0xFF, 0xD8]));
    assert_eq!(h.state_mut().send_current_page(&["mp4", "wav"], &dir), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn thumbnails_in_the_pages_panel_drag_out() {
    let mut h = harness(&[("panel", "pages")]);
    let dir = temp_dir("panel");
    let from = h.get_by_label("Page 2").rect().center();
    let outside = egui::pos2(from.x - 600.0, from.y);
    drag(&mut h, from, outside);
    assert_eq!(h.state().outgoing_drag().as_deref(), Some("Page 2"));
    h.drop_at(outside);
    h.run_steps(1);
    let files = h.state_mut().take_outgoing_files(&["png"], &dir);
    assert_eq!(files, [dir.join("doc (page 2).png")]);
    // Gone with the next press, and a drag let go over the panel itself goes nowhere.
    h.drag_at(outside);
    h.drop_at(outside);
    h.run_steps(1);
    assert!(h.state_mut().take_outgoing_files(&["png"], &dir).is_empty());
    drag(&mut h, from, from + egui::vec2(0.0, -40.0));
    h.drop_at(from + egui::vec2(0.0, -40.0));
    h.run_steps(1);
    assert!(h.state_mut().take_outgoing_files(&["png"], &dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_host_build_leaves_out_the_community_links() {
    pdfcraft_ui_egui::hosted::set_hosted(true);
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|cc| {
        cc.egui_ctx.set_fonts(pdfcraft_ui_egui::theme::installed_font_definitions(false));
        PdfCraftApp::new()
    });
    h.run_steps(4);
    // Home: no community card, no Discord button; PdfCraft itself is still welcome.
    assert!(h.query_by_label_contains("Welcome to PdfCraft").is_some());
    for gone in ["Join the community", "Discord", "Join our Discord", "ArtCraft"] {
        assert!(h.query_by_label(gone).is_none(), "{gone} shows");
    }
    // About: the credit in plain text and PdfCraft's own links, not the community's.
    h.state_mut().set_option("dialog", "about").unwrap();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Based on PdfCraft by the ArtCraft team").is_some());
    assert!(h.query_by_label("PdfCraft on GitHub").is_some());
    for gone in ["ArtCraft", "Original authors' website", "Join our Discord"] {
        assert!(h.query_by_label(gone).is_none(), "{gone} shows in About");
    }
}

#[test]
fn a_data_root_holds_the_per_user_folders() {
    let root = temp_dir("root").join("Data").join("PdfCraft");
    pdfcraft_ui_egui::hosted::set_data_root(Some(root.clone()));
    assert!(root.is_dir(), "created");
    assert_eq!(pdfcraft_ui_egui::RecoveryStore::default_dir(), Some(root.join("Recovery")));
    pdfcraft_ui_egui::hosted::set_data_root(None);
    assert_ne!(pdfcraft_ui_egui::RecoveryStore::default_dir(), Some(root.join("Recovery")));
    let _ = std::fs::remove_dir_all(root.parent().unwrap().parent().unwrap());
}
