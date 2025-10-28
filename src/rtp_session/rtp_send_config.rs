use super::rtp_codec::RtpCodec;
use rand::{RngCore, rngs::OsRng};

#[derive(Debug, Clone)]
pub struct RtpSendConfig {
    pub codec: RtpCodec,
    pub local_ssrc: u32,
}

impl RtpSendConfig {
    pub fn new(codec: RtpCodec) -> Self {
        Self {
            codec,
            local_ssrc: OsRng.next_u32(),
        }
    }
    pub fn with_ssrc(codec: RtpCodec, ssrc: u32) -> Self {
        Self {
            codec,
            local_ssrc: ssrc,
        }
    }
}
