use std::net::SocketAddr;

use crate::{
    app::log_msg::LogMsg,
    congestion_controller::congestion_controller::NetworkMetrics,
    media_agent::{spec::CodecSpec, video_frame::VideoFrame},
};

#[derive(Debug, Clone)]
pub struct RtpIn {
    pub pt: u8,
    pub marker: bool,
    pub timestamp_90khz: u32,
    pub seq: u16,
    pub ssrc: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    Status(String),
    Log(LogMsg),
    IceNominated {
        local: SocketAddr,
        remote: SocketAddr,
    },
    Established,
    Closing {
        graceful: bool,
    },
    Closed,
    Error(String),
    RtpIn(RtpIn),
    NetworkMetrics(NetworkMetrics),
    UpdateBitrate(u32),
}
