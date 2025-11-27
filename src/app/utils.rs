use eframe::egui;

pub fn show_camera_in_ui(
    ui: &mut egui::Ui,
    texture: Option<(egui::TextureId, (u32, u32))>,
    max_w: f32,
    max_h: f32,
) {
    if let Some((texture_id, (w, h))) = texture {
        let tex_size = egui::vec2(w as f32, h as f32);
        let mut img_size = tex_size;
        if tex_size.x > max_w {
            img_size.y = max_w * tex_size.y / tex_size.x;
            img_size.x = max_w;
        }
        if img_size.y > max_h {
            img_size.x = max_h * img_size.x / img_size.y;
            img_size.y = max_h;
        }
        ui.image(egui::ImageSource::Texture(egui::load::SizedTexture::new(texture_id, img_size)));
    } else {
        let size = egui::vec2(max_w.min(128.0), max_h.min(128.0));
        ui.allocate_ui(size, |ui| {
            ui.visuals_mut().override_text_color = Some(egui::Color32::DARK_GRAY);
            ui.centered_and_justified(|ui| {
                ui.label("(no camera)");
            });
        });
    }
}
