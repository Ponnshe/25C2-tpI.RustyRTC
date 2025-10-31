use crate::rtp_session::rtp_codec::RtpCodec;

#[derive(Debug, Clone)]
pub struct CodecDescriptor {
    pub name: &'static str,
    pub rtp: RtpCodec,
    pub fmtp: Option<String>,
}

impl CodecDescriptor {
    pub fn h264_dynamic(pt: u8) -> Self {
        Self {
            name: "H264",
            rtp: RtpCodec::with_name(pt, 90_000, "H264"),
            fmtp: Some("profile-level-id=42e01f;packetization-mode=1".into()),
        }
    }
}
