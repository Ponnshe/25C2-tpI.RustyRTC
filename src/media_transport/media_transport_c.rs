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

/// The high-level orchestrator that bridges the Application Layer (`MediaAgent`)
/// and the Network Layer (`RtpSession`).
///
/// `MediaTransport` owns the entire media pipeline infrastructure. It is responsible for:
/// 1. **Initialization**: Setting up codecs, negotiating Payload Types (PT), and allocating buffers.
/// 2. **Lifecycle Management**: Spawning and stopping the Packetizer, Depacketizer, and Event Loops.
/// 3. **Routing**: Connecting the output of the Encoder to the Packetizer, and the output of the
///    Depacketizer to the Decoder.
pub struct MediaTransport {
    logger: Arc<dyn LogSink>,
    /// Channel to bubble up critical status events to the main engine.
    event_tx: Sender<EngineEvent>,
    
    /// The application-side logic (Camera, Encoder, Decoder).
    media_agent: MediaAgent,
    
    // --- Event Loops (Logic Processors) ---
    media_agent_event_loop: MediaAgentEventLoop,
    depacketizer_event_loop: DepacketizerEventLoop,
    packetizer_event_loop: PacketizerEventLoop,

    // --- Networking & Threading ---
    /// Synchronous sender to the RTP network socket (outbound).
    rtp_tx: Option<SyncSender<RtpIn>>,
    depacketizer_handle: Option<JoinHandle<()>>,
    packetizer_handle: Option<JoinHandle<()>>,

    // --- Shared State ---
    /// Maps RTP Payload Types (e.g., 96) to internal Codec Descriptors.
    payload_map: Arc<HashMap<u8, CodecDescriptor>>,
    /// Tracks state for outbound RTP streams (SSRCs, sequence numbers).
    outbound_tracks: Arc<Mutex<HashMap<u8, OutboundTrackHandle>>>,
    /// Filter set for incoming RTP packets (only allow negotiated PTs).
    allowed_pts: Option<Arc<RwLock<HashSet<u8>>>>,

    // --- Internal Channels ---
    media_transport_event_tx: Option<Sender<MediaTransportEvent>>,
    media_transport_event_rx: Option<Receiver<MediaTransportEvent>>,
}

impl MediaTransport {
    /// Creates a new `MediaTransport` instance.
    ///
    /// This initializes the internal structures and the `MediaAgent`, but does **not**
    /// start the background threads or event loops yet. Call [`start_event_loops`](Self::start_event_loops)
    /// to activate the pipeline.
    pub fn new(
        event_tx: Sender<EngineEvent>,
        logger: Arc<dyn LogSink>,
        config: Arc<Config>,
    ) -> Self {
        let media_agent = MediaAgent::new(logger.clone(), config.clone());
        let target_fps = config
            .get("Media", "fps")
            .and_then(|s| s.parse().ok())
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

    /// Activates the media pipeline and connects it to the network session.
    ///
    /// This method:
    /// 1. Starts the `MediaAgent` (Camera/Encoder/Decoder).
    /// 2. Maps supported codecs to dynamic RTP Payload Types (starting at 96).
    /// 3. Spawns the **Depacketizer Worker** (RTP -> Frames).
    /// 4. Spawns the **Packetizer Worker** (Frames -> RTP).
    /// 5. Starts all event loops to manage message routing.
    ///
    /// # Arguments
    ///
    /// * `session` - The network session (RTP/UDP socket wrapper) used to send packets.
    ///
    /// # Panics
    ///
    /// Panics if the `media_transport_event_rx` has already been consumed (i.e., called twice).
    #[allow(clippy::expect_used)]
    pub fn start_event_loops(&mut self, session: Arc<Mutex<Option<Session>>>) {
        let logger = self.logger.clone();
        let maybe_media_transport_event_tx = self.media_transport_event_tx();

        let maybe_media_transport_event_tx_clone = maybe_media_transport_event_tx.clone();
        self.media_transport_event_tx = maybe_media_transport_event_tx_clone;

        let (packetizer_order_tx, packetizer_order_rx) = mpsc::channel();
        let (packetizer_event_tx, packetizer_event_rx) = mpsc::channel();

        // 1. Start MediaAgent (Application Logic)
        if let Some(media_transport_event_tx) = maybe_media_transport_event_tx {
            let _ = self
                .media_agent
                .start(self.event_tx.clone(), media_transport_event_tx);
        }

        // 2. Build Payload Map (Negotiate Codecs)
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

        // 3. Start Depacketizer (Ingress)
        let (depacketizer_event_tx, depacketizer_event_rx) = mpsc::channel();
        self.depacketizer_handle = Some(spawn_depacketizer_worker(
            logger.clone(),
            allowed_pts.clone(),
            rtp_rx,
            depacketizer_event_tx,
            payload_map_for_worker.clone(),
        ));
        
        // Connect Depacketizer output -> MediaAgent input
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

        // 4. Start Event Loop (Control Logic)
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

        // 5. Start Packetizer (Egress)
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

    /// Passthrough to get the latest video snapshots from the `MediaAgent`.
    #[must_use]
    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        self.media_agent.snapshot_frames()
    }

    /// Returns the list of supported codecs as descriptors for SDP generation.
    #[must_use]
    pub fn codec_descriptors(&self) -> Vec<CodecDescriptor> {
        self.payload_map.values().cloned().collect()
    }

    /// Returns the RTP specific codec configurations (PT, ClockRate, Name).
    pub fn local_rtp_codecs(&self) -> Vec<RtpCodec> {
        self.payload_map
            .values()
            .map(|c| c.rtp_representation.clone())
            .collect()
    }

    /// Clones the sender channel for internal event routing.
    pub fn media_transport_event_tx(&self) -> Option<Sender<MediaTransportEvent>> {
        self.media_transport_event_tx.clone()
    }

    /// Stops all threads and cleans up resources.
    ///
    /// This stops the `MediaAgent` first, then the transport event loops,
    /// ensuring a graceful shutdown of the pipeline from Top (App) to Bottom (Network).
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
