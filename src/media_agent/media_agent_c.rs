use super::constants::{KEYINT, TARGET_FPS};
use crate::config::Config;
use crate::media_agent::constants::DEFAULT_CAMERA_ID;
use crate::{
    core::events::EngineEvent,
    log::log_sink::LogSink,
    media_agent::{
        audio_capture_worker::{AudioCaptureEvent, spawn_audio_capture_worker},
        audio_codec,
        audio_player_worker::{AudioPlayerCommand, spawn_audio_player_worker},
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
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// The central orchestrator of the media pipeline.
///
/// `MediaAgent` is responsible for managing the lifecycle of all media-related subsystems:
/// 1. **Capture**: Spawns and manages the `CameraWorker`.
/// 2. **Encoding**: Spawns the `EncoderWorker` to compress local video.
/// 3. **Decoding**: Spawns the `DecoderWorker` to decompress remote video.
/// 4. **Routing**: Runs a central `Listener` thread that routes messages between workers and the `MediaTransport`.
///
/// # Shared State
/// It holds shared `Mutex` protected references to the latest `local_frame` and `remote_frame`,
/// allowing the UI layer to poll for the most recent images to render without blocking the processing pipeline.
pub struct MediaAgent {
    logger: Arc<dyn LogSink>,
    /// The most recent frame captured from the local camera (for UI preview).
    local_frame: Arc<Mutex<Option<VideoFrame>>>,
    /// The most recent frame decoded from the remote peer (for UI display).
    remote_frame: Arc<Mutex<Option<VideoFrame>>>,
    /// List of supported codecs and media types.
    supported_media: Vec<MediaSpec>,

    // --- Thread Handles ---
    decoder_handle: Option<JoinHandle<()>>,
    encoder_handle: Option<JoinHandle<()>>,
    listener_handle: Option<JoinHandle<()>>,
    camera_handle: Option<JoinHandle<()>>,
    audio_handle: Option<JoinHandle<()>>,
    audio_player_handle: Option<JoinHandle<()>>,

    /// Flag to track if we have successfully sent at least one keyframe.
    sent_any_frame: Arc<AtomicBool>,

    // --- Channels ---
    /// Channel to send events back to the listener loop from outside.
    media_agent_event_tx: Option<Sender<MediaAgentEvent>>,
    /// Channel to send instructions to the encoder worker.
    ma_encoder_event_tx: Option<Sender<EncoderInstruction>>,
    /// Channel to send instructions to the audio player worker.
    audio_player_tx: Option<Sender<AudioPlayerCommand>>,

    running: Arc<AtomicBool>,
    is_audio_muted: Arc<AtomicBool>,
    config: Arc<Config>,
}

struct MediaAgentContext<'a> {
    logger: &'a Arc<dyn LogSink>,
    ma_decoder_event_tx: &'a Sender<DecoderEvent>,
    ma_encoder_event_tx: &'a Sender<EncoderInstruction>,
    audio_player_tx: &'a Sender<AudioPlayerCommand>,
    media_transport_event_tx: &'a Sender<MediaTransportEvent>,
    remote_frame: &'a Arc<Mutex<Option<VideoFrame>>>,
    config: &'a Arc<Config>,
}

impl MediaAgent {
    /// Creates a new `MediaAgent` instance.
    ///
    /// This only initializes the data structures. To start the processing threads,
    /// call [`start`](Self::start).
    pub fn new(logger: Arc<dyn LogSink>, config: Arc<Config>) -> Self {
        let sent_any_frame = Arc::new(AtomicBool::new(false));

        let supported_media = vec![
            MediaSpec {
                media_type: MediaType::Video,
                codec_spec: CodecSpec::H264,
            },
            MediaSpec {
                media_type: MediaType::Audio,
                codec_spec: CodecSpec::G711U,
            },
        ];

        Self {
            logger,
            local_frame: Arc::new(Mutex::new(None)),
            remote_frame: Arc::new(Mutex::new(None)),
            supported_media,
            decoder_handle: None,
            encoder_handle: None,
            listener_handle: None,
            camera_handle: None,
            audio_handle: None,
            audio_player_handle: None,
            sent_any_frame,
            media_agent_event_tx: None,
            ma_encoder_event_tx: None,
            audio_player_tx: None,
            running: Arc::new(AtomicBool::new(false)),
            is_audio_muted: Arc::new(AtomicBool::new(false)),
            config,
        }
    }

    /// Bootstraps the media pipeline.
    ///
    /// Spawns the Camera, Encoder, Decoder, and Listener threads.
    /// It also reads configuration values (FPS, Camera ID) from `Config`.
    ///
    /// # Arguments
    ///
    /// * `event_tx` - Channel to send status updates back to the main Engine.
    /// * `media_transport_event_tx` - Channel to send encoded packets to the network layer.
    ///
    /// # Errors
    ///
    /// Returns `MediaAgentError` if any worker thread fails to spawn.
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

        let default_camera_id = self
            .config
            .get("Media", "default_camera")
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_CAMERA_ID);

        // --- 1. Start Camera Worker ---
        let camera_id = discover_camera_id().unwrap_or(default_camera_id);
        sink_debug!(logger.clone(), "[MediaAgent] Starting Camera Worker...");

        let target_fps = self
            .config
            .get("Media", "fps")
            .and_then(|s| s.parse().ok())
            .unwrap_or(TARGET_FPS);

        let (local_frame_rx, status, handle) =
            spawn_camera_worker(target_fps, logger.clone(), camera_id, running.clone());
        sink_debug!(logger.clone(), "[MediaAgent] Camera Worker Started");

        if let Some(msg) = status {
            let _ = event_tx.send(EngineEvent::Status(format!("[MediaAgent] {msg}")));
        }
        self.camera_handle = handle;

        // --- Start Audio Capture Worker ---
        sink_debug!(
            logger.clone(),
            "[MediaAgent] Starting Audio Capture Worker..."
        );
        let (audio_frame_rx, audio_handle) = spawn_audio_capture_worker(
            logger.clone(),
            running.clone(),
            self.is_audio_muted.clone(),
        );
        self.audio_handle = audio_handle;
        sink_debug!(logger.clone(), "[MediaAgent] Audio Capture Worker Started");

        // --- Start Audio Player Worker ---
        let (audio_player_tx, audio_player_rx) = mpsc::channel();
        self.audio_player_tx = Some(audio_player_tx.clone());

        sink_debug!(
            logger.clone(),
            "[MediaAgent] Starting Audio Player Worker..."
        );
        let audio_player_handle =
            spawn_audio_player_worker(logger.clone(), audio_player_rx, running.clone());
        self.audio_player_handle = Some(audio_player_handle);
        sink_debug!(logger.clone(), "[MediaAgent] Audio Player Worker Started");

        // Setup internal channels
        let (ma_decoder_event_tx, ma_decoder_event_rx) = mpsc::channel::<DecoderEvent>();
        let (media_agent_event_tx, media_agent_event_rx) = mpsc::channel::<MediaAgentEvent>();
        let media_agent_event_tx_clone = media_agent_event_tx.clone();
        self.media_agent_event_tx = Some(media_agent_event_tx_clone);

        // --- 2. Start Decoder Worker ---
        sink_debug!(logger.clone(), "[MediaAgent] Starting Decoder Worker...");
        let decoder_handle = Some(spawn_decoder_worker(
            logger.clone(),
            ma_decoder_event_rx,
            media_agent_event_tx.clone(),
            running.clone(),
        ));
        self.decoder_handle = decoder_handle;
        sink_debug!(logger.clone(), "[MediaAgent] Decoder Worker Started");

        // --- 3. Start Encoder Worker ---
        let (ma_encoder_event_tx, ma_encoder_event_rx) = mpsc::channel::<EncoderInstruction>();
        let ma_encoder_event_tx_clone = ma_encoder_event_tx.clone();
        self.ma_encoder_event_tx = Some(ma_encoder_event_tx_clone);

        sink_debug!(logger.clone(), "[MediaAgent] Starting Encoder Worker...");
        let encoder_handle = spawn_encoder_worker(
            logger.clone(),
            ma_encoder_event_rx,
            media_agent_event_tx,
            running.clone(),
            self.config.clone(),
        )
        .map_err(|e| MediaAgentError::EncoderSpawn(e.to_string()))?;
        self.encoder_handle = Some(encoder_handle);
        sink_debug!(logger.clone(), "[MediaAgent] Encoder Worker Started");

        // --- 4. Start Central Listener ---
        sink_debug!(logger.clone(), "[MediaAgent] Starting Listener...");
        let listener_handle = Self::spawn_listener_thread(
            logger.clone(),
            local_frame_rx,
            audio_frame_rx,
            media_agent_event_rx,
            ma_decoder_event_tx,
            ma_encoder_event_tx,
            audio_player_tx,
            media_transport_event_tx,
            local_frame,
            remote_frame,
            self.sent_any_frame.clone(),
            running,
            self.config.clone(),
        );
        self.listener_handle = listener_handle;
        sink_info!(logger.clone(), "[MediaAgent] Listener Started");

        Ok(())
    }

    /// Stops all worker threads and cleans up resources.
    ///
    /// Signals the `running` atomic flag to false and joins all threads.
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

        if let Some(handle) = self.audio_handle.take() {
            let _ = handle.join();
        }

        if let Some(handle) = self.audio_player_handle.take() {
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

    pub fn set_audio_mute(&self, mute: bool) {
        self.is_audio_muted.store(mute, Ordering::SeqCst);
        let status = if mute { "muted" } else { "unmuted" };
        sink_info!(self.logger, "[MediaAgent] Microphone {}", status);
    }

    /// Enqueues an event into the MediaAgent's internal processing loop.
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

    /// Returns a snapshot of the current local and remote frames.
    ///
    /// Used by the UI layer to render the latest available video.
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
        audio_frame_rx: Receiver<AudioCaptureEvent>,
        media_agent_event_rx: Receiver<MediaAgentEvent>,
        ma_decoder_event_tx: Sender<DecoderEvent>,
        ma_encoder_event_tx: Sender<EncoderInstruction>,
        audio_player_tx: Sender<AudioPlayerCommand>,
        media_transport_event_tx: Sender<MediaTransportEvent>,
        local_frame: Arc<Mutex<Option<VideoFrame>>>,
        remote_frame: Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: Arc<AtomicBool>,
        running: Arc<AtomicBool>,
        config: Arc<Config>,
    ) -> Option<JoinHandle<()>> {
        sink_info!(logger, "[MA Listener] Starting...");
        thread::Builder::new()
            .name("media-agent-listener".into())
            .spawn(move || {
                Self::listener_loop(
                    logger,
                    local_frame_rx,
                    audio_frame_rx,
                    media_agent_event_rx,
                    ma_decoder_event_tx,
                    ma_encoder_event_tx,
                    audio_player_tx,
                    media_transport_event_tx,
                    local_frame,
                    remote_frame,
                    sent_any_frame,
                    running,
                    config,
                );
            })
            .ok()
    }

    /// The main event loop of the MediaAgent.
    ///
    /// It performs two main tasks repeatedly:
    /// 1. Drains incoming camera frames and sends them to the encoder.
    /// 2. Processes system events (decoded frames, network packets, config changes).
    #[allow(clippy::too_many_arguments)]
    fn listener_loop(
        logger: Arc<dyn LogSink>,
        local_frame_rx: Receiver<VideoFrame>,
        audio_frame_rx: Receiver<AudioCaptureEvent>,
        media_agent_event_rx: Receiver<MediaAgentEvent>,
        ma_decoder_event_tx: Sender<DecoderEvent>,
        ma_encoder_event_tx: Sender<EncoderInstruction>,
        audio_player_tx: Sender<AudioPlayerCommand>,
        media_transport_event_tx: Sender<MediaTransportEvent>,
        local_frame: Arc<Mutex<Option<VideoFrame>>>,
        remote_frame: Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: Arc<AtomicBool>,
        running: Arc<AtomicBool>,
        config: Arc<Config>,
    ) {
        while running.load(Ordering::Relaxed) {
            // Prioritize clearing the camera buffer to avoid latency build-up
            Self::drain_camera_frames(
                &logger,
                &local_frame_rx,
                &ma_encoder_event_tx,
                &local_frame,
                &sent_any_frame,
            );

            Self::drain_audio_frames(&logger, &audio_frame_rx, &media_transport_event_tx);

            // Poll for other events with a short timeout to keep the loop responsive
            match media_agent_event_rx.recv_timeout(Duration::from_millis(5)) {
                Ok(event) => {
                    let ctx = MediaAgentContext {
                        logger: &logger,
                        ma_decoder_event_tx: &ma_decoder_event_tx,
                        ma_encoder_event_tx: &ma_encoder_event_tx,
                        audio_player_tx: &audio_player_tx,
                        media_transport_event_tx: &media_transport_event_tx,
                        remote_frame: &remote_frame,
                        config: &config,
                    };
                    Self::handle_media_agent_event(ctx, event);
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

    /// Consumes all available frames from the camera channel.
    ///
    /// This ensures we always process the latest frame and don't lag behind
    /// if the camera produces frames faster than we process events.
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

    fn drain_audio_frames(
        logger: &Arc<dyn LogSink>,
        audio_frame_rx: &Receiver<AudioCaptureEvent>,
        media_transport_event_tx: &Sender<MediaTransportEvent>,
    ) {
        loop {
            match audio_frame_rx.try_recv() {
                Ok(event) => match event {
                    AudioCaptureEvent::Frame(frame) => {
                        sink_trace!(
                            logger,
                            "[MediaAgent] Received AudioFrame: ts={}, samples={}",
                            frame.timestamp_ms,
                            frame.samples
                        );

                        let encoded_payload = audio_codec::encode(&frame.data);

                        let _ = media_transport_event_tx.send(
                            MediaTransportEvent::SendEncodedAudioFrame {
                                payload: encoded_payload,
                                timestamp_ms: frame.timestamp_ms,
                                codec_spec: CodecSpec::G711U,
                            },
                        );
                    }
                    AudioCaptureEvent::Error(e) => {
                        sink_warn!(logger, "[MediaAgent] Audio capture error: {}", e);
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    sink_debug!(logger, "[MediaAgent] audio capture worker disconnected");
                    break;
                }
            }
        }
    }

    /// Updates the local frame state and forwards the frame to the encoder.
    fn handle_local_frame(
        logger: &Arc<dyn LogSink>,
        frame: VideoFrame,
        ma_encoder_event_tx: &Sender<EncoderInstruction>,
        local_frame: &Arc<Mutex<Option<VideoFrame>>>,
        sent_any_frame: &Arc<AtomicBool>,
    ) {
        // Update the UI snapshot
        if let Ok(mut guard) = local_frame.lock() {
            *guard = Some(frame.clone());
        } else {
            sink_warn!(logger, "[MediaAgent] failed to lock local frame for update");
        }

        // Check if we need to force a keyframe (e.g., first frame sent)
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

    /// Routes system events to their appropriate destinations.
    fn handle_media_agent_event(ctx: MediaAgentContext, event: MediaAgentEvent) {
        match event {
            MediaAgentEvent::DecodedVideoFrame(frame) => {
                sink_trace!(ctx.logger, "[MediaAgent] Received DecodedVideoFrame");
                let frame = *frame;
                let ts = frame.timestamp_ms;

                // Update remote UI snapshot
                if let Ok(mut guard) = ctx.remote_frame.lock() {
                    *guard = Some(frame);
                } else {
                    sink_warn!(ctx.logger, "[MediaAgent] failed to update remote frame");
                    return;
                }
                sink_debug!(
                    ctx.logger,
                    "[MediaAgent] updated remote frame snapshot (ts={ts})"
                );
            }
            MediaAgentEvent::EncodedVideoFrame {
                annexb_frame,
                timestamp_ms,
                codec_spec,
            } => {
                sink_trace!(
                    ctx.logger,
                    "[MediaAgent] encoded frame ready for transport (ts={timestamp_ms})"
                );
                sink_debug!(
                    ctx.logger,
                    "[MediaAgent] Received EncodedVideoFrame from Encoder. Now sending SendEncodedFrame to Media Transport"
                );
                // Forward to network layer
                if ctx
                    .media_transport_event_tx
                    .send(MediaTransportEvent::SendEncodedFrame {
                        annexb_frame,
                        timestamp_ms,
                        codec_spec,
                    })
                    .is_err()
                {
                    sink_warn!(
                        ctx.logger,
                        "[MediaAgent] media transport channel dropped encoded frame"
                    );
                }
            }
            MediaAgentEvent::AnnexBFrameReady { codec_spec, bytes } => {
                sink_trace!(
                    ctx.logger,
                    "[MediaAgent] forwarding AnnexB payload to decoder ({:?})",
                    codec_spec
                );
                // Forward to decoder worker
                if ctx
                    .ma_decoder_event_tx
                    .send(DecoderEvent::AnnexBFrameReady { codec_spec, bytes })
                    .is_err()
                {
                    sink_warn!(
                        ctx.logger,
                        "[MediaAgent] decoder worker offline, dropping AnnexB frame"
                    );
                }
            }
            MediaAgentEvent::UpdateBitrate(b) => {
                let fps = ctx
                    .config
                    .get("Media", "fps")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(TARGET_FPS);
                let keyint = ctx
                    .config
                    .get("Media", "keyframe_interval")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(KEYINT);

                let instruction = EncoderInstruction::SetConfig {
                    fps,
                    bitrate: b,
                    keyint,
                };
                if ctx.ma_encoder_event_tx.send(instruction).is_ok() {
                    sink_debug!(ctx.logger, "Reconfigured H264 encoder: bitrate={}bps", b,);
                }
            }
            MediaAgentEvent::EncodedAudioFrame {
                payload,
                codec_spec,
            } => {
                sink_trace!(
                    ctx.logger,
                    "[MediaAgent] Decoding audio frame ({:?})",
                    codec_spec
                );
                let decoded_samples = audio_codec::decode(&payload);
                if let Err(e) = ctx
                    .audio_player_tx
                    .send(AudioPlayerCommand::PlayFrame(decoded_samples))
                {
                    sink_error!(
                        ctx.logger,
                        "[MediaAgent] Failed to send PlayFrame command: {}",
                        e
                    );
                }
            }
        }
    }
}
