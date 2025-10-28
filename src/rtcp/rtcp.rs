use crate::rtcp::packet_type;

use super::{
    app::App, bye::Bye, common_header::CommonHeader, generic_nack::GenericNack,
    packet_type::RtcpPacketType, picture_loss::PictureLossIndication,
    receiver_report::ReceiverReport, rtcp_error::RtcpError, sdes::Sdes,
    sender_report::SenderReport,
};

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
                packet_type::PT_SR => SenderReport::decode(&hdr, payload)?,
                packet_type::PT_RR => ReceiverReport::decode(&hdr, payload)?,
                packet_type::PT_SDES => Sdes::decode(&hdr, payload)?,
                packet_type::PT_BYE => Bye::decode(&hdr, payload)?,
                packet_type::PT_APP => App::decode(&hdr, payload)?,
                packet_type::PT_RTPFB => GenericNack::decode(&hdr, payload)?,
                packet_type::PT_PSFB => PictureLossIndication::decode(&hdr, payload)?,
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
