use super::{config::RTCP_VERSION, rtcp_error::RtcpError};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonHeader {
    version: u8,       // 2
    padding: bool,     // P
    rc_or_fmt: u8,     // 5 bits (report count or FMT)
    pt: u8,            // packet type
    length_words: u16, // number of 32-bit words minus one
}

impl CommonHeader {
    pub fn new(rc_or_fmt: u8, pt: u8, padding: bool) -> Self {
        Self {
            version: RTCP_VERSION,
            padding,
            rc_or_fmt,
            pt,
            length_words: 0,
        }
    }

    pub fn with_length(rc_or_fmt: u8, pt: u8, padding: bool, length_words: u16) -> Self {
        Self {
            version: RTCP_VERSION,
            padding,
            rc_or_fmt,
            pt,
            length_words,
        }
    }
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), RtcpError> {
        if buf.len() < 4 {
            return Err(RtcpError::TooShort);
        }
        let vprc = buf[0];
        let version = vprc >> 6;
        if version != RTCP_VERSION {
            return Err(RtcpError::BadVersion(version));
        }
        let padding = ((vprc >> 5) & 1) != 0;
        let rc_or_fmt = vprc & 0x1F;
        let pt = buf[1];
        let length_words =
            u16::from_be_bytes(buf[2..4].try_into().map_err(|_| RtcpError::TooShort)?);

        let total_bytes = ((length_words as usize) + 1) * 4;
        if buf.len() < total_bytes {
            return Err(RtcpError::TooShort);
        }

        Ok((
            Self {
                version,
                padding,
                rc_or_fmt,
                pt,
                length_words,
            },
            total_bytes,
        ))
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        let vprc = (self.version & 0b11) << 6 | (self.padding as u8) << 5 | (self.rc_or_fmt & 0x1F);
        out.push(vprc);
        out.push(self.pt);
        out.extend_from_slice(&self.length_words.to_be_bytes());
    }

    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn padding(&self) -> bool {
        self.padding
    }

    pub fn rc_or_fmt(&self) -> u8 {
        self.rc_or_fmt
    }

    pub fn pt(&self) -> u8 {
        self.pt
    }

    pub fn length_words(&self) -> u16 {
        self.length_words
    }
}
