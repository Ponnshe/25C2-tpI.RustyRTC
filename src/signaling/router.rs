use std::collections::HashMap;
use std::sync::Arc;

use crate::app::log_sink::{LogSink, NoopLogSink};
use crate::signaling::protocol::Msg;
use crate::signaling::server::Server;
use crate::signaling::types::{ClientId, OutgoingMsg};

/// Router glues the Server state machine to per-client "sinks".
pub struct Router {
    server: Server,
    outboxes: HashMap<ClientId, Vec<Msg>>,
}

impl Router {
    pub fn new() -> Self {
        Self::with_log(Arc::new(NoopLogSink))
    }

    pub fn with_log(log: Arc<dyn LogSink>) -> Self {
        Self {
            server: Server::with_log(log),
            outboxes: HashMap::new(),
        }
    }

    /// Register a new client with this Router.
    ///
    /// For now this just ensures an outbox exists.
    pub fn register_client(&mut self, client_id: ClientId) {
        self.outboxes.entry(client_id).or_insert_with(Vec::new);
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
    /// This calls into the Server and enqueues any resulting messages into the
    /// appropriate client outboxes.
    pub fn handle_from_client(&mut self, from_cid: ClientId, msg: Msg) {
        let out_msgs = self.server.handle(from_cid, msg);
        for out_msg in out_msgs {
            self.enqueue(out_msg);
        }
    }

    /// Drain and return all outgoing messages for a given client.
    ///
    /// Useful for tests, and later for polling connections in a simple loop.
    pub fn take_outgoing_for(&mut self, client_id: ClientId) -> Vec<Msg> {
        self.outboxes.remove(&client_id).unwrap_or_default()
    }

    /// Peek (non-destructive) at outgoing messages for a client.
    /// Mostly helpful in tests.
    pub fn outgoing_for(&self, client_id: ClientId) -> &[Msg] {
        self.outboxes
            .get(&client_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Drain all pending outgoing messages for all clients.
    ///
    /// Each entry is (client_id_target, msg).
    pub fn drain_all_outgoing(&mut self) -> Vec<(ClientId, Msg)> {
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
    pub fn server(&self) -> &Server {
        &self.server
    }

    pub fn server_mut(&mut self) -> &mut Server {
        &mut self.server
    }

    fn enqueue(&mut self, out_msg: OutgoingMsg) {
        let queue = self
            .outboxes
            .entry(out_msg.client_id_target)
            .or_insert_with(Vec::new);
        queue.push(out_msg.msg);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::signaling::protocol::Msg;

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
            Msg::Login {
                username: "alice".into(),
                password: "pw1".into(),
            },
        );
        router.handle_from_client(
            c2,
            Msg::Login {
                username: "bob".into(),
                password: "pw2".into(),
            },
        );

        let outs1 = router.take_outgoing_for(c1);
        let outs2 = router.take_outgoing_for(c2);

        assert_eq!(outs1.len(), 1);
        assert_eq!(outs2.len(), 1);

        match &outs1[0] {
            Msg::LoginOk { username } => assert_eq!(username, "alice"),
            other => panic!("expected LoginOk for c1, got {:?}", other),
        }
        match &outs2[0] {
            Msg::LoginOk { username } => assert_eq!(username, "bob"),
            other => panic!("expected LoginOk for c2, got {:?}", other),
        }

        // 2) Client 1 creates a session
        router.handle_from_client(c1, Msg::CreateSession { capacity: 2 });

        let outs1 = router.take_outgoing_for(c1);
        assert_eq!(outs1.len(), 1);

        let (session_id, session_code) = match &outs1[0] {
            Msg::Created {
                session_id,
                session_code,
            } => (session_id.clone(), session_code.clone()),
            other => panic!("expected Created, got {:?}", other),
        };

        assert!(session_id.starts_with("sess-"));
        assert_eq!(session_code.len(), 6);

        // 3) Client 2 joins using session_code
        router.handle_from_client(
            c2,
            Msg::Join {
                session_code: session_code.clone(),
            },
        );

        let outs2 = router.take_outgoing_for(c2);
        assert_eq!(outs2.len(), 1);
        match &outs2[0] {
            Msg::JoinOk { session_id: sid } => assert_eq!(sid, &session_id),
            other => panic!("expected JoinOk, got {:?}", other),
        }

        // 4) Client 1 sends an Offer to bob; router should emit it to c2
        let fake_sdp = b"v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\n".to_vec();
        router.handle_from_client(
            c1,
            Msg::Offer {
                txn_id: 42,
                to: "bob".into(),
                sdp: fake_sdp.clone(),
            },
        );

        // No direct response to c1 for now
        let outs1 = router.take_outgoing_for(c1);
        assert!(outs1.is_empty());

        let outs2 = router.take_outgoing_for(c2);
        assert_eq!(outs2.len(), 1);
        match &outs2[0] {
            Msg::Offer { txn_id, to, sdp } => {
                assert_eq!(*txn_id, 42);
                assert_eq!(to, "bob");
                assert_eq!(sdp, &fake_sdp);
            }
            other => panic!("expected forwarded Offer, got {:?}", other),
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
            Msg::Login {
                username: "alice".into(),
                password: "pw1".into(),
            },
        );
        router.handle_from_client(
            c2,
            Msg::Login {
                username: "bob".into(),
                password: "pw2".into(),
            },
        );

        let mut outgoing = router.drain_all_outgoing();

        // We don't care about cross-client ordering; make it deterministic
        outgoing.sort_by_key(|(cid, _)| *cid);

        assert_eq!(outgoing.len(), 2);

        let (cid1, msg1) = &outgoing[0];
        let (cid2, msg2) = &outgoing[1];

        assert_eq!(*cid1, c1);
        match msg1 {
            Msg::LoginOk { username } => assert_eq!(username, "alice"),
            other => panic!("expected LoginOk for c1, got {:?}", other),
        }

        assert_eq!(*cid2, c2);
        match msg2 {
            Msg::LoginOk { username } => assert_eq!(username, "bob"),
            other => panic!("expected LoginOk for c2, got {:?}", other),
        }

        // After draining, nothing else should be pending
        let again = router.drain_all_outgoing();
        assert!(again.is_empty());
    }
}
