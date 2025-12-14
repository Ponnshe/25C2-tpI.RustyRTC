use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{
    log::log_sink::LogSink,
    sink_debug, sink_error, sink_info, sink_trace, sink_warn,
};

/// Commands sent from the MediaAgent to the AudioPlayerWorker.
pub enum AudioPlayerCommand {
    /// Play a chunk of decoded audio samples.
    PlayFrame(Vec<f32>),
}

/// Max buffer size in samples before dropping data to reduce latency.
/// 48kHz * 0.5s = 24000 samples.
const MAX_BUFFER_SIZE: usize = 24000;

/// Spawns the audio player worker.
///
/// This worker manages the audio output device and a jitter buffer.
/// It receives decoded audio frames via `command_rx` and plays them.
///
/// # Arguments
///
/// * `logger` - Logger instance.
/// * `command_rx` - Channel to receive playback commands.
/// * `running` - Atomic flag to control the worker's lifecycle.
///
/// # Returns
///
/// The `JoinHandle` of the worker thread.
pub fn spawn_audio_player_worker(
    logger: Arc<dyn LogSink>,
    command_rx: Receiver<AudioPlayerCommand>,
    running: Arc<AtomicBool>,
) -> JoinHandle<()> {
    sink_info!(logger, "[AudioPlayer] Starting...");

    thread::Builder::new()
        .name("media-agent-audio-player".into())
        .spawn(move || {
            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(d) => d,
                None => {
                    sink_error!(logger, "[AudioPlayer] No default output device found");
                    return;
                }
            };
            
            sink_info!(logger, "[AudioPlayer] Using output device: {}", device.name().unwrap_or_default());

            let config = cpal::StreamConfig {
                channels: 1,
                sample_rate: cpal::SampleRate(48000),
                buffer_size: cpal::BufferSize::Default,
            };

            // Shared buffer between the event loop (producer) and the audio callback (consumer).
            let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(MAX_BUFFER_SIZE * 2)));
            let buffer_cb = buffer.clone();
            
            let logger_cb = logger.clone();

            let err_fn = move |err| {
                sink_warn!(logger_cb, "[AudioPlayer] Stream error: {}", err);
            };

            let stream = match device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut buf = buffer_cb.lock().expect("audio buffer lock poisoned");
                    for sample in data.iter_mut() {
                        if let Some(s) = buf.pop_front() {
                            *sample = s;
                        } else {
                            // Buffer empty (underrun), play silence
                            *sample = 0.0;
                        }
                    }
                },
                err_fn,
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    sink_error!(logger, "[AudioPlayer] Failed to build output stream: {}", e);
                    return;
                }
            };

            if let Err(e) = stream.play() {
                sink_error!(logger, "[AudioPlayer] Failed to play stream: {}", e);
                return;
            }

            sink_debug!(logger, "[AudioPlayer] Playback started");

            while running.load(Ordering::Relaxed) {
                // Poll for commands
                match command_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(cmd) => match cmd {
                        AudioPlayerCommand::PlayFrame(samples) => {
                            let mut buf = buffer.lock().expect("audio buffer lock poisoned");
                            
                            // Latency control: if buffer is too full, drop old data
                            let current_len = buf.len();
                            let incoming_len = samples.len();
                            
                            if current_len + incoming_len > MAX_BUFFER_SIZE {
                                let drop_count = (current_len + incoming_len) - MAX_BUFFER_SIZE;
                                let to_drop = drop_count.min(current_len);
                                sink_trace!(logger, "[AudioPlayer] Buffer full, dropping {} samples for latency catch-up", drop_count);
                                buf.drain(0..to_drop);
                            }
                            
                            buf.extend(samples);
                            sink_trace!(logger, "[AudioPlayer] Buffered {} samples. Total buffered: {}", incoming_len, buf.len());
                        }
                    },
                    Err(RecvTimeoutError::Timeout) => {
                        // Continue checking running flag
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        sink_debug!(logger, "[AudioPlayer] Channel disconnected, stopping");
                        break;
                    }
                }
            }
            
            sink_debug!(logger, "[AudioPlayer] Stopped");
        })
        .expect("spawn media-agent-audio-player")
}
