use super::rx_tracker_error::RxTrackerError;
use std::fmt;
pub enum RtpRecvError {
    RxTracker(RxTrackerError),
}

impl fmt::Display for RtpRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use RtpRecvError::*;
        match self {
            RxTracker(e) => write!(f, "RxTracker error: {e}"),
        }
    }
}

impl From<RxTrackerError> for RtpRecvError {
    fn from(e: RxTrackerError) -> Self {
        Self::RxTracker(e)
    }
}
