/// A single RTP payload chunk plus whether it carries the end-of-frame marker.
#[derive(Debug, Clone)]
pub struct RtpPayloadChunk {
    pub bytes: Vec<u8>,
    /// true only for the *last* chunk of the access unit (frame)
    pub marker: bool,
}
