use crate::rtp::rtp_error::RtpError;
use std::fmt;
use std::io;
pub enum RtpSendError {
    Network(io::Error),
    Rtp(RtpError),
}

impl fmt::Display for RtpSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use RtpSendError::*;
        match self {
            Network(e) => write!(f, "Network error: {e}"),
            Rtp(e) => write!(f, "Rtp error: {e}"),
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
