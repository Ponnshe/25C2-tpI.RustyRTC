use std::{fmt, io};

use crate::signaling::protocol::FrameError;

/// Errors that can occur while sending signaling messages.
///
/// In this design, the only thing `send()` can reliably report is that the
/// signaling client is disconnected (i.e. the network thread has exited and
/// dropped its command receiver).
#[derive(Debug)]
pub enum SignalingClientError {
    Io(io::Error),
    Frame(FrameError),
    Poisoned,
    Disconnected,
}

impl fmt::Display for SignalingClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Frame(e) => write!(f, "protocol error: {e:?}"),
            Self::Poisoned => write!(f, "stream lock poisoned"),
            Self::Disconnected => write!(f, "signaling client disconnected"),
        }
    }
}

impl std::error::Error for SignalingClientError {}
