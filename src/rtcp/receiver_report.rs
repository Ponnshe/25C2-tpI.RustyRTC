use crate::rtcp::packet_type::PT_RR;

use super::{
    RtcpPacket, common_header::CommonHeader, packet_type::RtcpPacketType,
    report_block::ReportBlock, rtcp_error::RtcpError,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReceiverReport {
    pub ssrc: u32,
    pub reports: Vec<ReportBlock>,
    pub profile_ext: Vec<u8>,
}

impl RtcpPacketType for ReceiverReport {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), RtcpError> {
        let start = out.len();
        let hdr = CommonHeader::new(self.reports.len() as u8, PT_RR, false);
        hdr.encode_into(out);
        out.extend_from_slice(&self.ssrc.to_be_bytes());
        for rb in &self.reports {
            rb.encode_into(out);
        }
        out.extend_from_slice(&self.profile_ext);

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

    fn decode(hdr: &CommonHeader, payload: &[u8]) -> Result<RtcpPacket, RtcpError> {
        if payload.len() < 4 {
            return Err(RtcpError::TooShort);
        }
        let ssrc = u32::from_be_bytes(payload[0..4].try_into().map_err(|_| RtcpError::TooShort)?);
        let mut idx = 4usize;

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
        let profile_ext = payload[idx..].to_vec();
        Ok(RtcpPacket::Rr(ReceiverReport {
            ssrc,
            reports,
            profile_ext,
        }))
    }
}

impl ReceiverReport {
    pub fn new(ssrc: u32, reports: Vec<ReportBlock>) -> Self {
        Self {
            ssrc,
            reports,
            profile_ext: Vec::new(),
        }
    }
}
