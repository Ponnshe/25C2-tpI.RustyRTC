use crate::ice::type_ice::candidate::Candidate;
use crate::ice::type_ice::candidate_type::CandidateType;
use std::fmt;
pub struct ICEToSDP {
    candidate: Candidate,
}

impl ICEToSDP {
    pub fn new(candidate: Candidate) -> Self {
        Self { candidate }
    }

    pub fn set_candidate(&mut self, candidate: Candidate) {
        self.candidate = candidate;
    }

    fn get_typ_as_sdp_string(&self) -> String {
        match self.candidate.cand_type {
            CandidateType::Host => "host".to_owned(),
            CandidateType::PeerReflexive => "prflx".to_owned(),
            CandidateType::Relayed => "relay".to_owned(),
            CandidateType::ServerReflexive => "srflx".to_owned(),
        }
    }

    fn get_related_addr_as_sdp_string(&self) -> Option<String> {
        if let Some(s) = self.candidate.related_address {
            return Some(format!("raddr {} rport {}", s.ip(), s.port()));
        }
        None
    }
}

impl fmt::Display for ICEToSDP {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let typ = self.get_typ_as_sdp_string(); // e.g. "host", "srflx"
        let rel = self.get_related_addr_as_sdp_string(); // e.g. Some("raddr 1.2.3.4 rport 5678")

        write!(
            f,
            "{} {} {} {} {} {} typ {}",
            self.candidate.foundation,
            self.candidate.component,
            self.candidate.transport,
            self.candidate.priority,
            self.candidate.address.ip(),
            self.candidate.address.port(),
            typ,
        )?;

        if let Some(s) = rel {
            write!(f, " {}", s)?;
        }

        Ok(())
    }
}
pub struct ICEToSDP {
    candidate: Candidate,
}

impl ICEToSDP {
    pub fn new(candidate: Candidate) -> Self {
        Self { candidate }
    }

    pub fn set_candidate(&mut self, candidate: Candidate) {
        self.candidate = candidate;
    }

    fn get_typ_as_sdp_string(&self) -> String {
        match self.candidate.cand_type {
            CandidateType::Host => "host".to_owned(),
            CandidateType::PeerReflexive => "prflx".to_owned(),
            CandidateType::Relayed => "relay".to_owned(),
            CandidateType::ServerReflexive => "srflx".to_owned(),
        }
    }

    fn get_related_addr_as_sdp_string(&self) -> Option<String> {
        if let Some(s) = self.candidate.related_address {
            return Some(format!("raddr {} rport {}", s.ip(), s.port()));
        }
        None
    }
}

