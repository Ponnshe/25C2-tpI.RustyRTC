/// Represents the current phase of the ICE process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcePhase {
    /// No ICE processing has begun.
    Idle,
    /// The ICE agent is gathering local candidates.
    Gathering,
    /// The ICE agent is performing connectivity checks.
    Checking,
    /// A candidate pair has been nominated for use.
    Nominated,
}
