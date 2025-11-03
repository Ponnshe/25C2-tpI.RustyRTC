use std::fmt;
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AddrType {
    IP4,
    IP6,
}

impl fmt::Display for AddrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::IP4 => "IP4",
            Self::IP6 => "IP6",
        })
    }
}

impl std::str::FromStr for AddrType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "IP4" => Ok(Self::IP4),
            "IP6" => Ok(Self::IP6),
            _ => Err(()),
        }
    }
}
