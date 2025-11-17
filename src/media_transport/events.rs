use crate::media_agent::spec::CodecSpec;

use super::packetizer_worker::PacketizedFrame;

#[derive(Debug)]
pub enum DepacketizerEvent {
    ChunkReady {
        codec_spec: CodecSpec,
        chunk: Vec<u8>,
    },
}

#[derive(Debug)]
pub enum PacketizerEvent {
    FramePacketized(PacketizedFrame),
}
