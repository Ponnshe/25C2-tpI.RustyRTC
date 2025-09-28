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
#[derive(Debug)]
pub struct Origin {
    pub username: String,
    pub session_id: u64,
    pub session_version: u64,
    pub net_type: String,    // usually "IN"
    pub addr_type: AddrType, // IP4 or IP6
    pub unicast_address: String,
}

#[derive(Debug)]
pub struct Connection {
    pub net_type: String,    // "IN"
    pub addr_type: AddrType, // IP4/IP6
    /// e.g. "203.0.113.1" or multicast with optional "/ttl[/num]"
    pub connection_address: String,
}
#[derive(Debug)]
pub struct Bandwidth {
    pub bwtype: String, // e.g. "AS", "TIAS"
    pub bandwidth: u64,
}

#[derive(Debug)]
pub struct TimeDesc {
    pub start: u64,           // NTP seconds, often 0
    pub stop: u64,            // NTP seconds, often 0
    pub repeats: Vec<String>, // raw r= lines (spec grammar is tedious; keep raw)
    pub zone: Option<String>, // raw z= line
}
#[derive(Debug, Clone, Copy)]
pub struct PortSpec {
    pub base: u16,        // m=<media> <port>[/<num>] ...
    pub num: Option<u16>, // for hierarchical encoding (rare in WebRTC)
}
impl fmt::Display for PortSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.num {
            Some(n) => write!(f, "{}/{}", self.base, n),
            None => write!(f, "{}", self.base),
        }
    }
}
#[derive(Debug)]
pub enum MediaKind {
    Audio,
    Video,
    Text,
    Application,
    Message,
    Other(String),
}
impl fmt::Display for MediaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MediaKind::*;
        match self {
            Audio => f.write_str("audio"),
            Video => f.write_str("video"),
            Text => f.write_str("text"),
            Application => f.write_str("application"),
            Message => f.write_str("message"),
            Other(s) => f.write_str(s),
        }
    }
}
impl From<&str> for MediaKind {
    fn from(s: &str) -> Self {
        match s {
            "audio" => MediaKind::Audio,
            "video" => MediaKind::Video,
            "text" => MediaKind::Text,
            "application" => MediaKind::Application,
            "message" => MediaKind::Message,
            other => MediaKind::Other(other.to_string()),
        }
    }
}
#[derive(Debug)]
pub struct Attribute {
    pub key: String,           // e.g. "rtpmap", "fmtp", "rtcp-mux"
    pub value: Option<String>, // entire value part after "key:" (if any)
}
#[derive(Debug)]
pub struct Media {
    pub kind: MediaKind,                // m=<media>
    pub port: PortSpec,                 // m=<media> <port>[/num]
    pub proto: String,                  // e.g. "UDP/TLS/RTP/SAVPF"
    pub fmts: Vec<String>,              // the <fmt> tokens (often payload types)
    pub title: Option<String>,          // i=
    pub connection: Option<Connection>, // c= at media
    pub bandwidth: Vec<Bandwidth>,      // b=*
    pub attrs: Vec<Attribute>,          // a=*
    pub extra_lines: Vec<String>,       // any unknown/unsupported lines to round-trip
}
}
