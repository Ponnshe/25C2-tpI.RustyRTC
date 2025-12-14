use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, RwLock,
        mpsc::{Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

use crate::media_transport::{codec::CodecDescriptor, events::DepacketizerEvent};
use crate::{
    log::log_sink::LogSink,
    media_agent::spec::CodecSpec,
    media_transport::{
        depacketizer::h264_depacketizer::H264Depacketizer, media_transport_event::RtpIn,
    },
    sink_trace,
};

/// Spawns a dedicated thread responsible for reassembling RTP packets into video frames.
///
/// This worker consumes raw RTP packets from the `rtp_packet_rx` channel, validates them
/// against negotiated codecs, and feeds them into a specific depacketizer (currently H.264).
/// When a complete frame is reconstructed, it emits a `DepacketizerEvent`.
///
/// # Architecture
///
/// 1. **Filtering**: Checks if the packet's Payload Type (PT) is in the `allowed_pts` set.
///    This allows dynamic filtering based on SDP negotiation (e.g., ignoring unnegotiated streams).
/// 2. **Lookup**: Retrieves codec details from `payload_map` to associate the PT with a codec spec.
/// 3. **Reassembly**: Uses `H264Depacketizer` to buffer fragments (FU-A) until the "Marker" bit
///    or a complete NAL unit signifies the end of a frame.
/// 4. **Output**: Sends `AnnexBFrameReady` containing the full byte buffer of the frame.
///
/// # Arguments
///
/// * `logger` - Shared logger for tracing packet flow.
/// * `allowed_pts` - A thread-safe set of currently valid RTP Payload Types (updated via SDP).
/// * `rtp_packet_rx` - Input channel for raw RTP packets.
/// * `event_tx` - Output channel for reassembled frames.
/// * `payload_map` - Static mapping between Payload Types and `CodecDescriptor`s.
///
/// # Panics
///
/// Panics if the OS fails to create the thread (`expect` on `thread::spawn`).
#[allow(clippy::expect_used)]
pub fn spawn_depacketizer_worker(
    logger: Arc<dyn LogSink>,
    allowed_pts: Arc<RwLock<HashSet<u8>>>,
    rtp_packet_rx: Receiver<RtpIn>,
    event_tx: Sender<DepacketizerEvent>,
    payload_map: Arc<HashMap<u8, CodecDescriptor>>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-transport-depack".into())
        .spawn(move || {
            // Currently hardcoded to H264. 
            // In the future, this could be a dynamic trait object based on the Payload Type.
            let mut depacketizer = H264Depacketizer::new();

            while let Ok(pkt) = rtp_packet_rx.recv() {
                sink_trace!(logger, "[Depacketizer] Received RTP Packet");

                sink_trace!(
                    logger,
                    "[Depacketizer] ssrc: {}, seq: {}",
                    pkt.ssrc,
                    pkt.seq
                );
                
                // 1. Verify if this Payload Type is currently negotiated/allowed.
                let ok_pt = allowed_pts
                    .read()
                    .map(|set| set.contains(&pkt.pt))
                    .unwrap_or(false);

                if !ok_pt {
                    sink_trace!(logger, "[MediaTransport] dropping RTP PT={}", pkt.pt);
                    continue;
                }

                // 2. Resolve the codec specification.
                let Some(codec_desc) = payload_map.get(&pkt.pt) else {
                    sink_trace!(logger, "[MediaTransport] unknown payload type {}", pkt.pt);
                    continue;
                };

                sink_trace!(logger, "[Depacketizer] Pushing RTP Packet to depacketizer");
                sink_trace!(
                    logger,
                    "[Depacketizer] ssrc: {}, seq: {}",
                    pkt.ssrc,
                    pkt.seq
                );

                match codec_desc.spec {
                    CodecSpec::H264 => {
                        // 3. Feed the packet into the reassembly logic.
                        // The depacketizer returns `Some(bytes)` only when a full frame is complete.
                        if let Some(annex_b_frame) =
                            depacketizer.push_rtp(&pkt.payload, pkt.marker, pkt.timestamp_90khz, pkt.seq)
                        {
                            sink_trace!(
                                logger,
                                "[Depacketizer] AnnexBFrameReady sending it to DepcketizerEventLoop (MT)"
                            );
                            let _ = event_tx.send(DepacketizerEvent::AnnexBFrameReady {
                                codec_spec: codec_desc.spec,
                                bytes: annex_b_frame,
                            });
                        }
                    }
                    CodecSpec::G711U => {
                         let _ = event_tx.send(DepacketizerEvent::EncodedAudioFrameReady {
                            codec_spec: codec_desc.spec,
                            payload: pkt.payload,
                        });
                    }
                }
            }
        })
        .expect("spawn media-transport-depack")
}
