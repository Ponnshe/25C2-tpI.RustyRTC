use super::constants::*;
use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    core::events::EngineEvent,
    rtcp::report_block::ReportBlock,
    rtp_session::tx_tracker::TxTracker,
    sink_log,
};
use std::{
    sync::{Arc, mpsc::Sender},
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct NetworkMetrics {
    pub round_trip_time: Duration,
    pub fraction_lost: u8, // Valor entre 0 y 255
    pub packets_lost: i32,
    pub highest_sequence_number: u32,
}

impl NetworkMetrics {
    pub fn from_tracker(tracker: &TxTracker, rb: &ReportBlock) -> Option<Self> {
        tracker.rtt_ms.map(|rtt_ms| Self {
            round_trip_time: Duration::from_millis(rtt_ms as u64),
            fraction_lost: tracker.remote_fraction_lost,
            packets_lost: tracker.remote_cum_lost,
            highest_sequence_number: rb.highest_seq_no_received,
        })
    }
}

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
    pub fn new(
        initial_bitrate: u32,
        min_bitrate: u32,
        max_bitrate: u32,
        logger: Arc<dyn LogSink>,
        tx_evt: Sender<EngineEvent>,
    ) -> Self {
        Self {
            current_bitrate_bps: initial_bitrate,
            min_bitrate_bps: min_bitrate,
            max_bitrate_bps: max_bitrate,
            last_update: Instant::now(),
            loss_threshold: LOSS_TRESHOLD,
            rtt_threshold: Duration::from_millis(RTT_TRESHOLD_MILIS),
            increase_interval: Duration::from_secs(INCREASE_INTERVAL),
            increase_factor: INCREASE_FACTOR,
            decrease_factor: DECREASE_FACTOR,
            logger,
            tx_evt,
        }
    }

    pub fn on_network_metrics(&mut self, metrics: NetworkMetrics) {
        let now = Instant::now();
        let mut new_bitrate = self.current_bitrate_bps;

        let fraction_lost_float = metrics.fraction_lost as f32 / 255.0;
        sink_log!(
            self.logger.as_ref(),
            LogLevel::Info,
            "[Congestion] Packet Loss: {:.2}%",
            fraction_lost_float * 100.0,
        );

        sink_log!(
            self.logger.as_ref(),
            LogLevel::Info,
            "[Congestion] RTT: {}ms",
            metrics.round_trip_time.as_millis(),
        );

        // Si la pérdida supera un umbral, reducimos el bitrate drásticamente.
        if fraction_lost_float > self.loss_threshold {
            new_bitrate = (new_bitrate as f64 * self.decrease_factor) as u32;
            sink_log!(
                self.logger.as_ref(),
                LogLevel::Warn,
                "[Congestion] High packet loss ({:.2}%), decreasing bitrate to {} bps",
                fraction_lost_float * 100.0,
                new_bitrate,
            )

        // Si el RTT es muy alto, también reducimos el bitrate.
        } else if metrics.round_trip_time > self.rtt_threshold {
            new_bitrate = (new_bitrate as f64 * self.decrease_factor) as u32;
            sink_log!(
                self.logger.as_ref(),
                LogLevel::Warn,
                "[Congestion] High RTT ({}ms), decreasing bitrate to {} bps",
                metrics.round_trip_time.as_millis(),
                new_bitrate
            )
        // Si la red está estable y ha pasado suficiente tiempo, intentamos aumentar el bitrate.
        } else if now.duration_since(self.last_update) > self.increase_interval {
            new_bitrate = (new_bitrate as f64 * self.increase_factor) as u32;
            sink_log!(
                self.logger.as_ref(),
                LogLevel::Info,
                "[Congestion] Network stable, increasing bitrate to {} bps",
                new_bitrate
            );
        }

        // Asegurarse de que el nuevo bitrate esté dentro de los límites
        new_bitrate = new_bitrate.clamp(self.min_bitrate_bps, self.max_bitrate_bps);

        if new_bitrate != self.current_bitrate_bps {
            self.current_bitrate_bps = new_bitrate;
            self.last_update = now;

            // Enviar evento al Engine para que actualice el encoder
            if let Err(e) = self.tx_evt.send(EngineEvent::UpdateBitrate(new_bitrate)) {
                sink_log!(
                    self.logger.as_ref(),
                    LogLevel::Error,
                    "[Congestion] Failed to send UpdateBitrate event: {}",
                    e
                );
            }
        }
    }
}
