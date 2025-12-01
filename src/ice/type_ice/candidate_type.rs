/// Represent different types of candidates
/// for example: type host(for local conecctions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateType {
    /// Host candidate: directly reachable by IP address.
    Host,
    /// Server reflexive candidate: discovered by a STUN server.
    ServerReflexive,
    /// Peer reflexive candidate: discovered by a peer.
    PeerReflexive,
    /// Relayed candidate: obtained from a TURN server.
    Relayed,
}
