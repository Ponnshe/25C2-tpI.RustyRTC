use crate::{
    camera_manager::{
        camera_error::CameraError, camera_manager_c::CameraManager, utils::tight_rgb_bytes,
    },
    log::log_sink::LogSink,
    logger_error, logger_warn,
    media_agent::{
        frame_format::FrameFormat,
        media_agent_error::{MediaAgentError, Result},
        utils::now_millis,
        video_frame::VideoFrame,
    },
    sink_info,
};
use opencv::{core::Mat, imgproc};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Runs the main capture loop for a physical camera device.
///
/// This function continuously captures frames from the provided `CameraManager`,
/// converts them to the internal `VideoFrame` format, and sends them through the channel.
/// It enforces the specified `target_fps` by sleeping the thread between captures.
///
/// # error handling
///
/// * Non-critical errors (e.g., `NotFrame`, `CaptureFailed`) are logged as warnings,
///   and the loop continues.
/// * Critical errors (e.g., `CameraOff`) are logged as errors.
/// * Conversion errors propagate and will terminate the loop.
///
/// # Errors
///
/// Returns a [`MediaAgentError`] if:
/// * The frame conversion from OpenCV BGR to internal RGB fails.
/// * Any underlying OpenCV operation returns a critical failure that cannot be handled gracefully.
pub fn camera_loop(
    logger: Arc<dyn LogSink>,
    mut cam: CameraManager,
    tx: Sender<VideoFrame>,
    target_fps: u32,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let fps = target_fps.clamp(1, 120);
    let period = Duration::from_millis(1000 / fps as u64);
    let mut next_deadline = Instant::now() + period;

    while running.load(Ordering::SeqCst) {
        match cam.get_frame() {
            Ok(frame) => {
                let w = cam.width();
                let h = cam.height();
                // Propagates conversion errors immediately
                let vf = convert_to_videoframe(&frame, w, h)?;

                // If the receiver hangs up, we exit the loop gracefully
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
                    // Log and continue; do not stop the app.
                }
                CameraError::CameraOff | CameraError::InitializationFailed(_) => {
                    logger_error!(logger, "Critical camera error: {err}");
                    // Potential recovery logic:
                    // cam.reinit()?;
                }
                CameraError::OpenCvError(e) => {
                    logger_error!(logger, "OpenCV error: {e}");
                }
                _ => {
                    logger_error!(logger, "Unexpected camera error: {err}");
                }
            },
        }

        // Enforce frame pacing
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

/// Helper function to convert an OpenCV `Mat` (BGR) to a `VideoFrame` (RGB).
///
/// # Errors
///
/// Returns `MediaAgentError::Io` if:
/// * `imgproc::cvt_color` fails (e.g., invalid input dimensions or types).
/// * The resulting RGB bytes cannot be tightly packed into the expected buffer size.
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
        data: crate::media_agent::video_frame::VideoFrameData::Rgb(Arc::new(bytes)),
    })
}

/// Runs a synthetic video loop generating generated test patterns.
///
/// Used as a fallback when the physical camera fails to initialize or is not available.
/// Generates a moving RGB pattern.
///
/// # Errors
///
/// Returns `Ok(())` upon successful completion (when `running` becomes false).
/// Logs an error and exits (returning `Ok(())`) if the channel receiver disconnects.
pub fn synthetic_loop(
    logger: Arc<dyn LogSink>,
    tx: Sender<VideoFrame>,
    target_fps: u32,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let fps = target_fps.clamp(1, 120);
    let period = Duration::from_millis(1_000 / fps as u64);
    let mut phase = 0u8;

    while running.load(Ordering::SeqCst) {
        let frame = VideoFrame::synthetic_rgb(320, 240, phase);
        phase = phase.wrapping_add(1);

        if tx.send(frame).is_err() {
            logger_error!(logger, "[Synthethic Loop]: an error occured, exiting!");
            break;
        }
        thread::sleep(period);
    }
    Ok(())
}

/// Initializes and spawns the camera background worker.
///
/// Tries to open the physical camera specified by `camera_id`. If successful, spawns
/// a thread running [`camera_loop`]. If the camera fails to open, it falls back to
/// spawning a thread running [`synthetic_loop`].
///
/// # Arguments
///
/// * `target_fps` - Desired frame rate.
/// * `logger` - Logger instance.
/// * `camera_id` - OpenCV camera index (usually 0 for default webcam).
/// * `running` - Atomic flag to control the worker's lifecycle.
///
/// # Returns
///
/// A tuple containing:
/// 1. `Receiver<VideoFrame>`: The channel to receive video frames.
/// 2. `Option<String>`: A status message describing the initialized source (Camera resolution or Error).
/// 3. `Option<JoinHandle<()>>`: The handle to the spawned background thread.
pub fn spawn_camera_worker(
    target_fps: u32,
    logger: Arc<dyn LogSink>,
    camera_id: i32,
    running: Arc<AtomicBool>,
) -> (Receiver<VideoFrame>, Option<String>, Option<JoinHandle<()>>) {
    sink_info!(logger, "[CameraWorker] Starting camera worker");
    let (local_frame_tx, local_frame_rx) = mpsc::channel();

    // Attempt to initialize physical hardware
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
            // Select strategy based on initialization success
            if let Ok(cam) = camera_manager {
                if let Err(e) = camera_loop(
                    log_for_cam,
                    cam,
                    local_frame_tx,
                    target_fps,
                    running.clone(),
                ) {
                    logger_error!(logger, "camera loop stopped: {e:?}");
                }
            } else if let Err(e) = synthetic_loop(
                log_for_synthetic,
                local_frame_tx,
                target_fps,
                running.clone(),
            ) {
                logger_error!(logger, "synthetic loop stopped: {e:?}");
            }
        })
        .ok();

    (local_frame_rx, status, handle)
}
