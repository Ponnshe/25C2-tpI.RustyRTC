//! A simple congestion controller that adjusts bitrate based on packet loss and RTT.
pub mod congestion_controller_c;
pub use congestion_controller_c::{CongestionController, NetworkMetrics};
mod constants;
