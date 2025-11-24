use crate::signaling::auth::{AuthError, RegisterError};

/// Trait for pluggable authentication backends.
///
/// For now: username + password check. Later we could add
/// registration, reset flows, etc. to a separate trait if needed.
pub trait AuthBackend: Send + Sync {
    fn verify(&self, username: &str, password: &str) -> Result<(), AuthError>;
    /// Create a new user account.
    ///
    /// Backends that don't support registration should return
    /// `Err(RegisterError::Unsupported)`.
    fn register(&mut self, _username: &str, _password: &str) -> Result<(), RegisterError> {
        Err(RegisterError::Unsupported)
    }
}
