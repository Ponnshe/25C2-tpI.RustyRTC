//! Camera manager module using `OpenCV`.
//!
//! This module provides a safe wrapper around `OpenCV's` `VideoCapture` for
//! capturing frames from a camera device. It handles initialization,
//! frame retrieval, and cleanup when the manager goes out of scope.

use opencv::{
    core,
    prelude::*,
    videoio::{self, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst},
};

use std::sync::Arc;

use crate::app::log_sink::LogSink;

use super::camera_error::CameraError;

/// Struct responsible for managing a single camera device.
///
/// Handles opening the camera, retrieving frames, and releasing the camera
/// when no longer needed.
pub struct CameraManager {
    cam: Option<VideoCapture>,
    logger: Arc<dyn LogSink>,
    width: u32,
    height: u32,
}

impl CameraManager {
    /// Creates a new `CameraManager` for the given device ID.
    ///
    /// # Arguments
    ///
    /// * `device_id` - The camera device index (typically 0 for the first camera).
    ///
    /// # Errors
    ///
    /// Returns `CameraError::InitializationFailed` if `OpenCV ` fails to create
    /// the capture object.
    /// Returns `CameraError::OpenFailed` if the camera cannot be opened.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use your_crate::camera_manager::{CameraManager, CameraError};
    /// let camera = CameraManager::new(0)?;
    /// # Ok::<(), CameraError>(())
    /// ```
    pub fn new(device_id: usize, logger: Arc<dyn LogSink>) -> Result<Self, CameraError> {
        let device_id_i32 = i32::try_from(device_id)
            .map_err(|_| CameraError::InvalidDeviceId(device_id))?;
        let cam = videoio::VideoCapture::new(device_id_i32, videoio::CAP_ANY)
            .map_err(|e| CameraError::InitializationFailed(e.to_string()))?;

        if !cam.is_opened().unwrap_or(false) {
            return Err(CameraError::OpenFailed(device_id));
        }

        let width_f64 = cam
            .get(videoio::CAP_PROP_FRAME_WIDTH)
            .map_err(|e| CameraError::InitializationFailed(e.to_string()))?
            .clamp(1.0, 8192.0);

        let height_f64 = cam
            .get(videoio::CAP_PROP_FRAME_HEIGHT)
            .map_err(|e| CameraError::InitializationFailed(e.to_string()))?
            .clamp(1.0, 8192.0);

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let width = width_f64.round() as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let height = height_f64.round() as u32;

        Ok(Self {
            cam: Some(cam),
            logger,
            width,
            height,
        })
    }

    /// Captures a single frame from the camera.
    ///
    /// # Returns
    ///
    /// Returns an `OpenCV` `core::Mat` containing the frame data if successful.
    ///
    /// # Errors
    ///
    /// Returns `CameraError::NotFrame` if a frame could not be read.
    /// Returns `CameraError::CameraOff` if the camera has already been released.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use your_crate::camera_manager::{CameraManager, CameraError};
    /// # let mut camera = CameraManager::new(0)?;
    /// let frame = camera.get_frame()?;
    /// # Ok::<(), CameraError>(())
    /// ```
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

    #[must_use]
    /// Returns the width of the camera frames.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use rusty_rtc::camera_manager::CameraManager;
    /// # let camera = CameraManager::new(0).unwrap();
    /// println!("Camera width: {}", camera.width());
    /// ```
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    /// Returns the height of the camera frames.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use your_crate::camera_manager::CameraManager;
    /// # let camera = CameraManager::new(0).unwrap();
    /// println!("Camera height: {}", camera.height());
    /// ```
    pub const fn height(&self) -> u32 {
        self.height
    }
}

impl Drop for CameraManager {
    /// Releases the camera resource when the `CameraManager` goes out of scope.
    fn drop(&mut self) {
        if let Some(mut cam) = self.cam.take() {
            let _ = cam.release();
        }
    }
}
