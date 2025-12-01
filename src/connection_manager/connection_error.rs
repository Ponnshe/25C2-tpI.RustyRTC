use crate::sdp::sdp_error::SdpError;
use std::fmt;
use std::io::Error;

/// Represents an error that can occur in the connection manager.
#[derive(Debug)]
pub enum ConnectionError {
    /// Invalid media specification.
    MediaSpec,
    /// A network error.
    Network(String),
    /// A socket error.
    Socket(Error),
    /// An ICE agent error.
    IceAgent,
    /// A negotiation error.
    Negotiation(String),
    /// An SDP parsing or encoding error.
    Sdp(SdpError),
    /// A closing protocol error.
    ClosingProt(String),
    /// An error related to RTP map.
    RtpMap(String),
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[allow(clippy::enum_glob_use)]
        use ConnectionError::*;
        match self {
            MediaSpec => write!(f, "Invalid media specification"),
            IceAgent => write!(f, "ICE agent error"),
            Negotiation(msg) => write!(f, "Negotiation error: {msg}"),
            Sdp(e) => write!(f, "SDP error: {e}"),
            Network(msg) => write!(f, "Network error: {msg}"),
            Socket(e) => write!(f, "Socket error: {e}"),
            ClosingProt(msg) => write!(f, "Closing protocol error: {msg}"),
            RtpMap(msg) => write!(f, "RtpMap error: {msg}"),
        }
    }
}

impl std::error::Error for ConnectionError {}

impl From<String> for ConnectionError {
    fn from(s: String) -> Self {
        ConnectionError::Negotiation(s)
    }
}

impl From<&str> for ConnectionError {
    fn from(s: &str) -> Self {
        ConnectionError::Negotiation(s.to_owned())
    }
}

impl From<SdpError> for ConnectionError {
    fn from(e: SdpError) -> Self {
        ConnectionError::Sdp(e)
    }
}
