use crate::media_agent::{spec::CodecSpec, video_frame::VideoFrame};

#[derive(Debug)]
pub enum MediaAgentEvent {
    EncodedVideoFrame {
        annexb_frame: Vec<u8>,
        timestamp_ms: u128,
        codec_spec: CodecSpec,
    },
    DecodedVideoFrame(Box<VideoFrame>),
}
