use super::candidate::Candidate;
use super::candidate_pair::CandidatePair;
use crate::ice::{gathering_service::gather_host_candidates, type_ice::candidate_pair::CandidatePairState};
use rand::{Rng, rngs::OsRng};
use std::{io::Error, net::UdpSocket, time::Duration};

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
    pub nominated_pair: Option<CandidatePair>
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
            nominated_pair: None,
        }
    }

    /// Start the ICE data channel simulating a local P2P communication.
    /// 
    /// Flow:
    /// 1. Verifies that a nominated pair (`nominated_pair`) exists.
    /// 2. Opens the local socket with `open_udp_channel()`.
    /// 3. Sends a test message "BINDING-DATA hello ICE".
    /// 4. Waits for a "BINDING-ACK" response.
    /// 
    /// #Returns
    /// Ok(()) - sucessful message if all the flow was correct
    /// 
    /// #Error
    /// Err(String) - If any error occurs in any of the steps
    pub fn start_data_channel(&mut self) -> Result<(), String> {
        println!("🔹 Starting ICE data channel...");

        // Validate the existence of a nominated pair
        if self.nominated_pair.is_none() {
            return Err(String::from(
                "Cannot start data channel: no nominated pair available.",
            ));
        }

        // Open local socket 
        let socket = match self.open_udp_channel() {
            Ok(sock) => sock,
            Err(e) => return Err(format!("Failed to open UDP channel: {}", e)),
        };

        // Send test message
        if let Err(e) = self.send_test_message(&socket, "hola ICE") {
            return Err(format!("Failed to send test message: {}", e));
        }

        // Waiting answer
        match IceAgent::receive_test_message(&socket) {
            Ok(msg) if msg.contains("BINDING-ACK") => {
                println!("ICE Data Channel established successfully!");
                Ok(())
            }
            Ok(msg) => Err(format!(
                "Unexpected message received instead of ACK: {}",
                msg
            )),
            Err(e) => Err(format!("Failed to receive ACK: {}", e)),
        }
    }

    /// Sends a test message (e.g. "BINDING-DATA hola ICE") to the remote candidate.
    ///
    /// # Arguments
    /// * `socket` - Reference to a bound UDP socket.
    /// * `msg` - The message to send.
    ///
    /// # Returns
    /// * `Ok(())` if the message was sent successfully.
    /// 
    /// #Error
    /// * `Err(String)` if there was no nominated pair or sending failed.
    pub fn send_test_message(&self, socket: &UdpSocket, msg: &str) -> Result<(), String> {
        let pair = match &self.nominated_pair {
            Some(p) => p,
            None => return Err(String::from("Cannot send message: no nominated pair available")),
        };

        let remote_addr = pair.remote.address;
        let payload = format!("BINDING-DATA {}", msg);

        match socket.send_to(payload.as_bytes(), remote_addr) {
            Ok(sent) => {
                println!(
                    "[SEND] Sent {} bytes → {} ({})",
                    sent, remote_addr, payload
                );
                Ok(())
            }
            Err(e) => Err(format!(
                "Failed to send UDP message to {}: {}",
                remote_addr, e
            )),
        }
    }

    /// Waits for a response message ("BINDING-ACK") from the remote peer.
    ///
    /// # Arguments
    /// * `socket` - UDP socket to listen on.
    ///
    /// # Returns
    /// * `Ok(String)` - The received message.
    /// 
    /// # Error
    /// * `Err(String)` - Timeout or read error.
    pub fn receive_test_message(socket: &UdpSocket) -> Result<String, String> {
        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .map_err(|e| format!("Failed to set timeout: {}", e))?;

        let mut buf = [0u8; 512];
        match socket.recv_from(&mut buf) {
            Ok((size, src)) => {
                let msg = String::from_utf8_lossy(&buf[..size]).to_string();
                println!("[RECV] From {} → \"{}\"", src, msg);
                Ok(msg)
            }
            Err(e) => Err(format!("Timeout or error while receiving UDP message: {}", e)),
        }
    }

    /// Opens a UDP socket using the local address of the nominated pair.
    /// # Description
    /// This function prepares the UDP data channel by binding to the local
    /// address of the nominated candidate pair.
    ///
    /// # Returns
    /// * `Ok(UdpSocket)` — Socket bound successfully to the nominated pair’s local address.
    ///
    /// # Error
    /// `Err(String)` — If no nominated pair exists or binding fails.
    pub fn open_udp_channel(&self) -> Result<UdpSocket, String> {
        // Ensure we have a nominated pair
        let pair = match &self.nominated_pair {
            Some(p) => p,
            None => return Err(String::from("No nominated pair available to open UDP channel.")),
        };

        // Check the pair is in valid state before opening the channel
        if !matches!(pair.state, CandidatePairState::Succeeded) {
            return Err(format!(
                "Cannot open UDP channel — pair not in Succeeded state (current: {:?})",
                pair.state
            ));
        }

        // Attempt to bind the UDP socket to the local candidate address
        match UdpSocket::bind(pair.local.address) {
            Ok(socket) => {
                println!(
                    "UDP channel opened successfully on {} (remote = {})",
                    pair.local.address, pair.remote.address
                );
                Ok(socket)
            }
            Err(e) => Err(format!(
                "Failed to bind UDP socket on {}: {}",
                pair.local.address, e
            )),
        }
    }

    /// Executes role-specific logic according to ICE role.
    /// - Controlling → select the best valid pair (nomination).
    /// - Controlled  → wait for nomination (mocked for local tests).
    ///
    /// This prepares both agents for the final ICE connection phase.
    pub fn run_role_logic(&mut self) {
        match self.role {
            IceRole::Controlling => {
                println!("Role: CONTROLLING — selecting the nominated pair...");
                if let Some(pair) = self.select_valid_pair() {
                    println!(
                        "Nominated pair: [local={}, remote={}, prio={}]",
                        pair.local.address, pair.remote.address, pair.priority
                    );
                } else {
                    eprintln!("No valid pair available for nomination.");
                }
            }

            IceRole::Controlled => {
                println!("Role: CONTROLLED — waiting for controlling peer to nominate...");
                // For now, we mock the reception of the nominated pair
                if let Some(pair) = self.candidate_pairs.first_mut() {
                    pair.is_nominated = true;
                    self.nominated_pair = Some(pair.clone_light());
                    println!(
                        "Simulated nomination received: [local={}, remote={}, prio={}]",
                        pair.local.address, pair.remote.address, pair.priority
                    );
                } else {
                    eprintln!("No candidate pairs available to mark as nominated.");
                }
            }
        }
    }

    /// Selects the valid (nominated) pair based on ICE role and priority.
    /// - Finds the `Succeeded` pair with the highest priority.
    /// - If role is Controlling → marks it as nominated.
    /// - Stores it in `self.nominated_pair` for later use.
    ///
    /// # Returns
    /// the nominated pair, if found.
    ///
    /// #Errors
    /// 
    pub fn select_valid_pair(&mut self) -> Option<&CandidatePair> {
        // Filter indices of succeeded pairs
        let succeeded_indices: Vec<usize> = self
            .candidate_pairs
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                if matches!(p.state, CandidatePairState::Succeeded) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
    
        if succeeded_indices.is_empty() {
            eprintln!("WARN: No succeeded pairs available for nomination.");
            return None;
        }
    
        // Find index of the highest-priority succeeded pair
        let best_index = succeeded_indices
            .into_iter()
            .max_by_key(|&i| self.candidate_pairs[i].priority);
    
        match best_index {
            Some(idx) => {
                let pair = &mut self.candidate_pairs[idx];
                pair.is_nominated = true;
    
                // Store reference safely
                self.nominated_pair = Some(CandidatePair {
                    local: pair.local.clone_light(),
                    remote: pair.remote.clone_light(),
                    priority: pair.priority,
                    state: pair.state.clone(),
                    is_nominated: true,
                });
    
                // Return immutable reference
                self.candidate_pairs.get(idx)
            }
            None => {
                eprintln!("ERROR: Could not determine nominated pair index.");
                None
            }
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

    fn mock_candidate_with_address(addr_str: &str) -> Candidate {
        let addr: SocketAddr = addr_str.parse().unwrap();
        Candidate::new(
            "mock_foundation".into(),
            1,
            "udp",
            100,
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

    fn mock_pair_with_states(state: CandidatePairState) -> CandidatePair {
        CandidatePair {
            local: mock_candidate_with_address("192.168.0.1:5000"),
            remote: mock_candidate_with_address("192.168.0.2:6000"),
            priority: 100,
            state,
            is_nominated: false,
        }
    }

    #[test]
    fn test_send_message_without_nominated_pair_error() {
        let agent = IceAgent::new(IceRole::Controlling);
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();

        let result = agent.send_test_message(&socket, "hola ICE");

        assert!(
            result.is_err(),
            "Debe fallar si no hay par nominado configurado"
        );
    }

    #[test]
    fn test_send_and_receive_message_ok() {
        const EXPECTED_ACK: &str = "BINDING-DATA hola ICE";

        // Creamos un socket local y otro remoto en 127.0.0.1
        let socket_a = UdpSocket::bind("127.0.0.1:0").unwrap();
        let socket_b = UdpSocket::bind("127.0.0.1:0").unwrap();

        // socket_b escuchará y responderá
        std::thread::spawn({
            let socket_b_clone = socket_b.try_clone().unwrap();
            move || {
                let mut buf = [0u8; 512];
                if let Ok((size, src)) = socket_b_clone.recv_from(&mut buf) {
                    let msg = String::from_utf8_lossy(&buf[..size]);
                    if msg.contains("BINDING-DATA") {
                        let _ = socket_b_clone.send_to(b"BINDING-ACK", src);
                    }
                }
            }
        });

        // Simulamos agente con par nominado
        let mut agent = IceAgent::new(IceRole::Controlling);
        let mut pair = CandidatePair {
            local: Candidate::new(
                "f1".into(),
                1,
                "udp",
                100,
                socket_a.local_addr().unwrap(),
                CandidateType::Host,
                None,
                None,
            ),
            remote: Candidate::new(
                "f2".into(),
                1,
                "udp",
                90,
                socket_b.local_addr().unwrap(),
                CandidateType::Host,
                None,
                None,
            ),
            priority: 1234,
            state: CandidatePairState::Succeeded,
            is_nominated: true,
        };
        agent.nominated_pair = Some(pair);

        let send_result = agent.send_test_message(&socket_a, "hola ICE");
        assert!(send_result.is_ok(), "El envío debe completarse correctamente");

        let recv_result = IceAgent::receive_test_message(&socket_a);
        assert!(
            recv_result.is_ok(),
            "Debe recibir mensaje de respuesta correctamente"
        );

        let msg = recv_result.unwrap();
        assert_eq!(msg, "BINDING-ACK", "Debe recibir el mensaje ACK esperado");
    }

    #[test]
    fn test_open_udp_channel_between_pairs_ok() {
        const EXPECTED_ERROR_MSG: &str = "Should bind successfully when pair is Succeeded";

        let mut agent = IceAgent::new(IceRole::Controlling);
        let mut pair = mock_pair_with_state(CandidatePairState::Succeeded);
        pair.is_nominated = true;
        agent.nominated_pair = Some(pair);

        let result = agent.open_udp_channel();

        assert!(
            result.is_ok(),
            "{} (error: {:?})",
            EXPECTED_ERROR_MSG,
            result.err()
        );
    }

    #[test]
    fn test_open_udp_channel_without_existing_nominated_pair_error() {
        const EXPECTED_ERROR_MSG: &str = "Should return error when no nominated pair exists";

        let agent = IceAgent::new(IceRole::Controlling);

        let result = agent.open_udp_channel();

        assert!(
            result.is_err(),
            "{} (got Ok instead)",
            EXPECTED_ERROR_MSG
        );
    }

    #[test]
    fn test_open_udp_channel_with_pair_in_not_succeeded_state_error() {
        const EXPECTED_ERROR_MSG: &str =
            "Should not allow binding if pair is not in Succeeded state";

        let mut agent = IceAgent::new(IceRole::Controlling);
        let mut pair = mock_pair_with_state(CandidatePairState::Failed);
        pair.is_nominated = true;
        agent.nominated_pair = Some(pair);

        let result = agent.open_udp_channel();

        assert!(
            result.is_err(),
            "{} (expected Err, got {:?})",
            EXPECTED_ERROR_MSG,
            result
        );
    }

    #[test]
    fn test_agent_with_role_controlling_selects_nominated_pair_ok() {
        const EXPECTED_ERROR_MSG: &str = "There must be a nominated pair in Controlling mode";
        let mut agent = IceAgent::new(IceRole::Controlling);
        let mut pair = mock_pair_with_states(CandidatePairState::Succeeded);
        
        pair.priority = 77;
        agent.candidate_pairs = vec![pair];

        agent.run_role_logic();

        assert!(
            agent.nominated_pair.is_some(),
            "{EXPECTED_ERROR_MSG}"
        );
    }

    #[test]
    fn test_agent_with_role_controlled_marks_first_pair_as_nominated_ok() {
        const EXPECTED_ERROR_MSG1: &str = "Must simulate nomination reception in Controlled mode";
        const EXPECTED_ERROR_MSG2: &str = "The first pair should be marked as nominated";
        let mut agent = IceAgent::new(IceRole::Controlled);
        let p = mock_pair_with_states(CandidatePairState::Succeeded);
        agent.candidate_pairs = vec![p];

        agent.run_role_logic();

        assert!(
            agent.nominated_pair.is_some(),
            "{EXPECTED_ERROR_MSG1}"
        );
        assert!(
            agent.candidate_pairs[0].is_nominated,
            "{EXPECTED_ERROR_MSG2}"
        );
    }

    #[test]
    fn test_nominate_valid_pair_with_highest_priority_ok() {
        let mut agent = IceAgent::new(IceRole::Controlling);

        let mut p1 = mock_pair_with_state(CandidatePairState::Succeeded);
        p1.priority = 50;

        let mut p2 = mock_pair_with_state(CandidatePairState::Succeeded);
        p2.priority = 100;

        agent.candidate_pairs = vec![p1, p2];

        let selected = agent.select_valid_pair();

        assert!(selected.is_some(), "Debe seleccionar un par válido");
        let pair = selected.unwrap();

        assert!(pair.is_nominated, "El par elegido debe marcarse como nominado");
        assert_eq!(pair.priority, 100, "Debe elegir el par con mayor prioridad");
    }

    #[test]
    fn test_empty_valid_pair_returns_none_ok() {
        let mut agent = IceAgent::new(IceRole::Controlling);

        let result = agent.select_valid_pair();

        assert!(result.is_none(), "No debe seleccionar nada si no hay pares.");
        assert!(
            agent.nominated_pair.is_none(),
            "El campo nominated_pair debe permanecer None."
        );
    }

    #[test]
    fn test_all_pairs_failed_returns_none_ok() {
        let mut agent = IceAgent::new(IceRole::Controlling);

        let mut p1 = mock_pair_with_state(CandidatePairState::Failed);
        p1.priority = 100;
        let mut p2 = mock_pair_with_state(CandidatePairState::Failed);
        p2.priority = 200;

        agent.candidate_pairs = vec![p1, p2];

        let result = agent.select_valid_pair();

        assert!(result.is_none(), "No debe seleccionar pares fallidos");
        assert!(
            agent.nominated_pair.is_none(),
            "No debe haberse guardado un par nominado"
        );
    }

    #[test]
    fn test_valid_pair_with_equal_priorities_ok() {
        let mut agent = IceAgent::new(IceRole::Controlling);

        let mut p1 = mock_pair_with_state(CandidatePairState::Succeeded);
        p1.priority = 42;
        let mut p2 = mock_pair_with_state(CandidatePairState::Succeeded);
        p2.priority = 42;

        agent.candidate_pairs = vec![p1, p2];

        let result = agent.select_valid_pair();

        assert!(result.is_some(), "Debe nominar al menos un par válido");
        let nominated = result.unwrap();
        assert!(nominated.is_nominated, "El par nominado debe marcarse como tal");
        assert_eq!(nominated.priority, 42, "Debe respetar la prioridad igual");
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
