use std::{
    net::{SocketAddr, UdpSocket},
    sync::Arc,
    time::Instant,
};

use super::rtp_send_error::RtpSendError;
use super::{rtp_codec::RtpCodec, rtp_send_config::RtpSendConfig, tx_tracker::TxTracker};

use crate::rtcp::{
    report_block::ReportBlock, sender_info::SenderInfo, sender_report::SenderReport,
};
use crate::rtp::rtp_packet::RtpPacket;
use crate::rtp_session::time;

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

    pub tx: TxTracker,
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
            tx: TxTracker::default(),
        }
    }

    /// Send one RTP packet carrying `payload`.
    /// Note: This does NOT advance the RTP timestamp; call `advance_timestamp(samples)` as appropriate for your codec pacing.
    pub fn send_frame(&mut self, payload: &[u8]) -> Result<(), RtpSendError> {
        let pt = &self.codec.payload_type;
        println!("Recibido payload, PT: {pt}");
        let rtp_packet = RtpPacket::simple(
            self.codec.payload_type,
            false,
            self.seq,
            self.ts,
            self.local_ssrc,
            payload.into(),
        );

        let encoded = rtp_packet.encode()?;

        self.sock.send_to(&encoded, self.peer)?;
        self.last_pkt_sent = Instant::now();

        // ——— accounting ———
        self.seq = self.seq.wrapping_add(1);
        self.pkt_count = self.pkt_count.wrapping_add(1);
        self.octet_count = self.octet_count.wrapping_add(payload.len() as u32);

        Ok(())
    }

    /// Advance RTP timestamp by `samples` in codec clock units.
    /// Call this according to your pacing (e.g., for audio: samples per packet; for video: frame-based tick).
    pub fn advance_timestamp(&mut self, samples: u32) {
        self.ts = self.ts.wrapping_add(samples);
    }

    /// Optionally set an absolute RTP timestamp (e.g., after a keyframe or clock reset).
    pub fn set_timestamp(&mut self, ts: u32) {
        self.ts = ts;
    }

    /// Build a Sender Report if we have sent packets since the last SR.
    /// Also records the compact-NTP identifier so we can compute RTT when RRs arrive.
    pub fn maybe_build_sr(&mut self) -> Option<SenderReport> {
        if self.last_pkt_sent <= self.last_sr_built {
            return None;
        }

        let (ntp_most_sw, now_least_sw) = time::ntp_now();

        // Tell the tracker which SR we’re about to publish (for RTT via LSR/DLSR)
        self.tx.mark_sr_sent(ntp_most_sw, now_least_sw);

        let sender_info = SenderInfo::new(
            ntp_most_sw,
            now_least_sw,
            self.ts,
            self.pkt_count,
            self.octet_count,
        );

        let sr = SenderReport::new(self.local_ssrc, sender_info, vec![]);
        self.last_sr_built = Instant::now();
        Some(sr)
    }

    /// Deliver a ReportBlock (from a remote SR/RR) to this sender stream so it can update outbound metrics/RTT.
    /// `arrival_ntp_compact` is the compact NTP time when *we* received the SR/RR that carried this block.
    pub fn on_report_block(&mut self, rb: &ReportBlock, arrival_ntp_compact: u32) {
        self.tx.on_report_block(rb, arrival_ntp_compact);
    }

    /// Optional: expose some outbound health summary for logging/telemetry.
    pub fn outbound_summary(&self) -> String {
        let rtt = self
            .tx
            .rtt_ms
            .map(|v| format!("{v} ms"))
            .unwrap_or_else(|| "-".into());
        format!(
            "SSRC={:#010x} sent={} pkts, {} bytes; remote_lost={} (frac={}), remote_jitter={}, RTT={}",
            self.local_ssrc,
            self.pkt_count,
            self.octet_count,
            self.tx.remote_cum_lost,
            self.tx.remote_fraction_lost,
            self.tx.remote_jitter,
            rtt,
        )
    }
}
