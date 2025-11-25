use std::net::SocketAddr;

use crate::{app::log_msg::LogMsg, congestion_controller::congestion_controller::NetworkMetrics, media_transport::media_transport_event::RtpIn};

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
