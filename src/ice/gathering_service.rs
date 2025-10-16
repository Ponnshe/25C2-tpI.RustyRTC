use std::net::{IpAddr, SocketAddr, UdpSocket};

use crate::ice::type_ice::candidate::Candidate;

const ERROR_MSG: &str = "ERROR";
const WHITESPACE: &str = " ";
const QUOTE: &str = "\"";
const EOM: &str = "\n";
const SOCKET_CREATE_ERROR: &str = "Error creating test socket";
const BIND_SOCKET_ERROR: &str = "Error binding main socket";
const ADDRESS_MAIN_SOCKET_ERROR: &str = "Error getting address of main socket";
const GET_SOCKET_LOOPBACK_ERROR: &str = "Error getting loopback socket address";
const BINDING_SOCKET_LOOPBACK_ERROR: &str = "Loopback socket binding error";
const INVALID_IP_ADDRESS_ERROR: &str = "Not found a valid IPv4 address.";
const GET_LOCAL_ADDRESS_ERROR: &str = "Error getting local address";

const DISCOVERY_TARGET_IP: &str = "8.8.8.8";
const DEFAULT_GATEWAY: &str = "0.0.0.0:0";
const DISCOVERY_TARGET_PORT: u16 = 80;

const DEFAULT_COMPONENT_ID: u8 = 1; // RTP/Data, good enough for mock
const TRANSPORT_UDP: &str = "udp"; // lowercase is safer across stacks

/// Gathers a single IPv4 host candidate for the primary egress interface.
/// (No deps, robust enough for LAN tests.)
pub fn gather_host_candidates() -> Vec<Candidate> {
    let mut out = Vec::new();

    // Discover primary local IPv4 via a TEMP socket
    let local_ip = match discover_local_ipv4() {
        Ok(ip) => ip,
        Err(e) => {
            eprintln!("{}", e);
            return out;
        }
    };

    // Fresh, unconnected socket bound to that interface
    match create_main_socket(local_ip) {
        Ok((addr, sock)) => {
            out.push(Candidate::host(addr, TRANSPORT_UDP, DEFAULT_COMPONENT_ID, Some(sock)));
        }
        Err(e) => {
            eprintln!("{}", e);
            return out;
        }
    }

    //(Opcional) add loopback
    if let Some(loopback_candidate) = gather_loopback_candidate() {
        out.push(loopback_candidate);
    }

    out
}

/// Format error messages
fn error_message(msg: &str) -> String {
    format!("{}{}{}{}{}", ERROR_MSG, WHITESPACE, QUOTE, msg, QUOTE)
}

/// Discover the primary IPv4 local IP using a temporary socket.
fn discover_local_ipv4() -> Result<IpAddr, String> {
    let probe = UdpSocket::bind(DEFAULT_GATEWAY)
        .map_err(|_| error_message(SOCKET_CREATE_ERROR))?;

    let _ = probe.connect((DISCOVERY_TARGET_IP, DISCOVERY_TARGET_PORT));

    let local_ip = probe
        .local_addr()
        .map_err(|_| error_message(GET_LOCAL_ADDRESS_ERROR))?
        .ip();

    drop(probe);

    if local_ip.is_loopback() || !local_ip.is_ipv4() {
        Err(error_message(INVALID_IP_ADDRESS_ERROR))
    } else {
        Ok(local_ip)
    }
}

/// Creates the main socket on the discovered local interface.
fn create_main_socket(local_ip: IpAddr) -> Result<(SocketAddr, UdpSocket), String> {
    let sock = UdpSocket::bind(SocketAddr::new(local_ip, 0))
        .map_err(|_| error_message(BIND_SOCKET_ERROR))?;

    let addr = sock
        .local_addr()
        .map_err(|_| error_message(ADDRESS_MAIN_SOCKET_ERROR))?;

    Ok((addr, sock))
}

//loopback for same-host demos only
#[cfg(feature = "loopback-candidate")]
fn gather_loopback_candidate() -> Option<Candidate> {
    match UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)) {
        Ok(loop_sock) => match loop_sock.local_addr() {
            Ok(loop_addr) => Some(Candidate::host(
                loop_addr,
                TRANSPORT_UDP,
                DEFAULT_COMPONENT_ID,
                Some(loop_sock),
            )),
            Err(_) => {
                eprintln!("{}", error_message(GET_SOCKET_LOOPBACK_ERROR));
                None
            }
        },
        Err(_) => {
            eprintln!("{}", error_message(BINDING_SOCKET_LOOPBACK_ERROR));
            None
        }
    }
}

#[cfg(not(feature = "loopback-candidate"))]
fn gather_loopback_candidate() -> Option<Candidate> {
    None
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
