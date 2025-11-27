#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterError {
    UsernameTaken,
    InvalidUsername,
    WeakPassword,
    Internal,
    Unsupported, // backend does not support registration
}
