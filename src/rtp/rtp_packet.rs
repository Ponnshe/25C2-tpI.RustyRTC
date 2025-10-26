use std::io;

#[derive(Debug, Clone)]
pub struct RtpHeader {
    pub version: u8, // 2
    pub padding: bool,
    pub extension: bool,
    pub csrc_count: u8, // 0..15
    pub marker: bool,
    pub payload_type: u8, // 0..127
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub csrc: Vec<u32>,
}

impl Default for RtpHeader {
    fn default() -> Self {
        Self {
            version: 2,
            padding: false,
            extension: false,
            csrc_count: 0,
            marker: false,
            payload_type: 96, // dynamic
            sequence_number: 0,
            timestamp: 0,
            ssrc: 0,
            csrc: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RtpPacket {
    pub header: RtpHeader,
    pub payload: Vec<u8>,
}

impl RtpPacket {
    pub fn encode(&self) -> Vec<u8> {
        let h = &self.header;
        let mut buf = Vec::with_capacity(12 + self.payload.len() + (h.csrc_count as usize) * 4);
        let b0 = (h.version & 0x03) << 6
            | ((h.padding as u8) << 5)
            | ((h.extension as u8) << 4)
            | (h.csrc_count & 0x0F);
        buf.push(b0);
        let b1 = ((h.marker as u8) << 7) | (h.payload_type & 0x7F);
        buf.push(b1);
        buf.extend_from_slice(&h.sequence_number.to_be_bytes());
        buf.extend_from_slice(&h.timestamp.to_be_bytes());
        buf.extend_from_slice(&h.ssrc.to_be_bytes());
        for c in &h.csrc {
            buf.extend_from_slice(&c.to_be_bytes());
        }
        // No header extension nor padding in this minimal impl
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 12 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "rtp<12"));
        }
        let b0 = bytes[0];
        let b1 = bytes[1];
        let version = b0 >> 6;
        if version != 2 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "rtp v!=2"));
        }
        let padding = (b0 & 0x20) != 0;
        let extension = (b0 & 0x10) != 0;
        let csrc_count = b0 & 0x0F;
        let marker = (b1 & 0x80) != 0;
        let payload_type = b1 & 0x7F;
        let seq = u16::from_be_bytes([bytes[2], bytes[3]]);
        let ts = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let ssrc = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let mut offset = 12;
        let mut csrc = Vec::with_capacity(csrc_count as usize);
        for _ in 0..csrc_count {
            if bytes.len() < offset + 4 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "rtp csrc"));
            }
            csrc.push(u32::from_be_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]));
            offset += 4;
        }
        if extension {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "rtp hdr ext not supported",
            ));
        }
        let payload_end = if padding {
            // simple padding parse
            let pad = *bytes.last().unwrap() as usize;
            bytes.len().saturating_sub(pad)
        } else {
            bytes.len()
        };
        let payload = bytes[offset..payload_end].to_vec();
        Ok(Self {
            header: RtpHeader {
                version,
                padding,
                extension,
                csrc_count,
                marker,
                payload_type,
                sequence_number: seq,
                timestamp: ts,
                ssrc,
                csrc,
            },
            payload,
        })
    }
}

/// Helpers for extended sequence (wrap tracking)
#[derive(Debug, Default, Clone)]
pub struct SeqExt {
    pub cycles: u32,
    pub max_seq: u16,
}
impl SeqExt {
    pub fn update(&mut self, seq: u16) -> u32 {
        // RFC3550 A.1 seq wrap
        if seq < self.max_seq && (self.max_seq - seq) > 0x8000 {
            self.cycles = self.cycles.wrapping_add(1 << 16);
        }
        if seq > self.max_seq {
            self.max_seq = seq;
        }
        self.cycles | seq as u32
    }
}
