use crate::rtcp::{
    common_header::CommonHeader,
    packet_type::{PT_PSFB, RtcpPacketType},
    rtcp::RtcpPacket,
    rtcp_error::RtcpError,
};

// Feedback: PLI (PSFB, FMT=1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureLossIndication {
    pub sender_ssrc: u32,
    pub media_ssrc: u32,
}

impl RtcpPacketType for PictureLossIndication {
    fn encode_into(&self, out: &mut Vec<u8>) {
        let start = out.len();
        let hdr = CommonHeader::new(1, PT_PSFB, false);
        hdr.encode_into(out);
        out.extend_from_slice(&self.sender_ssrc.to_be_bytes());
        out.extend_from_slice(&self.media_ssrc.to_be_bytes());
        // no FCI for PLI
        let pad = (4 - (out.len() - start) % 4) % 4;
        if pad != 0 {
            out.extend(std::iter::repeat(0u8).take(pad));
        }
        let total = out.len() - start;
        let len_words = (total / 4) - 1;
        out[start + 2] = ((len_words >> 8) & 0xFF) as u8;
        out[start + 3] = (len_words & 0xFF) as u8;
    }

    fn decode(
        hdr: &super::common_header::CommonHeader,
        payload: &[u8],
    ) -> Result<RtcpPacket, RtcpError> {
        // Payload-specific feedback (206); support FMT=1 (PLI) only.
        if payload.len() < 8 {
            return Err(RtcpError::TooShort);
        }
        let sender_ssrc = u32::from_be_bytes(payload[0..4].try_into().unwrap());
        let media_ssrc = u32::from_be_bytes(payload[4..8].try_into().unwrap());
        match hdr.rc_or_fmt() {
            1 => Ok(RtcpPacket::Pli(PictureLossIndication {
                sender_ssrc,
                media_ssrc,
            })),
            _ => Err(RtcpError::Invalid),
        }
    }
}

impl PictureLossIndication {
    pub fn new(sender_ssrc: u32, media_ssrc: u32) -> Self {
        Self {
            sender_ssrc,
            media_ssrc,
        }
    }
}
