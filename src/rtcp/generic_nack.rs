use crate::rtcp::{
    RtcpPacket,
    common_header::CommonHeader,
    packet_type::{PT_RTPFB, RtcpPacketType},
    rtcp_error::RtcpError,
};

// Feedback: Generic NACK (RTPFB, FMT=1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericNack {
    pub sender_ssrc: u32,
    pub media_ssrc: u32,
    /// Each entry is (PID, BLP) as in RFC4585 §6.2.1
    pub entries: Vec<(u16, u16)>,
}

impl RtcpPacketType for GenericNack {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), RtcpError> {
        let start = out.len();
        let hdr = CommonHeader::new(1, PT_RTPFB, false);
        hdr.encode_into(out);
        out.extend_from_slice(&self.sender_ssrc.to_be_bytes());
        out.extend_from_slice(&self.media_ssrc.to_be_bytes());
        for (pid, blp) in &self.entries {
            out.extend_from_slice(&pid.to_be_bytes());
            out.extend_from_slice(&blp.to_be_bytes());
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

    fn decode(
        hdr: &super::common_header::CommonHeader,
        payload: &[u8],
    ) -> Result<RtcpPacket, RtcpError> {
        // Transport layer feedback (205). We only support FMT=1 (Generic NACK).
        if payload.len() < 8 {
            return Err(RtcpError::TooShort);
        }
        let sender_ssrc =
            u32::from_be_bytes(payload[0..4].try_into().map_err(|_| RtcpError::TooShort)?);
        let media_ssrc =
            u32::from_be_bytes(payload[4..8].try_into().map_err(|_| RtcpError::TooShort)?);
        match hdr.rc_or_fmt() {
            1 => {
                // Generic NACK entries (pid, blp) pairs
                let mut idx = 8usize;
                let mut entries = Vec::new();
                while idx + 4 <= payload.len() {
                    let pid = u16::from_be_bytes(
                        payload[idx..idx + 2]
                            .try_into()
                            .map_err(|_| RtcpError::TooShort)?,
                    );
                    let blp = u16::from_be_bytes(
                        payload[idx + 2..idx + 4]
                            .try_into()
                            .map_err(|_| RtcpError::TooShort)?,
                    );
                    entries.push((pid, blp));
                    idx += 4;
                }
                if idx != payload.len() {
                    return Err(RtcpError::Truncated);
                }
                Ok(RtcpPacket::Nack(GenericNack {
                    sender_ssrc,
                    media_ssrc,
                    entries,
                }))
            }
            _ => Err(RtcpError::Invalid),
        }
    }
}

impl GenericNack {
    pub fn new(sender_ssrc: u32, media_ssrc: u32, entries: Vec<(u16, u16)>) -> Self {
        Self {
            sender_ssrc,
            media_ssrc,
            entries,
        }
    }
}
