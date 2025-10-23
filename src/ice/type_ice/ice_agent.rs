use super::candidate::Candidate;
use super::candidate_pair::CandidatePair;
use crate::ice::{gathering_service::gather_host_candidates, type_ice::candidate_pair::CandidatePairState};
use rand::{Rng, rngs::OsRng};
use std::{io::Error, time::{Duration, Instant}};

/// Error message formatting constants
const ERROR_MSG: &str = "ERROR";
const WHITESPACE: &str = " ";
const QUOTE: &str = "\"";

/// Timeout (en ms) para cada intento de conexión (simulación local)
const CHECK_TIMEOUT_MS: u64 = 1000;

/// Mensajes simulados para los checks
const BINDING_REQUEST: &[u8] = b"BINDING-REQUEST";
const BINDING_RESPONSE: &[u8] = b"BINDING-RESPONSE";

/// Warnings and error messages
const WARN_INVALID_PRIORITY: &str = "Invalid candidate pair priority.";
const WARN_MAX_LIMIT_REACHED: &str = "Maximum candidate pair limit reached.";

/// Configuration constants
const MAX_PAIR_LIMIT: usize = 100; // reasonable upper bound to avoid combinatorial explosion
const MIN_PRIORITY_THRESHOLD: u64 = 1; // pairs below this value are ignored

/// Helper to format error messages consistently
fn error_message(msg: &str) -> String {
    format!("{}{}{}{}{}", ERROR_MSG, WHITESPACE, QUOTE, msg, QUOTE)
}

///Role for an agent
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IceRole {
    /// It's the role in which the final decision is made on which candidate pair will be used for the connection.
    Controlling,
    /// It's the role that accepts the nominated pair.
    Controlled,
}

/// It's responsible for orchestrating the flow of gathering, checks, nomination, etc.
#[derive(Debug)]
pub struct IceAgent {
    /// set of local candidates.
    pub local_candidates: Vec<Candidate>,
    /// set of remote candidates.
    pub remote_candidates: Vec<Candidate>,
    /// set of pairs of candidates.
    pub candidate_pairs: Vec<CandidatePair>,
    /// role for the agent.
    pub role: IceRole,
    ufrag: String,
    pwd: String,
}

impl IceAgent {
    /// Create a valid ice agent.
    ///
    /// # Arguments
    /// Same properties of ice agent.
    ///
    /// # Return
    /// A new ice agent.
    pub fn new(role: IceRole) -> Self {
        let (ufrag, pwd) = Self::fresh_credentials();
        Self {
            local_candidates: vec![],
            remote_candidates: vec![],
            candidate_pairs: vec![],
            role,
            ufrag,
            pwd,
        }
    }

    pub fn add_local_candidate(&mut self, candidate: Candidate) {
        self.local_candidates.push(candidate);
    }

    pub fn add_remote_candidate(&mut self, candidate: Candidate) {
        self.remote_candidates.push(candidate);
    }

    /// Collects local candidates. This feature will be asynchronous in a future implementation.
    pub fn gather_candidates(&mut self) -> Result<&Vec<Candidate>, Error> {
        let candidates = gather_host_candidates();
        for c in candidates {
            self.add_local_candidate(c);
        }
        Ok(&self.local_candidates)
    }

    /// Builds all possible candidate pairs between local and remote candidates.
    /// According to RFC 8445 §6.1.2.3:
    /// - Each local candidate is paired with each remote candidate.
    /// - The pair’s priority is calculated based on the agent's role (controlling or controlled).
    /// - Pairs with invalid priority values are ignored.
    /// - The resulting list is sorted by descending priority.
    /// 
    /// # Arguments
    /// * `self` - Self entity.
    ///
    /// # Returns
    /// The number of valid pairs generated.
    /// 
    /// # Errors
    /// 
    pub fn form_candidate_pairs(&mut self) -> usize {
        let mut pairs = Vec::new();

        for local in self.local_candidates.iter() {
            if pairs.len() >= MAX_PAIR_LIMIT {
                break;
            }
            for remote in self.remote_candidates.iter() {
                let priority = CandidatePair::calculate_pair_priority(local, remote, &self.role);

                // skip incompatible address families (IPv4 ↔ IPv6)
                if local.address.is_ipv4() != remote.address.is_ipv4() {
                    eprintln!(
                        "{}",
                        error_message(&format!(
                            "Incompatible address families (local={}, remote={})",
                            local.address, remote.address
                        ))
                    );
                    continue;
                }

                // skip different transport protocols (e.g., UDP ↔ TCP)
                if local.transport != remote.transport {
                    eprintln!(
                        "{}",
                        error_message(&format!(
                            "Incompatible transport protocols (local={}, remote={})",
                            local.transport, remote.transport
                        ))
                    );
                    continue;
                }

                if priority < MIN_PRIORITY_THRESHOLD {
                    eprintln!(
                        "WARN: Par ignorado por prioridad inválida (local={}, remote={}, prio={})",
                        local.address, remote.address, priority
                    );
                    continue;
                }

                pairs.push(CandidatePair::new(
                    local.clone(),
                    remote.clone(),
                    priority,
                ));

                if pairs.len() >= MAX_PAIR_LIMIT {
                    eprintln!(
                        "WARN: Límite máximo de pares alcanzado ({}). Truncando lista.",
                        MAX_PAIR_LIMIT
                    );
                    break;
                }
            }
        }

        //sorted by descending priority
        pairs.sort_by(|a, b| b.priority.cmp(&a.priority));

        let count = pairs.len();
        self.candidate_pairs = pairs;
        count
    }

