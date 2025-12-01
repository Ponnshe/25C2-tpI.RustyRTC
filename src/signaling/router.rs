use std::collections::HashMap;
use std::sync::Arc;

use crate::log::NoopLogSink;
use crate::log::log_sink::LogSink;
use crate::signaling::auth::AuthBackend;
use crate::signaling::protocol::SignalingMsg;
use crate::signaling::server_engine::ServerEngine;
use crate::signaling::types::{ClientId, OutgoingMsg};

/// Router glues the `ServerEngine` state machine to per-client "sinks".
pub struct Router {
    server: ServerEngine,
    outboxes: HashMap<ClientId, Vec<SignalingMsg>>,
}

impl Router {
    #[must_use]
    pub fn new() -> Self {
        Self::with_log(Arc::new(NoopLogSink))
    }

    #[must_use]
    pub fn with_log(log: Arc<dyn LogSink>) -> Self {
        Self {
            server: ServerEngine::with_log(log),
            outboxes: HashMap::new(),
        }
    }
    /// New: build a Router with explicit log sink *and* auth backend.
    #[must_use]
    pub fn with_log_and_auth(log: Arc<dyn LogSink>, auth_backend: Box<dyn AuthBackend>) -> Self {
        Self {
            server: ServerEngine::with_log_and_auth(log, auth_backend),
            outboxes: HashMap::new(),
        }
    }

    /// Register a new client with this Router.
    ///
    /// For now this just ensures an outbox exists.
    pub fn register_client(&mut self, client_id: ClientId) {
        self.outboxes.entry(client_id).or_default();
    }

    /// Unregister a client:
    /// - removes its outbox
    /// - lets the server clean up presence/sessions and emit any notifications.
    pub fn unregister_client(&mut self, client_id: ClientId) {
        self.outboxes.remove(&client_id);

        let out_msgs = self.server.handle_disconnect(client_id);
        for out_msg in out_msgs {
            self.enqueue(out_msg);
        }
    }

    /// Main entrypoint: handle a message coming *from* a client.
    ///
    /// This calls into the `ServerEngine` and enqueues any resulting messages into the
    /// appropriate client outboxes.
    pub fn handle_from_client(&mut self, from_cid: ClientId, msg: SignalingMsg) {
        let out_msgs = self.server.handle(from_cid, msg);
        for out_msg in out_msgs {
            self.enqueue(out_msg);
        }
    }

    /// Drain and return all outgoing messages for a given client.
    ///
    /// Useful for tests, and later for polling connections in a simple loop.
    pub fn take_outgoing_for(&mut self, client_id: ClientId) -> Vec<SignalingMsg> {
        self.outboxes.remove(&client_id).unwrap_or_default()
    }

    /// Peek (non-destructive) at outgoing messages for a client.
    /// Mostly helpful in tests.
    #[must_use]
    pub fn outgoing_for(&self, client_id: ClientId) -> &[SignalingMsg] {
        self.outboxes
            .get(&client_id)
            .map_or(&[], std::vec::Vec::as_slice)
    }

    /// Drain all pending outgoing messages for all clients.
    ///
    /// Each entry is (`client_id_target`, msg).
    pub fn drain_all_outgoing(&mut self) -> Vec<(ClientId, SignalingMsg)> {
        let mut result = Vec::new();

        // Collect keys first to avoid borrowing issues.
        let client_ids: Vec<ClientId> = self.outboxes.keys().copied().collect();

        for cid in client_ids {
            if let Some(msgs) = self.outboxes.remove(&cid) {
                for m in msgs {
                    result.push((cid, m));
                }
            }
        }

        result
    }

    /// Access to the underlying server, if we ever need to inspect it in tests.
    #[must_use]
    pub const fn server(&self) -> &ServerEngine {
        &self.server
    }

    #[must_use]
    pub const fn server_mut(&mut self) -> &mut ServerEngine {
        &mut self.server
    }

