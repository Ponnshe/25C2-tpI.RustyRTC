use std::num::ParseIntError;

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
