use std::{
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

use crate::{
    app::log_sink::LogSink,
    core::events::EngineEvent,
    logger_debug, logger_error,
    media_agent::{events::MediaAgentEvent, h264_decoder::H264Decoder, spec::CodecSpec},
};

pub fn spawn_decoder_worker(
    logger: Arc<dyn LogSink>,
    event_rx: Receiver<MediaAgentEvent>,
    event_tx: Sender<EngineEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-agent-decoder".into())
        .spawn(move || {
            let mut h264_decoder = H264Decoder::new();

            while let Ok(event) = event_rx.recv() {
                match event {
                    MediaAgentEvent::AnnexBFrameReady {
                        codec_spec,
                        bytes: chunk,
                    } => match codec_spec {
                        CodecSpec::H264 => match h264_decoder.decode_chunk(&chunk) {
                            Ok(Some(frame)) => {
                                let _ =
                                    event_tx.send(EngineEvent::DecodedVideoFrame(Box::new(frame)));
                            }
                            Ok(None) => {
                                logger_debug!(
                                    logger,
                                    "[MediaAgent] decoder needs more NALs for this AU"
                                );
                            }
                            Err(e) => {
                                logger_error!(logger, "[MediaAgent] decode error: {e:?}");
                            }
                        },
                    },
                }
            }
        })
        .expect("spawn media-agent-decoder")
}
