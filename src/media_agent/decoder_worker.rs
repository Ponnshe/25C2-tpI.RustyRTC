use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    log::log_sink::LogSink,
    logger_debug, logger_error,
    media_agent::{
        constants::CHANNELS_TIMEOUT, decoder_event::DecoderEvent, events::MediaAgentEvent,
        frame_format::FrameFormat, h264_decoder::H264Decoder, spec::CodecSpec,
    },
    sink_debug, sink_info,
};

/// Target pixel format for the decoder output.
/// Currently fixed to YUV420 as it is the standard for most WebRTC/Video pipelines.
const FRAME_FORMAT: FrameFormat = FrameFormat::Yuv420;

/// Spawns a dedicated background thread for video decoding.
///
/// This worker listens for encoded video chunks (H.264 Annex B) on the `ma_decoder_event_rx` channel,
/// feeds them into the `H264Decoder`, and forwards successfully decoded frames to the
/// `media_agent_event_tx` channel.
///
/// # Architecture
///
/// 1. **Input**: Receives `DecoderEvent::AnnexBFrameReady` containing NAL units.
/// 2. **Process**:
///    - Inspects NAL headers for diagnostic logging (identifying Keyframes/IDR, SPS, PPS).
///    - Feeds data to the underlying decoder (e.g., OpenH264 or FFmpeg wrapper).
/// 3. **Output**: Sends `MediaAgentEvent::DecodedVideoFrame` containing the raw YUV image.
///
/// # Lifecycle
///
/// The thread runs a continuous loop that checks the `running` atomic flag.
/// It uses a timeout on the receiver (`CHANNELS_TIMEOUT`) to ensure the thread can
/// verify the `running` flag and exit cleanly even if no data is arriving.
///
/// # Arguments
///
/// * `logger` - Shared logger instance for diagnostic output.
/// * `ma_decoder_event_rx` - Channel receiver for incoming encoded data packets.
/// * `media_agent_event_tx` - Channel sender for outgoing decoded video frames.
/// * `running` - Atomic flag to control the shutdown of the worker thread.
///
/// # Panics
///
/// This function panics if the OS fails to create the new thread (`thread::spawn`).
#[allow(clippy::expect_used)]
pub fn spawn_decoder_worker(
    logger: Arc<dyn LogSink>,
    ma_decoder_event_rx: Receiver<DecoderEvent>,
    media_agent_event_tx: Sender<MediaAgentEvent>,
    running: Arc<AtomicBool>,
) -> JoinHandle<()> {
    sink_info!(logger, "[Decoder] Starting...");
    thread::Builder::new()
        .name("media-agent-decoder".into())
        .spawn(move || {
            let mut h264_decoder = H264Decoder::new(logger.clone());

            while running.load(Ordering::Relaxed){
                match ma_decoder_event_rx.recv_timeout(Duration::from_millis(CHANNELS_TIMEOUT)) {
                    Ok(event) => {
                        match event {
                            DecoderEvent::AnnexBFrameReady { codec_spec, bytes } => {
                                // --- Diagnostic Logging (NAL Inspection) ---
                                if bytes.len() > 4 {
                                    let nal_type = bytes[4] & 0x1F;
                                    logger_debug!(
                                        logger,
                                        "[Decoder] NAL type: {nal_type} ({})",
                                        match nal_type {
                                            1 => "Non-IDR slice",
                                            5 => "IDR slice",
                                            6 => "SEI",
                                            7 => "SPS",
                                            8 => "PPS",
                                            _ => "other",
                                        }
                                    );
                                }
                                if bytes.len() < 6 {
                                    logger_error!(
                                        logger,
                                        "[Decoder] Frame too small! Size={} bytes. Data={:02X?}",
                                        bytes.len(),
                                        bytes
                                    );
                                }
                                if bytes.len() > 4 {
                                    let nal_type = bytes[4] & 0x1F;
                                    if nal_type == 7 || nal_type == 8 {
                                        logger_debug!(logger, "[Decoder] Got SPS/PPS NAL type={}", nal_type);
                                    }
                                }

                                // --- Decoding Logic ---
                                match codec_spec {
                                    CodecSpec::H264 => {
                                        logger_debug!(
                                            logger,
                                            "[Decoder] Received AnnexB frame: size={} bytes, head={:02X?}",
                                            bytes.len(),
                                            &bytes[..bytes.len().min(12)]
                                        );
                                        let t0 = std::time::Instant::now();
                                        
                                        match h264_decoder.decode_frame(&bytes, FRAME_FORMAT) {
                                            Ok(Some(frame)) => {
                                                let took = t0.elapsed();
                                                sink_info!(
                                                    logger,
                                                    "[Decoder] Frame Ready sending MediaAgentEvent::DecodedVideoFrame"
                                                );
                                                sink_debug!(
                                                    logger,
                                                    "[Decoder] [Decoder] decode_frame total took: {:?}(including rgb conversion)", 
                                                    took
                                                );
                                                let _ = media_agent_event_tx
                                                    .send(MediaAgentEvent::DecodedVideoFrame(Box::new(frame)));
                                            }
                                            Ok(None) => {
                                                // Decoder needs more data (e.g. buffered frames or missing SPS/PPS)
                                                logger_debug!(
                                                    logger,
                                                    "[Decoder] Incomplete AU after NAL type={} (need SPS/PPS/IDR?)",
                                                    bytes.get(4).map(|b| b & 0x1F).unwrap_or(0)
                                                );
                                            }
                                            Err(e) => {
                                                logger_error!(
                                                    logger,
                                                    "[Decoder] ERROR: {e:?}\n  Frame size: {}\n  First bytes: {:02X?}",
                                                    bytes.len(),
                                                    &bytes[..bytes.len().min(12)]
                                                );
                                            }
                                        }
                                    },
                                }
                            },
                        }
                    },
                    Err(RecvTimeoutError::Timeout) => {
                        // Timeout is expected; it allows the loop to check `running` flag.
                        #[cfg(debug_assertions)]
                        logger_debug!(
                            logger,
                            "[MediaAgent Decoder] The channel received nothing in {}ms",
                            CHANNELS_TIMEOUT
                        );
                    },

                    Err(RecvTimeoutError::Disconnected) => {
                        logger_error!(
                            logger,
                            "[MediaAgent Decoder] The channel has been disconnected"
                        );
                        // Optional: break loop here if channel death implies worker death
                        // break; 
                    },
                }
            }
        })
        .expect("spawn media-agent-decoder")
}
