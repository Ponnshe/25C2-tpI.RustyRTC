use crate::{
    app::log_sink::LogSink,
    camera_manager::{
        camera_error::CameraError, camera_manager_c::CameraManager, utils::tight_rgb_bytes,
    },
    logger_error, logger_warn,
    media_agent::{
        frame_format::FrameFormat,
        media_agent_error::{MediaAgentError, Result},
        utils::now_millis,
        video_frame::VideoFrame,
    }, sink_info,
};
use opencv::{core::Mat, imgproc};
use std::{
    sync::{
        Arc, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, Sender}
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub fn camera_loop(
    logger: Arc<dyn LogSink>,
    mut cam: CameraManager,
    tx: Sender<VideoFrame>,
    target_fps: u32,
    running: Arc<AtomicBool>
) -> Result<()> {
    let fps = target_fps.clamp(1, 120);
    let period = Duration::from_millis(1000 / fps as u64);
    let mut next_deadline = Instant::now() + period;

    while running.load(Ordering::SeqCst){
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
                    logger_warn!(
                        logger,
                        "Warning: camera did not return a valid frame: {}",
                        err
                    );
                    // Loggear y continuar, no detiene la app
                }
                CameraError::CameraOff | CameraError::InitializationFailed(_) => {
                    logger_error!(logger, "Critical camera error: {err}");
                    // Mostrar UI o intentar reinicializar la cámara
                    // opcional: intentar reinicializar
                    // cam.reinit()?;
                }
                CameraError::OpenCvError(e) => {
                    // Loggear y decidir si continuar o no
                    logger_error!(logger, "OpenCV error: {e}");
                }
                _ => {
                    logger_error!(logger, "Unexpected camera error: {err}");
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

pub fn synthetic_loop(
    logger: Arc<dyn LogSink>,
    tx: Sender<VideoFrame>,
    target_fps: u32,
    running: Arc<AtomicBool>
) -> Result<()> {
    let fps = target_fps.clamp(1, 120);
    let period = Duration::from_millis(1_000 / fps as u64);
    let mut phase = 0u8;
    while running.load(Ordering::SeqCst) {
        let frame = VideoFrame::synthetic(320, 240, phase);
        phase = phase.wrapping_add(1);
        if tx.send(frame).is_err() {
            logger_error!(logger, "[Synthethic Loop]: an error occured, exiting!");
            break;
        }
        thread::sleep(period);
    }
    Ok(())
}

pub fn spawn_camera_worker(
    target_fps: u32,
    logger: Arc<dyn LogSink>,
    camera_id: i32,
    running: Arc<AtomicBool>
) -> (Receiver<VideoFrame>, Option<String>, Option<JoinHandle<()>>) {
    sink_info!(
        logger,
        "[CameraWorker] Starting camera worker"
    );
    let (local_frame_tx, local_frame_rx) = mpsc::channel();
    let camera_manager = CameraManager::new(camera_id, logger.clone());

    let status = match &camera_manager {
        Ok(cam) => Some(format!(
            "Using camera source with resolution {}x{}",
            cam.width(),
            cam.height()
        )),
        Err(e) => Some(format!("Camera error: {}. Using test pattern.", e)),
    };

    let log_for_cam = logger.clone();
    let log_for_synthetic = logger.clone();

    let handle = thread::Builder::new()
        .name("media-agent-camera".into())
        .spawn(move || {
            if let Ok(cam) = camera_manager {
                if let Err(e) = camera_loop(log_for_cam, cam, local_frame_tx, target_fps, running.clone()) {
                    logger_error!(logger, "camera loop stopped: {e:?}");
                }
            } else {
                if let Err(e) = synthetic_loop(log_for_synthetic, local_frame_tx, target_fps, running.clone()) {
                    logger_error!(logger, "synthetic loop stopped: {e:?}");
                }
            }
        })
        .ok();

    (local_frame_rx, status, handle)
}
