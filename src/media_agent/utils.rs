use opencv::{
    core::{AlgorithmHint, Mat, MatTraitConstManual, prelude::*},
    imgproc,
    videoio::{CAP_ANY, VideoCapture, VideoCaptureTraitConst},
};
use std::time::SystemTime;

pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

pub fn mat_to_color_image(mat: &Mat) -> Option<egui::ColorImage> {
    // Si la cámara no devolvió un frame válido
    if mat.empty() {
        return None;
    }

    // Convertimos BGR → RGBA
    let mut rgba = Mat::default();
    if let Err(e) = imgproc::cvt_color(
        mat,
        &mut rgba,
        imgproc::COLOR_BGR2RGBA,
        0,
        AlgorithmHint::ALGO_HINT_DEFAULT,
    ) {
        eprintln!("Color conversion failed: {:?}", e);
        return None;
    }

    // Obtenemos los bytes
    let size = rgba.size().ok()?;
    let width = size.width as usize;
    let height = size.height as usize;

    // Esto usa el trait MatTraitManual (ya implementado por Mat)
    let data = rgba.data_bytes().ok()?;

    Some(egui::ColorImage::from_rgba_unmultiplied(
        [width, height],
        data,
    ))
}

pub fn i420_to_rgb(yuv_bytes: &[u8], width: u32, height: u32) -> Vec<u8> {
    let frame_size = (width * height) as usize;
    let chroma_size = frame_size / 4;

    let y_plane = &yuv_bytes[..frame_size];
    let u_plane = &yuv_bytes[frame_size..frame_size + chroma_size];
    let v_plane = &yuv_bytes[frame_size + chroma_size..];

    let mut rgb = vec![0u8; frame_size * 3];

    for j in 0..height {
        for i in 0..width {
            let y = y_plane[(j * width + i) as usize] as f32;
            let u = u_plane[((j / 2) * (width / 2) + (i / 2)) as usize] as f32;
            let v = v_plane[((j / 2) * (width / 2) + (i / 2)) as usize] as f32;

            let r = (y + 1.402 * (v - 128.0)).clamp(0.0, 255.0);
            let g = (y - 0.344136 * (u - 128.0) - 0.714136 * (v - 128.0)).clamp(0.0, 255.0);
            let b = (y + 1.772 * (u - 128.0)).clamp(0.0, 255.0);

            let offset = ((j * width + i) * 3) as usize;
            rgb[offset] = r as u8;
            rgb[offset + 1] = g as u8;
            rgb[offset + 2] = b as u8;
        }
    }

    rgb
}
pub fn discover_camera_id() -> Option<i32> {
    for idx in 0..16 {
        if let Ok(cam) = VideoCapture::new(idx, CAP_ANY)
            && cam.is_opened().unwrap_or(false)
        {
            return Some(idx);
        }
    }
    None
}
