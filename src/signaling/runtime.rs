use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use crate::signaling::protocol::Msg;
use crate::signaling::router::Router;
use crate::signaling::types::ClientId;

/// Events sent *to* the central server thread.
pub enum ServerEvent {
    /// A client sent a signaling message.
    MsgFromClient { client_id: ClientId, msg: Msg },

    /// A client disconnected (TCP/TLS closed or errored).
    Disconnected { client_id: ClientId },

    /// A new client is registered with its outgoing channel.
    RegisterClient {
        client_id: ClientId,
        to_client: Sender<Msg>,
    },
}

/// Central server loop: owns Router + maps client_id -> Sender<Msg>.
pub fn run_server_loop(mut router: Router, rx: Receiver<ServerEvent>) {
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
            }

            MsgFromClient { client_id, msg } => {
                // Let Router+Server handle it
                router.handle_from_client(client_id, msg);

                // Drain all pending outgoing and deliver them
                let outgoing = router.drain_all_outgoing();
                for (target_id, out_msg) in outgoing {
                    if let Some(tx) = clients.get(&target_id) {
                        let _ = tx.send(out_msg);
                    } else {
                        eprintln!(
                            "[server-loop] no client {} to deliver msg {:?}",
                            target_id, out_msg
                        );
                    }
                }
            }

            Disconnected { client_id } => {
                router.unregister_client(client_id);
                clients.remove(&client_id);
            }
        }
    }

    eprintln!("[server-loop] event channel closed, shutting down");
}
