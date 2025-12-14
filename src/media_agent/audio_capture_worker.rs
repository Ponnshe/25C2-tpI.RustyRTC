use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc::Sender};
use std::thread;
use std::time::Duration;
use crate::log::log_sink::LogSink;
use crate::media_agent::{
    audio_frame::AudioFrame,
    audio_capture_error::AudioCaptureError,
    media_agent_error::{MediaAgentError, Result},
    utils::now_millis
};
use crate::{sink_debug, sink_error, sink_info, sink_warn};

/// Event sent by the AudioCaptureWorker.
#[derive(Debug)]
pub enum AudioCaptureEvent {
    Frame(AudioFrame),
    Error(AudioCaptureError),
}

/// Spawns the audio capture worker.
pub fn spawn_audio_capture_worker(
    logger: Arc<dyn LogSink>,
    running: Arc<AtomicBool>,
) -> (std::sync::mpsc::Receiver<AudioCaptureEvent>, Option<thread::JoinHandle<()>>) {
    let (tx, rx) = std::sync::mpsc::channel();
    
    let handle = thread::Builder::new()
        .name("media-agent-audio-capture".into())
        .spawn(move || {
            if let Err(e) = run_audio_capture(logger.clone(), tx.clone(), running) {
                sink_error!(logger, "[AudioCaptureWorker] Error: {}", e);
                let _ = tx.send(AudioCaptureEvent::Error(AudioCaptureError::Runtime(e.to_string())));
            }
        })
        .ok();
        
    (rx, handle)
}

fn run_audio_capture(
    logger: Arc<dyn LogSink>,
    tx: Sender<AudioCaptureEvent>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host.default_input_device().expect("Failed to get default input device");
    
    sink_info!(logger, "[AudioCaptureWorker] Using audio device: {}", device.name().unwrap_or_default());

    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(48000),
        buffer_size: cpal::BufferSize::Default,
    };

    let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(1920)));
    let buffer_clone = buffer.clone();
    
    let logger_clone = logger.clone();
    let tx_err = tx.clone();
    let tx_data = tx.clone();
    
    let err_fn = move |err: cpal::StreamError| {
        sink_warn!(logger_clone, "[AudioCaptureWorker] Stream error: {}", err);
        let _ = tx_err.send(AudioCaptureEvent::Error(AudioCaptureError::StreamPlay(err.to_string())));
    };

    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mut buf = buffer_clone.lock().expect("Failed to lock audio buffer");
            buf.extend(data.iter().cloned());
            
            while buf.len() >= 960 {
                let chunk: Vec<f32> = buf.drain(0..960).collect();
                let frame = AudioFrame {
                    data: Arc::new(chunk),
                    samples: 960,
                    sample_rate: 48000,
                    channels: 1,
                    timestamp_ms: now_millis(),
                };
                
                if let Err(_) = tx_data.send(AudioCaptureEvent::Frame(frame)) {
                    // Receiver disconnected
                }
            }
        },
        err_fn,
        None
    ).map_err(|e| MediaAgentError::Io(format!("Failed to build input stream: {}", e)))?;

    stream.play().map_err(|e| MediaAgentError::Io(format!("Failed to play stream: {}", e)))?;
    
    sink_debug!(logger, "[AudioCaptureWorker] Audio capture started");

    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }
    
    Ok(())
}