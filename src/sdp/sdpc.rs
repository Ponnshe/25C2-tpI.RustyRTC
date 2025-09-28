use std::fmt;
use std::num::ParseIntError;
#[derive(Debug, PartialEq, Eq)]
pub enum AddrType {
    IP4,
    IP6,
}
impl fmt::Display for AddrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AddrType::IP4 => "IP4",
            AddrType::IP6 => "IP6",
        })
    }
}
impl std::str::FromStr for AddrType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "IP4" => Ok(AddrType::IP4),
            "IP6" => Ok(AddrType::IP6),
            _ => Err(()),
        }
    }
}
struct SdpC {
    version: i64,
    origin: Origin,
    session_name: String,
    time_active: (i64, i64),
    media_description: MediaDescription,
    attributes: Vec<Attribute>,
}
