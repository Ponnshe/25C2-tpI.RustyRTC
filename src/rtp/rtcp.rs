// RTCP packet types
pub const PT_SR: u8 = 200;
pub const PT_RR: u8 = 201;
pub const PT_SDES: u8 = 202;
pub const PT_BYE: u8 = 203;
pub const PT_APP: u8 = 204; // unused here
// RFC4585/5104 feedback
pub const PT_RTPFB: u8 = 205; // Generic NACK
pub const PT_PSFB: u8 = 206; // PLI, FIR, etc.

#[derive(Debug, Clone)]
pub enum RtcpPacket {
    Sr(SenderReport),
    Rr(ReceiverReport),
    Sdes(Sdes),
    Bye(Bye),
    Pli(PictureLossIndication),
    Nack(GenericNack),
}

#[derive(Debug, Clone, Default)]
pub struct ReportBlock {
    pub ssrc: u32,
    pub fraction_lost: u8,
    pub cumulative_lost: u32, // 24-bit stored into u32
    pub highest_seq: u32,
    pub jitter: u32,
    pub lsr: u32,
    pub dlsr: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SenderReport {
    pub ssrc: u32,
    pub ntp_msw: u32,
    pub ntp_lsw: u32,
    pub rtp_ts: u32,
    pub packet_count: u32,
    pub octet_count: u32,
    pub reports: Vec<ReportBlock>,
}

#[derive(Debug, Clone, Default)]
pub struct ReceiverReport {
    pub ssrc: u32,
    pub reports: Vec<ReportBlock>,
}

#[derive(Debug, Clone)]
pub struct SdesItem {
    pub ssrc: u32,
    pub cname: String,
}

#[derive(Debug, Clone)]
pub struct Sdes {
    pub items: Vec<SdesItem>,
}

#[derive(Debug, Clone)]
pub struct Bye {
    pub ssrc: u32,
}

#[derive(Debug, Clone)]
pub struct PictureLossIndication {
    pub sender_ssrc: u32,
    pub media_ssrc: u32,
}

#[derive(Debug, Clone)]
pub struct GenericNack {
    pub sender_ssrc: u32,
    pub media_ssrc: u32,
    pub pid: u16,
    pub blp: u16,
}

impl SenderReport {
    pub fn encode(&self) -> Vec<u8> {
        let rc = self.reports.len() as u8;
        let mut body = Vec::new();
        body.extend_from_slice(&self.ssrc.to_be_bytes());
        body.extend_from_slice(&self.ntp_msw.to_be_bytes());
        body.extend_from_slice(&self.ntp_lsw.to_be_bytes());
        body.extend_from_slice(&self.rtp_ts.to_be_bytes());
        body.extend_from_slice(&self.packet_count.to_be_bytes());
        body.extend_from_slice(&self.octet_count.to_be_bytes());
        for r in &self.reports {
            body.extend_from_slice(&encode_report_block(r));
        }
        wrap_rtcp(PT_SR, rc, &body)
    }
}

impl ReceiverReport {
    pub fn encode(&self) -> Vec<u8> {
        let rc = self.reports.len() as u8;
        let mut body = Vec::new();
        body.extend_from_slice(&self.ssrc.to_be_bytes());
        for r in &self.reports {
            body.extend_from_slice(&encode_report_block(r));
        }
        wrap_rtcp(PT_RR, rc, &body)
    }
}

impl Sdes {
    pub fn encode(&self) -> Vec<u8> {
        // Only SDES/CNAME for first item
        let mut body = Vec::new();
        for it in &self.items {
            body.extend_from_slice(&it.ssrc.to_be_bytes());
            body.push(1); // CNAME type
            body.push(it.cname.len() as u8);
            body.extend_from_slice(it.cname.as_bytes());
            body.push(0); // end
            // pad to 4B
            while (body.len() % 4) != 0 {
                body.push(0);
            }
        }
        wrap_rtcp(PT_SDES, self.items.len() as u8, &body)
    }
}

impl Bye {
    pub fn encode(&self) -> Vec<u8> {
        wrap_rtcp(PT_BYE, 1, &self.ssrc.to_be_bytes())
    }
}

impl PictureLossIndication {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&self.sender_ssrc.to_be_bytes());
        body.extend_from_slice(&self.media_ssrc.to_be_bytes());
        wrap_rtcp(PT_PSFB, 1, &body) // FMT=1 encoded in first 5 bits of count — simplified here
    }
}

impl GenericNack {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&self.sender_ssrc.to_be_bytes());
        body.extend_from_slice(&self.media_ssrc.to_be_bytes());
        body.extend_from_slice(&self.pid.to_be_bytes());
        body.extend_from_slice(&self.blp.to_be_bytes());
        wrap_rtcp(PT_RTPFB, 1, &body) // FMT=1 (generic NACK)
    }
}

fn encode_report_block(r: &ReportBlock) -> [u8; 24] {
    let mut out = [0u8; 24];
    out[0..4].copy_from_slice(&r.ssrc.to_be_bytes());
    out[4] = r.fraction_lost;
    let cl = r.cumulative_lost & 0x00FF_FFFF;
    out[5] = ((cl >> 16) & 0xFF) as u8;
    out[6] = ((cl >> 8) & 0xFF) as u8;
    out[7] = (cl & 0xFF) as u8;
    out[8..12].copy_from_slice(&r.highest_seq.to_be_bytes());
    out[12..16].copy_from_slice(&r.jitter.to_be_bytes());
    out[16..20].copy_from_slice(&r.lsr.to_be_bytes());
    out[20..24].copy_from_slice(&r.dlsr.to_be_bytes());
    out
}

fn wrap_rtcp(pt: u8, count: u8, body: &[u8]) -> Vec<u8> {
    // Common header: V(2)=2, P=0, RC=count, PT, length(words-1)
    let len_words = ((body.len() + 4) / 4) as u16; // header excluded in RFC length? include header → minus 1
    let length = len_words - 1;
    let mut out = Vec::with_capacity(4 + body.len());
    out.push(0x80 | (count & 0x1F)); // V=2
    out.push(pt);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(body);
    out
}

pub fn is_rtcp(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && (bytes[1] >= 200 && bytes[1] <= 206)
}
