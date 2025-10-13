use std::net::{IpAddr, SocketAddr, UdpSocket};

use crate::ice::type_ice::candidate::Candidate;

const DISCOVERY_TARGET_IP: &str = "8.8.8.8";
const DISCOVERY_TARGET_PORT: u16 = 80;

const DEFAULT_COMPONENT_ID: u8 = 1; // RTP/Data, good enough for mock
const TRANSPORT_UDP: &str = "udp"; // lowercase is safer across stacks

/// Gathers a single IPv4 host candidate for the primary egress interface.
/// (No deps, robust enough for LAN tests.)
pub fn gather_host_candidates() -> Vec<Candidate> {
    let mut out = Vec::new();

    // Discover primary local IPv4 via a TEMP socket
    let probe = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return out,
    };
    let _ = probe.connect((DISCOVERY_TARGET_IP, DISCOVERY_TARGET_PORT));
    let local_ip = match probe.local_addr().map(|a| a.ip()) {
        Ok(IpAddr::V4(ipv4)) if !ipv4.is_loopback() => IpAddr::V4(ipv4),
        _ => return out,
    };
    drop(probe);

    // Fresh, unconnected socket bound to that interface
    let sock = match UdpSocket::bind(SocketAddr::new(local_ip, 0)) {
        Ok(s) => s,
        Err(_) => return out,
    };
    let addr = sock.local_addr().unwrap();

    out.push(Candidate::host(
        addr,
        TRANSPORT_UDP,
        DEFAULT_COMPONENT_ID,
        Some(sock),
    ));

    //loopback for same-host demos only
    #[cfg(feature = "loopback-candidate")]
    if let Ok(loop_sock) = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)) {
        let loop_addr = loop_sock.local_addr().unwrap();
        out.push(Candidate::host(
            loop_addr,
            TRANSPORT_UDP,
            DEFAULT_COMPONENT_ID,
            Some(loop_sock),
        ));
    }

    out
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
