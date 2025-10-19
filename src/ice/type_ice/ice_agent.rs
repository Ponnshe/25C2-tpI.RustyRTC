use super::candidate::Candidate;
use super::candidate_pair::CandidatePair;
use crate::ice::gathering_service::gather_host_candidates;
use rand::{Rng, rngs::OsRng};
use std::io::Error;

/// Error message formatting constants
const ERROR_MSG: &str = "ERROR";
const WHITESPACE: &str = " ";
const QUOTE: &str = "\"";

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

    /// Runs connectivity checks between candidate pairs.
    /// Selects the best candidate pair.
    pub async fn run_connectivity_checks(&mut self) {
        todo!()
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
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::ice::type_ice::candidate_type::CandidateType;
    use std::net::SocketAddr;

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
