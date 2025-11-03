use opencv::Error as CvError;
use std::fmt;

#[derive(Debug)]
pub enum CameraError {
    InitializationFailed(String),
    OpenFailed(usize),
    CaptureFailed(String),
    OpenCvError(CvError),
    NotFrame,
    CameraOff,
    InvalidDeviceId(usize),
}

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CameraError::InitializationFailed(msg) => {
                write!(f, "Camera initialization failed: {msg}")
            }
            CameraError::OpenFailed(id) => {
                write!(f, "Failed to open camera with device_id: {id}")
            }
            CameraError::CaptureFailed(msg) => write!(f, "Failed to capture frame: {msg}"),
            CameraError::OpenCvError(e) => write!(f, "OpenCV error: {e}"),
            CameraError::NotFrame => write!(f, "No valid frame available"),
            CameraError::CameraOff => write!(f, "Camera not initialized"),
            CameraError::InvalidDeviceId(id) => write!(f, "Invalid Device ID: {id}"),
        }
    }
}

impl std::error::Error for CameraError {}

impl From<CvError> for CameraError {
    fn from(err: CvError) -> Self {
        CameraError::OpenCvError(err)
    }
}
