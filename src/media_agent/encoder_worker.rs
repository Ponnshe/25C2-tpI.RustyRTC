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
    log::log_sink::LogSink,
    logger_debug, logger_error,
    media_agent::{
        constants::CHANNELS_TIMEOUT, encoder_instruction::EncoderInstruction,
        events::MediaAgentEvent, h264_encoder::H264Encoder, spec::CodecSpec,
    },
    sink_debug,
};

use super::constants::{BITRATE, KEYINT, TARGET_FPS};

pub fn spawn_encoder_worker(
    logger: Arc<dyn LogSink>,
    ma_encoder_event_rx: Receiver<EncoderInstruction>,
    media_agent_event_tx: Sender<MediaAgentEvent>,
    running: Arc<AtomicBool>,
) -> Result<JoinHandle<()>, Error> {
    sink_debug!(logger.clone(), "[Encoder] Starting...");
    thread::Builder::new()
        .name("media-agent-encoder".into())
        .spawn(move || {
            let mut h264_encoder = H264Encoder::new(TARGET_FPS, BITRATE, KEYINT);

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
                            if let Err(e) = h264_encoder.set_config(fps, bitrate, keyint) {
                                logger_error!(logger, "[EncoderWorker] set_config error: {e:?}");
                            }
                        }
                    },

                    Err(RecvTimeoutError::Timeout) => {
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
                    }
                }
            }
        })
}
