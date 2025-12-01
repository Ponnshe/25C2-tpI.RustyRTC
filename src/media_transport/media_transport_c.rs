use crate::{
    config::Config,
    core::{events::EngineEvent, session::Session},
    log::log_sink::LogSink,
    media_agent::{MediaAgent, constants::TARGET_FPS, spec::CodecSpec, video_frame::VideoFrame},
    media_transport::{
        codec::CodecDescriptor,
        constants::{DYNAMIC_PAYLOAD_TYPE_START, RTP_TX_CHANNEL_SIZE},
        depacketizer_worker::spawn_depacketizer_worker,
        event_loops::{
            depacketizer_event_loop::DepacketizerEventLoop,
            media_agent_event_loop::MediaAgentEventLoop,
            packetizer_event_loop::PacketizerEventLoop,
        },
        media_transport_event::{MediaTransportEvent, RtpIn},
        packetizer_worker::spawn_packetizer_worker,
    },
    rtp_session::{outbound_track_handle::OutboundTrackHandle, rtp_codec::RtpCodec},
    sink_error, sink_info,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, RwLock,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::JoinHandle,
};

pub struct MediaTransport {
    logger: Arc<dyn LogSink>,
    event_tx: Sender<EngineEvent>,
    media_agent: MediaAgent,
    media_agent_event_loop: MediaAgentEventLoop,
    depacketizer_event_loop: DepacketizerEventLoop,
    packetizer_event_loop: PacketizerEventLoop,
    rtp_tx: Option<SyncSender<RtpIn>>,
    depacketizer_handle: Option<JoinHandle<()>>,
    packetizer_handle: Option<JoinHandle<()>>,
    payload_map: Arc<HashMap<u8, CodecDescriptor>>,
    outbound_tracks: Arc<Mutex<HashMap<u8, OutboundTrackHandle>>>,
    allowed_pts: Option<Arc<RwLock<HashSet<u8>>>>,
    media_transport_event_tx: Option<Sender<MediaTransportEvent>>,
    media_transport_event_rx: Option<Receiver<MediaTransportEvent>>,
}

impl MediaTransport {
    pub fn new(
        event_tx: Sender<EngineEvent>,
        logger: Arc<dyn LogSink>,
        config: Arc<Config>,
    ) -> Self {
        let media_agent = MediaAgent::new(logger.clone(), config.clone());
        let target_fps = config
            .get_or_default("Media", "fps", "30")
            .parse()
            .unwrap_or(TARGET_FPS);
        let media_agent_event_loop = MediaAgentEventLoop::new(target_fps, logger.clone());

        let depacketizer_event_loop = DepacketizerEventLoop::new(logger.clone());

        let packetizer_event_loop = PacketizerEventLoop::new(logger.clone());

        let (mt_event_tx, mt_event_rx) = mpsc::channel();
        let media_transport_event_tx = Some(mt_event_tx);
        let media_transport_event_rx = Some(mt_event_rx);
        Self {
            logger,
            media_agent,
            media_agent_event_loop,
            depacketizer_event_loop,
            packetizer_event_loop,
            event_tx,
            rtp_tx: None,
            depacketizer_handle: None,
            packetizer_handle: None,
            payload_map: Arc::new(HashMap::new()),
            outbound_tracks: Arc::new(Mutex::new(HashMap::new())),
            allowed_pts: None,
            media_transport_event_tx,
            media_transport_event_rx,
        }
    }

