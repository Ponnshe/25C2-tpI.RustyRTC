use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    camera_manager::camera_manager_c::CameraManager,
    core::events::EngineEvent,
    media_agent::{
        camera_worker::{camera_loop, synthetic_loop},
        encoder_worker::{spawn_encoder_worker, EncoderOrder},
        events::MediaAgentEvent,
        h264_decoder::H264Decoder,
        media_agent_error::Result,
        spec::{CodecSpec, MediaSpec, MediaType},
        utils::discover_camera_id,
        video_frame::VideoFrame,
    },
    sink_log,
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
    event_tx: Sender<MediaAgentEvent>,
    encoder_tx: Sender<EncoderOrder>,
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

        let (media_agent_event_tx, media_agent_event_rx) = mpsc::channel();

        let decoder_handle = Some(spawn_decoder_worker(
            logger.clone(),
            media_agent_event_rx,
            event_tx.clone(),
        ));

        let (encoder_tx, encoder_rx) = mpsc::channel();
        let encoder_handle = Some(spawn_encoder_worker(
            logger.clone(),
            encoder_rx,
            event_tx,
        ));

        Self {
            local_frame_rx: Mutex::new(Some(rx)),
            local_frame: Arc::new(Mutex::new(None)),
            remote_frame,
            logger,
            supported_media,
            _decoder_handle: decoder_handle,
            _encoder_handle: encoder_handle,
            event_tx: media_agent_event_tx,
            encoder_tx,
            sent_any_frame: Arc::new(Mutex::new(false)),
        }
    }

    pub fn supported_media(&self) -> &[MediaSpec] {
        &self.supported_media
    }

    pub fn tick(&self) {
        self.drain_local_frames_and_encode();
    }

    pub fn post_event(&self, event: MediaAgentEvent) {
        let _ = self.event_tx.send(event);
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
                    let order = EncoderOrder::Encode(frame, force_keyframe);
                    let _ = self.encoder_tx.send(order);
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

        let order = EncoderOrder::SetConfig {
            fps: new_fps,
            bitrate: new_bitrate,
            keyint: new_keyint,
        };
        if self.encoder_tx.send(order).is_ok() {
            sink_log!(
                self.logger.as_ref(),
                LogLevel::Info,
                "Reconfigured H264 encoder: bitrate={}bps, fps={}, keyint={}",
                new_bitrate,
                new_fps,
                new_keyint,
            );
        }
    }
}

fn spawn_camera_worker(
    target_fps: u32,
    logger: Arc<dyn LogSink>,
    camera_id: i32,
) -> (
    Receiver<VideoFrame>,
    Option<String>,
    Option<JoinHandle<()>>,
) {
    let (local_frame_tx, local_frame_rx) = mpsc::channel();
    let camera_manager = CameraManager::new(camera_id, logger);

    let status = match &camera_manager {
        Ok(cam) => Some(format!(
            "Using camera source with resolution {}x{}",
            cam.width(),
            cam.height()
        )),
        Err(e) => Some(format!("Camera error: {}. Using test pattern.", e)),
    };

    let handle = thread::Builder::new()
        .name("media-agent-camera".into())
        .spawn(move || {
            if let Ok(cam) = camera_manager {
                if let Err(e) = camera_loop(cam, local_frame_tx, target_fps) {
                    eprintln!("camera loop stopped: {e:?}");
                }
            } else {
                if let Err(e) = synthetic_loop(local_frame_tx, target_fps) {
                    eprintln!("synthetic loop stopped: {e:?}");
                }
            }
        })
        .ok();

    (local_frame_rx, status, handle)
}

fn spawn_decoder_worker(
    logger: Arc<dyn LogSink>,
    event_rx: Receiver<MediaAgentEvent>,
    event_tx: Sender<EngineEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-agent-decoder".into())
        .spawn(move || {
            let mut h264_decoder = H264Decoder::new();

            while let Ok(event) = event_rx.recv() {
                match event {
                    MediaAgentEvent::ChunkReady { codec_spec, chunk } => match codec_spec {
                        CodecSpec::H264 => match h264_decoder.decode_chunk(&chunk) {
                            Ok(Some(frame)) => {
                                let _ =
                                    event_tx.send(EngineEvent::DecodedVideoFrame(Box::new(frame)));
                            }
                            Ok(None) => {
                                sink_log!(
                                    logger.as_ref(),
                                    LogLevel::Debug,
                                    "[MediaAgent] decoder needs more NALs for this AU"
                                );
                            }
                            Err(e) => {
                                sink_log!(
                                    logger.as_ref(),
                                    LogLevel::Error,
                                    "[MediaAgent] decode error: {e:?}"
                                );
                            }
                        },
                    },
                }
            }
        })
        .expect("spawn media-agent-decoder")
}
