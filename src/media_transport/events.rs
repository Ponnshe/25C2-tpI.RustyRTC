use crate::media_agent::spec::CodecSpec;

use super::packetizer_worker::PacketizedFrame;

#[derive(Debug)]
pub enum DepacketizerEvent {
    AnnexBFrameReady {
        codec_spec: CodecSpec,
        bytes: Vec<u8>,
    },
    EncodedAudioFrameReady {
        codec_spec: CodecSpec,
        payload: Vec<u8>,
    },
}

#[derive(Debug)]
pub enum PacketizerEvent {
    FramePacketized(PacketizedFrame),
}
