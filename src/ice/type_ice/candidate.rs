use crate::ice::type_ice::candidate_type::CandidateType;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::net::{SocketAddr, UdpSocket};

/// Component ID por defecto (1 = RTP, 2 = RTCP)
const DEFAULT_COMPONENT_ID: u8 = 1;

/// Protocolo de transporte por defecto
const DEFAULT_TRANSPORT: &str = "UDP";

/// Tipo de preferencia por tipo de candidato (según convenciones WebRTC/Pion)
const HOST_TYPE_PREF: u32 = 126;
const PEER_REFLEXIVE_TYPE_PREF: u32 = 110;
const SERVER_REFLEXIVE_TYPE_PREF: u32 = 100;
const RELAYED_TYPE_PREF: u32 = 0;

/// Preferencia local máxima (sin distinción de interfaz)
const MAX_LOCAL_PREF: u16 = u16::MAX; // 65535

/// Desplazamientos usados en la fórmula RFC 8445 §5.1.2.1
const TYPE_PREF_SHIFT: u32 = 24;
const LOCAL_PREF_SHIFT: u32 = 8;
const COMPONENT_OFFSET: u32 = 256;

#[derive(Debug)]
pub struct Candidate {
    pub foundation: String,
    pub component: u8,
    pub transport: String,
    pub priority: u32,
    pub address: SocketAddr,
    pub cand_type: CandidateType,
    pub related_address: Option<SocketAddr>,
    pub socket: Option<UdpSocket>,
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
        socket: Option<UdpSocket>,
    ) -> Self {
        let calculate_foundation = if foundation.is_empty() {
            Self::calculate_foundation(&cand_type, transport, &address.ip().to_string())
        } else {
            foundation
        };
        let calculated_priority = if priority == 0 {
            Self::calculate_priority(&cand_type, MAX_LOCAL_PREF, component)
        } else {
            priority
        };
        Candidate {
            foundation: calculate_foundation,
            component,
            transport: transport.to_string(),
            priority: calculated_priority,
            address,
            cand_type,
            related_address,
            socket,
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

    /// algorithm for calculating foundation, according to RFC 8445 §5.1.1.3
    fn calculate_foundation(cand_type: &CandidateType, transport: &str, base_ip: &str) -> String {
        let mut hasher = DefaultHasher::new();
        (format!("{:?}-{}-{}", cand_type, base_ip, transport)).hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// algorithm for calculating foundation, according to RFC 8445 §5.1.2.1
    fn calculate_priority(cand_type: &CandidateType, local_pref: u16, component_id: u8) -> u32 {
        let type_pref: u32 = match cand_type {
            CandidateType::Host => HOST_TYPE_PREF,
            CandidateType::ServerReflexive => SERVER_REFLEXIVE_TYPE_PREF,
            CandidateType::PeerReflexive => PEER_REFLEXIVE_TYPE_PREF,
            CandidateType::Relayed => RELAYED_TYPE_PREF,
        };

        (1 << TYPE_PREF_SHIFT) * type_pref
            + (1 << LOCAL_PREF_SHIFT) * local_pref as u32
            + (COMPONENT_OFFSET - component_id as u32)
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
        let c = Candidate::new(
            "1".into(),
            1,
            "UDP",
            100,
            addr,
            CandidateType::Host,
            None,
            None,
        );
        let display_str = format!("{}", c);
        assert!(display_str.contains("192.168.0.1:5000"));
        assert!(display_str.contains("Host"));
    }

    #[test]
    fn test_candidate_json_format_ok() {
        let addr: SocketAddr = "10.0.0.5:4000".parse().unwrap();
        let c = Candidate::new(
            "5".into(),
            1,
            "UDP",
            120,
            addr,
            CandidateType::Host,
            None,
            None,
        );
        let json_str = c.to_json();
        assert!(json_str.contains("\"foundation\":\"5\""));
        assert!(json_str.contains("\"address\":\"10.0.0.5:4000\""));
        assert!(json_str.contains("\"type\":\"Host\""));
    }

    #[test]
    fn test_calculate_foundation_with_different_ip_ok() {
        let f1 = Candidate::calculate_foundation(&CandidateType::Host, "UDP", "192.168.0.10");
        let f2 = Candidate::calculate_foundation(&CandidateType::Host, "UDP", "192.168.0.11");
        assert_ne!(f1, f2, "Foundation has change, if change base IP");
    }

    #[test]
    fn test_calculate_priority_ok() {
        let host_p = Candidate::calculate_priority(&CandidateType::Host, 65535, 1);
        let relay_p = Candidate::calculate_priority(&CandidateType::Relayed, 65535, 1);
        assert!(
            host_p > relay_p,
            "Host-type candidates should have, more higher priority than relayed candidates."
        );
    }
}
