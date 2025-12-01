use crate::signaling::protocol::SignalingMsg;

/// Commands issued by the GUI / application into the signaling client.
#[derive(Debug)]
pub enum SignalingCommand {
    Send(SignalingMsg),
    Disconnect,
}
