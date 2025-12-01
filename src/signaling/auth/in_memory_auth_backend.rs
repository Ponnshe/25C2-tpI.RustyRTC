use std::collections::HashMap;

use crate::signaling::{
    auth::{AuthBackend, AuthError, RegisterError},
    protocol::UserName,
};

/// Simple in-memory auth backend: username → password (plain text for now).
///
/// We can use this in dedicated tests or in a future “dev mode” server.
#[derive(Debug, Default)]
pub struct InMemoryAuthBackend {
    users: HashMap<UserName, String>,
}

impl InMemoryAuthBackend {
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
        }
    }

    /// Convenient builder-style helper.
    pub fn with_user(mut self, username: impl Into<UserName>, password: impl Into<String>) -> Self {
        self.users.insert(username.into(), password.into());
        self
    }
}

impl AuthBackend for InMemoryAuthBackend {
    fn verify(&self, username: &str, password: &str) -> Result<(), AuthError> {
        match self.users.get(username) {
            Some(stored) if stored == password => Ok(()),
            _ => Err(AuthError::InvalidCredentials),
        }
    }
    fn register(&mut self, username: &str, password: &str) -> Result<(), RegisterError> {
        if self.users.contains_key(username) {
            return Err(RegisterError::UsernameTaken);
        }

        // Here we could enforce rules:
        // - min length
        // - character set for username
        // For now we'll accept any non-empty username/password.
        if username.is_empty() {
            return Err(RegisterError::InvalidUsername);
        }
        if password.is_empty() {
            return Err(RegisterError::WeakPassword);
        }

        self.users.insert(username.to_owned(), password.to_owned());
        Ok(())
    }
}

/// Dev / test backend that accepts any username/password.
/// This keeps all your existing tests working with zero changes.
#[derive(Debug, Default)]
pub struct AllowAllAuthBackend;

impl AuthBackend for AllowAllAuthBackend {
    fn verify(&self, _username: &str, _password: &str) -> Result<(), AuthError> {
        Ok(())
    }

    fn register(&mut self, _username: &str, _password: &str) -> Result<(), RegisterError> {
        // For dev/test: pretend registration always works.
        Ok(())
    }
}
