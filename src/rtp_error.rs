#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtpError {
    TooShort,
    BadVersion(u8),
    CsrcCountMismatch { expected: usize, buf_left: usize },
    HeaderExtensionTooShort,
    PaddingTooShort,
    Invalid,
}

impl fmt::Display for RtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use RtpError::*;
        match self {
            TooShort => write!(f, "buffer too short"),
            BadVersion(v) => write!(f, "bad RTP version: {v}"),
            CsrcCountMismatch { expected, buf_left } => write!(
                f,
                "CSRC count mismatch: expected {}×4 bytes, but only {} bytes remain",
                expected, buf_left
            ),
            HeaderExtensionTooShort => write!(f, "RTP header extension too short"),
            PaddingTooShort => write!(f, "padding bit set but payload shorter than padding count"),
            Invalid => write!(f, "invalid RTP packet"),
        }
    }
}

impl std::error::Error for RtpError {}
