use crate::signaling::protocol::Msg;

/// Internal identifier for a connected client (TCP/TLS connection).
pub type ClientId = u64;

/// A message the server wants to send to a client.
#[derive(Debug)]
pub struct OutgoingMsg {
    pub client_id_target: ClientId,
    pub msg: Msg,
}
