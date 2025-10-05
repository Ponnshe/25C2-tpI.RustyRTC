use std::fmt;
use std::num::ParseIntError;

use crate::sdp::attribute::Attribute;
use crate::sdp::bandwith::Bandwidth;
use crate::sdp::connection::Connection;
use crate::sdp::origin::Origin;
use crate::sdp::port_spec::PortSpec;
use crate::sdp::time_desc::TimeDesc;

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
#[derive(Debug)]
pub struct Sdp {
    pub version: u8,                    // v= (always 0)
    pub origin: Origin,                 // o=
    pub session_name: String,           // s=
    pub session_info: Option<String>,   // i=*
    pub uri: Option<String>,            // u=*
    pub emails: Vec<String>,            // e=*
    pub phones: Vec<String>,            // p=*
    pub connection: Option<Connection>, // c= (optional at session)
    pub bandwidth: Vec<Bandwidth>,      // b=*
    pub times: Vec<TimeDesc>,           // one or more t= (with r=/z= hanging off last)
    pub attrs: Vec<Attribute>,          // a=* (session-level)
    pub media: Vec<Media>,              // zero or more m= sections
    pub extra_lines: Vec<String>,       // unknown session-level lines
}
#[derive(Debug)]
pub enum SdpError {
    Missing(&'static str),
    Invalid(&'static str),
    ParseInt(ParseIntError),
    AddrType,
}
impl From<ParseIntError> for SdpError {
    fn from(e: ParseIntError) -> Self {
        SdpError::ParseInt(e)
    }
}
impl Sdp {
    pub fn parse(input: &str) -> Result<Self, SdpError> {
        let mut version: Option<u8> = None;
        let mut origin: Option<Origin> = None;
        let mut session_name: Option<String> = None;
        let mut session_info = None;
        let mut uri = None;
        let mut emails = Vec::new();
        let mut phones = Vec::new();
        let mut connection: Option<Connection> = None;
        let mut bandwidth: Vec<Bandwidth> = Vec::new();
        let mut times: Vec<TimeDesc> = Vec::new();
        let mut sattrs: Vec<Attribute> = Vec::new();
        let mut media: Vec<Media> = Vec::new();
        let mut sextra: Vec<String> = Vec::new();

        // Tracks where we add attributes/lines: session or last media
        let mut in_media = false;

        for raw in input.split('\n') {
            let line = raw.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let mut it = line.splitn(2, '=');
            let (Some(prefix), Some(rest)) = (it.next(), it.next()) else {
                continue;
            };
            match prefix {
                "v" => {
                    version = Some(rest.parse::<u8>()?);
                    in_media = false;
                }
                "o" => {
                    let parts: Vec<_> = rest.split_whitespace().collect();
                    if parts.len() != 6 {
                        return Err(SdpError::Invalid("o="));
                    }
                    origin = Some(Origin::new(
                        parts[0].to_owned(),
                        parts[1].parse::<u64>()?,
                        parts[2].parse::<u64>()?,
                        parts[3].to_owned(),
                        parts[4].parse().map_err(|_| SdpError::AddrType)?,
                        parts[5].to_owned(),
                    ));
                    in_media = false;
                }
                "s" => {
                    session_name = Some(rest.to_string());
                    in_media = false;
                }
                "i" => {
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.title = Some(rest.to_string());
                        }
                    } else {
                        session_info = Some(rest.to_string());
                    }
                }
                "u" => {
                    uri = Some(rest.to_string());
                    in_media = false;
                }
                "e" => {
                    emails.push(rest.to_string());
                }
                "p" => {
                    phones.push(rest.to_string());
                }
                "c" => {
                    let parts: Vec<_> = rest.split_whitespace().collect();
                    if parts.len() != 3 {
                        return Err(SdpError::Invalid("c="));
                    }
                    let c = Connection::new(
                        parts[0].to_string(),
                        parts[1].parse().map_err(|_| SdpError::AddrType)?,
                        parts[2].to_string(),
                    );
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.connection = Some(c);
                        }
                    } else {
                        connection = Some(c);
                    }
                }
                "b" => {
                    let mut itb = rest.splitn(2, ':');
                    let (Some(typ), Some(val)) = (itb.next(), itb.next()) else {
                        return Err(SdpError::Invalid("b="));
                    };
                    let b = Bandwidth {
                        bwtype: typ.to_string(),
                        bandwidth: val.parse::<u64>()?,
                    };
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.bandwidth.push(b);
                        }
                    } else {
                        bandwidth.push(b);
                    }
                }
                "t" => {
                    let mut p = rest.split_whitespace();
                    let (Some(st), Some(et)) = (p.next(), p.next()) else {
                        return Err(SdpError::Invalid("t="));
                    };
                    times.push(TimeDesc {
                        start: st.parse::<u64>()?,
                        stop: et.parse::<u64>()?,
                        repeats: Vec::new(),
                        zone: None,
                    });
                    in_media = false;
                }
                "r" => {
                    if let Some(td) = times.last_mut() {
                        td.repeats.push(rest.to_string());
                    } else {
                        return Err(SdpError::Invalid("r= without t="));
                    }
                }
                "z" => {
                    if let Some(td) = times.last_mut() {
                        td.zone = Some(rest.to_string());
                    } else {
                        return Err(SdpError::Invalid("z= without t="));
                    }
                }
                "m" => {
                    // m=<media> <port>[/<num>] <proto> <fmt>...
                    let mut p = rest.split_whitespace();
                    let Some(mkind) = p.next() else {
                        return Err(SdpError::Invalid("m="));
                    };
                    let Some(port_tok) = p.next() else {
                        return Err(SdpError::Invalid("m= port"));
                    };
                    let (base, num) = if let Some((a, b)) = port_tok.split_once('/') {
                        (a.parse::<u16>()?, Some(b.parse::<u16>()?))
                    } else {
                        (port_tok.parse::<u16>()?, None)
                    };
                    let Some(proto) = p.next() else {
                        return Err(SdpError::Invalid("m= proto"));
                    };
                    let fmts = p.map(|s| s.to_string()).collect::<Vec<_>>();
                    media.push(Media {
                        kind: MediaKind::from(mkind),
                        port: PortSpec { base, num },
                        proto: proto.to_string(),
                        fmts,
                        title: None,
                        connection: None,
                        bandwidth: Vec::new(),
                        attrs: Vec::new(),
                        extra_lines: Vec::new(),
                    });
                    in_media = true;
                }
                "a" => {
                    let (key, val) = if let Some((k, v)) = rest.split_once(':') {
                        (k.trim().to_string(), Some(v.trim().to_string()))
                    } else {
                        (rest.trim().to_string(), None)
                    };
                    let attr = Attribute { key, value: val };
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.attrs.push(attr);
                        }
                    } else {
                        sattrs.push(attr);
                    }
                }
                _ => {
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.extra_lines.push(line.to_string());
                        }
                    } else {
                        sextra.push(line.to_string());
                    }
                }
            }
        }

        Ok(Sdp {
            version: version.ok_or(SdpError::Missing("v="))?,
            origin: origin.ok_or(SdpError::Missing("o="))?,
            session_name: session_name.ok_or(SdpError::Missing("s="))?,
            session_info,
            uri,
            emails,
            phones,
            connection,
            bandwidth,
            times,
            attrs: sattrs,
            media,
            extra_lines: sextra,
        })
    }

    pub fn to_string_crlf(&self) -> String {
        let mut out = String::new();
        macro_rules! pushln {
            ($s:expr) => {{
                out.push_str($s);
                out.push_str("\r\n");
            }};
        }

        pushln!(&format!("v={}", self.version));
        pushln!(&format!(
            "o={} {} {} {} {} {}",
            self.origin.username(),
            self.origin.session_id(),
            self.origin.session_version(),
            self.origin.net_type(),
            self.origin.addr_type(),
            self.origin.unicast_address()
        ));
        pushln!(&format!("s={}", self.session_name));
        if let Some(i) = &self.session_info {
            pushln!(&format!("i={}", i));
        }
        if let Some(u) = &self.uri {
            pushln!(&format!("u={}", u));
        }
        for e in &self.emails {
            pushln!(&format!("e={}", e));
        }
        for p in &self.phones {
            pushln!(&format!("p={}", p));
        }
        if let Some(c) = &self.connection {
            pushln!(&format!(
                "c={} {} {}",
                c.net_type(),
                c.addr_type(),
                c.connection_address()
            ));
        }
        for b in &self.bandwidth {
            pushln!(&format!("b={}:{}", b.bwtype, b.bandwidth));
        }

        // At least one t= block is required by the base spec; in WebRTC it's commonly "0 0".
        if self.times.is_empty() {
            pushln!("t=0 0");
        } else {
            for t in &self.times {
                pushln!(&format!("t={} {}", t.start, t.stop));
                for r in &t.repeats {
                    pushln!(&format!("r={}", r));
                }
                if let Some(z) = &t.zone {
                    pushln!(&format!("z={}", z));
                }
            }
        }

        for a in &self.attrs {
            match &a.value {
                Some(v) => pushln!(&format!("a={}:{}", a.key, v)),
                None => pushln!(&format!("a={}", a.key)),
            }
        }
        for x in &self.extra_lines {
            pushln!(x);
        }

        for m in &self.media {
            let fmts = if m.fmts.is_empty() {
                String::new()
            } else {
                format!(" {}", m.fmts.join(" "))
            };
            pushln!(&format!("m={} {} {}{}", m.kind, m.port, m.proto, fmts));
            if let Some(t) = &m.title {
                pushln!(&format!("i={}", t));
            }
            if let Some(c) = &m.connection {
                pushln!(&format!(
                    "c={} {} {}",
                    c.net_type(),
                    *c.addr_type(),
                    c.connection_address()
                ));
            }
            for b in &m.bandwidth {
                pushln!(&format!("b={}:{}", b.bwtype, b.bandwidth));
            }
            for a in &m.attrs {
                match &a.value {
                    Some(v) => pushln!(&format!("a={}:{}", a.key, v)),
                    None => pushln!(&format!("a={}", a.key)),
                }
            }
            for x in &m.extra_lines {
                pushln!(x);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper para leer archivos desde tests/sdp_test_files
    fn load_sdp_file(file_name: &str) -> String {
        let path = format!(
            "{}/tests/sdp_test_files/{}",
            env!("CARGO_MANIFEST_DIR"),
            file_name
        );
        fs::read_to_string(&path).expect(&format!("Failed to read {}", path))
    }

    #[test]
    fn parse_example_sdp1() {
        let sdp_str = load_sdp_file("deserialize_sdp_1.txt");
        let sdp = Sdp::parse(&sdp_str).expect("Failed to parse SDP");

        // Ejemplo de assertions
        assert_eq!(sdp.version, 0);
        assert_eq!(sdp.origin.username(), "jdoe");
        assert_eq!(sdp.origin.session_id(), 2890844526);
        assert_eq!(sdp.origin.session_version(), 2890842807);
        assert_eq!(sdp.origin.net_type(), "IN");
        assert_eq!(*sdp.origin.addr_type(), AddrType::IP4);
        assert_eq!(sdp.origin.unicast_address(), "203.0.113.1");
        assert_eq!(sdp.session_name, "Example Session");
        assert_eq!(sdp.session_info.as_deref(), Some("A simple test session"));

        let conn = sdp.connection.as_ref().expect("Expected connection");
        assert_eq!(conn.net_type(), "IN");
        assert_eq!(*conn.addr_type(), AddrType::IP4);
        assert_eq!(conn.connection_address(), "203.0.113.1");

        assert_eq!(sdp.times.len(), 1);
        assert_eq!(sdp.times[0].start, 0);
        assert_eq!(sdp.times[0].stop, 0);

        assert_eq!(sdp.attrs.len(), 1);
        assert_eq!(sdp.attrs[0].key, "tool");
        assert_eq!(sdp.attrs[0].value.as_deref(), Some("libSDP"));

        assert_eq!(sdp.media.len(), 1);
        assert_eq!(sdp.media[0].kind.to_string(), "audio");
        assert_eq!(sdp.media[0].port.to_string(), "49170");
        assert_eq!(sdp.media[0].proto, "RTP/AVP");
        assert_eq!(sdp.media[0].fmts.len(), 1);
        assert_eq!(sdp.media[0].fmts[0], "0");

        assert_eq!(sdp.media[0].title.as_deref(), Some("Audio stream"));
        assert_eq!(sdp.media[0].attrs.len(), 1);
        assert_eq!(sdp.media[0].attrs[0].key, "rtpmap");

        assert_eq!(sdp.media[0].attrs[0].value.as_deref(), Some("0 PCMU/8000"));
    }

    #[test]
    fn parse_multiple_media() {
        let sdp_str = load_sdp_file("deserialize_sdp_2.txt");
        let sdp = Sdp::parse(&sdp_str).expect("Failed to parse SDP");

        assert_eq!(sdp.media.len(), 2);

        // Audio
        assert_eq!(sdp.media[0].kind.to_string(), "audio");
        assert_eq!(sdp.media[0].fmts, vec!["0", "96"]);
        assert_eq!(sdp.media[0].attrs.len(), 2);

        // Video
        assert_eq!(sdp.media[1].kind.to_string(), "video");
        assert_eq!(sdp.media[1].fmts, vec!["97"]);
        assert_eq!(sdp.media[1].attrs[0].key, "rtpmap");
    }

    #[test]
    fn parse_invalid_missing_origin() {
        let sdp_str = load_sdp_file("deserialize_sdp_3.txt");
        let result = Sdp::parse(&sdp_str);
        assert!(matches!(result, Err(SdpError::Missing("o="))));
    }

    #[test]
    fn parse_invalid_connection() {
        let sdp_str = load_sdp_file("deserialize_sdp_4.txt");
        let result = Sdp::parse(&sdp_str);
        assert!(matches!(result, Err(SdpError::Invalid("c="))));
    }
}
