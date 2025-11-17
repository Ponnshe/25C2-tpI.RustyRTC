use std::{
    sync::{mpsc::{Receiver, Sender}, Arc},
    thread::{self, JoinHandle},
};

use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    core::events::EngineEvent,
    media_agent::{
        h264_encoder::H264Encoder,
        spec::CodecSpec,
        video_frame::VideoFrame,
    },
    sink_log,
};

use super::constants::{BITRATE, KEYINT, TARGET_FPS};

pub enum EncoderOrder {
    Encode(VideoFrame, bool), // (frame, force_keyframe)
    SetConfig { fps: u32, bitrate: u32, keyint: u32 },
}

pub fn spawn_encoder_worker(
    logger: Arc<dyn LogSink>,
    order_rx: Receiver<EncoderOrder>,
    event_tx: Sender<EngineEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-agent-encoder".into())
        .spawn(move || {
            let mut h264_encoder = H264Encoder::new(TARGET_FPS, BITRATE, KEYINT);

            while let Ok(order) = order_rx.recv() {
                match order {
                    EncoderOrder::Encode(frame, force_keyframe) => {
                        if force_keyframe {
                            h264_encoder.request_keyframe();
                        }
                        match h264_encoder.encode_frame_to_h264(&frame) {
                            Ok(annexb_frame) => {
                                let _ = event_tx.send(EngineEvent::EncodedVideoFrame {
                                    annexb_frame,
                                    timestamp_ms: frame.timestamp_ms,
                                    codec_spec: CodecSpec::H264,
                                });
                            }
                            Err(e) => {
                                sink_log!(
                                    logger.as_ref(),
                                    LogLevel::Error,
                                    "[EncoderWorker] encode error: {e:?}"
                                );
                            }
                        }
                    }
                    EncoderOrder::SetConfig { fps, bitrate, keyint } => {
                        if let Err(e) = h264_encoder.set_config(fps, bitrate, keyint) {
                            sink_log!(
                                logger.as_ref(),
                                LogLevel::Error,
                                "[EncoderWorker] set_config error: {e:?}"
                            );
                        }
                    }
                }
            }
        })
        .expect("spawn media-agent-encoder")
}
