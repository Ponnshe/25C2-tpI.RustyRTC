use std::{
    io::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    config::Config,
    log::log_sink::LogSink,
    logger_debug, logger_error,
    media_agent::{
        constants::CHANNELS_TIMEOUT, encoder_instruction::EncoderInstruction,
        events::MediaAgentEvent, h264_encoder::H264Encoder, spec::CodecSpec,
    },
    sink_debug,
};

use super::constants::{BITRATE, KEYINT, TARGET_FPS};

/// Spawns a dedicated background thread for H.264 video encoding.
///
/// This worker consumes `EncoderInstruction`s from the input channel, which can contain
/// either raw video frames to encode or configuration updates (bitrate, FPS, etc.).
/// Encoded frames are wrapped in `MediaAgentEvent`s and sent to the output channel.
///
/// # Architecture
///
/// 1. **Initialization**: Reads initial encoding parameters (FPS, Bitrate, Keyint) from the
///    provided `Config`, falling back to constants if keys are missing.
/// 2. **Loop**:
///    - Listens for `EncoderInstruction`.
///    - **On `Encode`**: Compresses the frame using `H264Encoder`. If `force_keyframe` is true,
///      it requests an IDR frame immediately.
///    - **On `SetConfig`**: Dynamically reconfigures the encoder without restarting the thread.
/// 3. **Output**: Sends `MediaAgentEvent::EncodedVideoFrame` (Annex B format) to the media agent.
///
/// # Arguments
///
/// * `logger` - Shared logger instance.
/// * `ma_encoder_event_rx` - Channel receiver for instructions (frames to encode or config changes).
/// * `media_agent_event_tx` - Channel sender for the resulting encoded video events.
/// * `running` - Atomic flag to control the worker's lifecycle.
/// * `config` - Application configuration for initial encoder settings.
///
/// # Errors
///
/// Returns an [`std::io::Error`] if the OS fails to create the thread (e.g., resource exhaustion).
///
/// # Panics
///
/// The worker thread itself does not panic; errors during encoding or configuration
/// are logged via `logger_error!` and the loop continues.
pub fn spawn_encoder_worker(
    logger: Arc<dyn LogSink>,
    ma_encoder_event_rx: Receiver<EncoderInstruction>,
    media_agent_event_tx: Sender<MediaAgentEvent>,
    running: Arc<AtomicBool>,
    config: Arc<Config>,
) -> Result<JoinHandle<()>, Error> {
    sink_debug!(logger.clone(), "[Encoder] Starting...");
    
    thread::Builder::new()
        .name("media-agent-encoder".into())
        .spawn(move || {
            // --- Initialization Phase ---
            // Parse configuration with fallbacks to compile-time constants.
            let target_fps = config
                .get("Media", "fps")
                .and_then(|s| s.parse().ok())
                .unwrap_or(TARGET_FPS);

            let bitrate = config
                .get("Media", "bitrate")
                .and_then(|s| s.parse().ok())
                .unwrap_or(BITRATE);

            let keyint = config
                .get("Media", "keyint")
                .and_then(|s| s.parse().ok())
                .unwrap_or(KEYINT);

            let mut h264_encoder = H264Encoder::new(target_fps, bitrate, keyint);

            // --- Main Loop ---
            while running.load(Ordering::Relaxed) {
                match ma_encoder_event_rx.recv_timeout(Duration::from_millis(CHANNELS_TIMEOUT)) {
                    Ok(order) => match order {
                        EncoderInstruction::Encode(frame, force_keyframe) => {
                            if force_keyframe {
                                h264_encoder.request_keyframe();
                            }
                            
                            match h264_encoder.encode_frame_to_h264(&frame) {
                                Ok(annexb_frame) => {
                                    sink_debug!(
                                        logger.clone(),
                                        "[Encoder] Sending EncodedVideoFrame to MediaAgent"
                                    );
                                    // Forward the encoded data to the main agent
                                    let _ = media_agent_event_tx.send(
                                        MediaAgentEvent::EncodedVideoFrame {
                                            annexb_frame,
                                            timestamp_ms: frame.timestamp_ms,
                                            codec_spec: CodecSpec::H264,
                                        },
                                    );
                                }
                                Err(e) => {
                                    logger_error!(logger, "[EncoderWorker] encode error: {e:?}");
                                }
                            }
                        }
                        EncoderInstruction::SetConfig {
                            fps,
                            bitrate,
                            keyint,
                        } => {
                            // Apply dynamic configuration changes
                            if let Err(e) = h264_encoder.set_config(fps, bitrate, keyint) {
                                logger_error!(logger, "[EncoderWorker] set_config error: {e:?}");
                            }
                        }
                    },

                    Err(RecvTimeoutError::Timeout) => {
                        // Timeout is expected; allows checking the `running` flag.
                        #[cfg(debug_assertions)]
                        logger_debug!(
                            logger,
                            "[MediaAgent Encoder] The channel received nothing in {}ms",
                            CHANNELS_TIMEOUT
                        );
                    }

                    Err(RecvTimeoutError::Disconnected) => {
                        logger_error!(
                            logger,
                            "[MediaAgent Encoder] The channel has been disconnected"
                        );
                        // Optional: break; // If the instruction channel dies, the worker could exit.
                    }
                }
            }
        })
}
