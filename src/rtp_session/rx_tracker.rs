use super::seq_ext::SeqExt;
use super::time;
use crate::rtcp::report_block::ReportBlock;

#[derive(Debug, Default, Clone)]
pub struct RxTracker {
    // sequence/loss
    seqext: SeqExt,
    base_ext_seq: Option<u32>,
    highest_ext_seq: u32,
    received_unique: u32, // count unique packets only
    expected_prev: u32,
    received_prev: u32,

    // jitter (RFC3550 A.8)
    jitter: u32,
    last_transit: Option<u32>,

    // SR timing for LSR/DLSR
    last_sr_compact: Option<u32>,         // LSR
    last_sr_arrival_compact: Option<u32>, // arrival time of that SR, in compact NTP
}

impl RxTracker {
    /// Call on every *unique* RTP packet (after dup filtering) for this SSRC.
    /// `arrival_rtp_units` is arrival time expressed in RTP clock units (use monotonic clock).
    pub fn on_rtp(&mut self, seq: u16, rtp_ts: u32, arrival_rtp_units: u32) {
        let ext = self.seqext.update(seq);
        if self.base_ext_seq.is_none() {
            self.base_ext_seq = Some(ext);
        }
        if ext > self.highest_ext_seq {
            self.highest_ext_seq = ext;
        }
        self.received_unique = self.received_unique.wrapping_add(1);

        // Jitter
        let transit = arrival_rtp_units.wrapping_sub(rtp_ts);
        if let Some(prev) = self.last_transit {
            let d_abs = if transit >= prev {
                transit - prev
            } else {
                prev - transit
            };
            self.jitter = self
                .jitter
                .wrapping_add(((d_abs as u64).saturating_sub(self.jitter as u64) / 16) as u32);
        }
        self.last_transit = Some(transit);
    }

    /// Call when an SR is received (to later fill LSR/DLSR in our RR).
    pub fn on_sr_received(&mut self, ntp_secs: u32, ntp_frac: u32, now_ntp: (u32, u32)) {
        self.last_sr_compact = Some(ntp_compact(ntp_secs, ntp_frac));
        self.last_sr_arrival_compact = Some(ntp_compact(now_ntp.0, now_ntp.1));
    }

    /// Build one RTCP ReportBlock for this remote SSRC (consumes interval deltas).
    pub fn build_report_block(&mut self, ssrc: u32) -> ReportBlock {
        let base = self.base_ext_seq.unwrap_or(0);
        let expected_total = self.highest_ext_seq.saturating_sub(base) + 1;
        let cumulative_lost_i64 = expected_total as i64 - self.received_unique as i64;

        // Interval deltas → fraction_lost
        let exp_delta = expected_total.saturating_sub(self.expected_prev);
        let rec_delta = self.received_unique.saturating_sub(self.received_prev);
        let lost_delta = exp_delta.saturating_sub(rec_delta);
        let fraction_lost = if exp_delta == 0 {
            0
        } else {
            ((lost_delta * 256) / exp_delta) as u8
        };

        self.expected_prev = expected_total;
        self.received_prev = self.received_unique;

        // LSR/DLSR
        let (lsr, dlsr) = match (self.last_sr_compact, self.last_sr_arrival_compact) {
            (Some(lsr), Some(arrival)) => {
                let now = now_ntp_compact();
                let d = now.wrapping_sub(arrival);
                (lsr, d)
            }
            _ => (0, 0),
        };

        ReportBlock {
            ssrc,
            fraction_lost,
            cumulative_lost: (cumulative_lost_i64 as i32) & 0x00FF_FFFF,
            highest_seq_no_received: self.highest_ext_seq,
            interarrival_jitter: self.jitter,
            lsr,
            dlsr,
        }
    }
}

// --- small NTP helpers (compact 32-bit) ---
fn ntp_compact(secs: u32, frac: u32) -> u32 {
    ((secs & 0xFFFF) << 16) | (frac >> 16)
}
fn now_ntp_compact() -> u32 {
    let (s, f) = time::ntp_now();
    ntp_compact(s, f)
}
