use std::time::Instant;
use crate::rtcp::report_block::ReportBlock;

/// Tracks outbound (sender-side) health and RTT based on RTCP feedback.
#[derive(Debug, Clone)]
pub struct TxTracker {
    /// Compact NTP identifier of the last SR we sent for this SSRC.
    /// Used to match remote RRs’ LSR and compute RTT.
    pub last_sr_ntp_compact: u32,

    /// When we last received any RR/SR containing a ReportBlock about this SSRC.
    pub last_rr_instant: Option<Instant>,

    // Remote-reported stats about our outbound stream:
    pub remote_fraction_lost: u8,     // 0..255 (1/256 steps)
    pub remote_cum_lost: i32,         // signed 24-bit in spec; stored as i32
    pub remote_highest_ext_seq: u32,  // extended highest sequence received
    pub remote_jitter: u32,           // remote interarrival jitter estimate

    /// Most recent round-trip time (ms), computed via RFC3550 A.3.
    pub rtt_ms: Option<u32>,
}

impl Default for TxTracker {
    fn default() -> Self {
        Self {
            last_sr_ntp_compact: 0,
            last_rr_instant: None,
            remote_fraction_lost: 0,
            remote_cum_lost: 0,
            remote_highest_ext_seq: 0,
            remote_jitter: 0,
            rtt_ms: None,
        }
    }
}

impl TxTracker {
    /// Call this right before (or when) you publish an SR.
    pub fn mark_sr_sent(&mut self, ntp_most_sw: u32, ntp_least_sw: u32) {
        self.last_sr_ntp_compact = ntp_to_compact(ntp_most_sw, ntp_least_sw);
    }

    /// Consume a ReportBlock that references *our* SSRC.
    /// `arrival_ntp_compact` is when *we* received the SR/RR containing this block.
    pub fn on_report_block(&mut self, rb: &ReportBlock, arrival_ntp_compact: u32) {
        // 1) Store the remote’s view of our outbound stream
        self.remote_fraction_lost   = rb.fraction_lost;
        self.remote_cum_lost        = rb.cumulative_lost;
        self.remote_highest_ext_seq = rb.highest_seq_no_received;
        self.remote_jitter          = rb.interarrival_jitter;
        self.last_rr_instant        = Some(Instant::now());

        // 2) If possible, compute RTT via: RTT = A - LSR - DLSR (mod 2^32), in units of 1/65536 s.
        if rb.lsr != 0 && rb.dlsr != 0 && self.last_sr_ntp_compact != 0 && rb.lsr == self.last_sr_ntp_compact {
            let rtt_units = arrival_ntp_compact
                .wrapping_sub(rb.lsr)
                .wrapping_sub(rb.dlsr);

            // Convert from 1/65536 s to ms: (x * 1000) / 65536
            let rtt_ms = ((rtt_units as u64) * 1000) >> 16;
            self.rtt_ms = Some(rtt_ms as u32);
        }
    }
}

/// Convert a 64-bit NTP timestamp to the 32-bit "compact" form used in RFC3550 A.3.
/// compact = (MSW << 16) | (LSW >> 16)
#[inline]
pub fn ntp_to_compact(msw: u32, lsw: u32) -> u32 {
    (msw << 16) | (lsw >> 16)
}
