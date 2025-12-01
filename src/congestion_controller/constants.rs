/// The packet loss threshold for reducing bitrate.
pub const LOSS_THRESHOLD: f32 = 0.1;
/// The RTT threshold in milliseconds for reducing bitrate.
pub const RTT_THRESHOLD_MILLIS: u64 = 200;
/// The interval in seconds for increasing bitrate.
pub const INCREASE_INTERVAL: u64 = 1;
/// The factor by which to increase bitrate.
pub const INCREASE_FACTOR: f64 = 1.1;
/// The factor by which to decrease bitrate.
pub const DECREASE_FACTOR: f64 = 0.85;
