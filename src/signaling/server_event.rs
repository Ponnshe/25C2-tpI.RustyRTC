use std::sync::mpsc::Sender;

use crate::signaling::{protocol::Msg, types::ClientId};

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
