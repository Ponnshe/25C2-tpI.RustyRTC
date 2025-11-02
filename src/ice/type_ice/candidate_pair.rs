use std::sync::Arc;

use super::ice_agent::IceRole;
use crate::{app::log_sink::LogSink, ice::type_ice::candidate::Candidate, sink_debug};

/// Constants used in the pair priority formula (RFC 8445 §6.1.2.3)
// 2^32 multiplier
const PRIORITY_BITS_SHIFT: u64 = 32;
// 2^32 multiplier
const PRIORITY_DOUBLE_MULTIPLIER: u64 = 2;
// added if G > D
const TIE_BREAK_FLAG_ONE: u64 = 1;
const TIE_BREAK_FLAG_ZERO: u64 = 0;

/// Represents the ICE connectivity check state for a CandidatePair.
/// (RFC 8445 §6.1.2.5)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidatePairState {
    /// The pair has been created but not yet checked.
    Waiting,
    /// A connectivity check is currently in progress.
    InProgress,
    /// A successful connectivity check has completed.
    Succeeded,
    /// The connectivity check failed (timeout or no response).
    Failed,
}

/// Contains a pair, local and remote Candidate.
/// Also a priority, to sort candidates.
#[derive(Debug)]
pub struct CandidatePair {
    pub local: Candidate,
    pub remote: Candidate,
    pub priority: u64,
    pub state: CandidatePairState,
    pub is_nominated: bool,
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
            //Default state waiting, by RFC 8445 §6.1.2.5
            state: CandidatePairState::Waiting,
            is_nominated: false,
        }
    }

    /// Lightweight clone: copies metadata but drops socket references.
    pub fn clone_light(&self) -> Self {
        CandidatePair {
            local: self.local.clone_light(),
            remote: self.remote.clone_light(),
            priority: self.priority,
            state: self.state.clone(),
            is_nominated: self.is_nominated,
        }
    }

    /// Calculates the priority for a candidate pair according to RFC 8445 §6.1.2.3.
    ///
    /// # Arguments
    /// * `local` - Local candidate.
    /// * `remote` - Remote candidate.
    /// * `role` - ICE role of the current agent (Controlling or Controlled).
    ///
    /// # Returns
    /// The priority for this candidate pair.
    pub fn calculate_pair_priority(local: &Candidate, remote: &Candidate, role: &IceRole) -> u64 {
        // Determine which candidate's priority is G (controlling) and which is D (controlled)
        let (g, d) = match role {
            IceRole::Controlling => (local.priority as u64, remote.priority as u64),
            IceRole::Controlled => (remote.priority as u64, local.priority as u64),
        };

        let min_val = g.min(d);
        let max_val = g.max(d);
        let tie_break = if g > d {
            TIE_BREAK_FLAG_ONE
        } else {
            TIE_BREAK_FLAG_ZERO
        };

        (1u64 << PRIORITY_BITS_SHIFT) * min_val + PRIORITY_DOUBLE_MULTIPLIER * max_val + tie_break
    }

    /// Updates the current pair state.
    pub fn set_state(&mut self, new_state: CandidatePairState) {
        self.state = new_state;
    }

    /// Prints a detailed summary of the candidate pair state.
    /// Useful for debugging and local ICE connectivity visualization.
    pub fn debug_state(&self, logger: &Arc<dyn LogSink>) {
        sink_debug!(
            logger,
            "[PAIR] local={}, remote={}, priority={}, state={:?}, nominated={}",
            self.local.address,
            self.remote.address,
            self.priority,
            self.state,
            if self.is_nominated { "true" } else { "false" }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ice::type_ice::candidate_type::CandidateType;
    use crate::ice::type_ice::ice_agent::IceRole;
    use std::net::SocketAddr;

    fn mock_candidate(priority: u32) -> Candidate {
        let address = "192.168.0.1:5000";
        let protocol = "udp";
        let foundation = "fnd1";
        let addr: SocketAddr = address.parse().unwrap();

        Candidate::new(
            String::from(foundation),
            1,
            protocol,
            priority,
            addr,
            CandidateType::Host,
            None,
            None,
        )
    }

    #[test]
    fn test_calculate_pair_priority_mode_controlling_ok() {
        let priority_local = 100;
        let priority_remote = 50;
        let local = mock_candidate(priority_local);
        let remote = mock_candidate(priority_remote);
        let prio = CandidatePair::calculate_pair_priority(&local, &remote, &IceRole::Controlling);
        assert!(prio > 0);
    }

    #[test]
    fn test_calculate_pair_priority_mode_controlled_ok() {
        let priority_local = 100;
        let priority_remote = 50;
        let local = mock_candidate(priority_local);
        let remote = mock_candidate(priority_remote);
        let prio = CandidatePair::calculate_pair_priority(&local, &remote, &IceRole::Controlled);
        assert!(prio > 0);
    }

    #[test]
    fn test_difference_between_priorities_is_minimum_ok() {
        const EXPECTED_ERROR_MSG: &str = "The difference between priorities should be at most 1";
        let priority_local = 100;
        let priority_remote = 50;
        let local = mock_candidate(priority_local);
        let remote = mock_candidate(priority_remote);

        let prio_controlling =
            CandidatePair::calculate_pair_priority(&local, &remote, &IceRole::Controlling);
        let prio_controlled =
            CandidatePair::calculate_pair_priority(&local, &remote, &IceRole::Controlled);

        assert!(
            (prio_controlling as i128 - prio_controlled as i128).abs() <= 1,
            "{EXPECTED_ERROR_MSG}"
        );
    }

    #[test]
    fn test_calculate_pair_priority_max_values_ok() {
        const EXPECTED_ERROR_MSG1: &str =
            "The pair priority must be positive even with extreme values";
        const EXPECTED_ERROR_MSG2: &str =
            "The calculated priority should not exceed the range of u64";
        let local = mock_candidate(u32::MAX);
        let remote = mock_candidate(u32::MAX - 1);

        let prio = CandidatePair::calculate_pair_priority(&local, &remote, &IceRole::Controlling);
        assert!(prio > 0, "{EXPECTED_ERROR_MSG1}");
        assert!(prio <= u64::MAX, "{EXPECTED_ERROR_MSG2}");
    }

    #[test]
    fn test_initialize_candidate_pair_in_waiting_state_ok() {
        let local = mock_candidate(100);
        let remote = mock_candidate(120);
        let pair = CandidatePair::new(local, remote, 12345);
        assert_eq!(pair.state, CandidatePairState::Waiting);
    }

    #[test]
    fn test_updates_state_in_candidate_pair_ok() {
        let local = mock_candidate(100);
        let remote = mock_candidate(120);
        let mut pair = CandidatePair::new(local, remote, 12345);

        pair.set_state(CandidatePairState::InProgress);
        assert_eq!(pair.state, CandidatePairState::InProgress);

        pair.set_state(CandidatePairState::Succeeded);
        assert_eq!(pair.state, CandidatePairState::Succeeded);
    }
}
