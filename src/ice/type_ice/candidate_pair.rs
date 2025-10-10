use crate::ice::type_ice::candidate::Candidate;

#[derive(Debug, Clone)]
pub struct CandidatePair {
    pub local: Candidate,
    pub remote: Candidate,
    pub priority: u64,
}

impl CandidatePair {
    pub fn new(local: Candidate, remote: Candidate, priority: u64) -> Self {
        CandidatePair {
            local,
            remote,
            priority,
        }
    }
}