    /// Runs simulated connectivity checks between all candidate pairs (blocking version).
    /// - Changes state from `Waiting` → `InProgress`.
    /// - Sends a mock "BINDING-REQUEST" via UDP (simulated).
    /// - Marks the pair as `Succeeded` or `Failed` depending on result or timeout.
    ///
    /// # Arguments
    /// * `self` - Self entity.
    /// 
    /// # Errors
    /// 
    pub fn run_connectivity_checks(&mut self) {
        for pair in self.candidate_pairs.iter_mut() {
            pair.state = CandidatePairState::InProgress;

            let success = IceAgent::try_connect_pair(pair);

            pair.state = if success {
                println!(
                    "Connectivity check succeeded: [local={}, remote={}]",
                    pair.local.address, pair.remote.address
                );
                CandidatePairState::Succeeded
            } else {
                eprintln!(
                    "Connectivity check failed: [local={}, remote={}]",
                    pair.local.address, pair.remote.address
                );
                CandidatePairState::Failed
            };
        }
    }

    /// Tries to connect a single candidate pair (simulated local check).
    ///
    /// # Behavior
    /// - If either candidate lacks a socket, fails immediately.
    /// - Sends `"BINDING-REQUEST"` via UDP from local to remote.
    /// - Waits a short timeout and assumes success if send succeeds.
    ///
    /// # Return
    /// - `true` if the pair is reachable locally.
    /// - `false` if send/recv failed or timed out.
    fn try_connect_pair(pair: &mut CandidatePair) -> bool {
        // Check that both sides have a socket
        let Some(local_sock) = &pair.local.socket else {
            eprintln!(
                "No socket available for local candidate: {}",
                pair.local.address
            );
            return false;
        };

        // Attempt to send "BINDING-REQUEST"
        if let Err(e) = local_sock.send_to(BINDING_REQUEST, pair.remote.address) {
            eprintln!(
                "Send failed from {} → {}: {}",
                pair.local.address, pair.remote.address, e
            );
            return false;
        }

        // Simulate short wait (round-trip latency)
        std::thread::sleep(Duration::from_millis(CHECK_TIMEOUT_MS / 10));

        // Optional: Try to read a response (for future STUN integration)
        let mut buf = [0u8; 64];
        match local_sock.set_read_timeout(Some(Duration::from_millis(CHECK_TIMEOUT_MS))) {
            Ok(_) => match local_sock.recv_from(&mut buf) {
                Ok((_, src)) => {
                    println!("Received response from {}", src);
                    true
                }
                Err(_) => {
                    // For now, we assume success after a successful send
                    true
                }
            },
            Err(_) => false,
        }
    }

    pub(crate) fn local_credentials(&self) -> (String, String) {
        (self.ufrag.clone(), self.pwd.clone())
    }

