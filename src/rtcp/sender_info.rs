use super::rtcp_error::RtcpError;
/// Sender info in SR (20 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SenderInfo {
    pub ntp_most_sw: u32,
    pub now_least_sw: u32,
    pub rtp_ts: u32,
    pub packet_count: u32,
    pub octet_count: u32,
}

impl SenderInfo {
    pub fn new(
        ntp_most_sw: u32,
        now_least_sw: u32,
        rtp_ts: u32,
        packet_count: u32,
        octet_count: u32,
    ) -> Self {
        Self {
            ntp_most_sw,
            now_least_sw,
            rtp_ts,
            packet_count,
            octet_count,
        }
    }
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), RtcpError> {
        if buf.len() < 20 {
            return Err(RtcpError::TooShort);
        }
        Ok((
            Self {
                ntp_most_sw: u32::from_be_bytes(
                    buf[0..4].try_into().map_err(|_| RtcpError::TooShort)?,
                ),
                now_least_sw: u32::from_be_bytes(
                    buf[4..8].try_into().map_err(|_| RtcpError::TooShort)?,
                ),
                rtp_ts: u32::from_be_bytes(buf[8..12].try_into().map_err(|_| RtcpError::TooShort)?),
                packet_count: u32::from_be_bytes(
                    buf[12..16].try_into().map_err(|_| RtcpError::TooShort)?,
                ),
                octet_count: u32::from_be_bytes(
                    buf[16..20].try_into().map_err(|_| RtcpError::TooShort)?,
                ),
            },
            20,
        ))
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.ntp_most_sw.to_be_bytes());
        out.extend_from_slice(&self.now_least_sw.to_be_bytes());
        out.extend_from_slice(&self.rtp_ts.to_be_bytes());
        out.extend_from_slice(&self.packet_count.to_be_bytes());
        out.extend_from_slice(&self.octet_count.to_be_bytes());
    }
}
