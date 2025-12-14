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
use crate::{log::log_sink::LogSink, media_agent::spec::CodecSpec, sink_trace};

/// Represents a request sent to the Packetizer worker to process a frame.
#[derive(Debug)]
pub struct PacketizeOrder {
    /// The raw encoded media data (Video: Annex B, Audio: Raw payload).
    pub payload: Vec<u8>,
    /// The RTP timestamp assigned to this frame.
    /// This timestamp will be shared by all RTP packets generated from this single frame.
    pub rtp_ts: u32,
    /// The codec used, determining the packetization strategy (e.g., H.264 NAL units).
    pub codec_spec: CodecSpec,
}

/// The result of the packetization process.
///
/// Contains the list of payloads ready to be wrapped in RTP headers and sent over the wire.
#[derive(Debug)]
pub struct PacketizedFrame {
    /// A vector of chunks, where each chunk fits within the network MTU.
    /// For H.264, these are either Single NAL Unit packets or Fragmentation Units (FU-A).
    pub chunks: Vec<RtpPayloadChunk>,
    /// The RTP timestamp to be applied to all chunks in this frame.
    pub rtp_ts: u32,
    /// The codec specification.
    pub codec_spec: CodecSpec,
}

/// Spawns a dedicated thread for fragmenting video frames into network packets.
///
/// This worker consumes `PacketizeOrder`s containing full video frames. It applies
/// codec-specific logic (currently H.264) to split the frame into MTU-safe chunks.
///
/// # MTU Strategy
/// The packetizer is initialized with a conservative MTU of **1200 bytes**.
/// This safeguards against IP fragmentation on the internet (where standard MTU is 1500),
/// leaving ample room for IP, UDP, and RTP headers.
///
/// # Arguments
///
/// * `order_rx` - Channel receiving frames to be packetized.
/// * `event_tx` - Channel to output the result (`PacketizedFrame`).
/// * `logger` - Logger instance.
///
/// # Panics
///
/// Panics if the OS fails to spawn the thread.
#[allow(clippy::expect_used)]
pub fn spawn_packetizer_worker(
    order_rx: Receiver<PacketizeOrder>,
    event_tx: Sender<PacketizerEvent>,
    logger: Arc<dyn LogSink>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-transport-packetizer".into())
        .spawn(move || {
            // MTU is hardcoded to 1200 bytes.
            // This leaves ~300 bytes of headroom for headers (IP+UDP+RTP+Extensions)
            // before hitting the standard 1500 byte Ethernet limit.
            let h264_packetizer = H264Packetizer::new(1200);

            while let Ok(order) = order_rx.recv() {
                sink_trace!(
                    logger.clone(),
                    "[Packetizer] Received Order"
                );
                
                match order.codec_spec {
                    CodecSpec::H264 => {
                        // Performs the slicing (identifies NAL boundaries, handles FU-A)
                        let chunks =
                            h264_packetizer.packetize_annexb_to_payloads(&order.payload);
                        
                        if !chunks.is_empty() {
                            let packetized_frame = PacketizedFrame {
                                chunks,
                                rtp_ts: order.rtp_ts,
                                codec_spec: order.codec_spec,
                            };
                            
                            sink_trace!(
                                logger.clone(),
                                "[Packetizer] Sending PacketizedFrame to MediaTranport Packetizer Event Loop"
                            );
                            
                            // Forward the chunks to the next stage (RTP encapsulation)
                            let _ =
                                event_tx.send(PacketizerEvent::FramePacketized(packetized_frame));
                        }
                    }
                    CodecSpec::G711U => {
                         let packetized_frame = PacketizedFrame {
                            chunks: vec![RtpPayloadChunk {
                                bytes: order.payload,
                                marker: true, 
                            }],
                            rtp_ts: order.rtp_ts,
                            codec_spec: order.codec_spec,
                        };
                        
                        sink_trace!(
                            logger.clone(),
                            "[Packetizer] Sending Audio PacketizedFrame"
                        );
                        
                        let _ = event_tx.send(PacketizerEvent::FramePacketized(packetized_frame));
                    }
                }
            }
        })
        .expect("spawn media-transport-packetizer")
}
