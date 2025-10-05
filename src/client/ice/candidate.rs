use crate::client::ice::candidate_type::CandidateType;
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub foundation: String,
    pub component: u8,
    pub transport: String,
    pub priority: u32,
    pub address: SocketAddr,
    pub cand_type: CandidateType,
    pub related_address: Option<SocketAddr>,
}

//TODO: por ahora se harcodea valores
impl Candidate {
    pub fn new(
        foundation: String,
        component: u8,
        transport: &str,
        priority: u32,
        address: SocketAddr,
        cand_type: CandidateType,
        related_address: Option<SocketAddr>,
    ) -> Self {
        Candidate {
            foundation,
            component: 1,
            transport: "UDP".to_string(),
            priority,
            address,
            cand_type,
            related_address,
        }
    }
}