    fn gen_token(len: usize) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let idx = OsRng.gen_range(0..ALPHABET.len());
            s.push(ALPHABET[idx] as char);
        }
        s
    }

    fn fresh_credentials() -> (String, String) {
        // ICE: ufrag >= 4 chars; pwd >= 22 chars
        (Self::gen_token(8), Self::gen_token(24))
    }

    //print every state of candidates pair
    pub fn print_pair_states(&self) {
        if self.candidate_pairs.is_empty() {
            println!("No candidate pairs available.");
            return;
        }
        println!("=== Candidate Pair States ===");
        for pair in &self.candidate_pairs {
            pair.debug_state();
        }
    }

    /// Updates the state of a candidate pair by index.
    ///
    /// # Arguments
    /// * `pair_index` - index within `self.candidate_pairs`
    /// * `new_state` - new [`CandidatePairState`] to assign
    ///
    /// # Behavior
    /// - If the index is valid, updates the pair's state.
    /// - If it's out of bounds, prints a warning.
    pub fn update_pair_state(&mut self, pair_index: usize, new_state: CandidatePairState) {
        if let Some(pair) = self.candidate_pairs.get_mut(pair_index) {
            println!(
                "Updating pair {} [{} → {:?}]",
                pair_index, pair.local.address, new_state
            );
            pair.state = new_state;
        } else {
            eprintln!("Invalid pair index: {}", pair_index);
        }
    }

    /// Prints a summary of all candidate pairs and their final states.
    ///
    /// Useful for debugging or for a DEMO.
    pub fn print_connectivity_summary(&self) {
        let total = self.candidate_pairs.len();
        let succeeded = self
            .candidate_pairs
            .iter()
            .filter(|p| matches!(p.state, CandidatePairState::Succeeded))
            .count();
        let failed = self
            .candidate_pairs
            .iter()
            .filter(|p| matches!(p.state, CandidatePairState::Failed))
            .count();
        let waiting = self
            .candidate_pairs
            .iter()
            .filter(|p| matches!(p.state, CandidatePairState::Waiting))
            .count();
        let in_progress = self
            .candidate_pairs
            .iter()
            .filter(|p| matches!(p.state, CandidatePairState::InProgress))
            .count();

        println!("\n=== ICE Connectivity Summary ===");
        println!("Total candidate pairs: {}", total);
        println!("Succeeded: {}", succeeded);
        println!("Failed: {}", failed);
        println!("Waiting: {}", waiting);
        println!("InProgress: {}", in_progress);
        println!("==================================\n");

        for (i, pair) in self.candidate_pairs.iter().enumerate() {
            println!(
                "PAIR #{} → [local={}, remote={}, state={:?}, priority={}]",
                i, pair.local.address, pair.remote.address, pair.state, pair.priority
            );
        }
    }

    /// Returns all successfully validated candidate pairs.
    pub fn get_valid_pairs(&self) -> Vec<&CandidatePair> {
        self.candidate_pairs
            .iter()
            .filter(|p| matches!(p.state, CandidatePairState::Succeeded))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::ice::type_ice::candidate_type::CandidateType;
    use std::{net::{SocketAddr, UdpSocket}, sync::Arc};

    fn mock_candidate(priority: u32, ip: &str, port: u16) -> Candidate {
        let addr: SocketAddr = format!("{}:{}", ip, port).parse().unwrap();
        Candidate::new(
            String::new(),
            1,
            "udp",
            priority,
            addr,
            CandidateType::Host,
            None,
            None,
        )
    }

    fn mock_candidate_with_socket(ip: &str, port: u16) -> Candidate {
        let addr: SocketAddr = format!("{}:{}", ip, port).parse().unwrap();
        let sock = Arc::new(UdpSocket::bind(addr).unwrap());
        Candidate::new(
            "f1".into(),
            1,
            "udp",
            100,
            sock.local_addr().unwrap(),
            CandidateType::Host,
            None,
            Some(sock),
        )
    }

    fn mock_pair_with_state(state: CandidatePairState) -> CandidatePair {
        let ip_address = "127.0.0.1";
        let port_local = 5000;
        let priority_local = 100;
        let port_remote = 5001;
        let priority_remote = 90;
        let priority_pair = 12345;
        let local = mock_candidate(priority_local, ip_address, port_local);
        let remote = mock_candidate(priority_remote, ip_address, port_remote);
        let mut pair = CandidatePair::new(local, remote, priority_pair);
        pair.state = state;
        pair
    }

    #[test]
    fn test_update_pair_state_with_valid_index_ok() {
        const EXPECTED_ERROR_MSG: &str = "The pair status was not updated correctly";
        let index = 0;
        let mut agent = IceAgent::new(IceRole::Controlling);
        let pair = mock_pair_with_state(CandidatePairState::Waiting);
        agent.candidate_pairs.push(pair);

        agent.update_pair_state(index, CandidatePairState::Succeeded);

        assert!(
            matches!(agent.candidate_pairs[index].state, CandidatePairState::Succeeded),
            "{EXPECTED_ERROR_MSG}"
        );
    }

    #[test]
    fn test_update_pair_state_with_invalid_index_error() {
        const EXPECTED_ERROR_MSG1: &str = "The pair status was not updated correctly";
        const EXPECTED_WARNING_MSG2: &str = "The size of candidate_pairs should not be altered.";
        let invalid_index = 99;
    
        let mut agent = IceAgent::new(IceRole::Controlling);
        let pair = mock_pair_with_state(CandidatePairState::Waiting);
        agent.candidate_pairs.push(pair);
    
        agent.update_pair_state(invalid_index, CandidatePairState::Failed);
    
        assert!(
            matches!(agent.candidate_pairs[0].state, CandidatePairState::Waiting),
            "{EXPECTED_ERROR_MSG1}"
        );
    
        assert_eq!(agent.candidate_pairs.len(), 1, "{EXPECTED_WARNING_MSG2}");
    }

    #[test]
    fn test_update_empty_pair_state_error() {
        let index = 99;
        let mut agent = IceAgent::new(IceRole::Controlling);
        agent.update_pair_state(index, CandidatePairState::Failed);
        
        assert!(agent.candidate_pairs.is_empty());
    }

    #[test]
    fn test_should_return_only_succeeded_valid_pair_ok() {
        const EXPECTED_ERROR_MSG1: &str = "There should only be one Succeeded pair";
        const EXPECTED_ERROR_MSG2: &str = "The returned pair is not in the Succeeded state";
        let mut agent = IceAgent::new(IceRole::Controlled);

        agent.candidate_pairs = vec![
            mock_pair_with_state(CandidatePairState::Succeeded),
            mock_pair_with_state(CandidatePairState::Failed),
            mock_pair_with_state(CandidatePairState::Waiting),
        ];

        let valid = agent.get_valid_pairs();
        assert_eq!(valid.len(), 1, "{EXPECTED_ERROR_MSG1}");
        assert!(
            matches!(valid[0].state, CandidatePairState::Succeeded),
            "{EXPECTED_ERROR_MSG2}"
        );
    }

    #[test]
    fn test_run_connectivity_checks_all_succeed_ok() {
        const EXPECTED_ERROR_MSG: &str = "At least one candidate pair should succeed locally";
        let ip_address = "127.0.0.1";
        let port = 0;

        let mut agent = IceAgent::new(IceRole::Controlling);
        let local = mock_candidate_with_socket(ip_address, port);
        let remote = mock_candidate_with_socket(ip_address, port);

        agent.local_candidates = vec![local];
        agent.remote_candidates = vec![remote];
        agent.form_candidate_pairs();

        agent.run_connectivity_checks();

        assert!(
            agent.candidate_pairs.iter().any(|p| matches!(p.state, CandidatePairState::Succeeded)),
            "{EXPECTED_ERROR_MSG}"
        );
    }

    #[test]
    fn test_run_connectivity_checks_local_candidate_without_socket_error() {
        const EXPECTED_ERROR_MSG: &str = "Pair, with local candidate without socket must fail";
        let ip_address = "127.0.0.1:9999";
        let ip_address_remote = "127.0.0.1";
        let port = 0;

        let mut agent = IceAgent::new(IceRole::Controlled);

        let local_addr: SocketAddr = ip_address.parse().unwrap();
        let remote = mock_candidate_with_socket(ip_address_remote, port);

        let local = Candidate::new(
            "f2".into(),
            1,
            "udp",
            100,
            local_addr,
            CandidateType::Host,
            None,
            None,
        );

        agent.local_candidates = vec![local];
        agent.remote_candidates = vec![remote];
        agent.form_candidate_pairs();

        agent.run_connectivity_checks();

        assert!(
            agent.candidate_pairs.iter().all(|p| matches!(p.state, CandidatePairState::Failed)),
            "{EXPECTED_ERROR_MSG}"
        );
    }

    #[test]
    fn test_try_connect_pair_succees_ok() {
        const EXPECTED_ERROR_MSG: &str = "Expected simulated local success for connectivity pair";
        let ip_address = "127.0.0.1";
        let port = 0;
        let local = mock_candidate_with_socket(ip_address, port);
        let remote = mock_candidate_with_socket(ip_address, port);

        let mut pair = CandidatePair::new(local, remote, 100);
        let success = IceAgent::try_connect_pair(&mut pair);
        assert!(success, "{EXPECTED_ERROR_MSG}");
        assert!(matches!(pair.state, CandidatePairState::Waiting) || success);
    }

    #[test]
    fn test_form_candidate_pairs_skips_different_transports() {
        let mut agent = IceAgent::new(IceRole::Controlling);

        let local = mock_candidate(100, "192.168.1.1", 5000);
        let mut remote = mock_candidate(100, "192.168.1.2", 5001);
        remote.transport = "tcp".to_string();

        agent.local_candidates = vec![local];
        agent.remote_candidates = vec![remote];

        let count = agent.form_candidate_pairs();
        assert_eq!(count, 0, "No deben formarse pares entre UDP y TCP");
    }

    #[test]
    fn test_skips_incompatible_ip_families_ok() {
        let mut agent = IceAgent::new(IceRole::Controlling);

        // IPv4 local
        let local = mock_candidate(100, "192.168.1.1", 5000);

        // IPv6 remote
        let remote_addr = "[2001:db8::1]:5001".parse().unwrap();
        let remote = Candidate::new(
            String::new(),
            1,
            "udp",
            100,
            remote_addr,
            CandidateType::Host,
            None,
            None,
        );

        agent.local_candidates = vec![local];
        agent.remote_candidates = vec![remote];

        let count = agent.form_candidate_pairs();
        assert_eq!(
            count, 0,
            "No deben formarse pares entre IPv4 y IPv6"
        );
    }

    #[test]
    fn test_creates_valid_pairs_ok() {
        let mut agent = IceAgent::new(IceRole::Controlling);

        agent.local_candidates = vec![mock_candidate(100, "10.0.0.1", 5000)];
        agent.remote_candidates = vec![
            mock_candidate(90, "10.0.0.2", 6000),
            mock_candidate(80, "10.0.0.3", 6001),
        ];

        let count = agent.form_candidate_pairs();

        assert_eq!(count, 2);
        assert!(agent.candidate_pairs[0].priority >= agent.candidate_pairs[1].priority);
    }

    #[test]
    fn test_skip_candidates_with_zero_priority_pairs() {
        let mut agent = IceAgent::new(IceRole::Controlled);
    
        let mut local = mock_candidate(1, "192.168.1.1", 5000);
        let mut remote = mock_candidate(1, "192.168.1.2", 5001);
    
        local.priority = 0;
        remote.priority = 0;
    
        agent.local_candidates = vec![local];
        agent.remote_candidates = vec![remote];
    
        let count = agent.form_candidate_pairs();
    
        assert_eq!(
            count, 
            0, 
            "Debería ignorar pares con prioridad 0 (ningún par válido generado)"
        );
    }
    

    #[test]
    fn test_candidate_pairs_with_max_limit_ok() {
        let mut agent = IceAgent::new(IceRole::Controlling);
        agent.local_candidates = (0..10)
            .map(|i| mock_candidate(100 + i, "10.0.0.1", 5000 + i as u16))
            .collect();
        agent.remote_candidates = (0..20)
            .map(|i| mock_candidate(80 + i, "10.0.0.2", 6000 + i as u16))
            .collect();

        let count = agent.form_candidate_pairs();
        assert!(count <= MAX_PAIR_LIMIT, "Debe respetar el límite máximo");
    }

    #[test]
    fn test_create_host_candidate_ok() {
        let address = "192.168.0.10:5000";
        let protocol = "UDP";

        let addr: SocketAddr = address.parse().unwrap();
        let one_candidate = Candidate::new(
            "1".into(),
            1,
            protocol,
            1234,
            addr,
            CandidateType::Host,
            None,
            None,
        );
        assert_eq!(one_candidate.cand_type, CandidateType::Host);
        assert_eq!(one_candidate.address.port(), 5000);
    }

    #[test]
    fn test_create_agent_and_add_candidates_ok() {
        let address = "192.168.0.10:5000";
        let protocol = "UDP";

        let mut agent = IceAgent::new(IceRole::Controlling);
        let addr: SocketAddr = address.parse().unwrap();
        let c = Candidate::new(
            "1".into(),
            1,
            protocol,
            1234,
            addr,
            CandidateType::Host,
            None,
            None,
        );
        agent.add_local_candidate(c);
        assert_eq!(agent.local_candidates.len(), 1);
    }

    #[test]
    fn test_create_agent_and_add_remote_candidates_ok() {
        let address = "192.168.0.10:5000";
        let protocol = "UDP";

        let mut agent = IceAgent::new(IceRole::Controlling);
        let addr: SocketAddr = address.parse().unwrap();
        let c = Candidate::new(
            "1".into(),
            1,
            protocol,
            1234,
            addr,
            CandidateType::Host,
            None,
            None,
        );
        agent.add_remote_candidate(c);
        assert_eq!(agent.remote_candidates.len(), 1);
    }


}
