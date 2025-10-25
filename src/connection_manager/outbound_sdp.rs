use crate::sdp::sdpc::Sdp;
#[derive(Debug)]
pub enum OutboundSdp {
    Offer(Sdp),
    Answer(Sdp),
    None,
}
