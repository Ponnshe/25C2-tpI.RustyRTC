use opencv::Error as CvError;
use std::fmt;

/// Represents an error that can occur while using the camera manager.
#[derive(Debug)]
pub enum CameraError {
    /// Failed to initialize the camera.
    InitializationFailed(String),
    /// Failed to open the camera with the given device ID.
    OpenFailed(i32),
    /// Failed to capture a frame from the camera.
    CaptureFailed(String),
    /// An error from the underlying OpenCV library.
    OpenCvError(CvError),
    /// No frame was available from the camera.
    NotFrame,
    /// The camera is not initialized.
    CameraOff,
    /// The provided device ID is invalid.
    InvalidDeviceId(i32),
}

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::camera_manager::camera_error::CameraError::{
            CameraOff, CaptureFailed, InitializationFailed, InvalidDeviceId, NotFrame, OpenCvError,
            OpenFailed,
        };
        match self {
            InitializationFailed(msg) => {
                write!(f, "Camera initialization failed: {msg}")
            }
            OpenFailed(id) => {
                write!(f, "Failed to open camera with device_id: {id}")
            }
            CaptureFailed(msg) => write!(f, "Failed to capture frame: {msg}"),
            OpenCvError(e) => write!(f, "OpenCV error: {e}"),
            NotFrame => write!(f, "No valid frame available"),
            CameraOff => write!(f, "Camera not initialized"),
            InvalidDeviceId(id) => write!(f, "Invalid Device ID: {id}"),
        }
    }
}

impl std::error::Error for CameraError {}

impl From<CvError> for CameraError {
    fn from(err: CvError) -> Self {
        CameraError::OpenCvError(err)
    }
}
