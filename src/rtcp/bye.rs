use crate::rtcp::{
    common_header::CommonHeader,
    packet_type::{PT_BYE, RtcpPacketType},
    rtcp::RtcpPacket,
    rtcp_error::RtcpError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bye {
    pub sources: Vec<u32>,
    pub reason: Option<String>,
}

impl RtcpPacketType for Bye {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), RtcpError> {
        if self.sources.len() >= 31 {
            return Err(RtcpError::TooManyByeSources(self.sources.len()));
        }
        let start = out.len();
        let hdr = CommonHeader::new(self.sources.len() as u8, PT_BYE, false);
        hdr.encode_into(out);
        for ssrc in &self.sources {
            out.extend_from_slice(&ssrc.to_be_bytes());
        }
        if let Some(reason) = &self.reason {
            let rbytes = reason.as_bytes();
            out.push(u8::try_from(rbytes.len()).unwrap_or(0));
            out.extend_from_slice(rbytes);
            // pad to 4 bytes
            let pad = (4 - ((1 + rbytes.len()) % 4)) % 4;
            if pad != 0 {
                out.extend(std::iter::repeat_n(0u8, pad));
            }
        }

        let pad = (4 - (out.len() - start) % 4) % 4;
        if pad != 0 {
            out.extend(std::iter::repeat(0u8).take(pad));
        }
        let total = out.len() - start;
        let len_words = (total / 4) - 1;
        out[start + 2] = ((len_words >> 8) & 0xFF) as u8;
        out[start + 3] = (len_words & 0xFF) as u8;
        Ok(())
    }

    fn decode(
        hdr: &super::common_header::CommonHeader,
        payload: &[u8],
    ) -> Result<RtcpPacket, RtcpError> {
        // First rc_or_fmt 5 bits indicate SSRC/CSRC count
        let sc = hdr.rc_or_fmt() as usize;
        if payload.len() < sc * 4 {
            return Err(RtcpError::Truncated);
        }
        let mut sources = Vec::with_capacity(sc);
        let mut idx = 0usize;
        for _ in 0..sc {
            let ssrc = u32::from_be_bytes(payload[idx..idx + 4].try_into().unwrap());
            sources.push(ssrc);
            idx += 4;
        }
        let reason = if payload.len() > idx {
            let len = payload[idx] as usize;
            idx += 1;
            if payload.len() < idx + len {
                return Err(RtcpError::Truncated);
            }
            let s = String::from_utf8_lossy(&payload[idx..idx + len]).into_owned();
            Some(s)
        } else {
            None
        };
        Ok(RtcpPacket::Bye(Bye { sources, reason }))
    }
}

impl Bye {
    pub fn single(ssrc: u32, reason: Option<String>) -> Self {
        Self {
            sources: vec![ssrc],
            reason,
        }
    }
}
