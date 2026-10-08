//! FilmCraft's own app icon (`assets/app-icon/`, MIT OR Apache-2.0) for the About dialog and the
//! Home screen, decoded once and kept as a texture.
//!
//! This modified version carries no ArtCraft marks (`docs/brand/README.md`): the ArtCraft wordmark
//! that used to be shown here was removed, as `docs/brand/LICENSE-brand.txt` requires.

const ICON: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/128x128/apps/ai.storyteller.filmcraft.png");

/// The app icon as a texture (square).
pub fn app_icon(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("filmcraft-app-icon");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let img = image::load_from_memory_with_format(ICON, image::ImageFormat::Png).ok()?.to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    let tex = ctx.load_texture("filmcraft-app-icon", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Draw the app icon `size` points square with its left edge at `left_center` (vertically
/// centred). Returns the drawn rect.
pub fn paint_app_icon(ui: &egui::Ui, left_center: egui::Pos2, size: f32) -> Option<egui::Rect> {
    let tex = app_icon(ui.ctx())?;
    let r = egui::Rect::from_min_size(egui::pos2(left_center.x, left_center.y - size / 2.0), egui::vec2(size, size));
    ui.painter().image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    Some(r)
}
