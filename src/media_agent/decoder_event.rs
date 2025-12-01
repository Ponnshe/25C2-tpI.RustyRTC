use crate::media_agent::spec::CodecSpec;

#[derive(Debug)]
pub enum DecoderEvent {
    AnnexBFrameReady {
        codec_spec: CodecSpec,
        bytes: Vec<u8>,
    },
}
