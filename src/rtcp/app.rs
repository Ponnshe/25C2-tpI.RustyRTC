use crate::rtcp::{
    RtcpPacket,
    common_header::CommonHeader,
    packet_type::{PT_APP, RtcpPacketType},
    rtcp_error::RtcpError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub subtype: u8, // from rc_or_fmt
    pub name: [u8; 4],
    pub ssrc: u32,
    pub data: Vec<u8>,
}

impl RtcpPacketType for App {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), RtcpError> {
        let start = out.len();
        let hdr = CommonHeader::new(self.subtype & 0x1F, PT_APP, false);
        hdr.encode_into(out);
        out.extend_from_slice(&self.ssrc.to_be_bytes());
        out.extend_from_slice(&self.name);
        out.extend_from_slice(&self.data);
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
        if payload.len() < 8 {
            return Err(RtcpError::TooShort);
        }
        let ssrc = u32::from_be_bytes(payload[0..4].try_into().map_err(|_| RtcpError::TooShort)?);
        let mut name = [0u8; 4];
        name.copy_from_slice(&payload[4..8]);
        let data = payload[8..].to_vec();
        Ok(RtcpPacket::App(App {
            subtype: hdr.rc_or_fmt() & 0x1F,
            name,
            ssrc,
            data,
        }))
    }
}
