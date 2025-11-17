use crate::media_agent::spec::CodecSpec;

#[derive(Debug)]
pub enum MediaAgentEvent {
    ChunkReady {
        codec_spec: CodecSpec,
        chunk: Vec<u8>,
    },
}
