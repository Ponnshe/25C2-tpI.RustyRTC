use crate::media_agent::video_frame::VideoFrame;
use eframe::egui;

pub fn update_camera_texture(
    ctx: &egui::Context,
    frame: &VideoFrame,
    tex_handle: &mut Option<egui::TextureHandle>,
) {
    // Convertir bytes a RGB
    let rgb_bytes = frame.bytes.clone();

    let image =
        egui::ColorImage::from_rgb([frame.width as usize, frame.height as usize], &rgb_bytes);

    if let Some(tex) = tex_handle {
        tex.set(image, Default::default());
    } else {
        *tex_handle = Some(ctx.load_texture("camera", image, Default::default()));
    }
}

pub fn show_camera_in_ui(
    ui: &mut egui::Ui,
    tex_handle: &Option<egui::TextureHandle>,
    width: f32,
    height: f32,
) {
    if let Some(tex) = tex_handle {
        let size = tex.size_vec2();
        let aspect_ratio = size.x / size.y;
        ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(width, width / aspect_ratio)));
    } else {
        // Placeholder si no hay textura
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
    }
}