    fn enqueue(&mut self, out_msg: OutgoingMsg) {
        let queue = self.outboxes.entry(out_msg.client_id_target).or_default();
        queue.push(out_msg.msg);
    }
}
impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::signaling::protocol::SignalingMsg;
    #[test]
    fn login_create_session_join_and_offer_are_routed() {
        let mut router = Router::new();
        let c1: ClientId = 1;
        let c2: ClientId = 2;

        router.register_client(c1);
        router.register_client(c2);

        // 1) Both clients log in
        router.handle_from_client(
            c1,
            SignalingMsg::Login {
                username: "alice".into(),
                password: "pw1".into(),
            },
        );
        router.handle_from_client(
            c2,
            SignalingMsg::Login {
                username: "bob".into(),
                password: "pw2".into(),
            },
        );

        let outs1 = router.take_outgoing_for(c1);
        let outs2 = router.take_outgoing_for(c2);

        let has_login_ok_1 = outs1
            .iter()
            .any(|m| matches!(m, SignalingMsg::LoginOk { .. }));
        let has_login_ok_2 = outs2
            .iter()
            .any(|m| matches!(m, SignalingMsg::LoginOk { .. }));

        assert!(has_login_ok_1, "c1 should have received LoginOk");
        assert!(has_login_ok_2, "c2 should have received LoginOk");

        // 2) Client 1 creates a session
        router.handle_from_client(c1, SignalingMsg::CreateSession { capacity: 2 });

        let outs1 = router.take_outgoing_for(c1);
        assert_eq!(outs1.len(), 1);

        let (session_id, session_code) = match &outs1[0] {
            SignalingMsg::Created {
                session_id,
                session_code,
            } => (session_id.clone(), session_code.clone()),
            other => panic!("expected Created, got {other:?}"),
        };

        assert!(session_id.starts_with("sess-"));
        assert_eq!(session_code.len(), 6);

        // 3) Client 2 joins using session_code
        router.handle_from_client(c2, SignalingMsg::Join { session_code });

        // Now we expect:
        // - JoinOk for c2
        // - PeerJoined(alice sees "bob") for c1
        let outs2 = router.take_outgoing_for(c2);
        let outs1_after_join = router.take_outgoing_for(c1);

        let has_join_ok_2 = outs2
            .iter()
            .any(|m| matches!(m, SignalingMsg::JoinOk { .. }));
        assert!(has_join_ok_2);

        let has_peer_joined_1 = outs1_after_join
            .iter()
            .any(|m| matches!(m, SignalingMsg::PeerJoined { .. }));
        assert!(has_peer_joined_1);

        // 4) Client 1 sends an Offer to bob; router should emit it to c2
        let fake_sdp = b"v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\n".to_vec();
        router.handle_from_client(
            c1,
            SignalingMsg::Offer {
                txn_id: 42,
                from: "alice".into(),
                to: "bob".into(),
                sdp: fake_sdp.clone(),
            },
        );

        let outs1_after_offer = router.take_outgoing_for(c1);
        assert!(
            outs1_after_offer.is_empty(),
            "expected no messages to c1 after Offer, got {outs1_after_offer:?}"
        );

        let outs2_after_offer = router.take_outgoing_for(c2);
        assert_eq!(outs2_after_offer.len(), 1);
        match &outs2_after_offer[0] {
            SignalingMsg::Offer {
                txn_id,
                from,
                to,
                sdp,
            } => {
                assert_eq!(*txn_id, 42);
                assert_eq!(from, "alice");
                assert_eq!(to, "bob");
                assert_eq!(sdp, &fake_sdp);
            }
            other => panic!("expected forwarded Offer, got {other:?}"),
        }
    }

    #[test]
    fn drain_all_outgoing_collects_messages_for_all_clients() {
        let mut router = Router::new();
        let c1: ClientId = 1;
        let c2: ClientId = 2;

        router.register_client(c1);
        router.register_client(c2);

        // Both clients log in
        router.handle_from_client(
            c1,
            SignalingMsg::Login {
                username: "alice".into(),
                password: "pw1".into(),
            },
        );
        router.handle_from_client(
            c2,
            SignalingMsg::Login {
                username: "bob".into(),
                password: "pw2".into(),
            },
        );

        let mut outgoing = router.drain_all_outgoing();

        // We don't care about cross-client ordering; make it deterministic
        outgoing.sort_by_key(|(cid, _)| *cid);

        assert!(outgoing.len() >= 2);

        let c1_msgs: Vec<_> = outgoing.iter().filter(|(cid, _)| *cid == c1).collect();
        let c2_msgs: Vec<_> = outgoing.iter().filter(|(cid, _)| *cid == c2).collect();

        assert!(
            c1_msgs
                .iter()
                .any(|(_, msg)| matches!(msg, SignalingMsg::LoginOk{username: u} if u == "alice"))
        );
        assert!(
            c2_msgs
                .iter()
                .any(|(_, msg)| matches!(msg, SignalingMsg::LoginOk{username: u} if u == "bob"))
        );

        // After draining, nothing else should be pending
        let again = router.drain_all_outgoing();
        assert!(again.is_empty());
    }
}
