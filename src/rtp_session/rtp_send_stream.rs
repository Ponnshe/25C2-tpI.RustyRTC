use std::{
    net::{SocketAddr, UdpSocket},
    sync::Arc,
    time::Instant,
};

use super::rtp_send_error::RtpSendError;
use super::{rtp_codec::RtpCodec, rtp_send_config::RtpSendConfig};
use crate::rtp::rtp_packet::RtpPacket;
use crate::{
    rtcp::{sender_info::SenderInfo, sender_report::SenderReport},
    rtp_session::time,
};

pub struct RtpSendStream {
    pub codec: RtpCodec,
    pub local_ssrc: u32,
    seq: u16,
    ts: u32,
    pkt_count: u32,
    octet_count: u32,
    sock: Arc<UdpSocket>,
    peer: SocketAddr,

    last_sr_built: Instant,
    last_pkt_sent: Instant,
}

impl RtpSendStream {
    pub fn new(cfg: RtpSendConfig, sock: Arc<UdpSocket>, peer: SocketAddr) -> Self {
        use rand::{RngCore, rngs::OsRng};
        Self {
            codec: cfg.codec,
            local_ssrc: cfg.local_ssrc,
            seq: (OsRng.next_u32() as u16),
            ts: OsRng.next_u32(),
            pkt_count: 0,
            octet_count: 0,
            sock,
            peer,
            last_sr_built: Instant::now(),
            last_pkt_sent: Instant::now(),
        }
    }
    //// sends payload (in bytes) according to payload type defined.
    pub fn send_frame(&mut self, payload: &[u8]) -> Result<(), RtpSendError> {
        // build RTP header { PT, seq, ts, SSRC=local_ssrc }, encode, send
        let rtp_packet = RtpPacket::simple(
            self.codec.payload_type,
            false,
            self.seq,
            self.ts,
            self.local_ssrc,
            payload.into(),
        );
        let encoded = rtp_packet.encode();
        self.sock.send(&encoded)?;
        self.last_pkt_sent = Instant::now();
        Ok(())
    }

    pub fn maybe_build_sr(&mut self) -> Option<SenderReport> {
        // return None if no packets sent since last SR
        if self.last_pkt_sent <= self.last_sr_built {
            return None;
        }
        let (ntp_msw, ntp_lsw) = time::ntp_now();
        let sender_info =
            SenderInfo::new(ntp_msw, ntp_lsw, self.ts, self.pkt_count, self.octet_count);
        let sender_report = SenderReport::new(self.local_ssrc, sender_info, vec![]);
        self.last_sr_built = Instant::now();
        Some(sender_report)
    }
}
