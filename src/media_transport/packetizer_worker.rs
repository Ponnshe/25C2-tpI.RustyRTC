use std::{
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

use super::events::PacketizerEvent;
use crate::media_transport::payload::{
    h264_packetizer::H264Packetizer, rtp_payload_chunk::RtpPayloadChunk,
};
use crate::{app::log_sink::LogSink, media_agent::spec::CodecSpec, sink_info};

#[derive(Debug)]
pub struct PacketizeOrder {
    pub annexb_frame: Vec<u8>,
    pub rtp_ts: u32,
    pub codec_spec: CodecSpec,
}

#[derive(Debug)]
pub struct PacketizedFrame {
    pub chunks: Vec<RtpPayloadChunk>,
    pub rtp_ts: u32,
    pub codec_spec: CodecSpec,
}

pub fn spawn_packetizer_worker(
    order_rx: Receiver<PacketizeOrder>,
    event_tx: Sender<PacketizerEvent>,
    logger: Arc<dyn LogSink>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-transport-packetizer".into())
        .spawn(move || {
            let h264_packetizer = H264Packetizer::new(1200); // MTU is hardcoded here, maybe configure it.

            while let Ok(order) = order_rx.recv() {
                sink_info!(
                    logger.clone(),
                    "[Packetizer] Received Order"
                );
                match order.codec_spec {
                    CodecSpec::H264 => {
                        let chunks =
                            h264_packetizer.packetize_annexb_to_payloads(&order.annexb_frame);
                        if !chunks.is_empty() {
                            let packetized_frame = PacketizedFrame {
                                chunks,
                                rtp_ts: order.rtp_ts,
                                codec_spec: order.codec_spec,
                            };
                            sink_info!(
                                logger.clone(),
                                "[Packetizer] Sending PacketizedFrame to MediaTranport Packetizer Event Loop"
                            );
                            let _ =
                                event_tx.send(PacketizerEvent::FramePacketized(packetized_frame));
                        }
                    }
                }
            }
        })
        .expect("spawn media-transport-packetizer")
}
