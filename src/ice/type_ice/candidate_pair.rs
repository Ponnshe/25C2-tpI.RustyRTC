use crate::ice::type_ice::candidate::Candidate;

/// Contains a pair, local and remote Candidate.
/// Also a priority, to sort candidates.
#[derive(Debug)]
pub struct CandidatePair {
    pub local: Candidate,
    pub remote: Candidate,
    pub priority: u64,
}

/// Create a pair of candidates.
///
/// # Arguments
/// * `local` - local candidate.
/// * `remote` - remote candidate.
/// * `priority` - number for sort pair of candidates.
///
/// # Return
/// A new candidates pair.
impl CandidatePair {
    pub fn new(local: Candidate, remote: Candidate, priority: u64) -> Self {
        CandidatePair {
            local,
            remote,
            priority,
        }
    }
}
