use crate::ice::type_ice::candidate_type::CandidateType;
use std::fmt;
use std::net::SocketAddr;

const DEFAULT_COMPONENT_ID: u8 = 1;
const DEFAULT_TRANSPORT: &str = "UDP";

#[derive(Debug, Clone)]
pub struct Candidate {
    pub foundation: String,
    pub component: u8,
    pub transport: String,
    pub priority: u32,
    pub address: SocketAddr,
    pub cand_type: CandidateType,
    pub related_address: Option<SocketAddr>,
}

//TODO: por ahora se harcodea valores
impl Candidate {
    pub fn new(
        foundation: String,
        component: u8,
        transport: &str,
        priority: u32,
        address: SocketAddr,
        cand_type: CandidateType,
        related_address: Option<SocketAddr>,
    ) -> Self {
        Candidate {
            foundation,
            component: DEFAULT_COMPONENT_ID,
            transport: DEFAULT_TRANSPORT.to_string(),
            priority,
            address,
            cand_type,
            related_address,
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            r#"{{"foundation":"{}","component":{},"transport":"{}","priority":{},"address":"{}","type":"{:?}"}}"#,
            self.foundation,
            self.component,
            self.transport,
            self.priority,
            self.address,
            self.cand_type
        )
    }
}

impl fmt::Display for Candidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {} {} typ {:?}",
            self.foundation,
            self.component,
            self.transport,
            self.priority,
            self.address,
            self.cand_type
        )
    }
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn test_candidate_display_format_ok() {
        let addr: SocketAddr = "192.168.0.1:5000".parse().unwrap();
        let c = Candidate::new("1".into(), 1, "UDP", 100, addr, CandidateType::Host, None);
        let display_str = format!("{}", c);
        assert!(display_str.contains("192.168.0.1:5000"));
        assert!(display_str.contains("Host"));
    }

    #[test]
    fn test_candidate_json_format_ok() {
        let addr: SocketAddr = "10.0.0.5:4000".parse().unwrap();
        let c = Candidate::new("5".into(), 1, "UDP", 120, addr, CandidateType::Host, None);
        let json_str = c.to_json();
        assert!(json_str.contains("\"foundation\":\"5\""));
        assert!(json_str.contains("\"address\":\"10.0.0.5:4000\""));
        assert!(json_str.contains("\"type\":\"Host\""));
    }
}
