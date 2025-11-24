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
            | Msg::PeerJoined { .. }
            | Msg::PeerLeft { .. }
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
        let mut out_msgs = Vec::new();

        // Remove from presence
        let username_opt = self.presence.logout(client);

        // Remove from any sessions (and find who remains)
        let left_sessions = self.sessions.leave_all(client);

        if let Some(username) = username_opt {
            sink_info!(self.log, "client {} ({}) disconnected", client, username);

            for (session_id, remaining_members) in left_sessions {
                for member in remaining_members {
                    out_msgs.push(OutgoingMsg {
                        client_id_target: member,
                        msg: Msg::PeerLeft {
                            session_id: session_id.clone(),
                            username: username.clone(),
                        },
                    });
                }
            }
        } else {
            sink_info!(
                self.log,
                "client {} disconnected (was not logged in)",
                client
            );
        }

        out_msgs
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
        let mut out_msgs = Vec::new();

        // require login
        let Some(username) = self.require_logged_in(client_id) else {
            let msg = Msg::JoinErr {
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
                // 1) JoinOk to the joiner
                let join_ok = Msg::JoinOk {
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
                            msg: Msg::PeerJoined {
                                session_id: session_id.clone(),
                                username: username.clone(),
                            },
                        });
                    }
                }
            }
            Err(JoinError::NotFound) => {
                let msg = Msg::JoinErr {
                    code: JoinErrorCode::NotFound.as_u16(),
                };
                out_msgs.push(OutgoingMsg {
                    client_id_target: client_id,
                    msg,
                });
            }
            Err(JoinError::Full) => {
                let msg = Msg::JoinErr {
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

    fn handle_ack(&mut self, from_cid: ClientId, txn_id: u64) -> Vec<OutgoingMsg> {
        let username = self.presence.username_for(from_cid).cloned();
        sink_debug!(
            self.log,
            "client {} ({:?}) ACK txn_id={}",
            from_cid,
            username,
            txn_id
        );
        // Still no reliability logic; we just swallow it for now.
        Vec::new()
    }

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
        let mut out_msgs = Vec::new();

        if let Some(username) = username_opt {
            for (session_id, remaining_members) in left_sessions {
                for member in remaining_members {
                    out_msgs.push(OutgoingMsg {
                        client_id_target: member,
                        msg: Msg::PeerLeft {
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

        let (session_id, session_code) = match &created[0].msg {
            Msg::Created {
                session_id,
                session_code,
            } => (session_id.clone(), session_code.clone()),
            other => panic!("expected Created, got {:?}", other),
        };

        // 3) bob joins that session
        let joined = server.handle(
            bob,
            Msg::Join {
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
                Msg::JoinOk { session_id: sid } => {
                    assert_eq!(m.client_id_target, bob);
                    assert_eq!(sid, &session_id);
                    saw_join_ok = true;
                }
                Msg::PeerJoined {
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

    // ---- Ack invariants ---------------------------------------------------

    #[test]
    fn ack_from_unauthenticated_client_is_dropped() {
        let mut server = new_server();

        let res = server.handle(1, Msg::Ack { txn_id: 123 });

        assert!(
            res.is_empty(),
            "expected no outgoing messages for unauthenticated Ack, got {:?}",
            res
        );
    }

    #[test]
    fn ack_from_logged_in_client_is_accepted_but_silent() {
        let mut server = new_server();

        login(&mut server, 1, "alice");

        let res = server.handle(1, Msg::Ack { txn_id: 123 });

        assert!(
            res.is_empty(),
            "expected Ack from logged-in client to be silent (no outgoing), got {:?}",
            res
        );
    }

    // ---- Bye invariants ---------------------------------------------------

    #[test]
    fn bye_from_unauthenticated_client_is_dropped() {
        let mut server = new_server();

        let res = server.handle(
            42,
            Msg::Bye {
                reason: Some("bye".into()),
            },
        );

        assert!(
            res.is_empty(),
            "expected no outgoing messages for unauthenticated Bye, got {:?}",
            res
        );
        // No sessions exist, so nothing else to check.
    }

    #[test]
    fn bye_removes_client_from_single_member_session_and_deletes_session() {
        let mut server = new_server();
        let alice: ClientId = 1;

        login(&mut server, alice, "alice");

        let created = server.handle(alice, Msg::CreateSession { capacity: 2 });

        assert_eq!(created.len(), 1);
        let (session_id, _session_code) = match &created[0].msg {
            Msg::Created {
                session_id,
                session_code,
            } => (session_id.clone(), session_code.clone()),
            other => panic!("expected Created, got {:?}", other),
        };

        {
            let sess = server
                .sessions
                .get(&session_id)
                .expect("session must exist");
            assert!(sess.members.contains(&alice));
            assert_eq!(sess.members.len(), 1);
        }

        // alice sends Bye
        let res = server.handle(
            alice,
            Msg::Bye {
                reason: Some("done".into()),
            },
        );
        assert!(
            res.is_empty(),
            "Bye should not produce any outgoing messages, got {:?}",
            res
        );

        // Session should be removed because it became empty
        assert!(
            server.sessions.get(&session_id).is_none(),
            "session should be deleted after sole member leaves with Bye"
        );
    }

    #[test]
    fn bye_removes_client_but_keeps_session_if_other_members_remain() {
        let mut server = new_server();
        let alice: ClientId = 1;
        let bob: ClientId = 2;

        login(&mut server, alice, "alice");
        login(&mut server, bob, "bob");

        let created = server.handle(alice, Msg::CreateSession { capacity: 2 });
        assert_eq!(created.len(), 1);
        let (session_id, session_code) = match &created[0].msg {
            Msg::Created {
                session_id,
                session_code,
            } => (session_id.clone(), session_code.clone()),
            other => panic!("expected Created, got {:?}", other),
        };

        // bob joins: we expect 2 messages (JoinOk to bob, PeerJoined to alice)
        let joined = server.handle(bob, Msg::Join { session_code });
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
                Msg::JoinOk { session_id: sid } => {
                    assert_eq!(m.client_id_target, bob);
                    assert_eq!(sid, &session_id);
                    saw_join_ok = true;
                }
                Msg::PeerJoined {
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

        {
            let sess = server
                .sessions
                .get(&session_id)
                .expect("session must exist");
            assert!(sess.members.contains(&alice));
            assert!(sess.members.contains(&bob));
            assert_eq!(sess.members.len(), 2);
        }

        // alice sends Bye
        let res = server.handle(alice, Msg::Bye { reason: None });

        // Bye produce a notification: PeerLeft(alice) to bob
        assert_eq!(
            res.len(),
            1,
            "Bye should produce PeerLeft to remaining member, got {:?}",
            res
        );
        let m = &res[0];
        assert_eq!(m.client_id_target, bob);
        match &m.msg {
            Msg::PeerLeft {
                session_id: sid,
                username,
            } => {
                assert_eq!(sid, &session_id);
                assert_eq!(username, "alice");
            }
            other => panic!("expected PeerLeft, got {:?}", other),
        }

        // Session should still exist, but only bob remains
        {
            let sess = server
                .sessions
                .get(&session_id)
                .expect("session must still exist");
            assert!(!sess.members.contains(&alice));
            assert!(sess.members.contains(&bob));
            assert_eq!(sess.members.len(), 1);
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
        let created = server.handle(alice, Msg::CreateSession { capacity: 2 });
        assert_eq!(created.len(), 1);

        let (session_id, session_code) = match &created[0].msg {
            Msg::Created {
                session_id,
                session_code,
            } => (session_id.clone(), session_code.clone()),
            other => panic!("expected Created, got {:?}", other),
        };

        // bob joins
        let out = server.handle(bob, Msg::Join { session_code });

        // We expect:
        // - JoinOk to bob
        // - PeerJoined to alice
        assert_eq!(out.len(), 2);
        let mut saw_join_ok = false;
        let mut saw_peer_joined = false;

        for m in &out {
            match &m.msg {
                Msg::JoinOk { session_id: sid } => {
                    assert_eq!(m.client_id_target, bob);
                    assert_eq!(sid, &session_id);
                    saw_join_ok = true;
                }
                Msg::PeerJoined {
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
    fn bye_sends_peerleft_to_remaining_members() {
        let mut server = new_server();
        let alice: ClientId = 1;
        let bob: ClientId = 2;

        login(&mut server, alice, "alice");
        login(&mut server, bob, "bob");

        // alice creates + bob joins
        let created = server.handle(alice, Msg::CreateSession { capacity: 2 });
        assert_eq!(created.len(), 1);

        let (session_id, session_code) = match &created[0].msg {
            Msg::Created {
                session_id,
                session_code,
            } => (session_id.clone(), session_code.clone()),
            other => panic!("expected Created, got {:?}", other),
        };

        let _ = server.handle(bob, Msg::Join { session_code });

        // alice Bye
        let out = server.handle(
            alice,
            Msg::Bye {
                reason: Some("bye".into()),
            },
        );

        // Only bob should get PeerLeft(alice)
        assert_eq!(out.len(), 1);
        let m = &out[0];
        assert_eq!(m.client_id_target, bob);

        match &m.msg {
            Msg::PeerLeft {
                session_id: sid,
                username,
            } => {
                assert_eq!(sid, &session_id);
                assert_eq!(username, "alice");
            }
            other => panic!("expected PeerLeft, got {:?}", other),
        }
    }
}
