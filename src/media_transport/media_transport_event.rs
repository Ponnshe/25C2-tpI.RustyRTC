use crate::media_agent::spec::CodecSpec;

#[derive(Debug)]
pub enum MediaTransportEvent {
    SendEncodedFrame {
        annexb_frame: Vec<u8>,
        timestamp_ms: u128,
        codec_spec: CodecSpec,
    },
}
