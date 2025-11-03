use crate::rtcp::{
    packet_type::{PT_SR, RtcpPacketType},
    rtcp::RtcpPacket,
    rtcp_error::RtcpError,
};

use super::{common_header::CommonHeader, report_block::ReportBlock, sender_info::SenderInfo};
const MAX_RC: usize = 31;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SenderReport {
    pub ssrc: u32,
    pub info: SenderInfo,
    pub reports: Vec<ReportBlock>,
    /// Optional profile-specific data trailing the SR block.
    pub profile_ext: Vec<u8>,
}

impl RtcpPacketType for SenderReport {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), RtcpError> {
        if self.reports.len() > MAX_RC {
            return Err(RtcpError::TooManyReportBlocks(self.reports.len()));
        };
        let start = out.len();
        // placeholder header
        let hdr = CommonHeader::new(self.reports.len() as u8, PT_SR, false);
        hdr.encode_into(out);
        out.extend_from_slice(&self.ssrc.to_be_bytes());
        self.info.encode_into(out);
        for rb in &self.reports {
            rb.encode_into(out);
        }
        out.extend_from_slice(&self.profile_ext);

        // Pad to 32-bit
        let pad = (4 - (out.len() - start) % 4) % 4;
        if pad != 0 {
            out.extend(std::iter::repeat(0u8).take(pad));
        }

        // fix length
        let total = out.len() - start;
        let len_words = (total / 4) - 1;
        out[start + 2] = ((len_words >> 8) & 0xFF) as u8;
        out[start + 3] = (len_words & 0xFF) as u8;
        Ok(())
    }

    fn decode(hdr: &CommonHeader, payload: &[u8]) -> Result<RtcpPacket, RtcpError> {
        if payload.len() < 24 {
            return Err(RtcpError::TooShort);
        }
        let ssrc = u32::from_be_bytes(payload[0..4].try_into().unwrap());
        let (info, used) = SenderInfo::decode(&payload[4..])?;
        let mut idx = 4 + used;

        // Report blocks
        let rc = hdr.rc_or_fmt() as usize;
        let mut reports = Vec::with_capacity(rc);
        for _ in 0..rc {
            if payload.len() < idx + 24 {
                return Err(RtcpError::Truncated);
            }
            let (rb, used) = ReportBlock::decode(&payload[idx..])?;
            idx += used;
            reports.push(rb);
        }

        // Remaining is profile-specific extension
        let profile_ext = payload[idx..].to_vec();

        Ok(RtcpPacket::Sr(SenderReport {
            ssrc,
            info,
            reports,
            profile_ext,
        }))
    }
}

impl SenderReport {
    pub fn new(ssrc: u32, info: SenderInfo, reports: Vec<ReportBlock>) -> Self {
        Self {
            ssrc,
            info,
            reports,
            profile_ext: Vec::new(),
        }
    }
}
