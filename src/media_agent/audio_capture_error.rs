use std::fmt;

#[derive(Debug, Clone)]
pub enum AudioCaptureError {
    StreamConfig(String),
    StreamBuild(String),
    StreamPlay(String),
    Runtime(String),
}

impl fmt::Display for AudioCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioCaptureError::StreamConfig(e) => write!(f, "Stream Config Error: {}", e),
            AudioCaptureError::StreamBuild(e) => write!(f, "Stream Build Error: {}", e),
            AudioCaptureError::StreamPlay(e) => write!(f, "Stream Play Error: {}", e),
            AudioCaptureError::Runtime(e) => write!(f, "Runtime Error: {}", e),
        }
    }
}

impl std::error::Error for AudioCaptureError {}
