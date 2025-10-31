use std::fmt;

pub type Result<T> = std::result::Result<T, MediaAgentError>;
#[derive(Debug)]
pub enum MediaAgentError {
    Camera(String),
    Codec(String),
    Send(String),
}
impl fmt::Display for MediaAgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MediaAgentError::*;
        match self {
            Camera(e) => write!(f, "Camera error: {e}"),
            Codec(e) => write!(f, "Codec error: {e}"),
            Send(e) => write!(f, "Send error: {e}"),
        }
    }
}

impl std::error::Error for MediaAgentError {}
