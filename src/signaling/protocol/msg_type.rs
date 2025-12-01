// ---- Message type byte ----------------------------------------------------

use crate::signaling::protocol::errors::ProtoError;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MsgType {
    Hello = 0x01,
    Login = 0x02,
    LoginOk = 0x03,
    LoginErr = 0x04,
    Register = 0x05,
    RegisterOk = 0x06,
    RegisterErr = 0x07,
    ListPeers = 0x08,
    PeersOnline = 0x09,

    CreateSession = 0x10,
    Created = 0x11,
    Join = 0x12,
    JoinOk = 0x13,
    JoinErr = 0x14,
    PeerJoined = 0x15,
    PeerLeft = 0x16,

    Offer = 0x20,
    Answer = 0x21,
    Candidate = 0x22,
    Ack = 0x23,
    Bye = 0x24,

    Ping = 0x30,
    Pong = 0x31,
}

impl MsgType {
    /// # Errors
    ///
    /// Returns `ProtoError::UnknownType` if the byte does not correspond to a valid message type.
    pub const fn from_u8(v: u8) -> Result<Self, ProtoError> {
        match v {
            0x01 => Ok(Self::Hello),
            0x02 => Ok(Self::Login),
            0x03 => Ok(Self::LoginOk),
            0x04 => Ok(Self::LoginErr),
            0x05 => Ok(Self::Register),
            0x06 => Ok(Self::RegisterOk),
            0x07 => Ok(Self::RegisterErr),
            0x08 => Ok(Self::ListPeers),
            0x09 => Ok(Self::PeersOnline),
            0x10 => Ok(Self::CreateSession),
            0x11 => Ok(Self::Created),
            0x12 => Ok(Self::Join),
            0x13 => Ok(Self::JoinOk),
            0x14 => Ok(Self::JoinErr),
            0x15 => Ok(Self::PeerJoined),
            0x16 => Ok(Self::PeerLeft),
            0x20 => Ok(Self::Offer),
            0x21 => Ok(Self::Answer),
            0x22 => Ok(Self::Candidate),
            0x23 => Ok(Self::Ack),
            0x24 => Ok(Self::Bye),
            0x30 => Ok(Self::Ping),
            0x31 => Ok(Self::Pong),
            other => Err(ProtoError::UnknownType(other)),
        }
    }

    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}
