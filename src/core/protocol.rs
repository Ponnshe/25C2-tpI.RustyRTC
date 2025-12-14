//! Defines the application-level signaling protocol messages used for session setup
//! and tear-down.
//!
//! These messages are exchanged over the nominated ICE transport after DTLS handshake
//! to establish and manage the application session state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMsg {
    /// SYN message for handshake initiation, carrying a local token.
    Syn { token: u64 },
    /// SYN-ACK message for handshake response, echoing peer's token and carrying local token.
    SynAck { your: u64, mine: u64 },
    /// ACK message for handshake completion, acknowledging peer's token.
    Ack { your: u64 },
    /// FIN message for graceful session termination initiation, carrying a local token.
    Fin { token: u64 },
    /// FIN-ACK message for graceful session termination response, echoing peer's token and carrying local token.
    FinAck { your: u64, mine: u64 },
    /// FIN-ACK2 message for graceful session termination completion, acknowledging peer's token.
    FinAck2 { your: u64 },
}

/// Encodes a SYN message.
#[must_use]
pub fn encode_syn(token: u64) -> String {
    format!("SYN {token:016x}")
}
/// Encodes a SYN-ACK message.
#[must_use]
pub fn encode_synack(your: u64, mine: u64) -> String {
    format!("SYN-ACK {your:016x} {mine:016x}")
}
/// Encodes an ACK message.
#[must_use]
pub fn encode_ack(your: u64) -> String {
    format!("ACK {your:016x}")
}
/// Encodes a FIN message.
#[must_use]
pub fn encode_fin(token: u64) -> String {
    format!("FIN {token:016x}")
}
/// Encodes a FIN-ACK message.
#[must_use]
pub fn encode_finack(your: u64, mine: u64) -> String {
    format!("FIN-ACK {your:016x} {mine:016x}")
}
/// Encodes a FIN-ACK2 message.
#[must_use]
pub fn encode_finack2(your: u64) -> String {
    format!("FIN-ACK2 {your:016x}")
}

fn parse_hex(t: &str) -> Option<u64> {
    u64::from_str_radix(t, 16).ok()
}

/// Parses a byte slice into an `AppMsg`.
#[must_use]
pub fn parse_app_msg(bytes: &[u8]) -> Option<AppMsg> {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim();
    let mut it = s.split_whitespace();
    let Some(kind) = it.next() else {
        return None;
    };

    match kind {
        "SYN" => {
            let token = it.next().and_then(parse_hex);
            token.map(|token| AppMsg::Syn { token })
        }
        "SYN-ACK" => {
            let your = it.next().and_then(parse_hex);
            let mine = it.next().and_then(parse_hex);
            match (your, mine) {
                (Some(your), Some(mine)) => Some(AppMsg::SynAck { your, mine }),
                _ => None,
            }
        }
        "ACK" => {
            let your = it.next().and_then(parse_hex);
            your.map(|your| AppMsg::Ack { your })
        }
        "FIN" => {
            let token = it.next().and_then(parse_hex);
            token.map(|token| AppMsg::Fin { token })
        }
        "FIN-ACK" => {
            let your = it.next().and_then(parse_hex);
            let mine = it.next().and_then(parse_hex);
            match (your, mine) {
                (Some(your), Some(mine)) => Some(AppMsg::FinAck { your, mine }),
                _ => None,
            }
        }
        "FIN-ACK2" => {
            let your = it.next().and_then(parse_hex);
            your.map(|your| AppMsg::FinAck2 { your })
        }
        _ => None,
    }
}
