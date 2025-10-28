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
#[cfg(test)]
mod tests {
    use crate::rtcp::app::App;
    use crate::rtcp::bye::Bye;
    use crate::rtcp::generic_nack::GenericNack;
    use crate::rtcp::packet_type::{PT_APP, PT_BYE, PT_PSFB, PT_RR, PT_RTPFB, PT_SDES, PT_SR};
    use crate::rtcp::picture_loss::PictureLossIndication;
    use crate::rtcp::receiver_report::ReceiverReport;
    use crate::rtcp::rtcp::RtcpPacket;
    use crate::rtcp::rtcp_error::RtcpError;
    use crate::rtcp::sdes::{Sdes, SdesChunk, SdesItem};
    use crate::rtcp::sender_info::SenderInfo;
    use crate::rtcp::sender_report::SenderReport;

    // --- helpers -------------------------------------------------------------

    fn be16(x: u16) -> [u8; 2] {
        x.to_be_bytes()
    }
    fn be32(x: u32) -> [u8; 4] {
        x.to_be_bytes()
    }

    /// Build a single RTCP packet (header+payload) bytes.
    /// length_words is calculated from payload len.
    fn mk_packet(version: u8, padding: bool, rc_or_fmt: u8, pt: u8, payload: &[u8]) -> Vec<u8> {
        let vprc = (version & 0b11) << 6 | ((padding as u8) << 5) | (rc_or_fmt & 0x1F);
        let total = 4 + payload.len();
        assert_eq!(total % 4, 0, "total bytes must be 32-bit aligned for RTCP");
        let length_words = u16::try_from((total / 4) - 1).unwrap();

        let mut out = Vec::with_capacity(total);
        out.push(vprc);
        out.push(pt);
        out.extend_from_slice(&be16(length_words));
        out.extend_from_slice(payload);
        out
    }

    // --- boundary: top-level compound decoding ------------------------------

    #[test]
    fn compound_empty_or_short_header() {
        // Empty buffer
        let err = RtcpPacket::decode_compound(&[]).unwrap_err();
        assert!(matches!(err, RtcpError::TooShort));

        // 3 bytes -> can't even read a header
        let err = RtcpPacket::decode_compound(&[0x80, 0, 0][..]).unwrap_err();
        assert!(matches!(err, RtcpError::TooShort));
    }

    #[test]
    fn bad_version_in_common_header() {
        // Version = 3 (invalid). Length indicates only the header (no payload).
        // Even though SR requires payload, we should fail first with BadVersion.
        let pkt = mk_packet(3, false, 0, PT_SR, &[]);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        match err {
            RtcpError::BadVersion(v) => assert_eq!(v, 3),
            _ => panic!("expected BadVersion, got {err:?}"),
        }
    }

    #[test]
    fn unknown_packet_type() {
        // PT = 255 (unknown), zero-length payload
        let pkt = mk_packet(2, false, 0, 255, &[]);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        match err {
            RtcpError::UnknownPacketType(255) => {}
            _ => panic!("expected UnknownPacketType(255), got {err:?}"),
        }
    }

    #[test]
    fn trailing_garbage_at_end() {
        // Valid PLI packet followed by 2 stray bytes -> decoder should error TooShort.
        let pli_payload = [be32(0xAABBCCDD), be32(0x11223344)].concat();
        let mut buf = mk_packet(2, false, 1, PT_PSFB, &pli_payload);
        buf.extend_from_slice(&[0xAA, 0xBB]); // trailing partial bytes
        let err = RtcpPacket::decode_compound(&buf).unwrap_err();
        assert!(matches!(err, RtcpError::TooShort));
    }

    // --- boundary: specific packet payload validation -----------------------

