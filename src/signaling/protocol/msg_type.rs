// ---- Message type byte ----------------------------------------------------

use crate::signaling::protocol::ProtoError;

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
    pub fn from_u8(v: u8) -> Result<MsgType, ProtoError> {
        use MsgType::*;
        match v {
            0x01 => Ok(Hello),
            0x02 => Ok(Login),
            0x03 => Ok(LoginOk),
            0x04 => Ok(LoginErr),
            0x05 => Ok(Register),
            0x06 => Ok(RegisterOk),
            0x07 => Ok(RegisterErr),
            0x10 => Ok(CreateSession),
            0x11 => Ok(Created),
            0x12 => Ok(Join),
            0x13 => Ok(JoinOk),
            0x14 => Ok(JoinErr),
            0x15 => Ok(PeerJoined),
            0x16 => Ok(PeerLeft),
            0x20 => Ok(Offer),
            0x21 => Ok(Answer),
            0x22 => Ok(Candidate),
            0x23 => Ok(Ack),
            0x24 => Ok(Bye),
            0x30 => Ok(Ping),
            0x31 => Ok(Pong),
            other => Err(ProtoError::UnknownType(other)),
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}
