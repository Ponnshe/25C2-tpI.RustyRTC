use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
};

use crate::{
    app::log_sink::LogSink,
    core::events::EngineEvent,
    logger_info,
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
};

use super::constants::{DEFAULT_CAMERA_ID, TARGET_FPS};

pub struct MediaAgent {
    local_frame_rx: Mutex<Option<Receiver<VideoFrame>>>,
    local_frame: Arc<Mutex<Option<VideoFrame>>>,
    remote_frame: Arc<Mutex<Option<VideoFrame>>>,
    pub logger: Arc<dyn LogSink>,
    supported_media: Vec<MediaSpec>,
    _decoder_handle: Option<JoinHandle<()>>,
    _encoder_handle: Option<JoinHandle<()>>,
    ma_decoder_event_tx: Sender<DecoderEvent>,
    ma_encoder_event_tx: Sender<EncoderInstruction>,
    media_agent_event_rx: Receiver<MediaAgentEvent>,
    sent_any_frame: Arc<Mutex<bool>>,
}

impl MediaAgent {
    pub fn new(event_tx: Sender<EngineEvent>, logger: Arc<dyn LogSink>) -> Self {
        let camera_id = discover_camera_id().unwrap_or(DEFAULT_CAMERA_ID);
        let remote_frame = Arc::new(Mutex::new(None));

        let (rx, status, _handle) = spawn_camera_worker(TARGET_FPS, logger.clone(), camera_id);
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
            media_agent_event_tx,
        ));

        Self {
            local_frame_rx: Mutex::new(Some(rx)),
            local_frame: Arc::new(Mutex::new(None)),
            remote_frame,
            logger,
            supported_media,
            _decoder_handle: decoder_handle,
            _encoder_handle: encoder_handle,
            ma_decoder_event_tx,
            ma_encoder_event_tx,
            media_agent_event_rx,
            sent_any_frame: Arc::new(Mutex::new(false)),
        }
    }

    pub fn supported_media(&self) -> &[MediaSpec] {
        &self.supported_media
    }

    pub fn tick(&self) {
        self.drain_local_frames_and_encode();
    }

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

    pub fn set_remote_frame(&self, frame: VideoFrame) {
        if let Ok(mut guard) = self.remote_frame.lock() {
            *guard = Some(frame);
        }
    }

    fn drain_local_frames_and_encode(&self) {
        if let Ok(mut rx_guard) = self.local_frame_rx.lock() {
            if let Some(rx) = rx_guard.as_mut() {
                while let Ok(frame) = rx.try_recv() {
                    if let Ok(mut frame_guard) = self.local_frame.lock() {
                        *frame_guard = Some(frame.clone());
                    }

                    let force_keyframe = {
                        let mut sent = self.sent_any_frame.lock().unwrap();
                        if !*sent {
                            *sent = true;
                            true
                        } else {
                            false
                        }
                    };
                    let instruction = EncoderInstruction::Encode(frame, force_keyframe);
                    let _ = self.ma_encoder_event_tx.send(instruction);
                }
            }
        }
    }

    pub fn set_bitrate(&self, new_bitrate: u32) {
        let new_fps;
        let new_keyint;

        if new_bitrate >= 1_500_000 {
            new_fps = 30;
            new_keyint = 60;
        } else if new_bitrate >= 800_000 {
            new_fps = 25;
            new_keyint = 90;
        } else {
            new_fps = 20;
            new_keyint = 120;
        }

        let instruction = EncoderInstruction::SetConfig {
            fps: new_fps,
            bitrate: new_bitrate,
            keyint: new_keyint,
        };
        if self.ma_encoder_event_tx.send(instruction).is_ok() {
            logger_info!(
                self.logger,
                "Reconfigured H264 encoder: bitrate={}bps, fps={}, keyint={}",
                new_bitrate,
                new_fps,
                new_keyint,
            );
        }
    }
}
