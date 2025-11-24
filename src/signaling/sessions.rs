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
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new session that was created by the server.
    pub fn insert(&mut self, session: Session) {
        let session_id = session.session_id.clone();
        let session_code = session.session_code.clone();
        self.by_sess_code.insert(session_code, session_id.clone());
        self.by_sess_id.insert(session_id, session);
    }

    pub fn get(&self, session_id: &SessionId) -> Option<&Session> {
        self.by_sess_id.get(session_id)
    }

    pub fn get_mut(&mut self, session_id: &SessionId) -> Option<&mut Session> {
        self.by_sess_id.get_mut(session_id)
    }

    /// Find session by code and add a member.
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
            .expect("consistent maps");

        if session.members.len() >= session.capacity as usize {
            return Err(JoinError::Full);
        }

        session.members.insert(client_id);
        Ok(session_id)
    }

    pub fn leave_all(&mut self, client_id: ClientId) {
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

        for sess_id in session_ids {
            if let Some(sess) = self.by_sess_id.get_mut(&sess_id) {
                sess.members.remove(&client_id);
            }
        }

        // remove empty sessions
        self.by_sess_id.retain(|_, s| !s.members.is_empty());
        self.by_sess_code
            .retain(|_, sess_id| self.by_sess_id.contains_key(sess_id));
    }

    /// Return true if both clients are members of at least one common session.
    pub fn share_session(&self, a: ClientId, b: ClientId) -> bool {
        // Scan all sessions and check membership.
        // For expected small #sessions this is totally fine.
        self.by_sess_id
            .values()
            .any(|sess| sess.members.contains(&a) && sess.members.contains(&b))
    }
}
