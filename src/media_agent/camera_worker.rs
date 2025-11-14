use crate::{
    camera_manager::{
        camera_manager_c::CameraManager,
        camera_error::CameraError,
        utils::tight_rgb_bytes,
    },
    media_agent::{
        video_frame::VideoFrame,
        utils::now_millis,
        media_agent_error::{MediaAgentError, Result},
        frame_format::FrameFormat,
    },
};
use std::{sync::{mpsc::Sender, Arc}, thread, time::{Duration, Instant}};
use opencv::{core::Mat, imgproc};

pub fn camera_loop(
    mut cam: CameraManager,
    tx: Sender<VideoFrame>,
    target_fps: u32,
) -> Result<()> {
    let fps = target_fps.clamp(1, 120);
    let period = Duration::from_millis(1000 / fps as u64);
    let mut next_deadline = Instant::now() + period;

    loop {
        match cam.get_frame() {
            Ok(frame) => {
                let w = cam.width();
                let h = cam.height();
                let vf = convert_to_videoframe(&frame, w, h)?;
                if tx.send(vf).is_err() {
                    break;
                }
            }
            Err(err) => match err {
                CameraError::NotFrame | CameraError::CaptureFailed(_) => {
                    // Loggear y continuar, no detiene la app
                    eprintln!("Warning: camera did not return a valid frame: {}", err);
                }
                CameraError::CameraOff | CameraError::InitializationFailed(_) => {
                    // Mostrar UI o intentar reinicializar la cámara
                    eprintln!("Critical camera error: {}", err);
                    // opcional: intentar reinicializar
                    // cam.reinit()?;
                }
                CameraError::OpenCvError(e) => {
                    // Loggear y decidir si continuar o no
                    eprintln!("OpenCV error: {}", e);
                }
                _ => {
                    eprintln!("Unexpected camera error: {}", err);
                }
            },
        }

        let now = Instant::now();
        if now < next_deadline {
            thread::sleep(next_deadline - now);
            next_deadline += period;
        } else {
            next_deadline = now + period;
        }
    }

    Ok(())
}

fn convert_to_videoframe(mat: &Mat, w: u32, h: u32) -> Result<VideoFrame> {

    let mut rgb_mat = Mat::default();

    imgproc::cvt_color(
        mat, 
        &mut rgb_mat, 
        imgproc::COLOR_BGR2RGB, 
        0,
        opencv::core::AlgorithmHint::ALGO_HINT_DEFAULT,
        )
        .map_err(|e| MediaAgentError::Io(format!("cvtColor: {e}")))?;
    let bytes = tight_rgb_bytes(&rgb_mat, w, h)
                .map_err(|e| MediaAgentError::Io(format!("pack RGB: {e}")))?;

    Ok(VideoFrame {
        width: w,
        height: h,
        timestamp_ms: now_millis(),
        format: FrameFormat::Rgb,
        bytes: Arc::new(bytes),
    })
}

pub fn synthetic_loop(tx: Sender<VideoFrame>, target_fps: u32) -> Result<()> {
    let fps = target_fps.clamp(1, 120);
    let period = Duration::from_millis(1_000 / fps as u64);
    let mut phase = 0u8;
    loop {
        let frame = VideoFrame::synthetic(320, 240, phase);
        phase = phase.wrapping_add(1);
        if tx.send(frame).is_err() {
            break;
        }
        thread::sleep(period);
    }
    Ok(())
}
