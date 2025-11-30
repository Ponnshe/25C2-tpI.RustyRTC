use super::rtcp_error::RtcpError;
/// ReportBlock per RFC3550 §6.4.2 (24 bytes)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReportBlock {
    pub ssrc: u32,
    pub fraction_lost: u8,
    /// 24-bit signed cumulative number of packets lost.
    /// Stored here as i32 (range: -8_388_608..=8_388_607).
    pub cumulative_lost: i32,
    pub highest_seq_no_received: u32, // extended highest seq no. received
    pub interarrival_jitter: u32,
    pub lsr: u32,
    pub dlsr: u32,
}

impl ReportBlock {
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), RtcpError> {
        if buf.len() < 24 {
            return Err(RtcpError::TooShort);
        }
        let ssrc = u32::from_be_bytes(buf[0..4].try_into().unwrap());
        let fraction_lost = buf[4];
        // 24-bit signed
        let cl_raw = ((buf[5] as u32) << 16) | ((buf[6] as u32) << 8) | (buf[7] as u32);
        let cumulative_lost = if (cl_raw & 0x80_0000) != 0 {
            // negative (sign-extend)
            (cl_raw | 0xFF00_0000) as i32
        } else {
            cl_raw as i32
        };
        let highest_seq_no_received = u32::from_be_bytes(buf[8..12].try_into().unwrap());
        let interarrival_jitter = u32::from_be_bytes(buf[12..16].try_into().unwrap());
        let lsr = u32::from_be_bytes(buf[16..20].try_into().unwrap());
        let dlsr = u32::from_be_bytes(buf[20..24].try_into().unwrap());

        Ok((
            Self {
                ssrc,
                fraction_lost,
                cumulative_lost,
                highest_seq_no_received,
                interarrival_jitter,
                lsr,
                dlsr,
            },
            24,
        ))
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.ssrc.to_be_bytes());
        out.push(self.fraction_lost);
        // 24-bit signed
        let cl = self.cumulative_lost.clamp(-8_388_608, 8_388_607);
        let cl_u = cl as u32 & 0x00FF_FFFF;
        out.push(((cl_u >> 16) & 0xFF) as u8);
        out.push(((cl_u >> 8) & 0xFF) as u8);
        out.push((cl_u & 0xFF) as u8);
        out.extend_from_slice(&self.highest_seq_no_received.to_be_bytes());
        out.extend_from_slice(&self.interarrival_jitter.to_be_bytes());
        out.extend_from_slice(&self.lsr.to_be_bytes());
        out.extend_from_slice(&self.dlsr.to_be_bytes());
    }
}
