use super::rtp_recv_error::RtpRecvError;
use super::rtp_send_error::RtpSendError;
use crate::rtcp::rtcp_error::RtcpError;
use crate::rtp::rtp_error::RtpError;
use std::fmt;
use std::sync::{MutexGuard, PoisonError};

#[derive(Debug)]
pub enum RtpSessionError {
    Rtcp(RtcpError),
    Rtp(RtpError),
    SendStream { source: RtpSendError, ssrc: u32 },
    SendStreamMissing { ssrc: u32 },
    RecvStream { source: RtpRecvError, ssrc: u32 },
    MutexPoisoned,
    EmptyMediaReceiver,
}

impl<'a, T> From<PoisonError<MutexGuard<'a, T>>> for RtpSessionError {
    fn from(_: PoisonError<MutexGuard<'a, T>>) -> Self {
        RtpSessionError::MutexPoisoned
    }
}

impl fmt::Display for RtpSessionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use RtpSessionError::*;
        match self {
            Rtcp(e) => write!(f, "RTCP error: {e}"),
            Rtp(e) => write!(f, "RTP error: {e}"),
            SendStream { source, ssrc } => {
                write!(f, "Send RTP Stream error (ssrc={ssrc}): {source}")
            }
            SendStreamMissing { ssrc } => {
                write!(f, "Send RTP Stream missing for ssrc={ssrc:#010x}")
            }
            RecvStream { source, ssrc } => {
                write!(f, "Recv RTP Stream error (ssrc={ssrc}): {source}")
            }
            MutexPoisoned => write!(f, "Mutex poisoned"),
            EmptyMediaReceiver => write!(f, "Empty Media Receiver"),
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
