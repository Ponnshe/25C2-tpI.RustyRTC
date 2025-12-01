use super::constants::*;
use crate::{
    core::events::EngineEvent, log::log_sink::LogSink, rtcp::report_block::ReportBlock,
    rtp_session::tx_tracker::TxTracker, sink_debug, sink_error, sink_warn,
};
use std::{
    sync::{Arc, mpsc::Sender},
    time::{Duration, Instant},
};

/// Represents network metrics used by the congestion controller.
#[derive(Debug, Clone)]
pub struct NetworkMetrics {
    /// The round trip time.
    pub round_trip_time: Duration,
    /// The fraction of packets lost, as a value between 0 and 255.
    pub fraction_lost: u8,
    /// The number of packets lost.
    pub packets_lost: i32,
    /// The highest sequence number received.
    pub highest_sequence_number: u32,
}

impl NetworkMetrics {
    /// Creates a `NetworkMetrics` from a `TxTracker` and a `ReportBlock`.
    pub fn from_tracker(tracker: &TxTracker, rb: &ReportBlock) -> Option<Self> {
        tracker.rtt_ms.map(|rtt_ms| Self {
            round_trip_time: Duration::from_millis(rtt_ms as u64),
            fraction_lost: tracker.remote_fraction_lost,
            packets_lost: tracker.remote_cum_lost,
            highest_sequence_number: rb.highest_seq_no_received,
        })
    }
}

/// A congestion controller that adjusts the bitrate based on network metrics.
pub struct CongestionController {
    current_bitrate_bps: u32,
    min_bitrate_bps: u32,
    max_bitrate_bps: u32,

    last_update: Instant,

    loss_threshold: f32,
    rtt_threshold: Duration,

    increase_interval: Duration,
    increase_factor: f64,
    decrease_factor: f64,

    logger: Arc<dyn LogSink>,
    tx_evt: Sender<EngineEvent>,
}

impl CongestionController {
    /// Creates a new `CongestionController`.
    pub fn new(
        initial_bitrate: u32,
        min_bitrate: u32,
        max_bitrate: u32,
        logger: Arc<dyn LogSink>,
        tx_evt: Sender<EngineEvent>,
    ) -> Self {
        if let Err(e) = tx_evt.send(EngineEvent::UpdateBitrate(initial_bitrate)) {
            sink_error!(
                logger.as_ref(),
                "[Congestion] Failed to send initial UpdateBitrate event: {}",
                e
            );
        }
        Self {
            current_bitrate_bps: initial_bitrate,
            min_bitrate_bps: min_bitrate,
            max_bitrate_bps: max_bitrate,
            last_update: Instant::now(),
            loss_threshold: LOSS_THRESHOLD,
            rtt_threshold: Duration::from_millis(RTT_THRESHOLD_MILLIS),
            increase_interval: Duration::from_secs(INCREASE_INTERVAL),
            increase_factor: INCREASE_FACTOR,
            decrease_factor: DECREASE_FACTOR,
            logger,
            tx_evt,
        }
    }

    /// Updates the congestion controller with new network metrics.
    pub fn on_network_metrics(&mut self, metrics: NetworkMetrics) {
        let now = Instant::now();
        let mut new_bitrate = self.current_bitrate_bps;

        let fraction_lost_float = metrics.fraction_lost as f32 / 255.0;
        sink_debug!(
            self.logger.as_ref(),
            "[Congestion] Packet Loss: {:.2}%",
            fraction_lost_float * 100.0,
        );

        sink_debug!(
            self.logger.as_ref(),
            "[Congestion] RTT: {}ms",
            metrics.round_trip_time.as_millis(),
        );

        // If loss exceeds a threshold, drastically reduce bitrate.
        if fraction_lost_float > self.loss_threshold {
            new_bitrate = (new_bitrate as f64 * self.decrease_factor) as u32;
            sink_warn!(
                self.logger.as_ref(),
                "[Congestion] High packet loss ({:.2}%), decreasing bitrate to {} bps",
                fraction_lost_float * 100.0,
                new_bitrate,
            );

        // If RTT is too high, also reduce bitrate.
        } else if metrics.round_trip_time > self.rtt_threshold {
            new_bitrate = (new_bitrate as f64 * self.decrease_factor) as u32;
            sink_warn!(
                self.logger.as_ref(),
                "[Congestion] High RTT ({}ms), decreasing bitrate to {} bps",
                metrics.round_trip_time.as_millis(),
                new_bitrate
            );
        // If the network is stable and enough time has passed, try to increase bitrate.
        } else if now.duration_since(self.last_update) > self.increase_interval {
            new_bitrate = (new_bitrate as f64 * self.increase_factor) as u32;
            sink_debug!(
                self.logger.as_ref(),
                "[Congestion] Network stable, increasing bitrate to {} bps",
                new_bitrate
            );
        }

        // Ensure the new bitrate is within limits
        new_bitrate = new_bitrate.clamp(self.min_bitrate_bps, self.max_bitrate_bps);

        if new_bitrate != self.current_bitrate_bps {
            self.current_bitrate_bps = new_bitrate;
            self.last_update = now;

            // Send event to Engine to update the encoder
            if let Err(e) = self.tx_evt.send(EngineEvent::UpdateBitrate(new_bitrate)) {
                sink_error!(
                    self.logger.as_ref(),
                    "[Congestion] Failed to send UpdateBitrate event: {}",
                    e
                );
            }
        }
    }
}
