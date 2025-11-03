#[derive(Debug, Clone)]
pub struct RtpCodec {
    pub payload_type: u8,
    pub clock_rate: u32, // e.g., 90_000 video, 48_000 Opus
    pub name: String,
}

impl RtpCodec {
    pub fn new(pt: u8, clock: u32) -> Self {
        Self {
            payload_type: pt,
            clock_rate: clock,
            name: String::new(),
        }
    }

    pub fn with_name<S: Into<String>>(pt: u8, clock: u32, name: S) -> Self {
        Self {
            payload_type: pt,
            clock_rate: clock,
            name: name.into(),
        }
    }
}
