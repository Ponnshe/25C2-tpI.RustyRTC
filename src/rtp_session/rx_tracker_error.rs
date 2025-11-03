use std::fmt;

#[derive(Debug)]
pub enum RxTrackerError {
    SeqExt,
}

impl fmt::Display for RxTrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use RxTrackerError::*;
        match self {
            SeqExt => write!(f, "SeqExt error"),
        }
    }
}
