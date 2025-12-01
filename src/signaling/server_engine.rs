use rand::Rng;
use std::collections::HashSet;
use std::sync::Arc;

use crate::log::NoopLogSink;
use crate::log::log_sink::LogSink;
use crate::signaling::AuthError;
use crate::signaling::auth::{AllowAllAuthBackend, AuthBackend};
use crate::signaling::errors::{JoinErrorCode, LoginErrorCode, RegisterErrorCode};
use crate::signaling::presence::Presence;
use crate::signaling::protocol::{SessionCode, SessionId, SignalingMsg, UserName};
use crate::signaling::sessions::{JoinError, Session, Sessions};
use crate::signaling::types::{ClientId, OutgoingMsg};
use crate::{sink_debug, sink_info, sink_trace, sink_warn};

pub struct ServerEngine {
    presence: Presence,
    sessions: Sessions,
    // Simple counters for IDs; we might use UUIDs or random codes in the future.
    next_session_id: u64,
    log: Arc<dyn LogSink>,
    auth: Box<dyn AuthBackend>,
}

impl ServerEngine {
    pub fn new() -> Self {
        Self::with_log_and_auth(Arc::new(NoopLogSink), Box::new(AllowAllAuthBackend))
    }

    /// Server with a custom logger, but still "accept all" auth backend.
    pub fn with_log(log: Arc<dyn LogSink>) -> Self {
        Self::with_log_and_auth(log, Box::new(AllowAllAuthBackend))
    }

    /// Server with a custom auth backend, but Noop logging.
    pub fn with_auth(auth: Box<dyn AuthBackend>) -> Self {
        Self::with_log_and_auth(Arc::new(NoopLogSink), auth)
    }

