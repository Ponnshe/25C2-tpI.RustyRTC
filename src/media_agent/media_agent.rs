use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    core::events::EngineEvent,
    log::log_sink::LogSink,
    media_agent::{
        camera_worker::spawn_camera_worker,
        decoder_event::DecoderEvent,
        decoder_worker::spawn_decoder_worker,
        encoder_instruction::EncoderInstruction,
        encoder_worker::spawn_encoder_worker,
        events::MediaAgentEvent,
        media_agent_error::MediaAgentError,
        spec::{CodecSpec, MediaSpec, MediaType},
        utils::discover_camera_id,
        video_frame::VideoFrame,
    },
    media_transport::media_transport_event::MediaTransportEvent,
    sink_debug, sink_error, sink_info, sink_trace, sink_warn,
};

use super::constants::{DEFAULT_CAMERA_ID, KEYINT, TARGET_FPS};

pub struct MediaAgent {
    logger: Arc<dyn LogSink>,
    local_frame: Arc<Mutex<Option<VideoFrame>>>,
    remote_frame: Arc<Mutex<Option<VideoFrame>>>,
    supported_media: Vec<MediaSpec>,
    decoder_handle: Option<JoinHandle<()>>,
    encoder_handle: Option<JoinHandle<()>>,
    listener_handle: Option<JoinHandle<()>>,
    camera_handle: Option<JoinHandle<()>>,
    sent_any_frame: Arc<AtomicBool>,
    media_agent_event_tx: Option<Sender<MediaAgentEvent>>,
    ma_encoder_event_tx: Option<Sender<EncoderInstruction>>,
    running: Arc<AtomicBool>,
}

