use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use crate::app::log_sink::LogSink;
use crate::signaling::protocol::Msg;
use crate::signaling::router::Router;
use crate::signaling::server_event::ServerEvent;
use crate::signaling::types::ClientId;
use crate::{sink_debug, sink_info, sink_warn};
/// Central server loop: owns Router + maps client_id -> Sender<Msg>.
pub fn run_server_loop(mut router: Router, log: Arc<dyn LogSink>, rx: Receiver<ServerEvent>) {
    use ServerEvent::*;

    let mut clients: HashMap<ClientId, Sender<Msg>> = HashMap::new();

    while let Ok(ev) = rx.recv() {
        match ev {
            RegisterClient {
                client_id,
                to_client,
            } => {
                router.register_client(client_id);
                clients.insert(client_id, to_client);

                sink_info!(
                    log,
                    "registered client {} in server loop (now {} clients)",
                    client_id,
                    clients.len()
                );
            }

            MsgFromClient { client_id, msg } => {
                sink_debug!(log, "MsgFromClient from {}: {}", client_id, msg_name(&msg));

                // Let Router+Server handle it
                router.handle_from_client(client_id, msg);

                // Drain all pending outgoing msgs and deliver them to reader
                // threads
                let outgoing_msgs = router.drain_all_outgoing();
                for (c_target_id, out_msg) in outgoing_msgs {
                    if let Some(tx) = clients.get(&c_target_id) {
                        if tx.send(out_msg).is_err() {
                            sink_warn!(
                                log,
                                "failed to deliver message to client {} (channel closed)",
                                c_target_id
                            );
                        }
                    } else {
                        sink_warn!(log, "no client {} to deliver outgoing message", c_target_id);
                    }
                }
            }

            Disconnected { client_id } => {
                sink_info!(log, "client {} disconnected (transport)", client_id);
                router.unregister_client(client_id);
                clients.remove(&client_id);
            }
        }
    }

    sink_info!(
        log,
        "ServerEvent channel closed; server loop shutting down ({} clients left)",
        clients.len()
    );
}
/// Helper: short variant name for logging.
/// We avoid logging full SDP/candidates here.
fn msg_name(msg: &Msg) -> &'static str {
    use Msg::*;
    match msg {
        Hello { .. } => "Hello",
        Login { .. } => "Login",
        LoginOk { .. } => "LoginOk",
        LoginErr { .. } => "LoginErr",
        Register { .. } => "Register",
        RegisterOk { .. } => "RegisterOk",
        RegisterErr { .. } => "RegisterErr",
        CreateSession { .. } => "CreateSession",
        Created { .. } => "Created",
        Join { .. } => "Join",
        JoinOk { .. } => "JoinOk",
        JoinErr { .. } => "JoinErr",
        PeerJoined { .. } => "PeerJoined",
        PeerLeft { .. } => "PeerLeft",
        Offer { .. } => "Offer",
        Answer { .. } => "Answer",
        Candidate { .. } => "Candidate",
        Ack { .. } => "Ack",
        Bye { .. } => "Bye",
        Ping { .. } => "Ping",
        Pong { .. } => "Pong",
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::app::log_sink::NoopLogSink;
    use crate::signaling::protocol::Msg;
    use crate::signaling::router::Router;
    use crate::signaling::types::ClientId;

    #[test]
    fn server_loop_handles_login_and_sends_loginok() {
        // Channel for events -> server loop
        let (ev_tx, ev_rx) = mpsc::channel::<ServerEvent>();
        let log = Arc::new(NoopLogSink);
        // Spawn the server loop in a background thread
        thread::spawn(move || {
            let router = Router::new();
            run_server_loop(router, log, ev_rx);
        });

        // Channel for server -> client 1
        let (to_client_tx, to_client_rx) = mpsc::channel::<Msg>();
        let client_id: ClientId = 1;

        // 1) Register client 1 with the server loop
        ev_tx
            .send(ServerEvent::RegisterClient {
                client_id,
                to_client: to_client_tx,
            })
            .unwrap();

        // 2) Simulate client 1 sending a Login message
        ev_tx
            .send(ServerEvent::MsgFromClient {
                client_id,
                msg: Msg::Login {
                    username: "alice".into(),
                    password: "secret".into(),
                },
            })
            .unwrap();

        // 3) Client should receive LoginOk via its channel
        let msg = to_client_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("expected a message from server");

        match msg {
            Msg::LoginOk { username } => assert_eq!(username, "alice"),
            other => panic!("expected LoginOk, got {:?}", other),
        }

        // Optional: drop the event sender so the server loop can exit cleanly
        drop(ev_tx);
    }
}
