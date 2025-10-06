use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use crate::IceAgent;

/// Guarda los candidatos locales del agente en un archivo JSON (uno por línea)
pub fn save_candidates_to_file(agent: &IceAgent, file_path: &str) -> io::Result<()> {
    let path = Path::new(file_path);
    let mut file = File::create(&path)?;

    for c in &agent.local_candidates {
        let json_line = c.to_json();
        writeln!(file, "{}", json_line)?;
    }

    Ok(())
}

/// Imprime los candidatos locales en stdout para debug
pub fn print_candidates_stdout(agent: &IceAgent) {
    println!("Candidatos locales serializados (JSON):");
    for c in &agent.local_candidates {
        println!("{}", c.to_json());
    }
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use std::fs;

    use crate::client::ice::{
        gathering_service::gather_host_candidates, type_ice::ice_agent::IceRole,
    };

    use super::*;

    #[test]
    fn test_save_candidates_in_file_ok() {
        let mut agent = IceAgent::new(IceRole::Controlling);
        let candidates = gather_host_candidates();
        for c in candidates {
            agent.add_local_candidate(c);
        }

        let path = "test_candidates.json";
        save_candidates_to_file(&agent, path).expect("No se pudo guardar archivo");

        let content = fs::read_to_string(path).expect("No se pudo leer archivo");
        assert!(content.contains("\"foundation\""));
        assert!(content.contains("\"address\""));

        fs::remove_file(path).unwrap();
    }
}
