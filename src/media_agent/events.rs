use crate::media_agent::spec::CodecSpec;

#[derive(Debug)]
pub enum MediaAgentEvent {
    AnnexBFrameReady {
        codec_spec: CodecSpec,
        bytes: Vec<u8>,
    },
}
