use std::fmt;

#[derive(Debug)]
pub enum MediaTransportError {
    Send(String),
    Mutex(String),
}

impl fmt::Display for MediaTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MediaTransportError::*;
        match self {
            Send(e) => write!(f, "Send error: {e}"),
            Mutex(e) => write!(f, "Mutex error: {e}"),
        }
    }
}

impl<T> From<std::sync::PoisonError<T>> for MediaTransportError {
    fn from(e: std::sync::PoisonError<T>) -> Self {
        Self::Mutex(e.to_string())
    }
}

impl std::error::Error for MediaTransportError {}

pub type Result<T> = std::result::Result<T, MediaTransportError>;
