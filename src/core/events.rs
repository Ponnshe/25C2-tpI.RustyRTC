use std::net::SocketAddr;

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
    Log(String),
    IceNominated {
        local: SocketAddr,
        remote: SocketAddr,
    },
    Established,
    Payload(String),
    Closing {
        graceful: bool,
    },
    Closed,
    Error(String),
    RtpIn(RtpIn),
    //Legacy
    RtpMedia {
        bytes: Vec<u8>,
        pt: u8,
    },
}
