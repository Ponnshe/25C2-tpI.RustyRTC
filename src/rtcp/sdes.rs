use crate::rtcp::{
    common_header::CommonHeader,
    packet_type::{PT_SDES, RtcpPacketType},
    rtcp::RtcpPacket,
    rtcp_error::RtcpError,
};
/// SDES items (subset, extend as needed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdesItem {
    Cname(String), // type=1
    Name(String),  // 2
    Email(String), // 3
    Phone(String), // 4
    Loc(String),   // 5
    Tool(String),  // 6
    Note(String),  // 7
    Priv(Vec<u8>), // 8 (opaque)
    Unknown(u8, Vec<u8>),
}

impl SdesItem {
    fn typ_code(&self) -> u8 {
        match self {
            SdesItem::Cname(_) => 1,
            SdesItem::Name(_) => 2,
            SdesItem::Email(_) => 3,
            SdesItem::Phone(_) => 4,
            SdesItem::Loc(_) => 5,
            SdesItem::Tool(_) => 6,
            SdesItem::Note(_) => 7,
            SdesItem::Priv(_) => 8,
            SdesItem::Unknown(t, _) => *t,
        }
    }
    fn as_bytes(&self) -> (u8, Vec<u8>) {
        match self {
            SdesItem::Cname(s)
            | SdesItem::Name(s)
            | SdesItem::Email(s)
            | SdesItem::Phone(s)
            | SdesItem::Loc(s)
            | SdesItem::Tool(s)
            | SdesItem::Note(s) => (self.typ_code(), s.as_bytes().to_vec()),
            SdesItem::Priv(v) => (self.typ_code(), v.clone()),
            SdesItem::Unknown(t, v) => (*t, v.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SdesChunk {
    pub ssrc: u32,
    pub items: Vec<SdesItem>,
}

impl SdesChunk {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), RtcpError> {
        let start = out.len();
        out.extend_from_slice(&self.ssrc.to_be_bytes());
        for item in &self.items {
            let (t, data) = item.as_bytes();
            // Fail fast if data is >255
            if data.len() > u8::MAX as usize {
                return Err(RtcpError::SdesItemTooLong);
            };
            out.push(t);
            out.push(data.len() as u8);
            out.extend_from_slice(&data);
        }
        out.push(0); // END
        let rem = (out.len() - start) % 4;
        if rem != 0 {
            out.extend(std::iter::repeat_n(0u8, 4 - rem));
        }
        Ok(())
    }

    fn decode(buf: &[u8]) -> Result<(Self, usize), RtcpError> {
        if buf.len() < 4 {
            return Err(RtcpError::TooShort);
        }
        let ssrc = u32::from_be_bytes(buf[0..4].try_into().map_err(|_| RtcpError::TooShort)?);
        let mut idx = 4usize;
        let mut items = Vec::new();

        // Items until END(0). After END, pad to 4-byte boundary.
        while idx < buf.len() {
            let t = buf[idx];
            idx += 1;
            if t == 0 {
                // move to 4-byte boundary relative to chunk start
                let chunk_len = idx; // includes END
                let pad = (4 - (chunk_len % 4)) % 4;
                if buf.len() < idx + pad {
                    return Err(RtcpError::Truncated);
                }
                idx += pad;
                break;
            }
            if buf.len() < idx + 1 {
                return Err(RtcpError::SdesItemTooShort);
            }
            let len = buf[idx] as usize;
            idx += 1;
            if buf.len() < idx + len {
                return Err(RtcpError::SdesItemTooShort);
            }
            let data = &buf[idx..idx + len];
            idx += len;

            let item = match t {
                1 => SdesItem::Cname(String::from_utf8_lossy(data).into_owned()),
                2 => SdesItem::Name(String::from_utf8_lossy(data).into_owned()),
                3 => SdesItem::Email(String::from_utf8_lossy(data).into_owned()),
                4 => SdesItem::Phone(String::from_utf8_lossy(data).into_owned()),
                5 => SdesItem::Loc(String::from_utf8_lossy(data).into_owned()),
                6 => SdesItem::Tool(String::from_utf8_lossy(data).into_owned()),
                7 => SdesItem::Note(String::from_utf8_lossy(data).into_owned()),
                8 => SdesItem::Priv(data.to_vec()),
                _ => SdesItem::Unknown(t, data.to_vec()),
            };
            items.push(item);
        }

        Ok((Self { ssrc, items }, idx))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sdes {
    pub chunks: Vec<SdesChunk>,
}

impl Sdes {
    pub fn cname(ssrc: u32, cname: impl Into<String>) -> Self {
        Self {
            chunks: vec![SdesChunk {
                ssrc,
                items: vec![SdesItem::Cname(cname.into())],
            }],
        }
    }
}

impl RtcpPacketType for Sdes {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), RtcpError> {
        let start = out.len();
        let hdr = CommonHeader::new(self.chunks.len() as u8, PT_SDES, false);
        hdr.encode_into(out);
        for ch in &self.chunks {
            ch.encode_into(out)?;
        }

        let pad = (4 - (out.len() - start) % 4) % 4;
        if pad != 0 {
            out.extend(std::iter::repeat_n(0u8, pad));
        }
        let total = out.len() - start;
        let len_words = (total / 4) - 1;
        out[start + 2] = ((len_words >> 8) & 0xFF) as u8;
        out[start + 3] = (len_words & 0xFF) as u8;
        Ok(())
    }

    fn decode(_hdr: &CommonHeader, payload: &[u8]) -> Result<RtcpPacket, RtcpError> {
        // SDES is a sequence of chunks occupying the whole payload.
        let mut chunks = Vec::new();
        let mut idx = 0usize;
        while idx + 4 <= payload.len() {
            let (chunk, used) = SdesChunk::decode(&payload[idx..])?;
            chunks.push(chunk);
            idx += used;
        }
        if idx != payload.len() {
            // trailing non-aligned data indicates malformed SDES
            return Err(RtcpError::Truncated);
        }
        Ok(RtcpPacket::Sdes(Sdes { chunks }))
    }
}
