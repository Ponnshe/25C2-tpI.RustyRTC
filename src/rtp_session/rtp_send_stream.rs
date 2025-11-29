use std::{
    net::{SocketAddr, UdpSocket},
    sync::{Arc, Mutex},
    time::Instant,
};

use super::rtp_send_error::RtpSendError;
use super::{rtp_codec::RtpCodec, rtp_send_config::RtpSendConfig, tx_tracker::TxTracker};

use crate::rtcp::{
    report_block::ReportBlock, sender_info::SenderInfo, sender_report::SenderReport,
};
use crate::rtp::rtp_packet::RtpPacket;
use crate::rtp_session::time;
use crate::{
    congestion_controller::congestion_controller::NetworkMetrics,
    dtls_srtp::srtp_context::SrtpContext,
};

pub struct RtpSendStream {
    pub codec: RtpCodec,
    pub local_ssrc: u32,
    seq: u16,
    timestamp: u32,
    packet_count: u32,
    octet_count: u32,

    sock: Arc<UdpSocket>,
    peer: SocketAddr,

    last_sr_built: Instant,
    last_pkt_sent: Instant,

    pub tx: TxTracker,
    srtp_context: Option<Arc<Mutex<SrtpContext>>>,
}

impl RtpSendStream {
    pub fn new(
        cfg: RtpSendConfig,
        sock: Arc<UdpSocket>,
        peer: SocketAddr,
        srtp_context: Option<Arc<Mutex<SrtpContext>>>,
    ) -> Self {
        use rand::{RngCore, rngs::OsRng};
        Self {
            codec: cfg.codec,
            local_ssrc: cfg.local_ssrc,
            seq: (OsRng.next_u32() as u16),
            timestamp: OsRng.next_u32(),
            packet_count: 0,
            octet_count: 0,
            sock,
            peer,
            last_sr_built: Instant::now(),
            last_pkt_sent: Instant::now(),
            tx: TxTracker::default(),
            srtp_context,
        }
    }

    /// Advance RTP timestamp by `samples` in codec clock units.
    /// Call this according to your pacing (e.g., for audio: samples per packet; for video: frame-based tick).
    pub const fn advance_timestamp(&mut self, samples: u32) {
        self.timestamp = self.timestamp.wrapping_add(samples);
    }

    /// Optionally set an absolute RTP timestamp (e.g., after a keyframe or clock reset).
    pub const fn set_timestamp(&mut self, ts: u32) {
        self.timestamp = ts;
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
            self.timestamp,
            self.packet_count,
            self.octet_count,
        );

        let sr = SenderReport::new(self.local_ssrc, sender_info, vec![]);
        self.last_sr_built = Instant::now();
        Some(sr)
    }

    /// Deliver a ReportBlock (from a remote SR/RR) to this sender stream so it can update outbound metrics/RTT.
    /// `arrival_ntp_compact` is the compact NTP time when *we* received the SR/RR that carried this block.
    pub fn on_report_block(
        &mut self,
        rb: &ReportBlock,
        arrival_ntp_compact: u32,
    ) -> Option<NetworkMetrics> {
        self.tx.on_report_block(rb, arrival_ntp_compact);
        NetworkMetrics::from_tracker(&self.tx, rb)
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
            self.packet_count,
            self.octet_count,
            self.tx.remote_cum_lost,
            self.tx.remote_fraction_lost,
            self.tx.remote_jitter,
            rtt,
        )
    }
    /// Send one RTP payload with explicit timestamp & marker.
    /// Increments seqno and updates SR counters. Does NOT change pacing itself.
    pub fn send_rtp_payload(
        &mut self,
        payload: &[u8],
        timestamp: u32,
        marker: bool,
    ) -> Result<(), RtpSendError> {
        let pkt = RtpPacket::simple(
            self.codec.payload_type,
            marker,
            self.seq,
            timestamp,
            self.local_ssrc,
            payload.to_vec(),
        );
        let mut encoded = pkt.encode()?;

        // SRTP Protect
        if let Some(ctx) = &self.srtp_context {
            // ssrc se necesita para el ROC
            ctx.lock()
                .unwrap()
                .protect(self.local_ssrc, &mut encoded)
                .map_err(|e| {
                    RtpSendError::SRTP(format!("[SRTP] could not protect packet: {e}").to_owned())
                })?;
        }
        self.sock.send_to(&encoded, self.peer)?;
        self.last_pkt_sent = Instant::now();

        // Accounting
        self.seq = self.seq.wrapping_add(1);
        self.packet_count = self.packet_count.wrapping_add(1);
        self.octet_count = self.octet_count.wrapping_add(payload.len() as u32);

        // Track last timestamp used so SRs reflect the current media clock
        self.timestamp = timestamp;
        Ok(())
    }
}
