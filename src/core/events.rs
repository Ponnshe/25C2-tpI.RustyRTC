use std::net::SocketAddr;

use crate::{
    congestion_controller::NetworkMetrics, log::log_msg::LogMsg,
    media_transport::media_transport_event::RtpIn,
};

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
