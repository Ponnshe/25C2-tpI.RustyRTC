use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use crate::client::ice::type_ice::candidate::Candidate;
use crate::client::ice::type_ice::candidate_type::CandidateType;

/// Dirección para bind inicial (todas las interfaces)
const BIND_IP: &str = "0.0.0.0";
const BIND_PORT: u16 = 0; // 0 = puerto aleatorio

/// IP de destino usada solo para descubrir interfaz local
const DISCOVERY_TARGET_IP: &str = "8.8.8.8";
const DISCOVERY_TARGET_PORT: u16 = 80;

/// Foundation y prioridad por defecto para candidatos host
const DEFAULT_FOUNDATION: &str = "1";
const DEFAULT_COMPONENT_ID: u8 = 1; // RTP
const DEFAULT_TRANSPORT: &str = "UDP";
const DEFAULT_PRIORITY: u32 = 100;

/// Datos para el candidato loopback (testing local)
const LOOPBACK_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
const LOOPBACK_PORT: u16 = 5000;
const LOOPBACK_FOUNDATION: &str = "2";
const LOOPBACK_PRIORITY: u32 = 90;

/// Obtiene las direcciones IP locales de la máquina (IPv4), filtrando loopback,
/// y crea candidatos host para cada dirección encontrada.
pub fn gather_host_candidates() -> Vec<Candidate> {
    let mut candidates = Vec::new();

    // Paso 1: descubrir IP local saliente mediante socket UDP temporal
    let bind_addr = format!("{}:{}", BIND_IP, BIND_PORT);
    if let Ok(socket) = UdpSocket::bind(&bind_addr) {
        let target_addr = format!("{}:{}", DISCOVERY_TARGET_IP, DISCOVERY_TARGET_PORT);
        if socket.connect(&target_addr).is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                let ip = local_addr.ip();
                if !ip.is_loopback() && ip.is_ipv4() {
                    let candidate = Candidate::new(
                        DEFAULT_FOUNDATION.to_string(),
                        DEFAULT_COMPONENT_ID,
                        DEFAULT_TRANSPORT,
                        DEFAULT_PRIORITY,
                        SocketAddr::new(ip, local_addr.port()),
                        CandidateType::Host,
                        None,
                    );
                    candidates.push(candidate);
                }
            }
        }
    }

    // Paso 2: agregar loopback (solo si querés usarlo para pruebas locales)
    let loopback_socket = SocketAddr::new(IpAddr::V4(LOOPBACK_IP), LOOPBACK_PORT);
    let loopback_candidate = Candidate::new(
        LOOPBACK_FOUNDATION.to_string(),
        DEFAULT_COMPONENT_ID,
        DEFAULT_TRANSPORT,
        LOOPBACK_PRIORITY,
        loopback_socket,
        CandidateType::Host,
        None,
    );
    candidates.push(loopback_candidate);

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
