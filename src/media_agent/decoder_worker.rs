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
    app::log_sink::LogSink,
    logger_debug, logger_error,
    media_agent::{
        constants::CHANNELS_TIMEOUT, decoder_event::DecoderEvent, events::MediaAgentEvent,
        h264_decoder::H264Decoder, spec::CodecSpec,
    },
    sink_info,
};

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
            let mut h264_decoder = H264Decoder::new();

            while running.load(Ordering::Relaxed){
                match ma_decoder_event_rx.recv_timeout(Duration::from_millis(CHANNELS_TIMEOUT)) {
                    Ok(event) => {
                        match event {
                            DecoderEvent::AnnexBFrameReady { codec_spec, bytes } => {
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
                                match codec_spec {
                                    CodecSpec::H264 => {
                                        logger_debug!(
                                            logger,
                                            "[Decoder] Received AnnexB frame: size={} bytes, head={:02X?}",
                                            bytes.len(),
                                            &bytes[..bytes.len().min(12)]
                                        );
                                        match h264_decoder.decode_frame(&bytes) {
                                            Ok(Some(frame)) => {
                                                let _ = media_agent_event_tx
                                                    .send(MediaAgentEvent::DecodedVideoFrame(Box::new(frame)));
                                            }
                                            Ok(None) => {
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
                    },
                }
            }
        })
        .expect("spawn media-agent-decoder")
}
