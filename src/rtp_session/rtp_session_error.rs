use super::rtp_recv_error::RtpRecvError;
use super::rtp_send_error::RtpSendError;
use crate::rtcp::rtcp_error::RtcpError;
use crate::rtp::rtp_error::RtpError;
use std::fmt;
pub enum RtpSessionError {
    Rtcp(RtcpError),
    Rtp(RtpError),
    SendStream {
        rtp_send_error: RtpSendError,
        ssrc: u16,
    },
    RecvStream {
        rtp_recv_error: RtpRecvError,
        ssrc: u32,
    },
}

impl fmt::Display for RtpSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use RtpSessionError::*;
        match self {
            Rtcp(e) => write!(f, "RTCP error: {e}"),
            Rtp(e) => write!(f, "RTP error: {e}"),
            SendStream {
                rtp_send_error,
                ssrc,
            } => write!(
                f,
                "Send RTP Stream error: {rtp_send_error} with ssrc: {ssrc}"
            ),
            RecvStream {
                rtp_recv_error,
                ssrc,
            } => write!(
                f,
                "Receive RTP Stream error: {rtp_recv_error} with ssrc: {ssrc}"
            ),
        }
    }
}

impl From<RtcpError> for RtpSessionError {
    fn from(e: RtcpError) -> Self {
        Self::Rtcp(e)
    }
}

impl From<RtpError> for RtpSessionError {
    fn from(e: RtpError) -> Self {
        Self::Rtp(e)
    }
}
