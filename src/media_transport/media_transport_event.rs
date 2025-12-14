use crate::media_agent::spec::CodecSpec;

#[derive(Debug, Clone)]
pub struct RtpIn {
    pub pt: u8,
    pub marker: bool,
    pub timestamp_90khz: u32,
    pub seq: u16,
    pub ssrc: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub enum MediaTransportEvent {
    SendEncodedFrame {
        annexb_frame: Vec<u8>,
        timestamp_ms: u128,
        codec_spec: CodecSpec,
    },
    SendEncodedAudioFrame {
        payload: Vec<u8>,
        timestamp_ms: u128,
        codec_spec: CodecSpec,
    },
    UpdateBitrate(u32),
    Established,
    Closed,
    RtpIn(RtpIn),
    Closing,
}
