use std::collections::HashMap;

use crate::signaling::protocol::UserName;
use crate::signaling::types::ClientId;

/// Tracks which clients are logged in as which users.
#[derive(Debug, Default)]
pub struct Presence {
    user_to_client: HashMap<UserName, ClientId>,
    client_to_user: HashMap<ClientId, UserName>,
}

impl Presence {
    pub fn new() -> Self {
        Self::default()
    }

    /// Log in a user on a given client.
    ///
    /// Returns:
    /// - Some(previous_client) if this user was already logged in somewhere else.
    /// - None if user was not previously logged in.
    pub fn login(&mut self, client_id: ClientId, username: UserName) -> Option<ClientId> {
        let old_client = self.user_to_client.insert(username.clone(), client_id);
        self.client_to_user.insert(client_id, username);
        old_client
    }
    /// Remove client from presence; returns the username if any.
    pub fn logout(&mut self, client_id: ClientId) -> Option<UserName> {
        if let Some(username) = self.client_to_user.remove(&client_id) {
            self.user_to_client.remove(&username);
            Some(username)
        } else {
            None
        }
    }

    /// Get the currently logged-in client for a username.
    pub fn client_id_for(&self, username: &UserName) -> Option<ClientId> {
        self.user_to_client.get(username).cloned()
    }

    /// Get username for a client, if logged in.
    pub fn username_for(&self, client: ClientId) -> Option<&UserName> {
        self.client_to_user.get(&client)
    }

    /// Return all usernames currently online.
    pub fn online_usernames(&self) -> Vec<UserName> {
        self.user_to_client.keys().cloned().collect()
    }
    /// Return all client IDs currently logged in.
    /// This is used to iterate over all clients to broadcast updates.
    pub fn all_client_ids(&self) -> Vec<ClientId> {
        self.client_to_user.keys().copied().collect()
    }
}
