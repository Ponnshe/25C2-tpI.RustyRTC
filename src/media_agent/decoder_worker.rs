use std::{
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

use crate::{
    app::log_sink::LogSink,
    logger_debug, logger_error,
    media_agent::{
        decoder_event::DecoderEvent, events::MediaAgentEvent, h264_decoder::H264Decoder,
        spec::CodecSpec,
    },
};

pub fn spawn_decoder_worker(
    logger: Arc<dyn LogSink>,
    ma_decoder_event_rx: Receiver<DecoderEvent>,
    media_agent_event_tx: Sender<MediaAgentEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-agent-decoder".into())
        .spawn(move || {
            let mut h264_decoder = H264Decoder::new();

            while let Ok(event) = ma_decoder_event_rx.recv() {
                match event {
                    DecoderEvent::AnnexBFrameReady { codec_spec, bytes } => match codec_spec {
                        CodecSpec::H264 => match h264_decoder.decode_frame(&bytes) {
                            Ok(Some(frame)) => {
                                let _ = media_agent_event_tx
                                    .send(MediaAgentEvent::DecodedVideoFrame(Box::new(frame)));
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
