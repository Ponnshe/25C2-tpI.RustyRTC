use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub enum EngineEvent {
    Status(String),
    Log(String),
    IceNominated {
        local: SocketAddr,
        remote: SocketAddr,
    },
    Established,
    Payload(String), // or bytes later
    Closing {
        graceful: bool,
    },
    Closed,
    Error(String),
    RtpMedia {
        bytes: Vec<u8>,
        pt: u8,
    },
}
