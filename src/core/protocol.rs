#[derive(Debug, Clone)]
pub enum AppMsg {
    Syn { token: u64 },
    SynAck { your: u64, mine: u64 },
    Ack { your: u64 },
    Fin { token: u64 },
    FinAck { your: u64, mine: u64 },
    FinAck2 { your: u64 },
    Other(Vec<u8>),
}

pub fn encode_syn(token: u64) -> String {
    format!("SYN {:016x}", token)
}
pub fn encode_synack(your: u64, mine: u64) -> String {
    format!("SYN-ACK {:016x} {:016x}", your, mine)
}
pub fn encode_ack(your: u64) -> String {
    format!("ACK {:016x}", your)
}
pub fn encode_fin(token: u64) -> String {
    format!("FIN {:016x}", token)
}
pub fn encode_finack(your: u64, mine: u64) -> String {
    format!("FIN-ACK {:016x} {:016x}", your, mine)
}
pub fn encode_finack2(your: u64) -> String {
    format!("FIN-ACK2 {:016x}", your)
}

pub fn parse_app_msg(bytes: &[u8]) -> AppMsg {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim();
    let mut it = s.split_whitespace();
    let Some(kind) = it.next() else {
        return AppMsg::Other(bytes.to_vec());
    };
    let parse_hex = |t: &str| u64::from_str_radix(t, 16).ok();
    match kind {
        "SYN" => it
            .next()
            .and_then(parse_hex)
            .map(|token| AppMsg::Syn { token })
            .unwrap_or_else(|| AppMsg::Other(bytes.to_vec())),
        "SYN-ACK" => match (it.next().and_then(parse_hex), it.next().and_then(parse_hex)) {
            (Some(your), Some(mine)) => AppMsg::SynAck { your, mine },
            _ => AppMsg::Other(bytes.to_vec()),
        },
        "ACK" => it
            .next()
            .and_then(parse_hex)
            .map(|your| AppMsg::Ack { your })
            .unwrap_or_else(|| AppMsg::Other(bytes.to_vec())),
        "FIN" => it
            .next()
            .and_then(parse_hex)
            .map(|token| AppMsg::Fin { token })
            .unwrap_or_else(|| AppMsg::Other(bytes.to_vec())),
        "FIN-ACK" => match (it.next().and_then(parse_hex), it.next().and_then(parse_hex)) {
            (Some(your), Some(mine)) => AppMsg::FinAck { your, mine },
            _ => AppMsg::Other(bytes.to_vec()),
        },
        "FIN-ACK2" => it
            .next()
            .and_then(parse_hex)
            .map(|your| AppMsg::FinAck2 { your })
            .unwrap_or_else(|| AppMsg::Other(bytes.to_vec())),
        _ => AppMsg::Other(bytes.to_vec()),
    }
}
