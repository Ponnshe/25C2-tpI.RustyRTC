use crate::app::log_level::LogLevel;
use crate::app::log_sink::LogSink;
use crate::core::events::{EngineEvent, RtpIn};
use crate::rtcp::report_block::ReportBlock;
use crate::rtcp::sender_info::SenderInfo;
use crate::rtp::rtp_packet::RtpPacket;
use crate::sink_log;

use super::{rtp_codec::RtpCodec, rtp_recv_config::RtpRecvConfig, rx_tracker::RxTracker};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::{
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

struct BufferedPacket {
    packet: RtpPacket,
    received_at: Instant,
}

pub struct RtpRecvStream {
    pub codec: RtpCodec,
    pub remote_ssrc: Option<u32>,
    pub rx: RxTracker,
    epoch: Instant,
    last_activity: Instant,

    event_transmitter: Sender<EngineEvent>,
    logger: Arc<dyn LogSink>,

    // Jitter buffer fields
    jitter_buffer: BTreeMap<u16, BufferedPacket>,
    next_seq: Option<u16>,
    max_latency: Duration,
}

impl RtpRecvStream {
    pub fn new(
        cfg: RtpRecvConfig,
        event_transmitter: Sender<EngineEvent>,
        logger: Arc<dyn LogSink>,
    ) -> Self {
        let now = Instant::now();
        Self {
            codec: cfg.codec,
            remote_ssrc: cfg.remote_ssrc,
            rx: RxTracker::default(),
            epoch: now,
            last_activity: now,
            event_transmitter,
            logger,
            jitter_buffer: BTreeMap::new(),
            next_seq: None,
            max_latency: Duration::from_millis(200),
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

        // 3) Update RX tracker immediately for stats
        self.rx
            .on_rtp(packet.seq(), packet.timestamp(), arrival_rtp);

        // 4) Buffer the packet for reordering and playout
        let seq = packet.seq();
        let buffered_packet = BufferedPacket {
            packet,
            received_at: now,
        };

        if self.jitter_buffer.insert(seq, buffered_packet).is_some() {
            sink_log!(
                &self.logger,
                LogLevel::Warn,
                "[RTP] duplicate packet seq={}",
                seq
            );
            return; // Already buffered
        }

        if self.next_seq.is_none() {
            self.next_seq = Some(seq);
        }

        // 5) Process buffer to drain any contiguous packets
        self.process_buffer();
    }

    fn process_buffer(&mut self) {
        let mut next_seq = if let Some(s) = self.next_seq {
            s
        } else {
            return; // Nothing to do if not initialized
        };

        loop {
            // Try to get the next in-sequence packet
            if let Some(buffered) = self.jitter_buffer.remove(&next_seq) {
                let packet = buffered.packet;
                // It's the one we were waiting for. Emit it.
                let evt = EngineEvent::RtpIn(RtpIn {
                    pt: packet.payload_type(),
                    marker: packet.marker(),
                    timestamp_90khz: packet.timestamp(),
                    seq: packet.seq(),
                    ssrc: packet.ssrc(),
                    payload: packet.payload,
                });
                let _ = self.event_transmitter.send(evt);

                // Advance to the next sequence number
                next_seq = next_seq.wrapping_add(1);
                continue; // And try to process the next one
            }

            // If we're here, `next_seq` is missing.
            // Check if we should declare it lost due to timeout.
            // We look at the *next available* packet in the buffer.
            if let Some((&buffered_seq, buffered_pkt)) = self.jitter_buffer.iter().next() {
                // If the oldest packet in our buffer is already too old,
                // then the gap before it is definitely lost.
                if buffered_pkt.received_at.elapsed() > self.max_latency {
                    sink_log!(
                        &self.logger,
                        LogLevel::Warn,
                        "[RTP] Skipping packets from {} to {} (lost)",
                        next_seq,
                        buffered_seq.wrapping_sub(1)
                    );
                    // Jump over the gap.
                    next_seq = buffered_seq;
                    // Loop again to try processing `next_seq` (which is now `buffered_seq`).
                    continue;
                }
            }

            // If we reach here, it means either:
            // a) the buffer is empty
            // b) the missing packet `next_seq` is not present, but the next available packet
            //    is not old enough to trigger a timeout.
            // In both cases, we should wait.
            break;
        }

        self.next_seq = Some(next_seq);
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
            .on_sr_received(info.ntp_most_sw, info.now_least_sw, arrival_ntp);

        // surface for logs/metrics
        //
        sink_log!(
            &self.logger,
            LogLevel::Debug,
            "[RTCP][SR] ssrc={:#010x} rtp_ts={} pkt={} octets={}",
            sender_ssrc,
            info.rtp_ts,
            info.packet_count,
            info.octet_count
        );
    }

    /// Build one RTCP ReportBlock for this remote SSRC.
    pub fn build_report_block(&mut self) -> Option<ReportBlock> {
        self.remote_ssrc
            .map(|ssrc| self.rx.build_report_block(ssrc))
    }
}
