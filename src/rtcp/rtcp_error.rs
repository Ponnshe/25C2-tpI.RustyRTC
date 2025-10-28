use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtcpError {
    TooShort,
    BadVersion(u8),
    LengthMismatch,
    UnknownPacketType(u8),
    Truncated,
    Invalid,
    SdesItemTooShort,
}

impl fmt::Display for RtcpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use RtcpError::*;
        match self {
            TooShort => write!(f, "buffer too short"),
            BadVersion(v) => write!(f, "bad RTCP version: {v}"),
            LengthMismatch => write!(f, "rendered length does not match header length"),
            UnknownPacketType(pt) => write!(f, "unknown RTCP packet type: {pt}"),
            Truncated => write!(f, "truncated RTCP structure"),
            Invalid => write!(f, "invalid RTCP packet"),
            SdesItemTooShort => write!(f, "SDES item too short"),
        }
    }
}
impl std::error::Error for RtcpError {}
