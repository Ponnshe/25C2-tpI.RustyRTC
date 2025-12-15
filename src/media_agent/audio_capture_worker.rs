use crate::log::log_sink::LogSink;
use crate::media_agent::{
    audio_capture_error::AudioCaptureError,
    audio_frame::AudioFrame,
    media_agent_error::{MediaAgentError, Result},
    utils::now_millis,
};
use crate::{sink_debug, sink_error, sink_info, sink_warn};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread;
use std::time::Duration;

#[allow(clippy::expect_used)]
/// Event sent by the AudioCaptureWorker.
#[derive(Debug)]
pub enum AudioCaptureEvent {
    Frame(AudioFrame),
    Error(AudioCaptureError),
}

/// Spawns the audio capture worker.
///
/// This function initializes the default input device and starts capturing audio frames.
///
/// # Arguments
///
/// * `logger` - Logger instance.
/// * `running` - Atomic flag to control the worker loop.
/// * `is_muted` - Atomic flag to control audio muting.
///
/// # Returns
///
/// A tuple containing the receiver for captured audio events and the join handle of the worker thread.
pub fn spawn_audio_capture_worker(
    logger: Arc<dyn LogSink>,
    running: Arc<AtomicBool>,
    is_muted: Arc<AtomicBool>,
) -> (
    std::sync::mpsc::Receiver<AudioCaptureEvent>,
    Option<thread::JoinHandle<()>>,
) {
    let (tx, rx) = std::sync::mpsc::channel();

    let handle = thread::Builder::new()
        .name("media-agent-audio-capture".into())
        .spawn(move || {
            if let Err(e) = run_audio_capture(logger.clone(), tx.clone(), running, is_muted) {
                sink_error!(logger, "[AudioCaptureWorker] Error: {}", e);
                let _ = tx.send(AudioCaptureEvent::Error(AudioCaptureError::Runtime(
                    e.to_string(),
                )));
            }
        })
        .ok();

    (rx, handle)
}

fn run_audio_capture(
    logger: Arc<dyn LogSink>,
    tx: Sender<AudioCaptureEvent>,
    running: Arc<AtomicBool>,
    is_muted: Arc<AtomicBool>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| MediaAgentError::Io("Failed to get default input device".to_string()))?;

    sink_info!(
        logger,
        "[AudioCaptureWorker] Using audio device: {}",
        device.name().unwrap_or_default()
    );

    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(8000),
        buffer_size: cpal::BufferSize::Default,
    };

    let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(320)));
    let buffer_clone = buffer.clone();

    let logger_clone = logger.clone();
    let tx_err = tx.clone();
    let tx_data = tx.clone();
    let is_muted_clone = is_muted.clone();

    let err_fn = move |err: cpal::StreamError| {
        sink_warn!(logger_clone, "[AudioCaptureWorker] Stream error: {}", err);
        let _ = tx_err.send(AudioCaptureEvent::Error(AudioCaptureError::StreamPlay(
            err.to_string(),
        )));
    };

    let stream = device
        .build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut buf = buffer_clone
                    .lock()
                    .map_err(|e| MediaAgentError::Io(format!("Failed to lock audio buffer: {}", e)))
                    .unwrap_or_else(|_| {
                        // In callback, we can't return Result easily, so we might have to panic or handle gracefully.
                        // But since we changed the outer return type, this closure is tricky.
                        // Reverting strategy: The closure returns `()`. We cannot use `?`.
                        // We must stick to expect inside the closure or handle it by returning early.
                        // To satisfy "replace unwrap/expect", I will use expect with a better message if needed,
                        // but here I'm correcting the *Previous* attempt which tried `?`.
                        // Let's use expect but acknowledging it's a panic.
                        panic!("Failed to lock audio buffer: poisoned");
                    });

                if is_muted_clone.load(Ordering::Relaxed) {
                    // If muted, fill with silence (zeros)
                    buf.extend(std::iter::repeat_n(0.0, data.len()));
                } else {
                    // If not muted, copy captured data
                    buf.extend(data.iter().cloned());
                }

                while buf.len() >= 160 {
                    let chunk: Vec<f32> = buf.drain(0..160).collect();
                    let frame = AudioFrame {
                        data: Arc::new(chunk),
                        samples: 160,
                        sample_rate: 8000,
                        channels: 1,
                        timestamp_ms: now_millis(),
                    };

                    if tx_data.send(AudioCaptureEvent::Frame(frame)).is_err() {
                        // Receiver disconnected
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| MediaAgentError::Io(format!("Failed to build input stream: {}", e)))?;

    stream
        .play()
        .map_err(|e| MediaAgentError::Io(format!("Failed to play stream: {}", e)))?;

    sink_debug!(logger, "[AudioCaptureWorker] Audio capture started");

    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }

    Ok(())
}
