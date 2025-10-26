use std::str;

pub const SYN_PREFIX: &str = "APP-SYN";
pub const SYNACK_PREFIX: &str = "APP-SYN-ACK";
pub const ACK_PREFIX: &str = "APP-ACK";

// New for graceful close:
pub const FIN_PREFIX: &str = "APP-FIN";
pub const FINACK_PREFIX: &str = "APP-FIN-ACK";
pub const FINACK2_PREFIX: &str = "APP-FIN-ACK2";

// --- Encoders ---

pub fn encode_syn(my_token: u64) -> String {
    format!("{SYN_PREFIX} {:016x}", my_token)
}
pub fn encode_synack(their: u64, mine: u64) -> String {
    format!("{SYNACK_PREFIX} {:016x} {:016x}", their, mine)
}
pub fn encode_ack(their: u64) -> String {
    format!("{ACK_PREFIX} {:016x}", their)
}

pub fn encode_fin(my_token: u64) -> String {
    format!("{FIN_PREFIX} {:016x}", my_token)
}
pub fn encode_finack(their: u64, mine: u64) -> String {
    format!("{FINACK_PREFIX} {:016x} {:016x}", their, mine)
}
pub fn encode_finack2(your: u64) -> String {
    format!("{FINACK2_PREFIX} {:016x}", your)
}

// --- Parser ---

pub enum AppMsg {
    Syn { token: u64 },
    SynAck { your: u64, mine: u64 },
    Ack { your: u64 },
    Fin { token: u64 },
    FinAck { your: u64, mine: u64 },
    FinAck2 { your: u64 },
    Other(Vec<u8>),
}

pub fn parse_app_msg(pkt: &[u8]) -> AppMsg {
    use std::str::from_utf8;

    let Ok(s) = from_utf8(pkt) else {
        return AppMsg::Other(pkt.to_vec());
    };
    let mut it = s.split_whitespace();
    let Some(tag) = it.next() else {
        return AppMsg::Other(pkt.to_vec());
    };

    match tag {
        // Check the longer/stricter tags explicitly
        "APP-FIN-ACK2" => {
            if let Some(y) = it.next() {
                if let Ok(yu) = u64::from_str_radix(y, 16) {
                    return AppMsg::FinAck2 { your: yu };
                }
            }
        }
        "APP-FIN-ACK" => {
            if let (Some(y), Some(m)) = (it.next(), it.next()) {
                if let (Ok(yu), Ok(mu)) = (u64::from_str_radix(y, 16), u64::from_str_radix(m, 16)) {
                    return AppMsg::FinAck { your: yu, mine: mu };
                }
            }
        }
        "APP-FIN" => {
            if let Some(t) = it.next() {
                if let Ok(tu) = u64::from_str_radix(t, 16) {
                    return AppMsg::Fin { token: tu };
                }
            }
        }
        "APP-SYN-ACK" => {
            if let (Some(y), Some(m)) = (it.next(), it.next()) {
                if let (Ok(yu), Ok(mu)) = (u64::from_str_radix(y, 16), u64::from_str_radix(m, 16)) {
                    return AppMsg::SynAck { your: yu, mine: mu };
                }
            }
        }
        "APP-SYN" => {
            if let Some(t) = it.next() {
                if let Ok(tu) = u64::from_str_radix(t, 16) {
                    return AppMsg::Syn { token: tu };
                }
            }
        }
        "APP-ACK" => {
            if let Some(y) = it.next() {
                if let Ok(yu) = u64::from_str_radix(y, 16) {
                    return AppMsg::Ack { your: yu };
                }
            }
        }
        _ => {}
    }

    AppMsg::Other(pkt.to_vec())
}
