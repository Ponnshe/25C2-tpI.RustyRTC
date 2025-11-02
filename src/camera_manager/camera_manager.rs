use std::sync::Arc;

use opencv::{
    core,
    prelude::*,
    videoio::{self, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst},
};

use crate::app::log_sink::LogSink;

use super::camera_error::CameraError;

pub struct CameraManager {
    cam: Option<VideoCapture>,
    logger: Arc<dyn LogSink>,
    width: u32,
    height: u32,
}

impl CameraManager {
    pub fn new(device_id: usize, logger: Arc<dyn LogSink>) -> Result<Self, CameraError> {
        let cam = videoio::VideoCapture::new(device_id as i32, videoio::CAP_ANY)
            .map_err(|e| CameraError::InitializationFailed(e.to_string()))?;

        if !cam.is_opened().unwrap_or(false) {
            return Err(CameraError::OpenFailed(device_id));
        }

        let width = cam.get(videoio::CAP_PROP_FRAME_WIDTH).unwrap_or(640.0) as u32;
        let height = cam.get(videoio::CAP_PROP_FRAME_HEIGHT).unwrap_or(480.0) as u32;

        Ok(Self {
            cam: Some(cam),
            logger,
            width,
            height,
        })
    }

    pub fn get_frame(&mut self) -> Result<core::Mat, CameraError> {
        if let Some(cam) = &mut self.cam {
            let mut frame = core::Mat::default();
            if cam.read(&mut frame).unwrap_or(false) && !frame.empty() {
                Ok(frame)
            } else {
                Err(CameraError::NotFrame)
            }
        } else {
            Err(CameraError::CameraOff)
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

impl Drop for CameraManager {
    fn drop(&mut self) {
        if let Some(mut cam) = self.cam.take() {
            let _ = cam.release();
        }
    }
}
