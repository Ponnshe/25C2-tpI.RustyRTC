/// Represent different types of candidates
/// for example: type host(for local conecctions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateType {
    Host,
    ServerReflexive,
    PeerReflexive,
    Relayed,
}
