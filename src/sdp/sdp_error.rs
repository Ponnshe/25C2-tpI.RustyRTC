use std::num::ParseIntError;
use std::fmt;

#[derive(Debug)]
pub enum SdpError {
    Missing(&'static str),
    Invalid(&'static str),
    ParseInt(ParseIntError),
    AddrType,
}
impl From<ParseIntError> for SdpError {
    fn from(e: ParseIntError) -> Self {
        Self::ParseInt(e)
    }
}

impl fmt::Display for SdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdpError::Missing(msg) => write!(f, "Missing field: {}", msg),
            SdpError::Invalid(msg) => write!(f, "Invalid field: {}", msg),
            SdpError::ParseInt(e) => write!(f, "Parse int error: {}", e),
            SdpError::AddrType => write!(f, "Invalid address type"),
        }
    }
}

impl std::error::Error for SdpError {}
