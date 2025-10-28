use super::{
    common_header::CommonHeader,
    rtcp_error::RtcpError,
    rtcp::RtcpPacket,
};

// RTCP packet types (per RFC3550; feedback per RFC4585/5104)
pub const PT_SR: u8 = 200;
pub const PT_RR: u8 = 201;
pub const PT_SDES: u8 = 202;
pub const PT_BYE: u8 = 203;
pub const PT_APP: u8 = 204;
pub const PT_RTPFB: u8 = 205; // Transport layer FB (e.g., Generic NACK)
pub const PT_PSFB: u8 = 206; // Payload-specific FB (e.g., PLI, FIR)

pub trait RtcpPacketType {
    /// Codifica el paquete completo (incluyendo CommonHeader)
    fn encode_into(&self, out: &mut Vec<u8>);

    /// Decodifica el paquete a partir del CommonHeader y del payload.
    fn decode(hdr: &CommonHeader, payload: &[u8]) -> Result<RtcpPacket, RtcpError>;
}
