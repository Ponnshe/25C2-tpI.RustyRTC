use crate::{media_agent::spec::CodecSpec, rtp_session::rtp_codec::RtpCodec};

/// Describes the complete configuration of a media codec for network negotiation.
///
/// This structure bridges the gap between the internal application logic (`CodecSpec`)
/// and the network transport layer (`RtpCodec`, SDP parameters). It provides all the
/// necessary information to construct the Session Description Protocol (SDP) lines.
#[derive(Debug, Clone)]
pub struct CodecDescriptor {
    /// The human-readable name of the codec (e.g., "H264", "OPUS").
    pub codec_name: &'static str,

    /// The RTP-specific configuration (Payload Type, Clock Rate, etc.).
    pub rtp_representation: RtpCodec,

    /// The SDP `fmtp` (Format Parameter) line.
    ///
    /// This string contains specific configuration parameters negotiated via SDP.
    /// For H.264, this includes the Profile-Level-ID and Packetization Mode.
    /// Example: `"profile-level-id=42e01f;packetization-mode=1"`
    pub sdp_fmtp: Option<String>,

    /// The internal enum identifier used by the `MediaAgent` logic.
    pub spec: CodecSpec,
}

impl CodecDescriptor {
    /// Creates a standard configuration for H.264 video using a dynamic Payload Type.
    ///
    /// This configuration uses the **Constrained Baseline Profile, Level 3.1** (`42e01f`),
    /// which is the most widely supported profile for WebRTC compatibility (works on
    /// Android, iOS, and browsers).
    ///
    /// # Arguments
    ///
    /// * `pt` - The dynamic RTP Payload Type (usually between 96 and 127).
    ///
    /// # Configuration Details
    ///
    /// * **Clock Rate**: 90,000 Hz (Standard for video).
    /// * **Packetization Mode**: 1 (Non-interleaved mode, allows fragmentation unit NALs).
    /// * **Profile Level ID**: `42e01f`
    ///   - `42`: Baseline Profile
    ///   - `e0`: Constraint set flags (Constrained Baseline)
    ///   - `1f`: Level 3.1 (supports up to 720p/30fps or equivalent macroblocks)
    pub fn h264_dynamic(pt: u8) -> Self {
        Self {
            codec_name: "H264",
            rtp_representation: RtpCodec::with_name(pt, 90_000, "H264"),
            // Packetization mode 1 is required for FU-A fragmentation support.
            sdp_fmtp: Some("profile-level-id=42e01f;packetization-mode=1".into()),
            spec: CodecSpec::H264,
        }
    }

    pub fn pcmu_dynamic(pt: u8) -> Self {
        Self {
            codec_name: "PCMU",
            rtp_representation: RtpCodec::with_name(pt, 8000, "PCMU"),
            sdp_fmtp: None,
            spec: CodecSpec::G711U,
        }
    }
}
