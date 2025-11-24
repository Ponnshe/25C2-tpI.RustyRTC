use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, RwLock,
        mpsc::{Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

use crate::{
    app::log_sink::LogSink, logger_debug, logger_warn,
    media_transport::{depacketizer::h264_depacketizer::H264Depacketizer, media_transport_event::RtpIn},
};
use crate::{
    media_transport::{
        codec::CodecDescriptor, 
        events::DepacketizerEvent,
    },
};

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
            let mut depacketizer = H264Depacketizer::new();

            while let Ok(pkt) = rtp_packet_rx.recv() {
                let ok_pt = allowed_pts
                    .read()
                    .map(|set| set.contains(&pkt.pt))
                    .unwrap_or(false);

                if !ok_pt {
                    logger_debug!(logger, "[MediaTransport] dropping RTP PT={}", pkt.pt);
                    continue;
                }

                let Some(codec_desc) = payload_map.get(&pkt.pt) else {
                    logger_warn!(logger, "[MediaTransport] unknown payload type {}", pkt.pt);
                    continue;
                };

                if let Some(annex_b_frame) =
                    depacketizer.push_rtp(&pkt.payload, pkt.marker, pkt.timestamp_90khz, pkt.seq)
                {
                    let _ = event_tx.send(DepacketizerEvent::AnnexBFrameReady {
                        codec_spec: codec_desc.spec,
                        bytes: annex_b_frame,
                    });
                }
            }
        })
        .expect("spawn media-transport-depack")
}
