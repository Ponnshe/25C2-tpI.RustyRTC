use std::{net::{SocketAddr, UdpSocket}, sync::Arc};

use crate::rtp::rtcp::SenderReport;

pub struct RtpSendStream {
    pub codec: RtpCodec,
    pub local_ssrc: u32,
    seq: u16,
    ts: u32,
    pkt_count: u32,
    octet_count: u32,
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
}

impl RtpSendStream {
    pub fn new(cfg: RtpSendConfig, sock: Arc<UdpSocket>, peer: SocketAddr) -> Self {
        use rand::{rngs::OsRng, RngCore};
        Self {
            codec: cfg.codec,
            local_ssrc: cfg.local_ssrc,
            seq: (OsRng.next_u32() as u16),
            ts: OsRng.next_u32(),
            pkt_count: 0,
            octet_count: 0,
            sock, peer,
        }
    }

    pub fn send_frame(&mut self, payload: &[u8]) {
        // build RTP header { PT, seq, ts, SSRC=local_ssrc }, encode, send
        self.seq = self.seq.wrapping_add(1);
        self.ts  = self.ts.wrapping_add(self.clock_rate / 50); // e.g., 20 ms pacing
        self.pkt_count = self.pkt_count.wrapping_add(1);
        self.octet_count = self.octet_count.wrapping_add(payload.len() as u32);
    }

    pub fn maybe_build_sr(&self, ntp_msw: u32, ntp_lsw: u32) -> Option<Vec<u8>> {
        // return None if no packets sent since last SR (optional optimization)
        let sr = SenderReport {
            ssrc: self.local_ssrc,
            ntp_msw, ntp_lsw,
            rtp_ts: self.ts,
            packet_count: self.pkt_count,
            octet_count: self.octet_count,
        }.encode();
        Some(sr)
}
}
