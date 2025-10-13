use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use crate::ice::type_ice::candidate::Candidate;
use crate::ice::type_ice::candidate_type::CandidateType;

/// Default gateway for all interfaces
const BIND_IP: &str = "0.0.0.0";
/// 0 = random port
const BIND_PORT: u16 = 0;

/// Destination IP and port used only to discover local interface
const DISCOVERY_TARGET_IP: &str = "8.8.8.8";
const DISCOVERY_TARGET_PORT: u16 = 80;

const EMPTY_VALUE: &str = "";
const ZERO_VALUE: u32 = 0;
const DEFAULT_COMPONENT_ID: u8 = 1; // RTP
/// Transmission protocol UDP
const DEFAULT_TRANSPORT: &str = "UDP";

/// Data for the loopback candidate (local testing)
const LOOPBACK_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
const LOOPBACK_PORT: u16 = 5000;

/// Get local IP addresses of this machine (IPv4), filtering loopback
/// and creates host candidates for each address found.
///
/// # Return
/// Return a list of local candidates.
pub fn gather_host_candidates() -> Vec<Candidate> {
    let mut candidates = Vec::new();

    // Step 1: Discover outgoing local IP using temporary UDP socket
    let bind_addr = format!("{}:{}", BIND_IP, BIND_PORT);
    if let Ok(socket) = UdpSocket::bind(&bind_addr) {
        let target_addr = format!("{}:{}", DISCOVERY_TARGET_IP, DISCOVERY_TARGET_PORT);
        if socket.connect(&target_addr).is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                let ip = local_addr.ip();
                if !ip.is_loopback() && ip.is_ipv4() {
                    let candidate = Candidate::new(
                        EMPTY_VALUE.to_string(),
                        DEFAULT_COMPONENT_ID,
                        DEFAULT_TRANSPORT,
                        ZERO_VALUE,
                        SocketAddr::new(ip, local_addr.port()),
                        CandidateType::Host,
                        None,
                        Some(socket),
                    );
                    candidates.push(candidate);
                }
            }
        }
    }

    // Step 2: Add loopback (only for local testing)
    if let Ok(loop_socket) =
        UdpSocket::bind(SocketAddr::new(IpAddr::V4(LOOPBACK_IP), LOOPBACK_PORT))
    {
        let loopback_candidate = Candidate::new(
            EMPTY_VALUE.to_string(),
            DEFAULT_COMPONENT_ID,
            DEFAULT_TRANSPORT,
            ZERO_VALUE,
            loop_socket.local_addr().unwrap(),
            CandidateType::Host,
            None,
            Some(loop_socket),
        );
        candidates.push(loopback_candidate);
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gather_host_return_candidates() {
        let candidates = gather_host_candidates();
        assert!(
            !candidates.is_empty(),
            "No se encontraron candidatos locales"
        );
    }
}
