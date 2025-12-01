// ---- Public message enum --------------------------------------------------

use crate::signaling::protocol::{
    SessionCode, SessionId, TxnId, UserName, peer_status::PeerStatus,
};

#[derive(Debug, PartialEq, Eq)]
pub enum SignalingMsg {
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
    Register {
        username: UserName,
        password: String,
    },
    RegisterOk {
        username: UserName,
    },
    RegisterErr {
        code: u16, // maps from RegisterErrorCode
    },
    ListPeers,
    PeersOnline {
        peers: Vec<(UserName, PeerStatus)>,
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
    // Session membership notifications (server → clients)
    PeerJoined {
        session_id: SessionId,
        username: UserName,
    },
    PeerLeft {
        session_id: SessionId,
        username: UserName,
    },

    // Signaling
    Offer {
        txn_id: TxnId,
        from: UserName,
        to: UserName, // for now, PeerId = username
        sdp: Vec<u8>, // raw UTF-8 text
    },
    Answer {
        txn_id: TxnId,
        from: UserName,
        to: UserName,
        sdp: Vec<u8>,
    },
    Candidate {
        from: UserName,
        to: UserName,
        mid: String,
        mline_index: u16,
        cand: Vec<u8>, // raw UTF-8 text
    },
    Ack {
        from: UserName,
        to: UserName,
        txn_id: TxnId,
    },
    Bye {
        from: UserName,
        to: UserName,
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
