#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMsg {
    Syn { token: u64 },
    SynAck { your: u64, mine: u64 },
    Ack { your: u64 },
    Fin { token: u64 },
    FinAck { your: u64, mine: u64 },
    FinAck2 { your: u64 },
    Other(Vec<u8>),
}


#[must_use]
pub fn encode_syn(token: u64) -> String {
    format!("SYN {token:016x}")
}
#[must_use]
pub fn encode_synack(your: u64, mine: u64) -> String {
    format!("SYN-ACK {your:016x} {mine:016x}")
}
#[must_use]
pub fn encode_ack(your: u64) -> String {
    format!("ACK {your:016x}")
}
#[must_use]
pub fn encode_fin(token: u64) -> String {
    format!("FIN {token:016x}")
}
#[must_use]
pub fn encode_finack(your: u64, mine: u64) -> String {
    format!("FIN-ACK {your:016x} {mine:016x}")
}
#[must_use]
pub fn encode_finack2(your: u64) -> String {
    format!("FIN-ACK2 {your:016x}")
}

fn parse_hex(t: &str) -> Option<u64> {
    u64::from_str_radix(t, 16).ok()
}

#[must_use]
pub fn parse_app_msg(bytes: &[u8]) -> AppMsg {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim();
    let mut it = s.split_whitespace();
    let Some(kind) = it.next() else {
        return AppMsg::Other(bytes.to_vec());
    };

    let msg = match kind {
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
    };

    msg.unwrap_or_else(|| AppMsg::Other(bytes.to_vec()))
}
