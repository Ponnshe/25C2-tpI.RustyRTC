use crate::sctp::events::SctpFileProperties;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct SctpStream {
    pub properties: SctpFileProperties,
    pub last_activity: Instant,
    pub next_seq: u64,
    pub timeout: Duration,
}

impl SctpStream {
    pub fn new(properties: SctpFileProperties) -> Self {
        Self {
            properties,
            last_activity: Instant::now(),
            next_seq: 0,
            timeout: Duration::from_secs(10), // Default timeout
        }
    }

    pub fn update_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    pub fn is_timed_out(&self) -> bool {
        self.last_activity.elapsed() > self.timeout
    }
}
