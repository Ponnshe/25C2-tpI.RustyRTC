use super::ProtoError;

// ---- Basic types ----------------------------------------------------------

pub type UserName = String;
pub type SessionId = String;
pub type SessionCode = String;
pub type TxnId = u64; // for offer/answer reliability

// ---- Message type byte ----------------------------------------------------

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MsgType {
    Hello = 0x01,
    Login = 0x02,
    LoginOk = 0x03,
    LoginErr = 0x04,

    CreateSession = 0x10,
    Created = 0x11,
    Join = 0x12,
    JoinOk = 0x13,
    JoinErr = 0x14,

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
            0x10 => Ok(CreateSession),
            0x11 => Ok(Created),
            0x12 => Ok(Join),
            0x13 => Ok(JoinOk),
            0x14 => Ok(JoinErr),
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

// ---- Public message enum --------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum Msg {
    // Handshake / auth
    Hello {
        client_version: String,
    },
    Login {
        username: UserName,
        password: String, // plain text, but sent over TLS
    },
    LoginOk {
        username: UserName,
    },
    LoginErr {
        code: u16, // map to our AuthErrorCode later
    },

    // Session management
    CreateSession {
        capacity: u8,
    },
    Created {
        session_id: SessionId,
        session_code: SessionCode,
    },
    Join {
        session_code: SessionCode,
    },
    JoinOk {
        session_id: SessionId,
    },
    JoinErr {
        code: u16, // map to JoinErrorCode
    },

    // Signaling
    Offer {
        txn_id: TxnId,
        to: UserName, // for now, PeerId = username
        sdp: Vec<u8>, // raw UTF-8 text
    },
    Answer {
        txn_id: TxnId,
        to: UserName,
        sdp: Vec<u8>,
    },
    Candidate {
        to: UserName,
        mid: String,
        mline_index: u16,
        cand: Vec<u8>, // raw UTF-8 text
    },
    Ack {
        txn_id: TxnId,
    },
    Bye {
        reason: Option<String>,
    },

    // Keepalive
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
}
