use std::env;
use std::process;

use rustyrtc::connection_manager::ConnectionManager;
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
    // let args: Vec<String> = env::args().collect();
    // let mode = args.get(1).map(|s| s.as_str()).unwrap_or("error");

    // match mode {
    //     // Guarda los candidatos de A
    //     "clientA" | "saveA" => {
    //         let mut agent = IceAgent::new(IceRole::Controlling);
    //         for c in gather_host_candidates() {
    //             agent.add_local_candidate(c);
    //         }

    //         print_candidates_stdout(&agent);

    //         if let Err(e) = save_candidates_to_file(&agent, FILE_A) {
    //             eprintln!("Error guardando {}: {}", FILE_A, e);
    //             process::exit(1);
    //         }
    //         println!("Archivo {} generado.", FILE_A);
    //     }

    //     // Guarda los candidatos de B
    //     "clientB" | "saveB" => {
    //         // (puede ser Controlled para diferenciar roles)
    //         let mut agent = IceAgent::new(IceRole::Controlled);
    //         for c in gather_host_candidates() {
    //             agent.add_local_candidate(c);
    //         }

    //         print_candidates_stdout(&agent);

    //         if let Err(e) = save_candidates_to_file(&agent, FILE_B) {
    //             eprintln!("Error guardando {}: {}", FILE_B, e);
    //             process::exit(1);
    //         }
    //         println!("Archivo {} generado.", FILE_B);
    //     }

    //     // Lee candidatos de A como "remotos"
    //     "readA" => {
    //         let mut agent = IceAgent::new(IceRole::Controlling);
    //         for c in gather_host_candidates() {
    //             agent.add_local_candidate(c);
    //         }

    //         match load_remote_candidates_from_file(&mut agent, FILE_A) {
    //             Ok(()) => {
    //                 println!("Remotos cargados desde {}", FILE_A);
    //                 println!("Remotos:");
    //                 for c in &agent.remote_candidates {
    //                     println!(" - {}", c);
    //                 }
    //             }
    //             Err(e) => {
    //                 eprintln!("Error leyendo {}: {}", FILE_A, e);
    //                 process::exit(1);
    //             }
    //         }
    //     }

    //     // Lee candidatos de B como "remotos"
    //     "readB" => {
    //         let mut agent = IceAgent::new(IceRole::Controlling);
    //         for c in gather_host_candidates() {
    //             agent.add_local_candidate(c);
    //         }

    //         match load_remote_candidates_from_file(&mut agent, FILE_B) {
    //             Ok(()) => {
    //                 println!("Remotos cargados desde {}", FILE_B);
    //                 println!("Remotos:");
    //                 for c in &agent.remote_candidates {
    //                     println!(" - {}", c);
    //                 }
    //             }
    //             Err(e) => {
    //                 eprintln!("Error leyendo {}: {}", FILE_B, e);
    //                 process::exit(1);
    //             }
    //         }
    //     }

    //     _ => {
    //         print_usage();
    //         process::exit(2);
    //     }
    // }
    let mut conn_manager = ConnectionManager::new();
    let sdp_offer = conn_manager.create_offer().unwrap();
    println!("{}", sdp_offer.encode());
}
