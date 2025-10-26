use crate::sdp::sdp_error::SdpError;
use std::fmt;
use std::io::Error;

#[derive(Debug)]
pub enum ConnectionError {
    MediaSpec,
    Network(String),
    Socket(Error),
    IceAgent,
    Negotiation(String),
    Sdp(SdpError),
    ClosingProt(String),
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionError::MediaSpec => write!(f, "Invalid media specification"),
            ConnectionError::IceAgent => write!(f, "ICE agent error"),
            ConnectionError::Negotiation(msg) => write!(f, "Negotiation error: {msg}"),
            ConnectionError::Sdp(e) => write!(f, "SDP error: {e}"),
            ConnectionError::Network(msg) => write!(f, "Network error: {msg}"),
            ConnectionError::Socket(e) => write!(f, "Socket error: {e}"),
            ConnectionError::ClosingProt(msg) => write!(f, "Closing protocol error: {msg}"),
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
