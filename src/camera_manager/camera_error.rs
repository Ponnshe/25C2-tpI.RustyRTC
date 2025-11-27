use opencv::Error as CvError;
use std::fmt;

#[derive(Debug)]
pub enum CameraError {
    InitializationFailed(String),
    OpenFailed(i32),
    CaptureFailed(String),
    OpenCvError(CvError),
    NotFrame,
    CameraOff,
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
