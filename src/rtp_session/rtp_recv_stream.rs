use crate::core::events::EngineEvent;
use crate::rtcp::report_block::ReportBlock;
use crate::rtcp::rtcp::RtcpPacket;
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
            epoch: now, // <— initialize epoch once
            last_activity: now,
            event_transmitter,
        }
    }

    /// Convert a monotonic Instant to RTP timestamp units using `codec.clock_rate`.
    #[inline]
    fn instant_to_rtp_units(&self, now: Instant) -> u32 {
        // Use u128 to avoid overflow and keep precision, then wrap to u32 (RTP timestamp space)
        let dur = now.duration_since(self.epoch);
        let rate = self.codec.clock_rate as u128; // Hz
        let ns = dur.as_nanos(); // total nanoseconds since epoch (u128)

        // units = ns * rate / 1e9  (no floating point)
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
                // SSRC collision or a different stream's packet — choose policy:
                // - ignore; or
                // - switch streams; or
                // - raise an event. Here we ignore to keep per-stream semantics.
                return;
            }
        } else {
            self.remote_ssrc = Some(pkt_ssrc);
        }

        // 2) Compute arrival time in *RTP units* (codec clock)
        let arrival_rtp = self.instant_to_rtp_units(now);

        // 3) Update receive tracker (seq/jitter/loss/etc.)
        //    on_rtp expects: (sequence_number, rtp_timestamp, arrival_in_rtp_units)
        self.rx
            .on_rtp(packet.seq(), packet.timestamp(), arrival_rtp);

        // 4) Push an event into engine (adapt to actual enum/fields)
        //    If you have a jitter buffer, you might enqueue the whole packet/payload here.
        let _ = self.event_transmitter.send(EngineEvent::Payload{packet.pt, packet.payload};
    }

    pub fn receive_rtcp_packet(&mut self, pkt: RtcpPacket) {
        // Typical handling:
        // - SR: call self.on_sr(sr.ntp_msw, sr.ntp_lsw, now_ntp())
        // - RR: update any per-ssrc state you keep for reporting back
        // - NACK/PLI: bubble up events for the sender side (if applicable)
        // TODO: implement based on your RTCP enum variants
        todo!()
    }

    pub fn build_report_block(&mut self) -> Option<ReportBlock> {
        self.remote_ssrc
            .map(|ssrc| self.rx.build_report_block(ssrc))
    }

    fn on_sr(&mut self, ntp_msw: u32, ntp_lsw: u32, now_ntp: (u32, u32)) {
        self.rx.on_sr_received(ntp_msw, ntp_lsw, now_ntp);
    }
}