    /// Fully explicit constructor: custom logger + custom auth backend.
    pub fn with_log_and_auth(log: Arc<dyn LogSink>, auth: Box<dyn AuthBackend>) -> Self {
        Self {
            presence: Presence::new(),
            sessions: Sessions::new(),
            next_session_id: 1,
            log,
            auth,
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
        // 6-digit numeric code, random, collision-checked.
        // Try a bounded number of random attempts, then fall back to a deterministic strategy.
        let mut rng = rand::thread_rng();

        // First, a bunch of random attempts.
        for _ in 0..100 {
            let n: u32 = rng.gen_range(0..1_000_000); // 0..=999_999
            let code = format!("{:06}", n);
            if !self.sessions.contains_code(&code) {
                return code;
            }
        }
        sink_warn!(
            self.log,
            "alloc_session_code: falling back to deterministic walk after many collisions"
        );

        // Fallback: deterministic walk (extremely unlikely to be used in practice).
        loop {
            let code = format!("{:06}", self.next_session_id);
            self.next_session_id = self.next_session_id.wrapping_add(1);
            if !self.sessions.contains_code(&code) {
                return code;
            }
        }
    }

    /// Main entrypoint: handle a message from a client.
    ///
    /// Returns a list of (target_client, Msg) to send.
    pub fn handle(&mut self, from_cid: ClientId, msg: SignalingMsg) -> Vec<OutgoingMsg> {
        match msg {
            SignalingMsg::Hello { client_version } => {
                // For now: ignore and maybe log. No reply required.
                sink_trace!(
                    self.log,
                    "client {} HELLO (version {})",
                    from_cid,
                    client_version
                );
                Vec::new()
            }

            SignalingMsg::Login { username, password } => {
                self.handle_login(from_cid, username, password)
            }

            SignalingMsg::Register { username, password } => {
                self.handle_register(from_cid, username, password)
            }

            SignalingMsg::ListPeers => self.handle_list_peers(from_cid),

            SignalingMsg::CreateSession { capacity } => {
                self.handle_create_session(from_cid, capacity)
            }

            SignalingMsg::Join { session_code } => self.handle_join(from_cid, session_code),

            SignalingMsg::Offer { .. }
            | SignalingMsg::Answer { .. }
            | SignalingMsg::Candidate { .. }
            | SignalingMsg::Ack { .. }
            | SignalingMsg::Bye { .. } => self.forward_signaling(from_cid, msg),

            SignalingMsg::Ping { nonce } => vec![OutgoingMsg {
                client_id_target: from_cid,
                msg: SignalingMsg::Pong { nonce },
            }],
            SignalingMsg::Pong { .. } => Vec::new(),
            SignalingMsg::LoginOk { .. }
            | SignalingMsg::LoginErr { .. }
            | SignalingMsg::RegisterOk { .. }
            | SignalingMsg::RegisterErr { .. }
            | SignalingMsg::PeersOnline { .. }
            | SignalingMsg::Created { .. }
            | SignalingMsg::JoinOk { .. }
            | SignalingMsg::JoinErr { .. }
            | SignalingMsg::PeerJoined { .. }
            | SignalingMsg::PeerLeft { .. } => {
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

    fn broadcast_peer_list_update(&self) -> Vec<OutgoingMsg> {
        let mut out_msgs = Vec::new();
        let all_usernames = self.presence.online_usernames();
        let all_clients = self.presence.all_client_ids();

        for client_id in all_clients {
            // Get the username for this specific client so we can filter it out of their list
            if let Some(my_username) = self.presence.username_for(client_id) {
                // Filter: everyone except me
                let peers: Vec<String> = all_usernames
                    .iter()
                    .filter(|u| *u != my_username)
                    .cloned()
                    .collect();

                out_msgs.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg: SignalingMsg::PeersOnline { peers },
                });
            }
        }
        out_msgs
    }

    /// Called when a TCP connection closes, to clean up state.
    pub fn handle_disconnect(&mut self, client: ClientId) -> Vec<OutgoingMsg> {
        let mut out_msgs = Vec::new();

        // Remove from presence
        let username_opt = self.presence.logout(client);

        // Remove from any sessions (and find who remains)
        let left_sessions = self.sessions.leave_all(client);
        let n_sessions = left_sessions.len();

        if let Some(username) = username_opt {
            sink_info!(
                self.log,
                "client {} ({}) disconnected; left {} sessions",
                client,
                username,
                n_sessions
            );

            for (session_id, remaining_members) in left_sessions {
                for member in remaining_members {
                    out_msgs.push(OutgoingMsg {
                        client_id_target: member,
                        msg: SignalingMsg::PeerLeft {
                            session_id: session_id.clone(),
                            username: username.clone(),
                        },
                    });
                }
            }

            // 2. Broadcast updated peer list to everyone else
            out_msgs.extend(self.broadcast_peer_list_update());
        } else {
            sink_info!(
                self.log,
                "client {} disconnected (was not logged in); left {} sessions",
                client,
                n_sessions
            );
        }

        out_msgs
    }

    // ---- Individual handlers ---------------------------------------------

    fn handle_login(
        &mut self,
        client: ClientId,
        username: UserName,
        password: String,
    ) -> Vec<OutgoingMsg> {
        sink_info!(
            self.log,
            "login attempt: client_id={} username={}",
            client,
            username
        );
        let mut out = Vec::new();
        // 1) Auth backend decides if username/password are valid.
        if let Err(err) = self.auth.verify(&username, &password) {
            sink_warn!(
                self.log,
                "login failed: client_id={} username={} err={:?}",
                client,
                username,
                err
            );
            // Map AuthError to our protocol-level login error code.
            let code = match err {
                AuthError::InvalidCredentials => LoginErrorCode::InvalidCredentials.as_u16(),
                AuthError::Internal => LoginErrorCode::Internal.as_u16(),
            };

            out.push(OutgoingMsg {
                client_id_target: client,
                msg: SignalingMsg::LoginErr { code },
            });
            return out;
        }

        // 2) Reject if the user is already logged in on another client.
        if let Some(existing_client) = self.presence.client_id_for(&username) {
            sink_warn!(
                self.log,
                "login rejected: username={} already logged in as client_id={}",
                username,
                existing_client
            );
            let code = LoginErrorCode::AlreadyLoggedIn.as_u16();
            out.push(OutgoingMsg {
                client_id_target: client,
                msg: SignalingMsg::LoginErr { code },
            });
            return out;
        }
        sink_info!(
            self.log,
            "login success: client_id={} username={}",
            client,
            username
        );
        // 3) Success: record presence and send LoginOk.
        let _ = self.presence.login(client, username.clone());
        out.push(OutgoingMsg {
            client_id_target: client,
            msg: SignalingMsg::LoginOk { username },
        });
        // 4) Broadcast updated peer list to everyone (including the new user)
        out.extend(self.broadcast_peer_list_update());
        out
    }

    fn handle_register(
        &mut self,
        client_id: ClientId,
        username: UserName,
        password: String,
    ) -> Vec<OutgoingMsg> {
        let mut out = Vec::new();

        let res = self.auth.register(&username, &password);

        match res {
            Ok(()) => {
                sink_info!(
                    self.log,
                    "registered new user '{}' from client_id={}",
                    username,
                    client_id
                );
                out.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg: SignalingMsg::RegisterOk {
                        username: username.clone(),
                    },
                });
            }
            Err(err) => {
                let code: RegisterErrorCode = err.into();
                sink_warn!(
                    self.log,
                    "registration failed for '{}' from client_id={}: {:?} (code={})",
                    username,
                    client_id,
                    err,
                    code.as_u16()
                );
                out.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg: SignalingMsg::RegisterErr {
                        code: code.as_u16(),
                    },
                });
            }
        }

        out
    }

    fn handle_list_peers(&mut self, client_id: ClientId) -> Vec<OutgoingMsg> {
        let mut out = Vec::new();
        let requester = self.require_logged_in(client_id);

        if requester.is_none() {
            sink_warn!(
                self.log,
                "client {} requested peer list without logging in",
                client_id
            );
            out.push(OutgoingMsg {
                client_id_target: client_id,
                msg: SignalingMsg::PeersOnline { peers: Vec::new() },
            });
            return out;
        } else if let Some(username) = requester.as_ref() {
            sink_info!(
                self.log,
                "client {} ({}) requested peer list",
                client_id,
                username
            );
        }

        let peers = {
            let mut all = self.presence.online_usernames();
            if let Some(current) = requester.as_ref() {
                all.retain(|peer| peer != current);
            }
            all
        };

        out.push(OutgoingMsg {
            client_id_target: client_id,
            msg: SignalingMsg::PeersOnline { peers },
        });
        out
    }

    fn handle_create_session(&mut self, client_id: ClientId, capacity: u8) -> Vec<OutgoingMsg> {
        let mut out_msg = Vec::new();

        // Require login first
        let Some(username) = self.require_logged_in(client_id) else {
            let msg = SignalingMsg::JoinErr {
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

        let msg = SignalingMsg::Created {
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
        let mut out_msgs = Vec::new();

        // require login
        let Some(username) = self.require_logged_in(client_id) else {
            let msg = SignalingMsg::JoinErr {
                code: JoinErrorCode::NotLoggedIn.as_u16(),
            };
            sink_warn!(
                self.log,
                "client {} attempted Join without login",
                client_id
            );
            out_msgs.push(OutgoingMsg {
                client_id_target: client_id,
                msg,
            });
            return out_msgs;
        };

        match self.sessions.join_by_code(&session_code, client_id) {
            Ok(session_id) => {
                sink_info!(
                    self.log,
                    "Join success: client_id={} ({}) joined session_code={} (session_id={})",
                    client_id,
                    username,
                    session_code,
                    session_id
                );
                // 1) JoinOk to the joiner
                let join_ok = SignalingMsg::JoinOk {
                    session_id: session_id.clone(),
                };
                out_msgs.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg: join_ok,
                });

                // 2) PeerJoined to existing members
                if let Some(sess) = self.sessions.get(&session_id) {
                    for &member in &sess.members {
                        if member == client_id {
                            continue; // skip the joiner
                        }
                        out_msgs.push(OutgoingMsg {
                            client_id_target: member,
                            msg: SignalingMsg::PeerJoined {
                                session_id: session_id.clone(),
                                username: username.clone(),
                            },
                        });
                    }
                }
            }
            Err(JoinError::NotFound) => {
                sink_warn!(
                    self.log,
                    "Join failed: client_id={} ({}) session_code={} not found",
                    client_id,
                    username,
                    session_code
                );
                let msg = SignalingMsg::JoinErr {
                    code: JoinErrorCode::NotFound.as_u16(),
                };
                out_msgs.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg,
                });
            }
            Err(JoinError::Full) => {
                sink_warn!(
                    self.log,
                    "Join failed: client_id={} ({}) session_code={} full",
                    client_id,
                    username,
                    session_code
                );
                let msg = SignalingMsg::JoinErr {
                    code: JoinErrorCode::Full.as_u16(),
                };
                out_msgs.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg,
                });
            }
        }

        out_msgs
    }

    /// Forward Offer/Answer/Candidate, enforcing:
    /// - sender must be logged in
    /// - target must be logged in
    ///
    /// On violation: log a warning and drop the message.
    fn forward_signaling(&mut self, from: ClientId, msg: SignalingMsg) -> Vec<OutgoingMsg> {
        // 1) sender must be logged in
        let Some(from_username) = self.require_logged_in(from) else {
            sink_warn!(
                self.log,
                "unauthenticated client {} attempted to send signaling message",
                from
            );
            return Vec::new();
        };

        match msg {
            SignalingMsg::Offer {
                txn_id, to, sdp, ..
            } => self.forward(from, &from_username, txn_id, to, |username, txn_id, to| {
                SignalingMsg::Offer {
                    txn_id,
                    from: username,
                    to,
                    sdp,
                }
            }),
            SignalingMsg::Answer {
                txn_id, to, sdp, ..
            } => self.forward(from, &from_username, txn_id, to, |username, txn_id, to| {
                SignalingMsg::Answer {
                    txn_id,
                    from: username,
                    to,
                    sdp,
                }
            }),
            SignalingMsg::Candidate {
                to,
                mid,
                mline_index,
                cand,
                ..
            } => self.forward(from, &from_username, 0, to, |username, _txn_id, to| {
                SignalingMsg::Candidate {
                    from: username,
                    to,
                    mid,
                    mline_index,
                    cand,
                }
            }),
            SignalingMsg::Ack { txn_id, to, .. } => {
                self.forward(from, &from_username, txn_id, to, |username, txn_id, to| {
                    SignalingMsg::Ack {
                        from: username,
                        to,
                        txn_id,
                    }
                })
            }
            SignalingMsg::Bye { to, reason, .. } => {
                self.forward(from, &from_username, 0, to, |username, _txn_id, to| {
                    SignalingMsg::Bye {
                        from: username,
                        to,
                        reason,
                    }
                })
            }
            other => {
                sink_warn!(
                    self.log,
                    "forward_signaling received unexpected message {:?}",
                    other
                );
                Vec::new()
            }
        }
    }

    fn forward<F>(
        &mut self,
        from: ClientId,
        from_username: &UserName,
        txn_id: u64,
        to_username: UserName,
        builder: F,
    ) -> Vec<OutgoingMsg>
    where
        F: FnOnce(UserName, u64, UserName) -> SignalingMsg,
    {
        // 2) resolve target client by username
        let Some(target_client) = self.presence.client_id_for(&to_username) else {
            sink_warn!(
                self.log,
                "client {} ({}) tried to send signaling to offline user {}",
                from,
                from_username,
                to_username
            );
            return Vec::new();
        };

        let msg = builder(from_username.clone(), txn_id, to_username.clone());

        let kind = match &msg {
            SignalingMsg::Offer { .. } => "Offer",
            SignalingMsg::Answer { .. } => "Answer",
            SignalingMsg::Candidate { .. } => "Candidate",
            _ => "Signaling",
        };

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

    #[allow(dead_code)]
    fn handle_ack(&mut self, from_cid: ClientId, txn_id: u64) -> Vec<OutgoingMsg> {
        let username = self.presence.username_for(from_cid).cloned();
        sink_trace!(
            self.log,
            "client {} ({:?}) ACK txn_id={}",
            from_cid,
            username,
            txn_id
        );
        // Still no reliability logic; we just swallow it for now.
        Vec::new()
    }

    #[allow(dead_code)]
    fn handle_bye(&mut self, from: ClientId, reason: Option<String>) -> Vec<OutgoingMsg> {
        let username_opt = self.presence.username_for(from).cloned();

        sink_info!(
            self.log,
            "client {} ({:?}) sent Bye {:?}",
            from,
            username_opt,
            reason
        );

        let left_sessions = self.sessions.leave_all(from);
        let n_sessions = left_sessions.len();
        let mut out_msgs = Vec::new();

        if let Some(username) = username_opt {
            sink_info!(
                self.log,
                "client {} ({}) sent bye; left {} sessions",
                from,
                username,
                n_sessions
            );

            for (session_id, remaining_members) in left_sessions {
                for member in remaining_members {
                    out_msgs.push(OutgoingMsg {
                        client_id_target: member,
                        msg: SignalingMsg::PeerLeft {
                            session_id: session_id.clone(),
                            username: username.clone(),
                        },
                    });
                }
            }
        }
        out_msgs
    }
}
impl Default for ServerEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::signaling::auth::InMemoryAuthBackend;
    use crate::signaling::protocol::SignalingMsg;

    fn new_server() -> ServerEngine {
        ServerEngine::with_log(Arc::new(NoopLogSink))
    }

    fn new_server_with_in_memory_auth() -> ServerEngine {
        let auth = InMemoryAuthBackend::new()
            .with_user("alice", "secret")
            .with_user("bob", "pw2");
        ServerEngine::with_auth(Box::new(auth))
    }

    fn login(server: &mut ServerEngine, client_id: ClientId, username: &str) {
        let out = server.handle(
            client_id,
            SignalingMsg::Login {
                username: username.to_string(),
                password: "pw".to_string(),
            },
        );

        // We expect a LoginOk back to that client.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].client_id_target, client_id);
        match &out[0].msg {
            SignalingMsg::LoginOk { username: u } => assert_eq!(u, username),
            other => panic!("expected LoginOk, got {:?}", other),
        }
    }

    #[test]
    fn login_and_create_session_roundtrip() {
        let mut server = ServerEngine::new();
        let client1 = 1;

        // client logs in
        let outs = server.handle(
            client1,
            SignalingMsg::Login {
                username: "alice".into(),
                password: "pw".into(),
            },
        );

        assert_eq!(outs.len(), 1);
        match &outs[0].msg {
            SignalingMsg::LoginOk { username } => assert_eq!(username, "alice"),
            other => panic!("expected LoginOk, got {:?}", other),
        }

        // client creates session
        let outs2 = server.handle(client1, SignalingMsg::CreateSession { capacity: 2 });
        assert_eq!(outs2.len(), 1);
        match &outs2[0].msg {
            SignalingMsg::Created {
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
            SignalingMsg::Offer {
                txn_id: 1,
                from: "alice".to_string(),
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
            SignalingMsg::Offer {
                txn_id: 1,
                from: "alice".to_string(),
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
    fn offer_without_shared_session_is_forwarded() {
        let mut server = new_server();

        // alice and bob both logged in, but in no sessions yet
        login(&mut server, 1, "alice");
        login(&mut server, 2, "bob");

        let res = server.handle(
            1,
            SignalingMsg::Offer {
                txn_id: 1,
                from: "alice".to_string(),
                to: "bob".to_string(),
                sdp: b"v=0".to_vec(),
            },
        );

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].client_id_target, 2);
        match &res[0].msg {
            SignalingMsg::Offer {
                txn_id,
                from,
                to,
                sdp,
            } => {
                assert_eq!(*txn_id, 1);
                assert_eq!(from, "alice");
                assert_eq!(to, "bob");
                assert_eq!(sdp, b"v=0");
            }
            other => panic!("expected forwarded Offer, got {:?}", other),
        }
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
        let created = server.handle(alice, SignalingMsg::CreateSession { capacity: 2 });

        assert_eq!(created.len(), 1);
        assert_eq!(created[0].client_id_target, alice);

        let (session_id, session_code) = match &created[0].msg {
            SignalingMsg::Created {
                session_id,
                session_code,
            } => (session_id.clone(), session_code.clone()),
            other => panic!("expected Created, got {:?}", other),
        };

        // 3) bob joins that session
        let joined = server.handle(
            bob,
            SignalingMsg::Join {
                session_code: session_code.clone(),
            },
        );

        // Now expect JoinOk (to bob) + PeerJoined (to alice)
        assert_eq!(
            joined.len(),
            2,
            "expected JoinOk + PeerJoined, got {:?}",
            joined
        );

        let mut saw_join_ok = false;
        let mut saw_peer_joined = false;
        for m in &joined {
            match &m.msg {
                SignalingMsg::JoinOk { session_id: sid } => {
                    assert_eq!(m.client_id_target, bob);
                    assert_eq!(sid, &session_id);
                    saw_join_ok = true;
                }
                SignalingMsg::PeerJoined {
                    session_id: sid,
                    username,
                } => {
                    assert_eq!(m.client_id_target, alice);
                    assert_eq!(sid, &session_id);
                    assert_eq!(username, "bob");
                    saw_peer_joined = true;
                }
                other => panic!("unexpected msg in join: {:?}", other),
            }
        }
        assert!(saw_join_ok);
        assert!(saw_peer_joined);

        // 4) now alice sends an Offer to bob; should be forwarded
        let txn_id = 42;
        let sdp = b"v=0".to_vec();

        let res = server.handle(
            alice,
            SignalingMsg::Offer {
                txn_id,
                from: "alice".to_string(),
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
            SignalingMsg::Offer {
                txn_id: t,
                from,
                to,
                sdp: s,
            } => {
                assert_eq!(*t, txn_id);
                assert_eq!(from, "alice");
                assert_eq!(to, "bob");
                assert_eq!(s, &sdp);
            }
            other => panic!("expected forwarded Offer, got {:?}", other),
        }
    }

    #[test]
    fn list_peers_excludes_requester() {
        let mut server = new_server();
        login(&mut server, 1, "alice");
        login(&mut server, 2, "bob");
        login(&mut server, 3, "carol");

        let res = server.handle(1, SignalingMsg::ListPeers);
        assert_eq!(res.len(), 1);
        let out = &res[0];
        assert_eq!(out.client_id_target, 1);
        match &out.msg {
            SignalingMsg::PeersOnline { peers } => {
                assert_eq!(peers.len(), 2);
                assert!(peers.contains(&"bob".to_string()));
                assert!(peers.contains(&"carol".to_string()));
                assert!(!peers.contains(&"alice".to_string()));
            }
            other => panic!("expected PeersOnline, got {:?}", other),
        }
    }

    #[test]
    fn list_peers_without_login_returns_empty() {
        let mut server = new_server();
        login(&mut server, 2, "bob");

        let res = server.handle(1, SignalingMsg::ListPeers);
        assert_eq!(res.len(), 1);
        match &res[0].msg {
            SignalingMsg::PeersOnline { peers } => assert!(peers.is_empty()),
            other => panic!("expected PeersOnline, got {:?}", other),
        }
    }

    #[test]
    fn register_success_emits_register_ok() {
        let mut server = new_server();
        let res = server.handle(
            5,
            SignalingMsg::Register {
                username: "newuser".into(),
                password: "pw".into(),
            },
        );

        assert_eq!(res.len(), 1);
        match &res[0].msg {
            SignalingMsg::RegisterOk { username } => assert_eq!(username, "newuser"),
            other => panic!("expected RegisterOk, got {:?}", other),
        }
    }

    // ---- Ack invariants ---------------------------------------------------

    #[test]
    fn ack_from_unauthenticated_client_is_dropped() {
        let mut server = new_server();

        let res = server.handle(
            1,
            SignalingMsg::Ack {
                from: "alice".into(),
                to: "bob".into(),
                txn_id: 123,
            },
        );

        assert!(
            res.is_empty(),
            "expected no outgoing messages for unauthenticated Ack, got {:?}",
            res
        );
    }

    #[test]
    fn ack_is_forwarded_between_logged_in_peers() {
        let mut server = new_server();

        login(&mut server, 1, "alice");
        login(&mut server, 2, "bob");

        let res = server.handle(
            1,
            SignalingMsg::Ack {
                from: "alice".into(),
                to: "bob".into(),
                txn_id: 123,
            },
        );

        assert_eq!(res.len(), 1);
        let out = &res[0];
        assert_eq!(out.client_id_target, 2);
        match &out.msg {
            SignalingMsg::Ack { from, to, txn_id } => {
                assert_eq!(from, "alice");
                assert_eq!(to, "bob");
                assert_eq!(*txn_id, 123);
            }
            other => panic!("expected forwarded Ack, got {:?}", other),
        }
    }

    // ---- Bye invariants ---------------------------------------------------

    #[test]
    fn bye_from_unauthenticated_client_is_dropped() {
        let mut server = new_server();

        let res = server.handle(
            42,
            SignalingMsg::Bye {
                from: "alice".into(),
                to: "bob".into(),
                reason: Some("bye".into()),
            },
        );

        assert!(
            res.is_empty(),
            "expected no outgoing messages for unauthenticated Bye, got {:?}",
            res
        );
    }

    #[test]
    fn bye_is_forwarded_between_logged_in_peers() {
        let mut server = new_server();
        login(&mut server, 1, "alice");
        login(&mut server, 2, "bob");

        let res = server.handle(
            1,
            SignalingMsg::Bye {
                from: "alice".into(),
                to: "bob".into(),
                reason: Some("done".into()),
            },
        );

        assert_eq!(res.len(), 1);
        let out = &res[0];
        assert_eq!(out.client_id_target, 2);
        match &out.msg {
            SignalingMsg::Bye { from, to, reason } => {
                assert_eq!(from, "alice");
                assert_eq!(to, "bob");
                assert_eq!(reason.as_deref(), Some("done"));
            }
            other => panic!("expected forwarded Bye, got {:?}", other),
        }
    }

    #[test]
    fn ping_replies_with_pong() {
        let mut server = new_server();
        login(&mut server, 1, "alice");

        let res = server.handle(1, SignalingMsg::Ping { nonce: 42 });
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].client_id_target, 1);
        match &res[0].msg {
            SignalingMsg::Pong { nonce } => assert_eq!(*nonce, 42),
            other => panic!("expected Pong, got {:?}", other),
        }
    }

    #[test]
    fn join_sends_peerjoined_to_existing_members() {
        let mut server = new_server();
        let alice: ClientId = 1;
        let bob: ClientId = 2;

        login(&mut server, alice, "alice");
        login(&mut server, bob, "bob");

        // alice creates session
        let created = server.handle(alice, SignalingMsg::CreateSession { capacity: 2 });
        assert_eq!(created.len(), 1);

        let (session_id, session_code) = match &created[0].msg {
            SignalingMsg::Created {
                session_id,
                session_code,
            } => (session_id.clone(), session_code.clone()),
            other => panic!("expected Created, got {:?}", other),
        };

        // bob joins
        let out = server.handle(bob, SignalingMsg::Join { session_code });

        // We expect:
        // - JoinOk to bob
        // - PeerJoined to alice
        assert_eq!(out.len(), 2);
        let mut saw_join_ok = false;
        let mut saw_peer_joined = false;

        for m in &out {
            match &m.msg {
                SignalingMsg::JoinOk { session_id: sid } => {
                    assert_eq!(m.client_id_target, bob);
                    assert_eq!(sid, &session_id);
                    saw_join_ok = true;
                }
                SignalingMsg::PeerJoined {
                    session_id: sid,
                    username,
                } => {
                    assert_eq!(m.client_id_target, alice);
                    assert_eq!(sid, &session_id);
                    assert_eq!(username, "bob");
                    saw_peer_joined = true;
                }
                other => panic!("unexpected msg: {:?}", other),
            }
        }

        assert!(saw_join_ok);
        assert!(saw_peer_joined);
    }

    #[test]
    fn login_fails_with_invalid_credentials() {
        let mut server = new_server_with_in_memory_auth();
        let client: ClientId = 1;

        let out = server.handle(
            client,
            SignalingMsg::Login {
                username: "alice".into(),
                password: "wrong".into(),
            },
        );

        assert_eq!(out.len(), 1);
        match &out[0].msg {
            SignalingMsg::LoginErr { code } => {
                assert_eq!(
                    *code,
                    LoginErrorCode::InvalidCredentials.as_u16(),
                    "expected InvalidCredentials code, got {}",
                    code
                );
            }
            other => panic!("expected LoginErr, got {:?}", other),
        }
    }

    #[test]
    fn login_succeeds_with_correct_credentials() {
        let mut server = new_server_with_in_memory_auth();
        let client: ClientId = 1;

        let out = server.handle(
            client,
            SignalingMsg::Login {
                username: "alice".into(),
                password: "secret".into(),
            },
        );

        assert_eq!(out.len(), 1);
        match &out[0].msg {
            SignalingMsg::LoginOk { username } => assert_eq!(username, "alice"),
            other => panic!("expected LoginOk, got {:?}", other),
        }
    }
}
