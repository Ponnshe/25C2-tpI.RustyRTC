use crate::rtp::rtp_error::RtpError;
use std::fmt;
use std::io;
#[derive(Debug)]
pub enum RtpSendError {
    Network(io::Error),
    Rtp(RtpError),
    SRTP(String),
}

impl fmt::Display for RtpSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use RtpSendError::*;
        match self {
            Network(e) => write!(f, "Network error: {e}"),
            Rtp(e) => write!(f, "Rtp error: {e}"),
            SRTP(s) => write!(f, "SRTP error: {s}"),
        }
    }
}

impl From<RtpError> for RtpSendError {
    fn from(e: RtpError) -> Self {
        Self::Rtp(e)
    }
}
impl From<io::Error> for RtpSendError {
    fn from(e: io::Error) -> Self {
        Self::Network(e)
    }
}
