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
}
