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
        h264_decoder::H264Decoder,
        h264_encoder::H264Encoder,
        media_agent_error::{MediaAgentError, Result},
        spec::{CodecSpec, MediaSpec, MediaType},
        utils::discover_camera_id,
        video_frame::VideoFrame,
        camera_worker::{
            camera_loop,
            synthetic_loop,
        }
    },
    sink_log,
};

use super::constants::{BITRATE, DEFAULT_CAMERA_ID, KEYINT, TARGET_FPS};

pub struct MediaAgent {
    h264_encoder: Mutex<H264Encoder>,
    h264_decoder: Mutex<H264Decoder>,
    local_frame_rx: Mutex<Option<Receiver<VideoFrame>>>,
    local_frame: Arc<Mutex<Option<VideoFrame>>>,
    remote_frame: Arc<Mutex<Option<VideoFrame>>>,
    pub logger: Arc<dyn LogSink>,
    supported_media: Vec<MediaSpec>,
}

impl MediaAgent {
    pub fn new(event_tx: Sender<EngineEvent>, logger: Arc<dyn LogSink>) -> Self {
        let camera_id = discover_camera_id().unwrap_or(DEFAULT_CAMERA_ID);
        let h264_encoder = Mutex::new(H264Encoder::new(TARGET_FPS, BITRATE, KEYINT));
        let h264_decoder = Mutex::new(H264Decoder::new());
        let remote_frame = Arc::new(Mutex::new(None));

        let (rx, status, _handle) = spawn_camera_worker(TARGET_FPS, logger.clone(), camera_id);
        if let Some(msg) = status {
            let _ = event_tx.send(EngineEvent::Status(format!("[MediaAgent] {msg}")));
        }

        let supported_media = vec![MediaSpec {
            media_type: MediaType::Video,
            codec_spec: CodecSpec::H264,
        }];

        Self {
            h264_encoder,
            h264_decoder,
            local_frame_rx: Mutex::new(Some(rx)),
            local_frame: Arc::new(Mutex::new(None)),
            remote_frame,
            logger,
            supported_media,
        }
    }

    pub fn supported_media(&self) -> &[MediaSpec] {
        &self.supported_media
    }

    pub fn tick(&self) {
        self.drain_local_frames();
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

    pub fn encode(
        &self,
        codec: CodecSpec,
        force_keyframe: bool,
    ) -> Result<Option<(Vec<u8>, u128)>> {
        match codec {
            CodecSpec::H264 => {
                let Some(frame) = self
                    .local_frame
                    .lock()
                    .ok()
                    .and_then(|g| g.as_ref().cloned())
                else {
                    return Ok(None);
                };

                let mut enc = self
                    .h264_encoder
                    .lock()
                    .map_err(|_| MediaAgentError::Codec("encoder poisoned".into()))?;

                if force_keyframe {
                    enc.request_keyframe();
                }

                let encoded = enc.encode_frame_to_h264(&frame)?;
                Ok(Some((encoded, frame.timestamp_ms)))
            }
        }
    }

    pub fn decode(
        &self,
        codec: CodecSpec,
        au: &crate::media_transport::payload::h264_depacketizer::AccessUnit,
    ) -> Result<Option<VideoFrame>> {
        match codec {
            CodecSpec::H264 => {
                let mut dec = self
                    .h264_decoder
                    .lock()
                    .map_err(|_| MediaAgentError::Codec("decoder poisoned".into()))?;
                dec.decode_au(au)
            }
        }
    }

    fn drain_local_frames(&self) {
        if let Ok(mut rx_guard) = self.local_frame_rx.lock() {
            if let Some(rx) = rx_guard.as_mut() {
                while let Ok(frame) = rx.try_recv() {
                    if let Ok(mut frame_guard) = self.local_frame.lock() {
                        *frame_guard = Some(frame);
                    }
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

        let mut updated = false;
        if let Ok(mut enc) = self.h264_encoder.lock() {
            match enc.set_config(new_fps, new_bitrate, new_keyint) {
                Ok(u) => updated = u,
                Err(e) => {
                    sink_log!(
                        self.logger.as_ref(),
                        LogLevel::Error,
                        "Failed to update H264 encoder config: {:?}",
                        e
                    );
                }
            }
        }

        if updated {
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
