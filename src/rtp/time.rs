use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Convert now() to NTP timestamp (seconds since 1900) split into (msw, lsw)
pub fn ntp_now() -> (u32, u32) {
    // NTP epoch offset from Unix (1900→1970)
    const NTP_UNIX_EPOCH_DIFF: u64 = 2_208_988_800; // seconds
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0));
    let secs = now.as_secs() + NTP_UNIX_EPOCH_DIFF;
    let frac = ((now.subsec_nanos() as u64) << 32) / 1_000_000_000u64;
    (secs as u32, frac as u32)
}
