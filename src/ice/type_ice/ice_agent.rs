use super::candidate::Candidate;
use super::candidate_pair::CandidatePair;
use super::candidate_type::CandidateType;
use std::io::Error;
use std::net::SocketAddr;
use crate::ice::gathering_service::gather_host_candidates;

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
        IceAgent {
            local_candidates: Vec::new(),
            remote_candidates: Vec::new(),
            candidate_pairs: Vec::new(),
            role,
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

    /// Pair local and remote candidates to initiate nominations.
    pub fn form_candidate_pairs(&mut self) {
        todo!()
    }

    /// Runs connectivity checks between candidate pairs.
    /// Selects the best candidate pair.
    pub async fn run_connectivity_checks(&mut self) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

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
