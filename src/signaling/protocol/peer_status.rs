#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerStatus {
    Available,
    Busy, // On a call
}
