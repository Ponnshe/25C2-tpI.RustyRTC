use crate::rtcp::packet_type;

use super::{
    rtcp_error::RtcpError,
    common_header::CommonHeader,
    sender_report::SenderReport,
    receiver_report::ReceiverReport,
    sdes::Sdes,
    bye::Bye,
    app::App,
    generic_nack::GenericNack,
    picture_loss::PictureLossIndication,
    packet_type::RtcpPacketType,
};
use std::convert::TryInto;

pub const RTCP_VERSION: u8 = 2;

/// The union of supported RTCP packets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtcpPacket {
    Sr(SenderReport),
    Rr(ReceiverReport),
    Sdes(Sdes),
    Bye(Bye),
    App(App),
    Nack(GenericNack),          // Transport FB (205/FMT=1)
    Pli(PictureLossIndication), // Payload FB (206/FMT=1)
}

impl RtcpPacket {
    /// Decode a *compound* RTCP buffer into individual packets.
    pub fn decode_compound(buf: &[u8]) -> Result<Vec<RtcpPacket>, RtcpError> {
        let mut out = Vec::new();
        let mut idx = 0usize;
        while idx + 4 <= buf.len() {
            let (hdr, total) = CommonHeader::decode(&buf[idx..])?;
            let pkt_bytes = &buf[idx..idx + total];
            let payload = &pkt_bytes[4..];

            let pkt = match hdr.pt {
                packet_type::PT_SR => decode_sr(&hdr, payload)?,
                packet_type::PT_RR => decode_rr(&hdr, payload)?,
                packet_type::PT_SDES => decode_sdes(&hdr, payload)?,
                packet_type::PT_BYE => decode_bye(&hdr, payload)?,
                packet_type::PT_APP => decode_app(&hdr, payload)?,
                packet_type::PT_RTPFB => decode_rtcpfb(&hdr, payload)?,
                packet_type::PT_PSFB => decode_psfb(&hdr, payload)?,
                other => return Err(RtcpError::UnknownPacketType(other)),
            };
            out.push(pkt);
            idx += total;
        }
        if idx != buf.len() {
            // trailing garbage / partial packet
            return Err(RtcpError::TooShort);
        }
        Ok(out)
    }

    /// Encode a compound RTCP packet (concatenation of packets).
    pub fn encode_compound(pkts: &[RtcpPacket]) -> Vec<u8> {
        let mut out = Vec::new();
        for pkt in pkts {
            encode_one(pkt, &mut out);
        }
        out
    }
}

fn encode_one(packet: &RtcpPacket, out: &mut Vec<u8>) {
    match packet {
        RtcpPacket::Sr(sr) => {
            sr.encode_into(out);
        }
        RtcpPacket::Rr(rr) => {
            rr.encode_into(out);
        }
        RtcpPacket::Sdes(sdes) => {
            sdes.encode_into(out);
        }
        RtcpPacket::Bye(bye) => {
            bye.encode_into(out);
        }
        RtcpPacket::App(app) => {
            app.encode_into(out);
        }
        RtcpPacket::Nack(nack) => {
            nack.encode_into(out);
        }
        RtcpPacket::Pli(pli) => {
            pli.encode_into(out);
        }
    }
}
