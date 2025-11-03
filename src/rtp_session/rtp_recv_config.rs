use super::rtp_codec::RtpCodec;
#[derive(Debug, Clone)]
pub struct RtpRecvConfig {
    pub codec: RtpCodec,
    /// If SDP didn’t expose an SSRC (common in WebRTC), allow None and learn on first RTP.
    pub remote_ssrc: Option<u32>,
}

impl RtpRecvConfig {
    pub fn new(codec: RtpCodec, remote_ssrc: Option<u32>) -> Self {
        Self { codec, remote_ssrc }
    }
}
