use std::num::ParseIntError;

use crate::sdp::addr_type::AddrType;
use crate::sdp::attribute::Attribute;
use crate::sdp::bandwidth::Bandwidth;
use crate::sdp::connection::Connection;
use crate::sdp::media::{Media, MediaKind};
use crate::sdp::origin::Origin;
use crate::sdp::port_spec::PortSpec;
use crate::sdp::time_desc::TimeDesc;

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
        Self::ParseInt(e)
    }
}
impl Sdp {
    #[allow(clippy::too_many_lines)]
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

        let mut in_media = false;

        for raw in input.split('\n') {
            let line = raw.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let Some((prefix, rest)) = split_line(line) else {
                continue;
            };

            match prefix {
                "v" => {
                    version = Some(rest.parse::<u8>()?);
                    in_media = false;
                }
                "o" => {
                    origin = Some(rest.parse::<Origin>()?);
                    in_media = false;
                }
                "s" => {
                    session_name = Some(rest.to_owned());
                    in_media = false;
                }
                "i" => {
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.set_title(Some(rest.to_owned()));
                        }
                    } else {
                        session_info = Some(rest.to_owned());
                    }
                }
                "u" => {
                    uri = Some(rest.to_owned());
                    in_media = false;
                }
                "e" => emails.push(rest.to_owned()),
                "p" => phones.push(rest.to_owned()),
                "c" => {
                    let c: Connection = rest.parse()?;
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.set_connection(Some(c));
                        }
                    } else {
                        connection = Some(c);
                    }
                }
                "b" => {
                    let b: Bandwidth = rest.parse()?;
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.add_bandwidth(b);
                        }
                    } else {
                        bandwidth.push(b);
                    }
                }
                "t" => {
                    times.push(rest.parse::<TimeDesc>()?);
                    in_media = false;
                }
                "r" => {
                    if let Some(td) = times.last_mut() {
                        td.add_repeat(rest.to_owned());
                    } else {
                        return Err(SdpError::Invalid("r= without t="));
                    }
                }
                "z" => {
                    if let Some(td) = times.last_mut() {
                        td.set_zone(Some(rest.to_owned()));
                    } else {
                        return Err(SdpError::Invalid("z= without t="));
                    }
                }
                "m" => {
                    media.push(rest.parse::<Media>()?);
                    in_media = true;
                }
                "a" => {
                    let attr: Attribute = rest.parse()?;
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.add_attr(attr);
                        }
                    } else {
                        sattrs.push(attr);
                    }
                }
                _ => {
                    if in_media {
                        if let Some(m) = media.last_mut() {
                            m.add_extra_line(line.to_owned());
                        }
                    } else {
                        sextra.push(line.to_owned());
                    }
                }
            }
        }

        Ok(Self {
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

    #[allow(clippy::too_many_lines)]
    pub fn to_string_crlf(&self) -> String {
        let mut out = String::new();
        macro_rules! pushln {
            ($($arg:tt)*) => {{
                use std::fmt::Write as _;
                let _ = write!(out, $($arg)*);
                let _ = out.write_str("\r\n");
            }};
        }

        pushln!("v={}", self.version);
        pushln!(
            "o={} {} {} {} {} {}",
            self.origin.username(),
            self.origin.session_id(),
            self.origin.session_version(),
            self.origin.net_type(),
            self.origin.addr_type(),
            self.origin.unicast_address()
        );
        pushln!("s={}", self.session_name);
        if let Some(i) = &self.session_info {
            pushln!("i={}", i);
        }
        if let Some(u) = &self.uri {
            pushln!("u={}", u);
        }
        for e in &self.emails {
            pushln!("e={}", e);
        }
        for p in &self.phones {
            pushln!("p={}", p);
        }
        if let Some(c) = &self.connection {
            pushln!(
                "c={} {} {}",
                c.net_type(),
                c.addr_type(),
                c.connection_address()
            );
        }
        for b in &self.bandwidth {
            pushln!("b={}:{}", b.bwtype(), b.bandwidth());
        }

        // At least one t= block is required by the base spec; in WebRTC it's commonly "0 0".
        if self.times.is_empty() {
            pushln!("t=0 0");
        } else {
            for t in &self.times {
                pushln!("t={} {}", t.start(), t.stop());
                for r in t.repeats() {
                    pushln!("r={}", r);
                }
                if let Some(z) = &t.zone() {
                    pushln!("z={}", z);
                }
            }
        }

        for a in &self.attrs {
            if let Some(v) = a.value() {
                pushln!("a={}:{}", a.key(), v);
            } else {
                pushln!("a={}", a.key());
            }
        }
        for x in &self.extra_lines {
            pushln!("{x}");
        }

        for m in &self.media {
            let fmts = if m.fmts().is_empty() {
                String::new()
            } else {
                format!(" {}", m.fmts().join(" "))
            };
            pushln!("m={} {} {}{}", m.kind(), m.port(), m.proto(), fmts);
            if let Some(t) = &m.title() {
                pushln!("i={}", t);
            }
            if let Some(c) = m.connection() {
                pushln!(
                    "c={} {} {}",
                    c.net_type(),
                    *c.addr_type(),
                    c.connection_address()
                );
            }
            for b in m.bandwidth() {
                pushln!("b={}:{}", b.bwtype(), b.bandwidth());
            }
            for a in m.attrs() {
                if let Some(v) = a.value() {
                    pushln!("a={}:{}", a.key(), v);
                } else {
                    pushln!("a={}", a.key());
                }
            }
            for x in m.extra_lines() {
                pushln!("{x}");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::fs;

    /// Helper para leer archivos desde `tests/sdp_test_files`
    #[allow(clippy::expect_fun_call)]
    fn load_sdp_file(file_name: &str) -> String {
        let path = format!(
            "{}/tests/sdp_test_files/{}",
            env!("CARGO_MANIFEST_DIR"),
            file_name
        );
        fs::read_to_string(&path).expect(&format!("Failed to read {path}"))
    }

    #[test]
    fn parse_example_sdp1() {
        let sdp_str = load_sdp_file("deserialize_sdp_1.txt");
        let sdp = Sdp::parse(&sdp_str).expect("Failed to parse SDP");

        // Ejemplo de assertions
        assert_eq!(sdp.version, 0);
        assert_eq!(sdp.origin.username(), "jdoe");
        assert_eq!(sdp.origin.session_id(), 2_890_844_526);
        assert_eq!(sdp.origin.session_version(), 2_890_842_807);
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
        assert_eq!(sdp.times[0].start(), 0);
        assert_eq!(sdp.times[0].stop(), 0);

        assert_eq!(sdp.attrs.len(), 1);
        assert_eq!(sdp.attrs[0].key(), "tool");
        assert_eq!(sdp.attrs[0].value().as_deref(), Some("libSDP"));

        assert_eq!(sdp.media.len(), 1);
        assert_eq!(sdp.media[0].kind().to_string(), "audio");
        assert_eq!(sdp.media[0].port().to_string(), "49170");
        assert_eq!(sdp.media[0].proto(), "RTP/AVP");
        assert_eq!(sdp.media[0].fmts().len(), 1);
        assert_eq!(sdp.media[0].fmts()[0], "0");

        assert_eq!(sdp.media[0].title().as_deref(), Some("Audio stream"));
        assert_eq!(sdp.media[0].attrs().len(), 1);
        assert_eq!(sdp.media[0].attrs()[0].key(), "rtpmap");

        assert_eq!(
            sdp.media[0].attrs()[0].value().as_deref(),
            Some("0 PCMU/8000")
        );
    }

    #[test]
    fn parse_multiple_media() {
        let sdp_str = load_sdp_file("deserialize_sdp_2.txt");
        let sdp = Sdp::parse(&sdp_str).expect("Failed to parse SDP");

        assert_eq!(sdp.media.len(), 2);

        // Audio
        assert_eq!(sdp.media[0].kind().to_string(), "audio");
        assert_eq!(*sdp.media[0].fmts(), vec!["0", "96"]);
        assert_eq!(sdp.media[0].attrs().len(), 2);

        // Video
        assert_eq!(sdp.media[1].kind().to_string(), "video");
        assert_eq!(*sdp.media[1].fmts(), vec!["97"]);
        assert_eq!(sdp.media[1].attrs()[0].key(), "rtpmap");
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