    #[allow(clippy::expect_used)]
    pub fn start_event_loops(&mut self, session: Arc<Mutex<Option<Session>>>) {
        let logger = self.logger.clone();
        let maybe_media_transport_event_tx = self.media_transport_event_tx();

        let maybe_media_transport_event_tx_clone = maybe_media_transport_event_tx.clone();
        self.media_transport_event_tx = maybe_media_transport_event_tx_clone;

        let (packetizer_order_tx, packetizer_order_rx) = mpsc::channel();
        let (packetizer_event_tx, packetizer_event_rx) = mpsc::channel();

        if let Some(media_transport_event_tx) = maybe_media_transport_event_tx {
            let _ = self
                .media_agent
                .start(self.event_tx.clone(), media_transport_event_tx);
        }

        // Start Depacketizer worker
        let mut payload_map_inner = HashMap::new();
        let mut current_pt = DYNAMIC_PAYLOAD_TYPE_START;

        for spec in self.media_agent.supported_media() {
            let codec_descriptor = match spec.codec_spec {
                CodecSpec::H264 => CodecDescriptor::h264_dynamic(current_pt),
            };
            payload_map_inner.insert(current_pt, codec_descriptor);
            current_pt += 1;
        }

        let payload_map = Arc::new(payload_map_inner);
        let (rtp_tx, rtp_rx) = mpsc::sync_channel::<RtpIn>(RTP_TX_CHANNEL_SIZE);
        let rtp_tx_clone = rtp_tx;
        self.rtp_tx = Some(rtp_tx_clone);
        let allowed_pts = Arc::new(RwLock::new(
            payload_map.keys().copied().collect::<HashSet<u8>>(),
        ));
        let allowed_pts_clone = allowed_pts.clone();
        self.allowed_pts = Some(allowed_pts_clone);

        let payload_map_for_worker = payload_map.clone();
        self.payload_map = payload_map;
        let (depacketizer_event_tx, depacketizer_event_rx) = mpsc::channel();
        self.depacketizer_handle = Some(spawn_depacketizer_worker(
            logger.clone(),
            allowed_pts.clone(),
            rtp_rx,
            depacketizer_event_tx,
            payload_map_for_worker.clone(),
        ));
        //Start DepacketizerEventLoop
        if let Some(media_agent_event_tx) = self.media_agent.media_agent_event_tx() {
            self.depacketizer_event_loop
                .start(depacketizer_event_rx, media_agent_event_tx);
        } else {
            sink_error!(
                self.logger,
                "[MediaTransport] My MediaAgent does not have an event_tx"
            );
        }

        let media_transport_event_rx = self
            .media_transport_event_rx
            .take()
            .expect("MediaTransport event receiver missing (already started?)");

        if let Some(rtp_tx) = self.rtp_tx.clone()
            && let Some(allowed_pts) = self.allowed_pts.clone()
            && let Some(media_agent_event_tx) = self.media_agent.media_agent_event_tx()
        {
            self.media_agent_event_loop.start(
                media_transport_event_rx,
                packetizer_order_tx,
                rtp_tx.clone(),
                session.clone(),
                payload_map_for_worker.clone(),
                self.outbound_tracks.clone(),
                self.event_tx.clone(),
                allowed_pts.clone(),
                media_agent_event_tx,
            );
        }

        self.packetizer_handle = Some(spawn_packetizer_worker(
            packetizer_order_rx,
            packetizer_event_tx,
            logger.clone(),
        ));
        self.packetizer_event_loop.start(
            packetizer_event_rx,
            self.outbound_tracks.clone(),
            payload_map_for_worker.clone(),
            session,
            self.event_tx.clone(),
        );
    }

    #[must_use]
    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        self.media_agent.snapshot_frames()
    }

    #[must_use]
    pub fn codec_descriptors(&self) -> Vec<CodecDescriptor> {
        self.payload_map.values().cloned().collect()
    }

    pub fn local_rtp_codecs(&self) -> Vec<RtpCodec> {
        self.payload_map
            .values()
            .map(|c| c.rtp_representation.clone())
            .collect()
    }

    pub fn media_transport_event_tx(&self) -> Option<Sender<MediaTransportEvent>> {
        self.media_transport_event_tx.clone()
    }

    pub fn stop(&mut self) {
        sink_info!(self.logger, "[MediaTransport] Stopping...");
        self.media_agent.stop();

        self.media_agent_event_loop.stop();
        self.depacketizer_event_loop.stop();
        self.packetizer_event_loop.stop();

        self.media_transport_event_tx = None;

        self.rtp_tx = None;

        if let Some(handle) = self.depacketizer_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.packetizer_handle.take() {
            let _ = handle.join();
        }

        self.allowed_pts = None;
        self.payload_map = Arc::new(HashMap::new());
        sink_info!(self.logger, "[MediaTransport] Stopped");
    }
}
