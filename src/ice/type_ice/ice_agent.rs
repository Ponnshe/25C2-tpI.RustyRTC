use super::candidate::Candidate;
use super::candidate_pair::CandidatePair;
use super::candidate_type::CandidateType;
use std::io::Error;
use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IceRole {
    Controlling,
    Controlled,
}

#[derive(Debug)]
pub struct IceAgent {
    pub local_candidates: Vec<Candidate>,
    pub remote_candidates: Vec<Candidate>,
    pub candidate_pairs: Vec<CandidatePair>,
    pub role: IceRole,
}

impl IceAgent {
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

    /// Recolecta candidatos locales, esta funcion sera asincrona en una implementacion futura.
    pub fn gather_candidates(&mut self) -> Result<Vec<Candidate>, Error> {
        todo!()
    }

    /// Forma pares de candidatos locales y remotos para iniciar las verificaciones.
    pub fn form_candidate_pairs(&mut self) {
        todo!()
    }

    /// Ejecuta las verificaciones de conectividad entre los pares de candidatos.
    /// Selecciona el mejor par de candidatos al finalizar.
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
