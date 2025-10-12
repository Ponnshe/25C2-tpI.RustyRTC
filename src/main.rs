use std::env;
use std::process;

use rustyrtc::ice::gathering_service::gather_host_candidates;
use rustyrtc::ice::signaling_mock::{
    load_remote_candidates_from_file, print_candidates_stdout, save_candidates_to_file,
};
use rustyrtc::ice::type_ice::ice_agent::{IceAgent, IceRole};

const FILE_A: &str = "candidates_A.json";
const FILE_B: &str = "candidates_B.json";

fn print_usage() {
    eprintln!(
        "Uso:
  cargo run -- clientA       # genera {fa}
  cargo run -- clientB       # genera {fb}
  cargo run -- readA         # lee {fa} y lo carga como 'remotos'
  cargo run -- readB         # lee {fb} y lo carga como 'remotos'",
        fa = FILE_A,
        fb = FILE_B
    );
}

fn main() {
    let candidates = gather_host_candidates();

    if candidates.is_empty() {
        println!("No se encontraron candidatos locales.");
        return;
    }

    println!("Se encontraron {} candidatos.\n", candidates.len());

    for (i, c) in candidates.iter().enumerate() {
        println!("Candidato #{} -> {:?}", i + 1, c.address);
        println!(
            "Candidato: {} | Foundation: {} | Priority: {}",
            c.address, c.foundation, c.priority
        );

        match &c.socket {
            Some(sock) => match sock.local_addr() {
                Ok(addr) => println!("Socket activo en {:?}", addr),
                Err(e) => println!("Error al obtener local_addr(): {}", e),
            },
            None => println!("Sin socket asociado (None)"),
        }
    }

    println!("\nPrueba finalizada.");
}