impl MediaAgent {
    pub fn new(logger: Arc<dyn LogSink>) -> Self {
        let sent_any_frame = Arc::new(AtomicBool::new(false));

        let supported_media = vec![MediaSpec {
            media_type: MediaType::Video,
            codec_spec: CodecSpec::H264,
        }];

        Self {
            logger,
            local_frame: Arc::new(Mutex::new(None)),
            remote_frame: Arc::new(Mutex::new(None)),
            supported_media,
            decoder_handle: None,
            encoder_handle: None,
            listener_handle: None,
            camera_handle: None,
            sent_any_frame,
            media_agent_event_tx: None,
            ma_encoder_event_tx: None,
            running: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn start(
        &mut self,
        event_tx: Sender<EngineEvent>,
        media_transport_event_tx: Sender<MediaTransportEvent>,
    ) -> Result<(), MediaAgentError> {
        let logger = self.logger.clone();
        sink_debug!(logger, "[MediaAgent] Starting MediaAgent");
        self.running.store(true, Ordering::SeqCst);
        let logger = self.logger.clone();
        let running = self.running.clone();
        let remote_frame = self.remote_frame.clone();
        let local_frame = self.local_frame.clone();

        //Start camera worker
        let camera_id = discover_camera_id().unwrap_or(DEFAULT_CAMERA_ID);
        sink_debug!(logger.clone(), "[MediaAgent] Starting Camera Worker...");
        let (local_frame_rx, status, handle) =
            spawn_camera_worker(TARGET_FPS, logger.clone(), camera_id, running.clone());
        sink_debug!(logger.clone(), "[MediaAgent] Camera Worker Started");
        if let Some(msg) = status {
            let _ = event_tx.send(EngineEvent::Status(format!("[MediaAgent] {msg}")));
        }
        self.camera_handle = handle;

        let (ma_decoder_event_tx, ma_decoder_event_rx) = mpsc::channel::<DecoderEvent>();
        let (media_agent_event_tx, media_agent_event_rx) = mpsc::channel::<MediaAgentEvent>();
        let media_agent_event_tx_clone = media_agent_event_tx.clone();
        self.media_agent_event_tx = Some(media_agent_event_tx_clone);

        // Start decoder worker
        sink_debug!(logger.clone(), "[MediaAgent] Starting Decoder Worker...");
        let decoder_handle = Some(spawn_decoder_worker(
            logger.clone(),
            ma_decoder_event_rx,
            media_agent_event_tx.clone(),
            running.clone(),
        ));
        self.decoder_handle = decoder_handle;
        sink_debug!(logger.clone(), "[MediaAgent] Decoder Worker Started");

        // Start encoder worker
        let (ma_encoder_event_tx, ma_encoder_event_rx) = mpsc::channel::<EncoderInstruction>();
        let ma_encoder_event_tx_clone = ma_encoder_event_tx.clone();
        self.ma_encoder_event_tx = Some(ma_encoder_event_tx_clone);
        sink_debug!(logger.clone(), "[MediaAgent] Starting Encoder Worker...");
        let encoder_handle = spawn_encoder_worker(
            logger.clone(),
            ma_encoder_event_rx,
            media_agent_event_tx,
            running.clone(),
        )
        .map_err(|e| MediaAgentError::EncoderSpawn(e.to_string()))?;
        self.encoder_handle = Some(encoder_handle);
        sink_debug!(logger.clone(), "[MediaAgent] Encoder Worker Started");

        // Start listener
        sink_debug!(logger.clone(), "[MediaAgent] Starting Listener...");
        let listener_handle = Self::spawn_listener_thread(
            logger.clone(),
            local_frame_rx,
            media_agent_event_rx,
            ma_decoder_event_tx,
            ma_encoder_event_tx,
            media_transport_event_tx,
            local_frame,
            remote_frame,
            self.sent_any_frame.clone(),
            running,
        );
        self.listener_handle = listener_handle;
        sink_info!(logger.clone(), "[MediaAgent] Listener Started");

        Ok(())
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        self.media_agent_event_tx = None;
        self.ma_encoder_event_tx = None;

        if let Some(handle) = self.listener_handle.take() {
            let _ = handle.join();
        }

        if let Some(handle) = self.decoder_handle.take() {
            let _ = handle.join();
        }

        if let Some(handle) = self.encoder_handle.take() {
            let _ = handle.join();
        }

        if let Some(handle) = self.camera_handle.take() {
            let _ = handle.join();
        }

        self.sent_any_frame.store(false, Ordering::SeqCst);

        if let Ok(mut lf) = self.local_frame.lock() {
            *lf = None;
        }

        if let Ok(mut rf) = self.remote_frame.lock() {
            *rf = None;
        }

        sink_debug!(self.logger, "[MediaAgent] stopped cleanly");
    }

    #[must_use]
    pub fn supported_media(&self) -> &[MediaSpec] {
        &self.supported_media
    }

    pub fn post_event(&self, event: MediaAgentEvent) {
        if let Some(media_agent_event_tx) = self.media_agent_event_tx.clone()
            && let Err(err) = media_agent_event_tx.send(event)
        {
            sink_error!(
                self.logger,
                "[MediaAgent] failed to enqueue event for listener: {err}"
            );
        }
    }

    #[must_use]
    pub fn media_agent_event_tx(&self) -> Option<Sender<MediaAgentEvent>> {
        self.media_agent_event_tx.clone()
    }

    #[must_use]
    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        let local = self
            .local_frame
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().cloned());
        let remote = self
            .remote_frame
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().cloned());
        (local, remote)
    }
    #[allow(clippy::too_many_arguments)]
    fn spawn_listener_thread(
        logger: Arc<dyn LogSink>,
        local_frame_rx: Receiver<VideoFrame>,
        media_agent_event_rx: Receiver<MediaAgentEvent>,
        ma_decoder_event_tx: Sender<DecoderEvent>,
        ma_encoder_event_tx: Sender<EncoderInstruction>,
        media_transport_event_tx: Sender<MediaTransportEvent>,
        local_frame: Arc<Mutex<Option<VideoFrame>>>,
        remote_frame: Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: Arc<AtomicBool>,
        running: Arc<AtomicBool>,
    ) -> Option<JoinHandle<()>> {
        sink_info!(logger, "[MA Listener] Starting...");
        thread::Builder::new()
            .name("media-agent-listener".into())
            .spawn(move || {
                Self::listener_loop(
                    logger,
                    local_frame_rx,
                    media_agent_event_rx,
                    ma_decoder_event_tx,
                    ma_encoder_event_tx,
                    media_transport_event_tx,
                    local_frame,
                    remote_frame,
                    sent_any_frame,
                    running,
                );
            })
            .ok()
    }
    #[allow(clippy::too_many_arguments)]
    fn listener_loop(
        logger: Arc<dyn LogSink>,
        local_frame_rx: Receiver<VideoFrame>,
        media_agent_event_rx: Receiver<MediaAgentEvent>,
        ma_decoder_event_tx: Sender<DecoderEvent>,
        ma_encoder_event_tx: Sender<EncoderInstruction>,
        media_transport_event_tx: Sender<MediaTransportEvent>,
        local_frame: Arc<Mutex<Option<VideoFrame>>>,
        remote_frame: Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: Arc<AtomicBool>,
        running: Arc<AtomicBool>,
    ) {
        while running.load(Ordering::Relaxed) {
            Self::drain_camera_frames(
                &logger,
                &local_frame_rx,
                &ma_encoder_event_tx,
                &local_frame,
                &sent_any_frame,
            );

            match media_agent_event_rx.recv_timeout(Duration::from_millis(5)) {
                Ok(event) => {
                    Self::handle_media_agent_event(
                        &logger,
                        event,
                        &ma_decoder_event_tx,
                        &ma_encoder_event_tx,
                        &media_transport_event_tx,
                        &remote_frame,
                    );
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    sink_debug!(
                        logger,
                        "[MediaAgent] listener thread exiting: event channel closed"
                    );
                    break;
                }
            }
        }
        sink_debug!(logger, "[MediaAgent Listener] Thread closing gracefully");
    }

    fn drain_camera_frames(
        logger: &Arc<dyn LogSink>,
        local_frame_rx: &Receiver<VideoFrame>,
        ma_encoder_event_tx: &Sender<EncoderInstruction>,
        local_frame: &Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: &Arc<AtomicBool>,
    ) {
        loop {
            match local_frame_rx.try_recv() {
                Ok(frame) => {
                    Self::handle_local_frame(
                        logger,
                        frame,
                        ma_encoder_event_tx,
                        local_frame,
                        sent_any_frame,
                    );
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    sink_debug!(logger, "[MediaAgent] camera worker disconnected");
                    break;
                }
            }
        }
    }

    fn handle_local_frame(
        logger: &Arc<dyn LogSink>,
        frame: VideoFrame,
        ma_encoder_event_tx: &Sender<EncoderInstruction>,
        local_frame: &Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: &Arc<AtomicBool>,
    ) {
        if let Ok(mut guard) = local_frame.lock() {
            *guard = Some(frame.clone());
        } else {
            sink_warn!(logger, "[MediaAgent] failed to lock local frame for update");
        }

        let force_keyframe = !sent_any_frame.swap(true, Ordering::SeqCst);

        let ts = frame.timestamp_ms;
        let instruction = EncoderInstruction::Encode(frame, force_keyframe);
        if ma_encoder_event_tx.send(instruction).is_err() {
            sink_error!(
                logger,
                "[MediaAgent] encoder worker offline, dropping local frame"
            );
        } else {
            sink_trace!(
                logger,
                "[MediaAgent] queued local frame (ts={}, force_keyframe={})",
                ts,
                force_keyframe
            );
        }
    }

    fn handle_media_agent_event(
        logger: &Arc<dyn LogSink>,
        event: MediaAgentEvent,
        ma_decoder_event_tx: &Sender<DecoderEvent>,
        ma_encoder_event_tx: &Sender<EncoderInstruction>,
        media_transport_event_tx: &Sender<MediaTransportEvent>,
        remote_frame: &Arc<Mutex<Option<VideoFrame>>>,
    ) {
        match event {
            MediaAgentEvent::DecodedVideoFrame(frame) => {
                sink_info!(logger, "[MediaAgent] Received DecodedVideoFrame");
                let frame = *frame;
                let ts = frame.timestamp_ms;
                if let Ok(mut guard) = remote_frame.lock() {
                    *guard = Some(frame);
                } else {
                    sink_warn!(logger, "[MediaAgent] failed to update remote frame");
                    return;
                }
                sink_debug!(
                    logger,
                    "[MediaAgent] updated remote frame snapshot (ts={ts})"
                );
            }
            MediaAgentEvent::EncodedVideoFrame {
                annexb_frame,
                timestamp_ms,
                codec_spec,
            } => {
                sink_trace!(
                    logger,
                    "[MediaAgent] encoded frame ready for transport (ts={timestamp_ms})"
                );
                sink_debug!(
                    logger,
                    "[MediaAgent] Received EncodedVideoFrame from Encoder. Now sending SendEncodedFrame to Media Transport"
                );
                if media_transport_event_tx
                    .send(MediaTransportEvent::SendEncodedFrame {
                        annexb_frame,
                        timestamp_ms,
                        codec_spec,
                    })
                    .is_err()
                {
                    sink_warn!(
                        logger,
                        "[MediaAgent] media transport channel dropped encoded frame"
                    );
                }
            }
            MediaAgentEvent::AnnexBFrameReady { codec_spec, bytes } => {
                sink_trace!(
                    logger,
                    "[MediaAgent] forwarding AnnexB payload to decoder ({:?})",
                    codec_spec
                );
                if ma_decoder_event_tx
                    .send(DecoderEvent::AnnexBFrameReady { codec_spec, bytes })
                    .is_err()
                {
                    sink_warn!(
                        logger,
                        "[MediaAgent] decoder worker offline, dropping AnnexB frame"
                    );
                }
            }
            MediaAgentEvent::UpdateBitrate(b) => {
                let instruction = EncoderInstruction::SetConfig {
                    fps: TARGET_FPS,
                    bitrate: b,
                    keyint: KEYINT,
                };
                if ma_encoder_event_tx.send(instruction).is_ok() {
                    sink_debug!(logger, "Reconfigured H264 encoder: bitrate={}bps", b,);
                }
            }
        }
    }
}
