use crate::{
    app::gpu_yuv_renderer::GpuYuvRenderer, log::log_sink::LogSink,
    media_agent::video_frame::VideoFrame,
};
use std::sync::Arc;

use eframe::{egui, egui_wgpu::RenderState};

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
        ui.image(egui::ImageSource::Texture(egui::load::SizedTexture::new(
            texture_id, img_size,
        )));
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

pub fn update_rgb_texture(
    ctx: &egui::Context,
    texture: &mut Option<(egui::TextureId, (u32, u32))>,
    width: u32,
    height: u32,
    rgb: &[u8],
    unique_name: &str,
) {
    let image = egui::ColorImage::from_rgb([width as usize, height as usize], rgb);
    let options = egui::TextureOptions::LINEAR;
    let tex_mngr = ctx.tex_manager();

    if let Some((id, (prev_w, prev_h))) = texture {
        if *prev_w != width || *prev_h != height {
            tex_mngr.write().free(*id);
            let new_id = tex_mngr
                .write()
                .alloc(unique_name.to_owned(), image.into(), options);
            *texture = Some((new_id, (width, height)));
        } else {
            let delta = egui::epaint::ImageDelta {
                image: egui::epaint::ImageData::Color(image.into()),
                options,
                pos: None,
            };
            tex_mngr.write().set(*id, delta);
        }
    } else {
        let new_id = tex_mngr
            .write()
            .alloc(unique_name.to_owned(), image.into(), options);
        *texture = Some((new_id, (width, height)));
    }
}

pub fn update_yuv_texture(
    frame: &VideoFrame,
    texture: &mut Option<(egui::TextureId, (u32, u32))>,
    yuv_renderer: &mut Option<GpuYuvRenderer>,
    render_state: Option<&RenderState>,
    logger: Arc<dyn LogSink>,
) {
    let (Some(renderer), Some(rs)) = (yuv_renderer, render_state) else {
        return;
    };

    renderer.update_frame(&rs.device, &rs.queue, frame, logger.clone());

    let Some(output_texture) = renderer.output_texture() else {
        return;
    };

    let desc = eframe::wgpu::TextureViewDescriptor::default();
    let view = output_texture.create_view(&desc);
    let filter = eframe::wgpu::FilterMode::Nearest;

    let mut wgpu_renderer = rs.renderer.write();

    if let Some((id, (pw, ph))) = texture {
        if *pw != frame.width || *ph != frame.height {
            wgpu_renderer.free_texture(id);
            let new_id = wgpu_renderer.register_native_texture(&rs.device, &view, filter);
            *texture = Some((new_id, (frame.width, frame.height)));
        } else {
            wgpu_renderer.update_egui_texture_from_wgpu_texture(&rs.device, &view, filter, *id);
        }
    } else {
        let new_id = wgpu_renderer.register_native_texture(&rs.device, &view, filter);
        drop(wgpu_renderer);
        *texture = Some((new_id, (frame.width, frame.height)));
    }
}
