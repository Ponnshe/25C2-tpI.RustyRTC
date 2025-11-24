use crate::signaling::auth::AuthError;

/// Trait for pluggable authentication backends.
///
/// For now: username + password check. Later we could add
/// registration, reset flows, etc. to a separate trait if needed.
pub trait AuthBackend: Send + Sync {
    fn verify(&self, username: &str, password: &str) -> Result<(), AuthError>;
}
