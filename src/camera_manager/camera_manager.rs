use opencv::{
    prelude::*,
    videoio::{self, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst},
    core,
};

pub struct CameraManager {
    cam: Option<VideoCapture>,
    width: u32,
    height: u32,
}

impl CameraManager {
    pub fn new(device_id: usize) -> Result<Self, String> {
        let cam = videoio::VideoCapture::new(device_id as i32, videoio::CAP_ANY)
            .map_err(|e| format!("Failed to create VideoCapture: {}", e))?;

        if !cam.is_opened().unwrap_or(false) {
            return Err(format!("Failed to open camera with device_id: {}", device_id));
        }

        let width = cam.get(videoio::CAP_PROP_FRAME_WIDTH).unwrap_or(640.0) as u32;
        let height = cam.get(videoio::CAP_PROP_FRAME_HEIGHT).unwrap_or(480.0) as u32;

        Ok(Self { cam: Some(cam), width, height })
    }

    pub fn get_frame(&mut self) -> Option<core::Mat> {
        if let Some(cam) = &mut self.cam {
            let mut frame = core::Mat::default();
            if cam.read(&mut frame).unwrap_or(false) && !frame.empty() {
                return Some(frame);
            }
        }
        None
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
