use eframe::egui;

use egui::{TextureHandle, TextureOptions};

/// Update (or create) the GPU texture for a camera frame.
/// IMPORTANT: pass a unique `unique_name` per stream (e.g., "camera/local", "camera/remote").
pub fn update_camera_texture(
    ctx: &egui::Context,
    frame: &crate::media_agent::video_frame::VideoFrame,
    handle: &mut Option<TextureHandle>,
    unique_name: &str,
) {
    // If your VideoFrame is RGBA8 already, this is fine.
    // If it's I420/NV12/etc., convert to RGBA8 beforehand.
    let w = frame.width as usize;
    let h = frame.height as usize;

    // SAFETY: assumes frame.bytes is RGBA (unmultiplied). Adjust if needed.
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &frame.bytes);

    match handle {
        Some(tex) => {
            // Cheapest path: reuse the same texture and just update pixels.
            // (If egui version complains on size change, recreate instead.)
            tex.set(image, TextureOptions::LINEAR);
        }
        None => {
            *handle = Some(ctx.load_texture(unique_name, image, TextureOptions::LINEAR));
        }
    }

    // Ensure UI refreshes even if there is no input event:
    ctx.request_repaint();
}

pub fn show_camera_in_ui(
    ui: &mut egui::Ui,
    texture: Option<&egui::TextureHandle>,
    want_w: f32,
    want_h: f32,
) {
    let desired = egui::Vec2::new(want_w, want_h);

    egui::Frame::none()
        .fill(egui::Color32::BLACK)
        .rounding(6.0)
        .show(ui, |ui| {
            ui.set_min_size(desired);

            if let Some(tex) = texture {
                // Fit image to the box while preserving aspect ratio
                #[allow(deprecated)]
                let sized = egui::load::SizedTexture {
                    id: tex.id(),
                    size: tex.size_vec2(),
                };
                let img = egui::Image::from_texture(sized).fit_to_exact_size(desired);
                ui.add(img);
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(desired.y * 0.4);
                    ui.label(egui::RichText::new("No video").color(egui::Color32::from_gray(140)));
                });
            }
        });
}
