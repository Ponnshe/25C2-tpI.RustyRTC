/// Represents the state of the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// The connection is idle.
    Idle,
    /// The connection is being established.
    Connecting,
    /// The connection is running.
    Running,
    /// The connection is stopped.
    Stopped,
}
