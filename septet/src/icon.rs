//! Septet's mark: the window icon and the logo in each tab strip.

const SVG: &[u8] = include_bytes!("../assets/septet.svg");

pub fn source() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/septet.svg")
}

pub fn paint_mark(ui: &egui::Ui, rect: egui::Rect) {
    egui::Image::new(source()).fit_to_exact_size(rect.size()).paint_at(ui, rect);
}

/// The mark rasterized for the window manager.
pub fn window_icon() -> egui::IconData {
    const SIZE: u32 = 128;
    let render = || -> Option<egui::IconData> {
        let tree = resvg::usvg::Tree::from_data(SVG, &resvg::usvg::Options::default()).ok()?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(SIZE, SIZE)?;
        let scale = SIZE as f32 / tree.size().width();
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        // tiny-skia stores premultiplied alpha; window icons want it straight.
        let rgba = pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        Some(egui::IconData { rgba, width: SIZE, height: SIZE })
    };
    render().unwrap_or_default()
}
