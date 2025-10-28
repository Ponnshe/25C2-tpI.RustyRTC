/// Sender info in SR (20 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SenderInfo {
    pub ntp_msw: u32,
    pub ntp_lsw: u32,
    pub rtp_ts: u32,
    pub packet_count: u32,
    pub octet_count: u32,
}

impl SenderInfo {
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), RtcpError> {
        if buf.len() < 20 {
            return Err(RtcpError::TooShort);
        }
        Ok((
            Self {
                ntp_msw: u32::from_be_bytes(buf[0..4].try_into().unwrap()),
                ntp_lsw: u32::from_be_bytes(buf[4..8].try_into().unwrap()),
                rtp_ts: u32::from_be_bytes(buf[8..12].try_into().unwrap()),
                packet_count: u32::from_be_bytes(buf[12..16].try_into().unwrap()),
                octet_count: u32::from_be_bytes(buf[16..20].try_into().unwrap()),
            },
            20,
        ))
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.ntp_msw.to_be_bytes());
        out.extend_from_slice(&self.ntp_lsw.to_be_bytes());
        out.extend_from_slice(&self.rtp_ts.to_be_bytes());
        out.extend_from_slice(&self.packet_count.to_be_bytes());
        out.extend_from_slice(&self.octet_count.to_be_bytes());
    }
}
