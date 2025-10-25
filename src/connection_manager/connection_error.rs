use crate::sdp::sdp_error::SdpError;
use std::fmt;
#[derive(Debug)]
pub enum ConnectionError {
    MediaSpec,
    IceAgent,
    Negotiation(&'static str),
    Sdp(SdpError),
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionError::MediaSpec => write!(f, "Invalid media specification"),
            ConnectionError::Negotiation(e) => write!(f, "Negotiation error: {e}"),
            ConnectionError::IceAgent => write!(f, "ICE agent error"),
            ConnectionError::Sdp(e) => write!(f, "SDP error: {e}"),
        }
    }
}

impl std::error::Error for ConnectionError {}
