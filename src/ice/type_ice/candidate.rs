use crate::ice::type_ice::candidate_type::CandidateType;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;

/// Preference type by candidate type (according to WebRTC conventions)
const HOST_TYPE_PREF: u32 = 126;
const PEER_REFLEXIVE_TYPE_PREF: u32 = 110;
const SERVER_REFLEXIVE_TYPE_PREF: u32 = 100;
const RELAYED_TYPE_PREF: u32 = 0;

/// Maximum local preference (interface-insensitive)
const MAX_LOCAL_PREF: u16 = u16::MAX; // 65535

/// Offsets used in the priority calculation -> RFC 8445 §5.1.2.1
const TYPE_PREF_SHIFT: u32 = 24;
const LOCAL_PREF_SHIFT: u32 = 8;
const COMPONENT_OFFSET: u32 = 256;

/// Represents a network address that a client can offer to connect.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Unique identifier that groups similar candidates
    pub foundation: String,
    /// 1 = RTP or 2 = RTCP, normally
    pub component: u8,
    ///UDP in this case.
    pub transport: String,
    /// number for sort candidates.
    pub priority: u32,
    /// IP + port.
    pub address: SocketAddr,
    /// a type of candidate.
    pub cand_type: CandidateType,
    /// this is for reflexive.
    pub related_address: Option<SocketAddr>,
    /// socket to establish the connection
    pub socket: Option<Arc<UdpSocket>>,
}

/// Create a valid candidate.
///
/// # Arguments
/// Same properties of a candidate.
///
/// # Return
/// A new candidate.
impl Candidate {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        foundation: String,
        component: u8,
        transport: &str,
        priority: u32,
        address: SocketAddr,
        cand_type: CandidateType,
        related_address: Option<SocketAddr>,
        socket: Option<Arc<UdpSocket>>,
    ) -> Self {
        let t = transport.to_ascii_lowercase();

        let foundation = if foundation.is_empty() {
            Self::calculate_foundation(&cand_type, &t, &address.ip().to_string())
        } else {
            foundation
        };

        let priority = if priority == 0 {
            Self::calculate_priority(&cand_type, MAX_LOCAL_PREF, component)
        } else {
            priority
        };

        Self {
            foundation,
            component,
            transport: t,
            priority,
            address,
            cand_type,
            related_address,
            socket,
        }
    }

    #[must_use]
    /// Convenience for host candidates
    pub fn host(
        address: SocketAddr,
        transport: &str,
        component: u8,
        socket: Option<Arc<UdpSocket>>,
    ) -> Self {
        Self::new(
            String::new(),
            component,
            transport,
            0, // let ctor compute
            address,
            CandidateType::Host,
            None,
            socket,
        )
    }

    #[must_use]
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

    // RFC 8445 §5.1.1.3 — foundation (any stable identifier OK)
    fn calculate_foundation(
        cand_type: &CandidateType,
        transport_lc: &str,
        base_ip: &str,
    ) -> String {
        let mut hasher = DefaultHasher::new();
        format!("{cand_type:?}-{transport_lc}-{base_ip}").hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    // RFC 8445 §5.1.2.1 — 32-bit candidate priority
    const fn calculate_priority(cand_type: &CandidateType, local_pref: u16, component_id: u8) -> u32 {
        let type_pref = match cand_type {
            CandidateType::Host => HOST_TYPE_PREF,
            CandidateType::ServerReflexive => SERVER_REFLEXIVE_TYPE_PREF,
            CandidateType::PeerReflexive => PEER_REFLEXIVE_TYPE_PREF,
            CandidateType::Relayed => RELAYED_TYPE_PREF,
        };

        (type_pref << TYPE_PREF_SHIFT)
            | ((local_pref as u32) << LOCAL_PREF_SHIFT)
            | (COMPONENT_OFFSET - component_id as u32)
    }

    #[must_use]
    /// Creates a shallow copy of a Candidate without cloning the underlying socket.
    pub fn clone_light(&self) -> Candidate {
        Candidate {
            foundation: self.foundation.clone(),
            component: self.component,
            transport: self.transport.clone(),
            priority: self.priority,
            address: self.address,
            cand_type: self.cand_type.clone(),
            related_address: self.related_address,
            socket: None,
        }
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
        let display_str = format!("{c}");
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
