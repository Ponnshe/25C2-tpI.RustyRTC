use std::collections::HashSet;
use std::sync::Arc;

use crate::app::log_sink::{LogSink, NoopLogSink};
use crate::signaling::errors::{JoinErrorCode, LoginErrorCode};
use crate::signaling::presence::Presence;
use crate::signaling::protocol::{Msg, SessionCode, SessionId, UserName};
use crate::signaling::sessions::{JoinError, Session, Sessions};
use crate::signaling::types::{ClientId, OutgoingMsg};
use crate::{sink_debug, sink_info, sink_warn};

pub struct Server {
    presence: Presence,
    sessions: Sessions,
    // Simple counters for IDs; we might use UUIDs or random codes in the future.
    next_session_id: u64,
    log: Arc<dyn LogSink>,
}

impl Server {
    pub fn new() -> Self {
        Self::with_log(Arc::new(NoopLogSink))
    }

    pub fn with_log(log: Arc<dyn LogSink>) -> Self {
        Self {
            presence: Presence::new(),
            sessions: Sessions::new(),
            next_session_id: 1,
            log,
        }
    }

    /// Returns Some(username) if client is logged in, None otherwise.
    fn require_logged_in(&self, client_id: ClientId) -> Option<UserName> {
        self.presence.username_for(client_id).cloned()
    }

    fn alloc_session_id(&mut self) -> SessionId {
        let id = format!("sess-{}", self.next_session_id);
        self.next_session_id += 1;
        id
    }

    fn alloc_session_code(&mut self) -> SessionCode {
        // Super naive: 6-digit code; replace with better generator later.
        format!("{:06}", self.next_session_id - 1)
    }

    /// Main entrypoint: handle a message from a client.
    ///
    /// Returns a list of (target_client, Msg) to send.
    pub fn handle(&mut self, from_cid: ClientId, msg: Msg) -> Vec<OutgoingMsg> {
        match msg {
            Msg::Hello { client_version } => {
                // For now: ignore, or maybe log. No reply required.
                sink_debug!(
                    self.log,
                    "client {} HELLO (version {})",
                    from_cid,
                    client_version
                );
                Vec::new()
            }

            Msg::Login {
                username,
                password: _,
            } => self.handle_login(from_cid, username),

            Msg::CreateSession { capacity } => self.handle_create_session(from_cid, capacity),

            Msg::Join { session_code } => self.handle_join(from_cid, session_code),

            Msg::Offer { .. } | Msg::Answer { .. } | Msg::Candidate { .. } => {
                self.forward_signaling(from_cid, msg)
            }

            Msg::Ack { txn_id } => {
                if self.require_logged_in(from_cid).is_none() {
                    sink_warn!(
                        self.log,
                        "unauthenticated client {} sent Ack({})",
                        from_cid,
                        txn_id
                    );
                    Vec::new()
                } else {
                    self.handle_ack(from_cid, txn_id)
                }
            }

            Msg::Bye { reason } => {
                if self.require_logged_in(from_cid).is_none() {
                    sink_warn!(
                        self.log,
                        "unauthenticated client {} sent Bye({:?})",
                        from_cid,
                        reason
                    );
                    Vec::new()
                } else {
                    self.handle_bye(from_cid, reason)
                }
            }
            Msg::LoginOk { .. }
            | Msg::LoginErr { .. }
            | Msg::Created { .. }
            | Msg::JoinOk { .. }
            | Msg::JoinErr { .. }
            | Msg::Ping { .. }
            | Msg::Pong { .. } => {
                sink_warn!(
                    self.log,
                    "ignoring server-only msg from client {}: {:?}",
                    from_cid,
                    msg
                );
                Vec::new()
            }
        }
    }

    /// Called when a TCP connection closes, to clean up state.
    pub fn handle_disconnect(&mut self, client: ClientId) -> Vec<OutgoingMsg> {
        let mut out = Vec::new();

        // Remove from presence
        if let Some(username) = self.presence.logout(client) {
            sink_info!(self.log, "client {} ({}) disconnected", client, username);
        }

        // Remove from any sessions
        self.sessions.leave_all(client);

        // Optionally: notify other clients in sessions that this peer left.
        // (left as TODO / future improvement)

        out
    }

    // ---- Individual handlers ---------------------------------------------

    fn handle_login(&mut self, client: ClientId, username: UserName) -> Vec<OutgoingMsg> {
        // TODO: real auth. For now accept everyone, but reject if already logged in somewhere else.
        let already = self.presence.client_id_for(&username);

        if let Some(_existing_client) = already {
            // user already logged in
            let resp = Msg::LoginErr {
                code: LoginErrorCode::AlreadyLoggedIn.as_u16(),
            };
            return vec![OutgoingMsg {
                client_id_target: client,
                msg: resp,
            }];
        }

        let _ = self.presence.login(client, username.clone());
        let resp = Msg::LoginOk { username };
        vec![OutgoingMsg {
            client_id_target: client,
            msg: resp,
        }]
    }

