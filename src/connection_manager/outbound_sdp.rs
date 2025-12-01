use crate::sdp::sdpc::Sdp;

/// Represents an SDP message to be sent to the remote peer.
#[derive(Debug)]
pub enum OutboundSdp {
    /// An SDP offer.
    Offer(Sdp),
    /// An SDP answer.
    Answer(Sdp),
    /// No SDP message to send.
    None,
}
