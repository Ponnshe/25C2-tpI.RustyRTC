#[derive(Debug, Clone, Copy)]
pub struct RtpCodec {
    pub payload_type: u8,
    pub clock_rate: u32, // e.g., 90_000 video, 48_000 Opus
}

impl RtpCodec {
    pub const fn new(pt: u8, clock: u32) -> Self { Self { payload_type: pt, clock_rate: clock } }
}
