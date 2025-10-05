mod client;
use crate::client::ice::gathering_service::gather_host_candidates;
use crate::client::ice::type_ice::ice_agent::IceAgent;
use crate::client::ice::type_ice::ice_agent::IceRole;

fn main() {
    let mut agent = IceAgent::new(IceRole::Controlling);
    let host_candidates = gather_host_candidates();
    for c in host_candidates {
        agent.add_local_candidate(c);
    }

    println!("Candidatos locales encontrados:");
    for c in &agent.local_candidates {
        println!(" - {}", c.address);
    }
}