    #[test]
    fn sr_too_short_payload() {
        // SR requires at least 24 bytes payload (SSRC 4 + SenderInfo 20).
        // Here: 0 bytes -> TooShort from SR decoder.
        let pkt = mk_packet(2, false, 0, PT_SR, &[]);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::TooShort));
    }

    #[test]
    fn rr_too_short_payload() {
        // RR requires at least 4 bytes payload (SSRC).
        let pkt = mk_packet(2, false, 0, PT_RR, &[]);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::TooShort));
    }

    #[test]
    fn sdes_item_too_short() {
        // SDES with one chunk:
        // ssrc(4) + item(type=1=CNAME, len=5, but only 2 bytes provided) + END not present
        // payload bytes = 4 + 1 + 1 + 2 = 8 (already 32-bit aligned)
        let payload = [be32(0x01020304).to_vec(), vec![1, 5], vec![0x41, 0x42]].concat();
        let pkt = mk_packet(2, false, 1, PT_SDES, &payload);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::SdesItemTooShort));
    }

    #[test]
    fn bye_truncated_sources() {
        // BYE with SC=2 but only one SSRC (4 bytes) present -> Truncated.
        let payload = be32(0xDEADBEEF).to_vec(); // only one SSRC
        let pkt = mk_packet(2, false, 2, PT_BYE, &payload);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::Truncated));
    }

    #[test]
    fn bye_truncated_reason() {
        // BYE with one SSRC and reason length=10 but only 5 reason bytes present.
        // Pad payload up to 32-bit boundary (2 zeros) so header length is valid.
        let mut payload = Vec::new();
        payload.extend_from_slice(&be32(0xCAFEBABE)); // SSRC
        payload.push(10u8); // reason length claims 10
        payload.extend_from_slice(b"short"); // only 5 bytes
        payload.extend_from_slice(&[0, 0]); // pad to multiple of 4 (total 12)
        let pkt = mk_packet(2, false, 1, PT_BYE, &payload);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::Truncated));
    }

    #[test]
    fn app_too_short_payload() {
        // APP requires at least 8 bytes payload (SSRC + 4-char name).
        let payload = be32(0x11111111).to_vec(); // only SSRC
        let pkt = mk_packet(2, false, 0, PT_APP, &payload);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::TooShort));
    }

    #[test]
    fn rtpfb_invalid_fmt() {
        // RTPFB with FMT != 1 (e.g., 0) should return Invalid.
        let payload = [be32(0x01020304), be32(0x05060708)].concat(); // sender_ssrc + media_ssrc
        let pkt = mk_packet(2, false, 0, PT_RTPFB, &payload);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::Invalid));
    }

    #[test]
    fn rtpfb_nack_too_short() {
        // RTPFB FMT=1 (NACK) but payload < 8 bytes -> TooShort.
        let payload = be32(0xDEAD_BEEF).to_vec(); // only sender_ssrc (4 bytes)
        let pkt = mk_packet(2, false, 1, PT_RTPFB, &payload);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::TooShort));
    }

    #[test]
    fn psfb_invalid_fmt() {
        // PSFB with FMT != 1 (e.g., 3) should return Invalid.
        let payload = [be32(0x11111111), be32(0x22222222)].concat(); // sender + media
        let pkt = mk_packet(2, false, 3, PT_PSFB, &payload);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::Invalid));
    }

    #[test]
    fn psfb_too_short() {
        // PSFB FMT=1 (PLI) but payload < 8 bytes -> TooShort.
        let payload = be32(0x11111111).to_vec(); // only sender_ssrc
        let pkt = mk_packet(2, false, 1, PT_PSFB, &payload);
        let err = RtcpPacket::decode_compound(&pkt).unwrap_err();
        assert!(matches!(err, RtcpError::TooShort));
    }

    // --- sanity round-trips (encode + decode) to ensure happy paths ----------

    #[test]
    fn roundtrip_pli_and_bye_and_app_compound() {
        let pli = RtcpPacket::Pli(PictureLossIndication {
            sender_ssrc: 0xAABBCCDD,
            media_ssrc: 0x11223344,
        });
        let bye = RtcpPacket::Bye(Bye {
            sources: vec![0xDEADBEEF],
            reason: Some("bye!".into()),
        });
        let app = RtcpPacket::App(App {
            subtype: 7,
            name: *b"TEST",
            ssrc: 0x12345678,
            data: vec![1, 2, 3, 4, 5, 6, 7, 8],
        });

        let enc = RtcpPacket::encode_compound(&[pli.clone(), bye.clone(), app.clone()]);
        let dec = RtcpPacket::decode_compound(&enc).expect("decode compound");

        assert_eq!(dec.len(), 3);
        // preserve order and kind
        match &dec[0] {
            RtcpPacket::Pli(p) => {
                assert_eq!(p.sender_ssrc, 0xAABBCCDD);
                assert_eq!(p.media_ssrc, 0x11223344);
            }
            _ => panic!("expected PLI"),
        }
        match &dec[1] {
            RtcpPacket::Bye(b) => {
                assert_eq!(b.sources, vec![0xDEADBEEF]);
                assert_eq!(b.reason.as_deref(), Some("bye!"));
            }
            _ => panic!("expected BYE"),
        }
        match &dec[2] {
            RtcpPacket::App(a) => {
                assert_eq!(a.subtype, 7);
                assert_eq!(&a.name, b"TEST");
                assert_eq!(a.ssrc, 0x12345678);
                assert_eq!(a.data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
            }
            _ => panic!("expected APP"),
        }
    }

    #[test]
    fn roundtrip_sr_and_rr_and_sdes() {
        // Minimal SR (no report blocks)
        let sr = RtcpPacket::Sr(SenderReport {
            ssrc: 0x01020304,
            info: SenderInfo {
                ntp_msw: 0x11111111,
                ntp_lsw: 0x22222222,
                rtp_ts: 0x33333333,
                packet_count: 10,
                octet_count: 1000,
            },
            reports: vec![],
            profile_ext: vec![],
        });

        // Minimal RR (no report blocks)
        let rr = RtcpPacket::Rr(ReceiverReport {
            ssrc: 0x0A0B0C0D,
            reports: vec![],
            profile_ext: vec![],
        });

        // SDES with one CNAME item
        let sdes = RtcpPacket::Sdes(Sdes {
            chunks: vec![SdesChunk {
                ssrc: 0xF0E0D0C0,
                items: vec![SdesItem::Cname("alice@example.com".into())],
            }],
        });

        let enc = RtcpPacket::encode_compound(&[sr.clone(), rr.clone(), sdes.clone()]);
        let dec = RtcpPacket::decode_compound(&enc).expect("decode compound");
        assert_eq!(dec.len(), 3);
        matches!(&dec[0], RtcpPacket::Sr(_));
        matches!(&dec[1], RtcpPacket::Rr(_));
        matches!(&dec[2], RtcpPacket::Sdes(_));
    }

    #[test]
    fn roundtrip_rtpfb_nack_single_entry() {
        let nack = RtcpPacket::Nack(GenericNack {
            sender_ssrc: 0x11112222,
            media_ssrc: 0x33334444,
            entries: vec![(1000, 0b0000_0000_0000_0011)],
        });

        let enc = RtcpPacket::encode_compound(&[nack.clone()]);
        let dec = RtcpPacket::decode_compound(&enc).expect("decode");
        assert_eq!(dec.len(), 1);
        match &dec[0] {
            RtcpPacket::Nack(n) => {
                assert_eq!(n.sender_ssrc, 0x11112222);
                assert_eq!(n.media_ssrc, 0x33334444);
                assert_eq!(n.entries, vec![(1000, 0b11)]);
            }
            _ => panic!("expected NACK"),
        }
    }
}
