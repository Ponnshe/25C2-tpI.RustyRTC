use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    app::log_sink::LogSink,
    core::events::EngineEvent,
    logger_debug, logger_error, logger_info, logger_warn,
    media_agent::{
        camera_worker::spawn_camera_worker,
        decoder_event::DecoderEvent,
        decoder_worker::spawn_decoder_worker,
        encoder_instruction::EncoderInstruction,
        encoder_worker::spawn_encoder_worker,
        events::MediaAgentEvent,
        spec::{CodecSpec, MediaSpec, MediaType},
        utils::discover_camera_id,
        video_frame::VideoFrame,
    },
    media_transport::media_transport_event::MediaTransportEvent,
};

use super::constants::{DEFAULT_CAMERA_ID, TARGET_FPS, KEYINT};

pub struct MediaAgent {
    local_frame: Arc<Mutex<Option<VideoFrame>>>,
    remote_frame: Arc<Mutex<Option<VideoFrame>>>,
    pub logger: Arc<dyn LogSink>,
    supported_media: Vec<MediaSpec>,
    _decoder_handle: Option<JoinHandle<()>>,
    _encoder_handle: Option<JoinHandle<()>>,
    _listener_handle: Option<JoinHandle<()>>,
    _ma_decoder_event_tx: Sender<DecoderEvent>,
    ma_encoder_event_tx: Sender<EncoderInstruction>,
    media_agent_event_tx: Sender<MediaAgentEvent>,
    _sent_any_frame: Arc<Mutex<bool>>,
}

impl MediaAgent {
    pub fn new(
        event_tx: Sender<EngineEvent>,
        logger: Arc<dyn LogSink>,
        media_transport_event_tx: Sender<MediaTransportEvent>,
    ) -> Self {
        let camera_id = discover_camera_id().unwrap_or(DEFAULT_CAMERA_ID);
        let remote_frame = Arc::new(Mutex::new(None));
        let local_frame = Arc::new(Mutex::new(None));
        let sent_any_frame = Arc::new(Mutex::new(false));

        let (local_frame_rx, status, _handle) =
            spawn_camera_worker(TARGET_FPS, logger.clone(), camera_id);
        if let Some(msg) = status {
            let _ = event_tx.send(EngineEvent::Status(format!("[MediaAgent] {msg}")));
        }

        let supported_media = vec![MediaSpec {
            media_type: MediaType::Video,
            codec_spec: CodecSpec::H264,
        }];

        let (ma_decoder_event_tx, ma_decoder_event_rx) = mpsc::channel::<DecoderEvent>();
        let (media_agent_event_tx, media_agent_event_rx) = mpsc::channel::<MediaAgentEvent>();

        let decoder_handle = Some(spawn_decoder_worker(
            logger.clone(),
            ma_decoder_event_rx,
            media_agent_event_tx.clone(),
        ));

        let (ma_encoder_event_tx, ma_encoder_event_rx) = mpsc::channel::<EncoderInstruction>();
        let encoder_handle = Some(spawn_encoder_worker(
            logger.clone(),
            ma_encoder_event_rx,
            media_agent_event_tx.clone(),
        ));

        let listener_handle = Self::spawn_listener_thread(
            logger.clone(),
            local_frame_rx,
            media_agent_event_rx,
            ma_decoder_event_tx.clone(),
            ma_encoder_event_tx.clone(),
            media_transport_event_tx,
            local_frame.clone(),
            remote_frame.clone(),
            sent_any_frame.clone(),
        );

        Self {
            local_frame,
            remote_frame,
            logger,
            supported_media,
            _decoder_handle: decoder_handle,
            _encoder_handle: encoder_handle,
            _listener_handle: listener_handle,
            _ma_decoder_event_tx: ma_decoder_event_tx,
            ma_encoder_event_tx,
            media_agent_event_tx,
            _sent_any_frame: sent_any_frame,
        }
    }

    #[must_use]
    pub fn supported_media(&self) -> &[MediaSpec] {
        &self.supported_media
    }

    pub fn post_event(&self, event: MediaAgentEvent) {
        if let Err(err) = self.media_agent_event_tx.send(event) {
            logger_error!(
                self.logger,
                "[MediaAgent] failed to enqueue event for listener: {err}"
            );
        }
    }

