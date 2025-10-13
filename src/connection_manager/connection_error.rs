use crate::sdp::sdp_error::SdpError;
#[derive(Debug)]
pub enum ConnectionError {
    MediaSpec,
    IceAgent,
    Sdp(SdpError),
}