    fn handle_create_session(&mut self, client_id: ClientId, capacity: u8) -> Vec<OutgoingMsg> {
        let mut out_msg = Vec::new();

        // Require login first
        let Some(username) = self.require_logged_in(client_id) else {
            let msg = Msg::JoinErr {
                code: JoinErrorCode::NotLoggedIn.as_u16(),
            };
            sink_warn!(
                self.log,
                "client {} attempted CreateSession without login",
                client_id
            );
            out_msg.push(OutgoingMsg {
                client_id_target: client_id,
                msg,
            });
            return out_msg;
        };

        let id = self.alloc_session_id();
        let code = self.alloc_session_code();

        let mut members = HashSet::new();
        members.insert(client_id);

        let session = Session {
            session_id: id.clone(),
            session_code: code.clone(),
            capacity,
            members,
        };

        self.sessions.insert(session);

        sink_info!(
            self.log,
            "client {} ({}) created session id={} code={} capacity={}",
            client_id,
            username,
            id,
            code,
            capacity
        );

        let msg = Msg::Created {
            session_id: id,
            session_code: code,
        };
        out_msg.push(OutgoingMsg {
            client_id_target: client_id,
            msg,
        });
        out_msg
    }

    fn handle_join(&mut self, client_id: ClientId, session_code: SessionCode) -> Vec<OutgoingMsg> {
        let mut out_msg = Vec::new();

        let Some(username) = self.require_logged_in(client_id) else {
            let msg = Msg::JoinErr {
                code: JoinErrorCode::NotLoggedIn.as_u16(),
            };
            sink_warn!(
                self.log,
                "client {} attempted Join(code={}) without being logged in",
                client_id,
                session_code
            );
            out_msg.push(OutgoingMsg {
                client_id_target: client_id,
                msg,
            });
            return out_msg;
        };

        match self.sessions.join_by_code(&session_code, client_id) {
            Ok(session_id) => {
                sink_info!(
                    self.log,
                    "client {} ({}) joined session id={} code={}",
                    client_id,
                    username,
                    session_id,
                    session_code
                );
                let msg = Msg::JoinOk { session_id };
                out_msg.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg,
                });
            }
            Err(JoinError::NotFound) => {
                sink_warn!(
                    self.log,
                    "client {} ({}) tried to join unknown session code={}",
                    client_id,
                    username,
                    session_code
                );
                let msg = Msg::JoinErr {
                    code: JoinErrorCode::NotFound.as_u16(),
                };
                out_msg.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg,
                });
            }
            Err(JoinError::Full) => {
                sink_warn!(
                    self.log,
                    "client {} ({}) tried to join full session code={}",
                    client_id,
                    username,
                    session_code
                );
                let msg = Msg::JoinErr {
                    code: JoinErrorCode::Full.as_u16(),
                };
                out_msg.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg,
                });
            }
        }
        out_msg
    }

    /// Forward Offer/Answer/Candidate, enforcing:
    /// - sender must be logged in
    /// - target must be logged in
    /// - both must share at least one session
    ///
    /// On violation: log a warning and drop the message.
    fn forward_signaling(&mut self, from: ClientId, msg: Msg) -> Vec<OutgoingMsg> {
        // Extract `to` username and a short kind name for logging.
        let (to_username, kind) = match &msg {
            Msg::Offer { to, .. } => (to.as_str(), "Offer"),
            Msg::Answer { to, .. } => (to.as_str(), "Answer"),
            Msg::Candidate { to, .. } => (to.as_str(), "Candidate"),
            _ => unreachable!("forward_signaling only for Offer/Answer/Candidate"),
        };

        // 1) sender must be logged in
        let Some(from_username) = self.require_logged_in(from) else {
            sink_warn!(
                self.log,
                "unauthenticated client {} attempted to send {} to {}",
                from,
                kind,
                to_username
            );
            return Vec::new();
        };

        // 2) resolve target client by username
        let Some(target_client) = self.presence.client_id_for(&to_username.to_owned()) else {
            sink_warn!(
                self.log,
                "client {} ({}) tried to send {} to offline user {}",
                from,
                from_username,
                kind,
                to_username
            );
            return Vec::new();
        };

        // 3) enforce they share at least one session
        if !self.sessions.share_session(from, target_client) {
            sink_warn!(
                self.log,
                "client {} ({}) tried to send {} to {} (no shared session)",
                from,
                from_username,
                kind,
                to_username
            );
            return Vec::new();
        }

        sink_debug!(
            self.log,
            "forwarding {} from client {} ({}) to client {} ({})",
            kind,
            from,
            from_username,
            target_client,
            to_username
        );

        vec![OutgoingMsg {
            client_id_target: target_client,
            msg,
        }]
    }

    fn handle_ack(&mut self, _from: ClientId, _txn_id: u64) -> Vec<OutgoingMsg> {
        // For now, ignore; when we add offer/answer reliability, track pending txns here.
        Vec::new()
    }

    fn handle_bye(&mut self, from: ClientId, reason: Option<String>) -> Vec<OutgoingMsg> {
        // You might want to remove them from sessions here or just rely on disconnect.
        sink_info!(self.log, "client {} BYE {:?}", from, reason);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signaling::protocol::Msg;

    fn new_server() -> Server {
        Server::with_log(Arc::new(NoopLogSink))
    }

    fn login(server: &mut Server, client_id: ClientId, username: &str) {
        let out = server.handle(
            client_id,
            Msg::Login {
                username: username.to_string(),
                password: "pw".to_string(),
            },
        );

        // We expect a LoginOk back to that client.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].client_id_target, client_id);
        match &out[0].msg {
            Msg::LoginOk { username: u } => assert_eq!(u, username),
            other => panic!("expected LoginOk, got {:?}", other),
        }
    }

    #[test]
    fn login_and_create_session_roundtrip() {
        let mut server = Server::new();
        let client1 = 1;

        // client logs in
        let outs = server.handle(
            client1,
            Msg::Login {
                username: "alice".into(),
                password: "pw".into(),
            },
        );

        assert_eq!(outs.len(), 1);
        match &outs[0].msg {
            Msg::LoginOk { username } => assert_eq!(username, "alice"),
            other => panic!("expected LoginOk, got {:?}", other),
        }

        // client creates session
        let outs2 = server.handle(client1, Msg::CreateSession { capacity: 2 });
        assert_eq!(outs2.len(), 1);
        match &outs2[0].msg {
            Msg::Created {
                session_id,
                session_code,
            } => {
                assert!(session_id.starts_with("sess-"));
                assert_eq!(session_code.len(), 6);
            }
            other => panic!("expected Created, got {:?}", other),
        }
    }

    #[test]
    fn offer_from_unauthenticated_client_is_dropped() {
        let mut server = new_server();

        let res = server.handle(
            1,
            Msg::Offer {
                txn_id: 1,
                to: "bob".to_string(),
                sdp: b"v=0".to_vec(),
            },
        );

        assert!(
            res.is_empty(),
            "expected no outgoing messages for unauthenticated Offer, got {:?}",
            res
        );
    }

    #[test]
    fn offer_to_offline_user_is_dropped() {
        let mut server = new_server();

        // Only alice logs in
        login(&mut server, 1, "alice");

        // alice sends Offer to bob, who is not logged in
        let res = server.handle(
            1,
            Msg::Offer {
                txn_id: 1,
                to: "bob".to_string(),
                sdp: b"v=0".to_vec(),
            },
        );

        assert!(
            res.is_empty(),
            "expected no outgoing messages when target user is offline, got {:?}",
            res
        );
    }

    #[test]
    fn offer_without_shared_session_is_dropped() {
        let mut server = new_server();

        // alice and bob both logged in, but in no sessions yet
        login(&mut server, 1, "alice");
        login(&mut server, 2, "bob");

        let res = server.handle(
            1,
            Msg::Offer {
                txn_id: 1,
                to: "bob".to_string(),
                sdp: b"v=0".to_vec(),
            },
        );

        assert!(
            res.is_empty(),
            "expected no outgoing messages when peers share no session, got {:?}",
            res
        );
    }

    #[test]
    fn offer_with_shared_session_is_forwarded() {
        let mut server = new_server();

        let alice: ClientId = 1;
        let bob: ClientId = 2;

        // 1) both log in
        login(&mut server, alice, "alice");
        login(&mut server, bob, "bob");

        // 2) alice creates a session
        let created = server.handle(alice, Msg::CreateSession { capacity: 2 });

        assert_eq!(created.len(), 1);
        assert_eq!(created[0].client_id_target, alice);

        let session_code = match &created[0].msg {
            Msg::Created {
                session_id: _,
                session_code,
            } => session_code.clone(),
            other => panic!("expected Created, got {:?}", other),
        };

        // 3) bob joins that session
        let joined = server.handle(
            bob,
            Msg::Join {
                session_code: session_code.clone(),
            },
        );

        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].client_id_target, bob);
        match &joined[0].msg {
            Msg::JoinOk { session_id: _ } => {}
            other => panic!("expected JoinOk, got {:?}", other),
        }

        // 4) now alice sends an Offer to bob; should be forwarded
        let txn_id = 42;
        let sdp = b"v=0".to_vec();

        let res = server.handle(
            alice,
            Msg::Offer {
                txn_id,
                to: "bob".to_string(),
                sdp: sdp.clone(),
            },
        );

        assert_eq!(
            res.len(),
            1,
            "expected one outgoing Offer message, got {:?}",
            res
        );

        let out = &res[0];
        assert_eq!(out.client_id_target, bob);

        match &out.msg {
            Msg::Offer {
                txn_id: t,
                to,
                sdp: s,
            } => {
                assert_eq!(*t, txn_id);
                assert_eq!(to, "bob");
                assert_eq!(s, &sdp);
            }
            other => panic!("expected forwarded Offer, got {:?}", other),
        }
    }
}
