use std::collections::{HashMap, HashSet};

use crate::signaling::protocol::{SessionCode, SessionId};
use crate::signaling::types::ClientId;

#[derive(Debug)]
pub struct Session {
    pub session_id: SessionId,
    pub session_code: SessionCode,
    pub capacity: u8,
    pub members: HashSet<ClientId>,
}

#[derive(Debug)]
pub enum JoinError {
    NotFound,
    Full,
}

#[derive(Debug, Default)]
pub struct Sessions {
    by_sess_id: HashMap<SessionId, Session>,
    by_sess_code: HashMap<SessionCode, SessionId>,
}

impl Sessions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new session that was created by the server.
    pub fn insert(&mut self, session: Session) {
        let session_id_key = session.session_id.clone();
        let session_code_key = session.session_code.clone();

        self.by_sess_code
            .insert(session_code_key, session_id_key.clone());
        self.by_sess_id.insert(session_id_key, session);
    }

    #[must_use]
    pub fn get(&self, session_id: &SessionId) -> Option<&Session> {
        self.by_sess_id.get(session_id)
    }

    pub fn get_mut(&mut self, session_id: &SessionId) -> Option<&mut Session> {
        self.by_sess_id.get_mut(session_id)
    }

    /// Find session by code and add a member.
    ///
    /// # Errors
    ///
    /// - Returns `JoinError::NotFound` if the session code does not correspond to an existing session.
    /// - Returns `JoinError::Full` if the session has already reached its member capacity.
    ///
    /// # Panics
    ///
    /// Panics if the internal state is inconsistent (i.e., a `session_code` points to a
    /// `session_id` that does not exist). This is considered a critical bug.
    #[allow(clippy::expect_used)]
    pub fn join_by_code(
        &mut self,
        session_code: &SessionCode,
        client_id: ClientId,
    ) -> Result<SessionId, JoinError> {
        let session_id = self
            .by_sess_code
            .get(session_code)
            .cloned()
            .ok_or(JoinError::NotFound)?;

        let session = self
            .by_sess_id
            .get_mut(&session_id)
            .expect("Internal state: session_code points to a non-existent session_id");

        if session.members.len() >= session.capacity as usize {
            return Err(JoinError::Full);
        }

        session.members.insert(client_id);
        Ok(session_id)
    }

    /// Remove `client_id` from all sessions.
    ///
    /// Returns a list of `(session_id, remaining_members)` for each session
    /// that the client was part of *before* removal.
    pub fn leave_all(&mut self, client_id: ClientId) -> Vec<(SessionId, Vec<ClientId>)> {
        let session_ids: Vec<SessionId> = self
            .by_sess_id
            .iter()
            .filter_map(|(sess_id, session)| {
                if session.members.contains(&client_id) {
                    Some(sess_id.clone())
                } else {
                    None
                }
            })
            .collect();

        let mut result = Vec::new();

        for sess_id in &session_ids {
            if let Some(sess) = self.by_sess_id.get_mut(sess_id) {
                sess.members.remove(&client_id);
                let remaining: Vec<ClientId> = sess.members.iter().copied().collect();
                result.push((sess_id.clone(), remaining));
            }
        }

        // remove empty sessions
        self.by_sess_id.retain(|_, s| !s.members.is_empty());
        self.by_sess_code
            .retain(|_, sess_id| self.by_sess_id.contains_key(sess_id));

        result
    }

    /// Return true if both clients are members of at least one common session.
    #[must_use]
    pub fn share_session(&self, a: ClientId, b: ClientId) -> bool {
        // Scan all sessions and check membership.
        // For expected small #sessions this is totally fine.
        self.by_sess_id
            .values()
            .any(|sess| sess.members.contains(&a) && sess.members.contains(&b))
    }

    /// Returns true if a session with this code already exists.
    #[must_use]
    pub fn contains_code(&self, code: &SessionCode) -> bool {
        self.by_sess_code.contains_key(code)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn mk_session(
        session_id: &str,
        session_code: &str,
        capacity: u8,
        members: &[ClientId],
    ) -> Session {
        let mut set = HashSet::new();
        for &m in members {
            set.insert(m);
        }
        Session {
            session_id: session_id.to_string(),
            session_code: session_code.to_string(),
            capacity,
            members: set,
        }
    }

    #[test]
    fn share_session_false_when_no_sessions() {
        let sessions = Sessions::new();
        assert!(!sessions.share_session(1, 2));
    }

    #[test]
    fn share_session_true_when_same_session() {
        let mut sessions = Sessions::new();

        let sess = mk_session("sess-1", "ABC123", 4, &[1, 2]);
        sessions.insert(sess);

        assert!(sessions.share_session(1, 2));
        assert!(sessions.share_session(2, 1));
        // same client trivially shares a session with itself
        assert!(sessions.share_session(1, 1));
    }

    #[test]
    fn share_session_false_when_only_in_different_sessions() {
        let mut sessions = Sessions::new();

        let s1 = mk_session("sess-1", "AAA111", 4, &[1, 3]);
        let s2 = mk_session("sess-2", "BBB222", 4, &[2, 4]);

        sessions.insert(s1);
        sessions.insert(s2);

        assert!(!sessions.share_session(1, 2));
        assert!(!sessions.share_session(3, 4));

        assert!(sessions.share_session(1, 3));
        assert!(sessions.share_session(2, 4));
    }
}
