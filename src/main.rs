mod client;
use crate::client::ice::gathering_service::gather_host_candidates;
use crate::client::ice::signaling_mock::print_candidates_stdout;
use crate::client::ice::signaling_mock::save_candidates_to_file;
use crate::client::ice::type_ice::ice_agent::IceAgent;
use crate::client::ice::type_ice::ice_agent::IceRole;

fn main() {
    let mut agent = IceAgent::new(IceRole::Controlling);
    let host_candidates = gather_host_candidates();
    for c in host_candidates {
        agent.add_local_candidate(c);
    }

    print_candidates_stdout(&agent);

    // Guardar en archivo (simulando signaling)
    let output_path = "candidates_A.json";
    match save_candidates_to_file(&agent, output_path) {
        Ok(_) => println!("Candidatos guardados en {}", output_path),
        Err(e) => eprintln!("Error guardando candidatos: {}", e),
    }
}
