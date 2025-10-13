use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::path::Path;

use crate::ice::type_ice::candidate::Candidate;
use crate::ice::type_ice::candidate_type::CandidateType;
use crate::ice::type_ice::ice_agent::IceAgent;
use crate::sdp::sdpc::Sdp;

/// Guarda los candidatos locales del agente en un archivo JSON (uno por línea)
pub fn save_sdp_to_file(sdp: &Sdp, file_path: &str) -> io::Result<()> {
    let path = Path::new(file_path);
    let mut file = File::create(&path)?;

    writeln!(file, "{}", sdp.encode())?;

    Ok(())
}

/// Imprime los candidatos locales en stdout para debug
pub fn print_candidates_stdout(agent: &IceAgent) {
    println!("Candidatos locales serializados (JSON):");
    for c in &agent.local_candidates {
        println!("{}", c.to_json());
    }
}

/// Carga candidatos remotos desde un archivo JSON y los agrega al agente ICE
pub fn load_remote_candidates_from_file(agent: &mut IceAgent, file_path: &str) -> io::Result<()> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!(
                    "No se pudo leer una línea del archivo '{}': {}",
                    file_path, e
                );
                continue;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match parse_candidate_json_line(trimmed) {
            Ok(candidate) => agent.add_remote_candidate(candidate),
            Err(e) => eprintln!("Error parseando línea '{}': {}", trimmed, e),
        }
    }

    Ok(())
}

/// Parseo básico de una línea JSON -> Candidate
/// TODO: modelar errores
fn parse_candidate_json_line(line: &str) -> Result<Candidate, String> {
    use std::net::UdpSocket;
    let json = line.trim();

    let foundation = extract_json_string(json, "foundation")
        .ok_or_else(|| "Falta campo 'foundation'".to_string())?;

    let component_str = extract_json_string(json, "component")
        .ok_or_else(|| "Falta campo 'component'".to_string())?;
    let component: u8 = component_str
        .parse()
        .map_err(|_| "Valor inválido en 'component'")?;

    let transport = extract_json_string(json, "transport")
        .ok_or_else(|| "Falta campo 'transport'".to_string())?;

    let priority_str = extract_json_string(json, "priority")
        .ok_or_else(|| "Falta campo 'priority'".to_string())?;
    let priority: u32 = priority_str
        .parse()
        .map_err(|_| "Valor inválido en 'priority'")?;

    let address_str =
        extract_json_string(json, "address").ok_or_else(|| "Falta campo 'address'".to_string())?;
    let address: SocketAddr = address_str
        .parse()
        .map_err(|_| "Formato inválido en 'address'")?;

    let type_str =
        extract_json_string(json, "type").ok_or_else(|| "Falta campo 'type'".to_string())?;
    let cand_type = match type_str.as_str() {
        "Host" => CandidateType::Host,
        "ServerReflexive" => CandidateType::ServerReflexive,
        "PeerReflexive" => CandidateType::PeerReflexive,
        "Relayed" => CandidateType::Relayed,
        other => return Err(format!("Tipo de candidato desconocido: {}", other)),
    };

    let socket = match UdpSocket::bind(address) {
        Ok(s) => Some(s),
        Err(_) => {
            eprintln!("Advertencia: no se pudo bindear socket en {}", address);
            None
        }
    };

    Ok(Candidate::new(
        foundation, component, &transport, priority, address, cand_type, None, socket,
    ))
}

/// Extrae un valor string o numérico de un JSON
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":", key);
    let start_idx = json.find(&pattern)?;
    let rest = &json[start_idx + pattern.len()..];

    if rest.starts_with('"') {
        if let Some(rel_end) = rest[1..].find('"') {
            Some(rest[1..1 + rel_end].to_string())
        } else {
            None
        }
    } else {
        let end_idx = if let Some(idx) = rest.find(|c: char| c == ',' || c == '}') {
            idx
        } else {
            rest.len()
        };
        Some(rest[..end_idx].trim().to_string())
    }
}
