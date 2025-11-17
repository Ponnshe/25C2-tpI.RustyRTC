use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
        RwLock,
    },
    thread::{self, JoinHandle},
};

use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    core::{
        events::{EngineEvent, RtpIn},
        session::Session,
    },
    media_agent::{events::MediaAgentEvent, media_agent::MediaAgent, spec::CodecSpec},
    media_transport::{
        codec::CodecDescriptor,
        constants::{DYNAMIC_PAYLOAD_TYPE_START, RTP_TX_CHANNEL_SIZE},
        depacketizer::h264_depacketizer::H264Depacketizer,
        error::{MediaTransportError, Result},
        events::{DepacketizerEvent, PacketizerEvent},
        packetizer_worker::{spawn_packetizer_worker, PacketizeOrder},
    },
    rtp_session::{outbound_track_handle::OutboundTrackHandle, rtp_codec::RtpCodec},
    sink_log,
};

pub struct MediaTransport {
    media_agent: Arc<MediaAgent>,
    event_tx: Sender<EngineEvent>,
    rtp_tx: mpsc::SyncSender<RtpIn>,
    _logger: Arc<dyn LogSink>,
    _depacketizer_handle: Option<JoinHandle<()>>,
    _packetizer_handle: Option<JoinHandle<()>>,
    payload_map: HashMap<u8, CodecDescriptor>,
    outbound_tracks: HashMap<u8, OutboundTrackHandle>,
    allowed_pts: Arc<RwLock<HashSet<u8>>>,
    rtp_ts: u32,
    rtp_ts_step: u32,
    last_received_local_ts_ms: Option<u128>,
    depacketizer_event_rx: Receiver<DepacketizerEvent>,
    packetizer_rx: Receiver<PacketizerEvent>,
    packetizer_tx: Sender<PacketizeOrder>,
}

impl MediaTransport {
    pub fn new(event_tx: Sender<EngineEvent>, logger: Arc<dyn LogSink>) -> Self {
        let media_agent = Arc::new(MediaAgent::new(event_tx.clone(), logger.clone()));
        let mut payload_map = HashMap::new();
        let mut current_pt = DYNAMIC_PAYLOAD_TYPE_START;

        for spec in media_agent.supported_media() {
            let codec_descriptor = match spec.codec_spec {
                CodecSpec::H264 => CodecDescriptor::h264_dynamic(current_pt),
            };
            payload_map.insert(current_pt, codec_descriptor);
            current_pt += 1;
        }

        let (rtp_tx, rtp_rx) = mpsc::sync_channel::<RtpIn>(RTP_TX_CHANNEL_SIZE);
        let allowed_pts = Arc::new(RwLock::new(
            payload_map.keys().copied().collect::<HashSet<u8>>(),
        ));

        let payload_map_for_worker = payload_map.clone();
        let (depacketizer_event_tx, depacketizer_event_rx) = mpsc::channel();

        let depacketizer_handle = Some(spawn_depacketizer_worker(
            logger.clone(),
            allowed_pts.clone(),
            rtp_rx,
            depacketizer_event_tx,
            payload_map_for_worker,
        ));

        let (packetizer_tx, packetizer_order_rx) = mpsc::channel();
        let (packetizer_event_tx, packetizer_rx) = mpsc::channel();
        let packetizer_handle = Some(spawn_packetizer_worker(
            packetizer_order_rx,
            packetizer_event_tx,
        ));

        let target_fps = 30; // TODO: Get from config

        Self {
            media_agent,
            event_tx,
            rtp_tx,
            _logger: logger,
            _depacketizer_handle: depacketizer_handle,
            _packetizer_handle: packetizer_handle,
            payload_map,
            outbound_tracks: HashMap::new(),
            allowed_pts,
            rtp_ts: rand::random::<u32>(),
            rtp_ts_step: 90_000 / target_fps,
            last_received_local_ts_ms: None,
            depacketizer_event_rx,
            packetizer_rx,
            packetizer_tx,
        }
    }

