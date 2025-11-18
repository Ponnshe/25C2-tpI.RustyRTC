use std::{
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

use crate::{
    app::log_sink::LogSink,
    logger_error,
    media_agent::{
        encoder_instruction::EncoderInstruction, events::MediaAgentEvent,
        h264_encoder::H264Encoder, spec::CodecSpec,
    },
};

use super::constants::{BITRATE, KEYINT, TARGET_FPS};

pub fn spawn_encoder_worker(
    logger: Arc<dyn LogSink>,
    ma_encoder_event_rx: Receiver<EncoderInstruction>,
    media_agent_event_tx: Sender<MediaAgentEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-agent-encoder".into())
        .spawn(move || {
            let mut h264_encoder = H264Encoder::new(TARGET_FPS, BITRATE, KEYINT);

            while let Ok(order) = ma_encoder_event_rx.recv() {
                match order {
                    EncoderInstruction::Encode(frame, force_keyframe) => {
                        if force_keyframe {
                            h264_encoder.request_keyframe();
                        }
                        match h264_encoder.encode_frame_to_h264(&frame) {
                            Ok(annexb_frame) => {
                                let _ =
                                    media_agent_event_tx.send(MediaAgentEvent::EncodedVideoFrame {
                                        annexb_frame,
                                        timestamp_ms: frame.timestamp_ms,
                                        codec_spec: CodecSpec::H264,
                                    });
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
                }
            }
        })
        .expect("spawn media-agent-encoder")
}
