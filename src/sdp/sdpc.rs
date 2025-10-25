//! `sdpc`: top-level SDP record and parse/encode entrypoints.
//!
//! This module exposes [`Sdp`] and its two main methods:
//! - [`Sdp::parse`] — parse a full SDP text into a structured value
//! - [`Sdp::encode`] — serialize an [`Sdp`] back to text (CRLF line endings)
//!
//! Parsing is line-oriented: we dispatch on the SDP prefix (`v/o/s/i/u/e/p/c/b/t/r/z/m/a`)
//! and delegate each RHS to component types that implement `FromStr<Err = SdpError>`
//! (e.g., [`Origin`], [`Connection`], [`Bandwidth`], [`Attribute`], [`TimeDesc`], [`Media`]).
//! The encoder relies on their `Display`/format helpers.
//!
//! **Input** accepts `\n` or `\r\n`; output always uses `\r\n` (CRLF).
//! Unknown session-level lines are preserved in [`Sdp::extra_lines`].
//! ### Examples
//! ```ignore
//! use crate::sdp::sdpc::Sdp;
//!
//! let raw = "\
//! v=0\r\n\
//! o=- 123 1 IN IP4 203.0.113.1\r\n\
//! s=Example\r\n\
//! t=0 0\r\n\
//! m=audio 49170 RTP/AVP 0\r\n\
//! a=rtpmap:0 PCMU/8000\r\n";
//!
//! let sdp = Sdp::parse(raw)?;
//! assert_eq!(sdp.version, 0);
//! let text = sdp.encode(); // always CRLF
//! ```

use crate::ice::type_ice::candidate::Candidate;
use crate::sdp::attribute::Attribute;
use crate::sdp::bandwidth::Bandwidth;
use crate::sdp::connection::Connection;
use crate::sdp::media::Media;
use crate::sdp::origin::Origin;
use crate::sdp::sdp_error::SdpError;
use crate::sdp::time_desc::TimeDesc;

/// In-memory representation of an SDP message (session + zero or more media sections).
///
/// Fields mirror well-known SDP lines. Session-vs-media routing rules:
/// - `i=`, `c=`, `b=`, `a=` lines are applied to the **current media** if we are
///   inside an `m=` section; otherwise they apply at the **session** level.
/// - `r=` and `z=` are attached to the **last** `t=` block.
/// - Any unknown session-level lines are preserved verbatim in [`extra_lines`].
#[derive(Debug, Clone)]
pub struct Sdp {
    /// `v=` — SDP version (per spec this is always `0`).
    pub version: u8,
    /// `o=` — session origin.
    pub origin: Origin,
    /// `s=` — session name.
    pub session_name: String,
    /// `i=` (session) — optional session information/title.
    pub session_info: Option<String>,
    /// `u=` — optional session URI.
    pub uri: Option<String>,
    /// `e=` — zero or more contact emails.
    pub emails: Vec<String>,
    /// `p=` — zero or more contact phones.
    pub phones: Vec<String>,
    /// `c=` (session) — optional session-level connection info.
    pub connection: Option<Connection>,
    /// `b=` (session) — zero or more bandwidth lines.
    pub bandwidth: Vec<Bandwidth>,
    /// One or more `t=` time descriptions; `r=`/`z=` hang off the last pushed `t=`.
    pub times: Vec<TimeDesc>,
    /// `a=` (session) — zero or more session-level attributes.
    pub attrs: Vec<Attribute>,
    /// Zero or more `m=` sections (each with their own `i/c/b/a` and extra lines).
    pub media: Vec<Media>,
    /// Unknown session-level lines preserved verbatim.
    pub extra_lines: Vec<String>,
}

impl Sdp {
    pub fn new(
        version: u8,
        origin: Origin,
        session_name: String,
        session_info: Option<String>,
        uri: Option<String>,
        emails: Vec<String>,
        phones: Vec<String>,
        connection: Option<Connection>,
        bandwidth: Vec<Bandwidth>,
        times: Vec<TimeDesc>,
        attrs: Vec<Attribute>,
        media: Vec<Media>,
        extra_lines: Vec<String>,
    ) -> Self {
        Self {
            version,
            origin,
            session_name,
            session_info,
            uri,
            emails,
            phones,
            connection,
            bandwidth,
            times,
            attrs,
            media,
            extra_lines,
        }
    }

    /// Parse a full SDP text into [`Sdp`].
    ///
    /// - Accepts `\n` or `\r\n` line endings; `\r` is stripped per line.
    /// - Routes `i/c/b/a` either to session or the current `m=` block.
    /// - Attaches `r=`/`z=` to the last `t=` seen; errors if none exists.
    /// - Returns `SdpError::Missing` if required lines (`v=`, `o=`, `s=`) are absent.
    ///
    /// # Errors
    /// Propagates component parsing errors as [`SdpError`], including:
    /// - `Missing("v=" | "o=" | "s=")`
    /// - `Invalid("<prefix>")` for arity/structure problems
    /// - `ParseInt` for numeric fields
    /// - `AddrType` for invalid address family tokens
    ///
    /// # Example
    /// ```ignore
    /// let sdp = Sdp::parse("v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=Test\r\nt=0 0\r\n")?;
    /// assert_eq!(sdp.session_name, "Test");
    /// ```
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

        // Tracks whether subsequent i=/c=/b=/a= lines target session or current media.
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

    /// Encode this [`Sdp`] into an SDP text with **CRLF** (`\r\n`) line endings.
    ///
    /// If no `t=` blocks were parsed/added, emits a single `t=0 0` (common WebRTC default).
    /// Media sections are serialized via `Media::fmt_lines`, which appends their `i/c/b/a`
    /// and any media-level extra lines.
    ///
    /// # Example
    /// ```ignore
    /// let s = sdp.encode();
    /// assert!(s.starts_with("v=0\r\n"));
    /// assert!(s.contains("\r\n"));
    /// ```
    #[allow(clippy::too_many_lines)]
    pub fn encode(&self) -> String {
        let mut out = String::new();
        macro_rules! pushln {
            ($($arg:tt)*) => {{
                use std::fmt::Write as _;
                let _ = write!(out, $($arg)*);
                let _ = out.write_str("\r\n");
            }};
        }

        pushln!("v={}", self.version);
        pushln!("o={}", self.origin);
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
            pushln!("c={}", c);
        }
        for b in &self.bandwidth {
            pushln!("b={}", b);
        }

        if self.times.is_empty() {
            pushln!("t=0 0");
        } else {
            for t in &self.times {
                t.fmt_lines(&mut out); // writes t=/r=/z= with CRLFs
            }
        }

        for a in &self.attrs {
            pushln!("a={}", a);
        }
        for x in &self.extra_lines {
            pushln!("{}", x);
        }

        for m in &self.media {
            m.fmt_lines(&mut out); // writes m=/i=/c=/b=/a=/extras with CRLFs
        }
        out
    }

    pub fn media(&self) -> &Vec<Media> {
        &self.media
    }
}
/// Split an SDP line into `(prefix, rhs)` by the first `=`, e.g. `"a=foo"` → `("a", "foo")`.
fn split_line(line: &str) -> Option<(&str, &str)> {
    let mut it = line.splitn(2, '=');
    Some((it.next()?, it.next()?))
}

pub(crate) fn push_crlf(out: &mut String, args: std::fmt::Arguments) {
    use std::fmt::Write as _;
    let _ = out.write_fmt(args);
    let _ = out.write_str("\r\n");
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::sdp::addr_type::AddrType;
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
