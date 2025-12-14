use std::net::SocketAddr;

use crate::{
    congestion_controller::NetworkMetrics, log::log_msg::LogMsg,
    media_transport::media_transport_event::RtpIn, sctp::events::SctpFileProperties,
};

/// Represents events that can be emitted by the `Engine` to the UI or other components.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// A status message for display in the UI.
    Status(String),
    /// A log message.
    Log(LogMsg),
    /// An ICE candidate pair has been nominated.
    IceNominated {
        local: SocketAddr,
        remote: SocketAddr,
    },
    /// The WebRTC connection has been established.
    Established,
    /// The WebRTC connection is closing.
    Closing {
        graceful: bool,
    },
    /// The WebRTC connection has been closed.
    Closed,
    /// An error occurred in the engine.
    Error(String),
    /// An incoming RTP packet.
    RtpIn(RtpIn),
    /// Network metrics updated by the congestion controller.
    NetworkMetrics(NetworkMetrics),
    /// Request to update the encoder bitrate.
    UpdateBitrate(u32),
    
    // File Transfer Events
    SendFileOffer(SctpFileProperties),
    SendFileAccept(u32),
    SendFileReject(u32),
    SendFileCancel(u32),
    SendFileChunk(u32, Vec<u8>),
    SendFileEnd(u32),
    ReceivedFileOffer(SctpFileProperties),
    ReceivedFileAccept(u32),
    ReceivedFileReject(u32),
    ReceivedFileCancel(u32),
    ReceivedFileChunk(u32, u32, Vec<u8>),
    ReceivedFileEnd(u32),
}
