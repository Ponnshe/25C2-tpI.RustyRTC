use super::rtp_codec::RtpCodec;

/// Lightweight handle for callers to keep track of an outbound RTP stream.
/// The handle carries the negotiated codec and the randomly generated local SSRC.
#[derive(Debug, Clone)]
pub struct OutboundTrackHandle {
    pub local_ssrc: u32,
    pub codec: RtpCodec,
}

impl OutboundTrackHandle {
    pub fn payload_type(&self) -> u8 {
        self.codec.payload_type
    }
}