    pub fn handle_engine_event(&mut self, evt: &EngineEvent, session: Option<&Session>) {
        match evt {
            EngineEvent::Established => {
                if let Some(sess) = session {
                    if let Err(e) = self.ensure_outbound_tracks(sess) {
                        let _ = self
                            .event_tx
                            .send(EngineEvent::Error(format!("media tracks: {e:?}")));
                    }
                    if let Ok(mut w) = self.allowed_pts.write() {
                        w.clear();
                        w.extend(sess.remote_codecs.iter().map(|c| c.payload_type));
                    }
                }
            }
            EngineEvent::Closed | EngineEvent::Closing { .. } => {
                self.outbound_tracks.clear();
            }
            EngineEvent::RtpIn(pkt) => {
                let _ = self.rtp_tx.try_send(pkt.clone());
            }
            EngineEvent::EncodedVideoFrame {
                annexb_frame,
                timestamp_ms,
                codec_spec,
            } => {
                if self.last_received_local_ts_ms == Some(*timestamp_ms) {
                    return;
                }
                self.last_received_local_ts_ms = Some(*timestamp_ms);

                let order = PacketizeOrder {
                    annexb_frame: annexb_frame.clone(),
                    rtp_ts: self.rtp_ts,
                    codec_spec: *codec_spec,
                };
                if self.packetizer_tx.send(order).is_ok() {
                    self.rtp_ts = self.rtp_ts.wrapping_add(self.rtp_ts_step);
                }
            }
            EngineEvent::DecodedVideoFrame(frame) => {
                self.media_agent.set_remote_frame(*frame.clone());
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, session: Option<&Session>) {
        self.media_agent.tick();
        self.process_depacketizer_events();
        self.process_packetizer_events(session);
    }

    fn process_depacketizer_events(&mut self) {
        while let Ok(event) = self.depacketizer_event_rx.try_recv() {
            match event {
                DepacketizerEvent::ChunkReady { codec_spec, chunk } => {
                    self.media_agent
                        .post_event(MediaAgentEvent::ChunkReady { codec_spec, chunk });
                }
            }
        }
    }

    fn process_packetizer_events(&mut self, session: Option<&Session>) {
        let Some(session) = session else { return };
        while let Ok(event) = self.packetizer_rx.try_recv() {
            match event {
                PacketizerEvent::FramePacketized(frame) => {
                    let Some((handle, _codec_spec)) = self.outbound_tracks.iter().next().map(|(pt, handle)| {
                        let spec = self.payload_map.get(pt).map(|d| d.spec).unwrap();
                        (handle, spec)
                    }) else { continue; };

                    if let Err(e) =
                        session.send_rtp_chunks_for_frame(handle.local_ssrc, &frame.chunks, frame.rtp_ts)
                    {
                        let _ = self.event_tx.send(EngineEvent::Error(format!(
                            "[MediaTransport] send local frame failed: {e:?}"
                        )));
                    }
                }
            }
        }
    }

    pub fn snapshot_frames(&self) -> (Option<crate::media_agent::video_frame::VideoFrame>, Option<crate::media_agent::video_frame::VideoFrame>) {
        self.media_agent.snapshot_frames()
    }

    pub fn set_bitrate(&mut self, new_bitrate: u32) {
        self.media_agent.set_bitrate(new_bitrate);
        let new_fps = if new_bitrate >= 1_500_000 {
            30
        } else if new_bitrate >= 800_000 {
            25
        } else {
            20
        };
        self.rtp_ts_step = 90_000 / new_fps;
    }

    pub fn codec_descriptors(&self) -> Vec<CodecDescriptor> {
        self.payload_map.values().cloned().collect()
    }

    pub fn local_rtp_codecs(&self) -> Vec<RtpCodec> {
        self.payload_map.values().map(|c| c.rtp.clone()).collect()
    }

    fn ensure_outbound_tracks(&mut self, session: &Session) -> Result<()> {
        for (pt, codec) in &self.payload_map {
            if self.outbound_tracks.contains_key(pt) {
                continue;
            }
            let handle = session
                .register_outbound_track(codec.rtp.clone())
                .map_err(|e| MediaTransportError::Send(e.to_string()))?;
            self.outbound_tracks.insert(*pt, handle);
        }
        Ok(())
    }
}

fn spawn_depacketizer_worker(
    logger: Arc<dyn LogSink>,
    allowed_pts: Arc<RwLock<HashSet<u8>>>,
    rtp_rx: Receiver<RtpIn>,
    event_tx: Sender<DepacketizerEvent>,
    payload_map: HashMap<u8, CodecDescriptor>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-transport-depack".into())
        .spawn(move || {
            let mut depack = H264Depacketizer::new();

            while let Ok(pkt) = rtp_rx.recv() {
                let ok_pt = allowed_pts
                    .read()
                    .map(|set| set.contains(&pkt.pt))
                    .unwrap_or(false);
                if !ok_pt {
                    sink_log!(
                        logger.as_ref(),
                        LogLevel::Debug,
                        "[MediaTransport] dropping RTP PT={}",
                        pkt.pt
                    );
                    continue;
                }

                let Some(codec_desc) = payload_map.get(&pkt.pt) else {
                    sink_log!(
                        logger.as_ref(),
                        LogLevel::Warn,
                        "[MediaTransport] unknown payload type {}",
                        pkt.pt
                    );
                    continue;
                };

                if let Some(chunk) =
                    depack.push_rtp(&pkt.payload, pkt.marker, pkt.timestamp_90khz, pkt.seq)
                {
                    let _ = event_tx.send(DepacketizerEvent::ChunkReady {
                        codec_spec: codec_desc.spec,
                        chunk,
                    });
                }
            }
        })
        .expect("spawn media-transport-depack")
}
