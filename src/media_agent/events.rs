use crate::{audio::types::AudioFrame, media_agent::{spec::CodecSpec, video_frame::VideoFrame}};

#[derive(Debug)]
pub enum MediaAgentEvent {
    AnnexBFrameReady {
        codec_spec: CodecSpec,
        bytes: Vec<u8>,
    },
    EncodedVideoFrame {
        annexb_frame: Vec<u8>,
        timestamp_ms: u128,
        codec_spec: CodecSpec,
    },
    DecodedVideoFrame(Box<VideoFrame>),
    UpdateBitrate(u32),
    /// Audio PCM recibido desde RTP (downlink)
    RemoteAudioFrame(AudioFrame),
}
