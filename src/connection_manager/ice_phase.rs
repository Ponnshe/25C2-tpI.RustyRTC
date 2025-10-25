#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcePhase {
    Idle,
    Gathering,
    Checking,
    Nominated,
}