    #[must_use]
    pub fn media_agent_event_tx(&self) -> Sender<MediaAgentEvent> {
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

    pub fn set_bitrate(&self, new_bitrate: u32) {
        let instruction = EncoderInstruction::SetConfig {
            fps: TARGET_FPS,
            bitrate: new_bitrate,
            keyint: KEYINT,
        };
        if self.ma_encoder_event_tx.send(instruction).is_ok() {
            logger_info!(
                self.logger,
                "Reconfigured H264 encoder: bitrate={}bps",
                new_bitrate,
            );
        }
    }

    fn spawn_listener_thread(
        logger: Arc<dyn LogSink>,
        local_frame_rx: Receiver<VideoFrame>,
        media_agent_event_rx: Receiver<MediaAgentEvent>,
        ma_decoder_event_tx: Sender<DecoderEvent>,
        ma_encoder_event_tx: Sender<EncoderInstruction>,
        media_transport_event_tx: Sender<MediaTransportEvent>,
        local_frame: Arc<Mutex<Option<VideoFrame>>>,
        remote_frame: Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: Arc<Mutex<bool>>,
    ) -> Option<JoinHandle<()>> {
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
                );
            })
            .ok()
    }

    fn listener_loop(
        logger: Arc<dyn LogSink>,
        local_frame_rx: Receiver<VideoFrame>,
        media_agent_event_rx: Receiver<MediaAgentEvent>,
        ma_decoder_event_tx: Sender<DecoderEvent>,
        ma_encoder_event_tx: Sender<EncoderInstruction>,
        media_transport_event_tx: Sender<MediaTransportEvent>,
        local_frame: Arc<Mutex<Option<VideoFrame>>>,
        remote_frame: Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: Arc<Mutex<bool>>,
    ) {
        loop {
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
                        &media_transport_event_tx,
                        &remote_frame,
                    );
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    logger_info!(
                        logger,
                        "[MediaAgent] listener thread exiting: event channel closed"
                    );
                    break;
                }
            }
        }
    }

    fn drain_camera_frames(
        logger: &Arc<dyn LogSink>,
        local_frame_rx: &Receiver<VideoFrame>,
        ma_encoder_event_tx: &Sender<EncoderInstruction>,
        local_frame: &Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: &Arc<Mutex<bool>>,
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
                    logger_warn!(logger, "[MediaAgent] camera worker disconnected");
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
        sent_any_frame: &Arc<Mutex<bool>>,
    ) {
        if let Ok(mut guard) = local_frame.lock() {
            *guard = Some(frame.clone());
        } else {
            logger_warn!(logger, "[MediaAgent] failed to lock local frame for update");
        }

        let force_keyframe = {
            let mut sent = sent_any_frame.lock().unwrap();
            if !*sent {
                *sent = true;
                true
            } else {
                false
            }
        };

        let ts = frame.timestamp_ms;
        let instruction = EncoderInstruction::Encode(frame, force_keyframe);
        if ma_encoder_event_tx.send(instruction).is_err() {
            logger_error!(
                logger,
                "[MediaAgent] encoder worker offline, dropping local frame"
            );
        } else {
            logger_debug!(
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
        media_transport_event_tx: &Sender<MediaTransportEvent>,
        remote_frame: &Arc<Mutex<Option<VideoFrame>>>,
    ) {
        match event {
            MediaAgentEvent::DecodedVideoFrame(frame) => {
                let frame = *frame;
                let ts = frame.timestamp_ms;
                if let Ok(mut guard) = remote_frame.lock() {
                    *guard = Some(frame);
                } else {
                    logger_warn!(logger, "[MediaAgent] failed to update remote frame");
                    return;
                }
                logger_debug!(
                    logger,
                    "[MediaAgent] updated remote frame snapshot (ts={ts})"
                );
            }
            MediaAgentEvent::EncodedVideoFrame {
                annexb_frame,
                timestamp_ms,
                codec_spec,
            } => {
                logger_debug!(
                    logger,
                    "[MediaAgent] encoded frame ready for transport (ts={timestamp_ms})"
                );
                if media_transport_event_tx
                    .send(MediaTransportEvent::SendEncodedFrame {
                        annexb_frame,
                        timestamp_ms,
                        codec_spec,
                    })
                    .is_err()
                {
                    logger_warn!(
                        logger,
                        "[MediaAgent] media transport channel dropped encoded frame"
                    );
                }
            }
            MediaAgentEvent::AnnexBFrameReady { codec_spec, bytes } => {
                logger_debug!(
                    logger,
                    "[MediaAgent] forwarding AnnexB payload to decoder ({:?})",
                    codec_spec
                );
                if ma_decoder_event_tx
                    .send(DecoderEvent::AnnexBFrameReady { codec_spec, bytes })
                    .is_err()
                {
                    logger_warn!(
                        logger,
                        "[MediaAgent] decoder worker offline, dropping AnnexB frame"
                    );
                }
            }
        }
    }
}
