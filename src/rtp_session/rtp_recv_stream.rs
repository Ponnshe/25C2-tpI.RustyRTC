use crate::core::events::EngineEvent;
use crate::rtcp::report_block::ReportBlock;
use crate::rtcp::sender_info::SenderInfo;
use crate::rtp::rtp_packet::RtpPacket;

use super::{rtp_codec::RtpCodec, rtp_recv_config::RtpRecvConfig, rx_tracker::RxTracker};
use std::{sync::mpsc::Sender, time::Instant};

pub struct RtpRecvStream {
    pub codec: RtpCodec,
    pub remote_ssrc: Option<u32>,
    pub rx: RxTracker,
    epoch: Instant,
    last_activity: Instant,

    event_transmitter: Sender<EngineEvent>,
}

impl RtpRecvStream {
    pub fn new(cfg: RtpRecvConfig, event_transmitter: Sender<EngineEvent>) -> Self {
        let now = Instant::now();
        Self {
            codec: cfg.codec,
            remote_ssrc: cfg.remote_ssrc,
            rx: RxTracker::default(),
            epoch: now,
            last_activity: now,
            event_transmitter,
        }
    }

    /// Convert a monotonic Instant to RTP timestamp units using `codec.clock_rate`.
    #[inline]
    fn instant_to_rtp_units(&self, now: Instant) -> u32 {
        let dur = now.duration_since(self.epoch);
        let rate = self.codec.clock_rate as u128; // Hz
        let ns = dur.as_nanos(); // u128
        let units = (ns.saturating_mul(rate)) / 1_000_000_000u128;
        units as u32
    }

    pub fn receive_rtp_packet(&mut self, packet: RtpPacket) {
        let now = Instant::now();
        self.last_activity = now;

        // 1) Learn/validate SSRC
        let pkt_ssrc = packet.ssrc();
        if let Some(expected) = self.remote_ssrc {
            if expected != pkt_ssrc {
                // Not our stream
                return;
            }
        } else {
            self.remote_ssrc = Some(pkt_ssrc);
        }

        // 2) Arrival time in RTP units (codec clock)
        let arrival_rtp = self.instant_to_rtp_units(now);

        // 3) Update RX tracker
        self.rx
            .on_rtp(packet.seq(), packet.timestamp(), arrival_rtp);

        // 4) Emit media event (prefer owned bytes over borrows; see note below)
        let _ = self.event_transmitter.send(EngineEvent::RtpMedia {
            pt: packet.payload_type(),
            bytes: packet.payload(),
        });
    }

    /// Called by the *session* when an SR for this remote SSRC arrives.
    /// `arrival_ntp` is the local receive time of the SR as (ntp_msw, ntp_lsw).
    pub fn on_sender_report(
        &mut self,
        sender_ssrc: u32,
        info: &SenderInfo,
        arrival_ntp: (u32, u32),
    ) {
        // Learn/validate SSRC
        if let Some(exp) = self.remote_ssrc {
            if exp != sender_ssrc {
                return; // SR from someone else
            }
        } else {
            self.remote_ssrc = Some(sender_ssrc);
        }

        self.last_activity = Instant::now();

        // Anchor SR timing so we can later fill LSR/DLSR in our RR
        self.rx
            .on_sr_received(info.ntp_msw, info.ntp_lsw, arrival_ntp);

        // (Optional) surface for logs/metrics
        let _ = self.event_transmitter.send(EngineEvent::Log(format!(
            "[RTCP][SR] ssrc={:#010x} rtp_ts={} pkt={} octets={}",
            sender_ssrc, info.rtp_ts, info.packet_count, info.octet_count
        )));
    }

    /// Build one RTCP ReportBlock for this remote SSRC.
    pub fn build_report_block(&mut self) -> Option<ReportBlock> {
        self.remote_ssrc
            .map(|ssrc| self.rx.build_report_block(ssrc))
    }
}
