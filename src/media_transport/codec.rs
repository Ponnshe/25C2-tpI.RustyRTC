use crate::{media_agent::spec::CodecSpec, rtp_session::rtp_codec::RtpCodec};

#[derive(Debug, Clone)]
pub struct CodecDescriptor {
    pub codec_name: &'static str,
    pub rtp_representation: RtpCodec,
    pub sdp_fmtp: Option<String>,
    pub spec: CodecSpec,
}

impl CodecDescriptor {
    pub fn h264_dynamic(pt: u8) -> Self {
        Self {
            codec_name: "H264",
            rtp_representation: RtpCodec::with_name(pt, 90_000, "H264"),
            sdp_fmtp: Some("profile-level-id=42e01f;packetization-mode=1".into()),
            spec: CodecSpec::H264,
        }
    }
}
