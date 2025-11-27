use crate::media_agent::video_frame::VideoFrame;

pub enum EncoderInstruction {
    Encode(VideoFrame, bool), // (frame, force_keyframe)
    SetConfig { fps: u32, bitrate: u32, keyint: u32 },
}
