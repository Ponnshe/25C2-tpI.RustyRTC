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
struct MediaDescription {
    media: String,
    port: i64,
    protocol: String,
    format: String,
}
enum Attribute {
    Name(String),
    NameValue(String, String),
}
struct SdpC {
    version: i64,
    origin: Origin,
    session_name: String,
    time_active: (i64, i64),
    media_description: MediaDescription,
    attributes: Vec<Attribute>,
}
