use std::io::{Read, Write};

mod codec;
mod constants;
mod errors;
mod framing;
mod types;

pub use codec::{decode_msg, encode_msg};
pub use constants::{MAX_BODY_LEN, PROTO_VERSION};
pub use errors::{FrameError, ProtoError};
pub use framing::{read_frame, write_frame};
pub use types::{Msg, MsgType, SessionCode, SessionId, TxnId, UserName};

/// High-level: write a full framed Msg to the wire.
pub fn write_msg<W: Write>(w: &mut W, msg: &Msg) -> Result<(), FrameError> {
    let (msg_type, body) = encode_msg(msg)?;
    write_frame(w, msg_type, &body)?;
    Ok(())
}

/// High-level: read a full framed Msg from the wire.
pub fn read_msg<R: Read>(r: &mut R) -> Result<Msg, FrameError> {
    let (msg_type, body) = read_frame(r, MAX_BODY_LEN)?;
    let msg = decode_msg(msg_type, &body)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor as IoCursor;

    fn roundtrip(msg: &Msg) -> Msg {
        let mut buf = IoCursor::new(Vec::<u8>::new());
        write_msg(&mut buf, msg).expect("write_msg failed");
        buf.set_position(0);
        read_msg(&mut buf).expect("read_msg failed")
    }

    // ---------- Happy-path roundtrips ----------

    #[test]
    fn roundtrip_hello() {
        let original = Msg::Hello {
            client_version: "roomrtc-0.1".to_string(),
        };

        let decoded = roundtrip(&original);
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_login() {
        let original = Msg::Login {
            username: "alice".to_string(),
            password: "secret".to_string(),
        };

        let decoded = roundtrip(&original);
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_created() {
        let original = Msg::Created {
            session_id: "sess-123".to_string(),
            session_code: "ABCD12".to_string(),
        };

        let decoded = roundtrip(&original);
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_joinok() {
        let original = Msg::JoinOk {
            session_id: "sess-123".to_string(),
        };

        let decoded = roundtrip(&original);
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_offer_answer_candidate() {
        let sdp = b"v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\n".to_vec();
        let cand = b"candidate:1 1 udp 2122252543 192.0.2.1 54400 typ host".to_vec();

        let offer = Msg::Offer {
            txn_id: 42,
            to: "bob".to_string(),
            sdp: sdp.clone(),
        };
        let decoded_offer = roundtrip(&offer);
        assert_eq!(decoded_offer, offer);

        let answer = Msg::Answer {
            txn_id: 43,
            to: "alice".to_string(),
            sdp: sdp.clone(),
        };
        let decoded_answer = roundtrip(&answer);
        assert_eq!(decoded_answer, answer);

        let candidate = Msg::Candidate {
            to: "bob".to_string(),
            mid: "0".to_string(),
            mline_index: 0,
            cand: cand.clone(),
        };
        let decoded_candidate = roundtrip(&candidate);
        assert_eq!(decoded_candidate, candidate);
    }

    #[test]
    fn roundtrip_bye_some_and_none() {
        let bye_some = Msg::Bye {
            reason: Some("done".to_string()),
        };
        let decoded_some = roundtrip(&bye_some);
        assert_eq!(decoded_some, bye_some);

        let bye_none = Msg::Bye { reason: None };
        let decoded_none = roundtrip(&bye_none);
        assert_eq!(decoded_none, bye_none);
    }

    #[test]
    fn roundtrip_ping_pong() {
        let ping = Msg::Ping { nonce: 123 };
        let pong = Msg::Pong { nonce: 456 };

        let decoded_ping = roundtrip(&ping);
        let decoded_pong = roundtrip(&pong);

        assert_eq!(decoded_ping, ping);
        assert_eq!(decoded_pong, pong);
    }

    // ---------- Encoding border cases ----------

    #[test]
    fn encode_str16_exact_u16_max_ok() {
        let s = "x".repeat(u16::MAX as usize); // exactly max size
        let msg = Msg::Hello { client_version: s };

        let res = encode_msg(&msg);
        assert!(res.is_ok(), "encode_msg should accept exact u16::MAX len");
    }

    #[test]
    fn encode_str16_too_long_fails() {
        let s = "x".repeat(u16::MAX as usize + 1);
        let msg = Msg::Hello {
            client_version: s.clone(),
        };

        let err = encode_msg(&msg).unwrap_err();
        match err {
            ProtoError::StringTooLong { max, actual } => {
                assert_eq!(max, u16::MAX as usize);
                assert_eq!(actual, s.len());
            }
            other => panic!("expected StringTooLong, got {:?}", other),
        }
    }

    // ---------- decode_msg errors (body-level) ----------

    #[test]
    fn decode_truncated_hello() {
        // len=5, but only 2 bytes follow => Truncated
        let mut body = Vec::new();
        body.extend_from_slice(&5u16.to_be_bytes());
        body.extend_from_slice(b"ab"); // 2 < 5

        let res = decode_msg(MsgType::Hello, &body);
        match res {
            Err(ProtoError::Truncated) => {}
            other => panic!("expected Truncated, got {:?}", other),
        }
    }

    #[test]
    fn decode_invalid_utf8_in_str16() {
        // len=1, byte=0xFF (invalid utf8)
        let mut body = Vec::new();
        body.extend_from_slice(&1u16.to_be_bytes());
        body.push(0xFF);

        let res = decode_msg(MsgType::Hello, &body);
        match res {
            Err(ProtoError::InvalidUtf8) => {}
            other => panic!("expected InvalidUtf8, got {:?}", other),
        }
    }

    #[test]
    fn decode_trailing_bytes_fails() {
        // Ping = u64 nonce; add an extra byte at the end
        let mut body = Vec::new();
        body.extend_from_slice(&123u64.to_be_bytes());
        body.push(0x00); // extra

        let res = decode_msg(MsgType::Ping, &body);
        match res {
            Err(ProtoError::InvalidFormat(msg)) => {
                assert_eq!(msg, "trailing bytes in message body");
            }
            other => panic!("expected InvalidFormat(trailing bytes), got {:?}", other),
        }
    }

    // ---------- read_frame / frame-level errors ----------

    #[test]
    fn read_frame_rejects_bad_version() {
        let mut header = [0u8; 8];
        header[0] = PROTO_VERSION.wrapping_add(1); // wrong version
        header[1] = MsgType::Ping.as_u8();
        header[2] = 0;
        header[3] = 0;
        header[4..8].copy_from_slice(&0u32.to_be_bytes()); // body len = 0

        let mut cursor = IoCursor::new(header.to_vec());
        let res = read_frame(&mut cursor, MAX_BODY_LEN);

        match res {
            Err(FrameError::Proto(ProtoError::InvalidFormat(msg))) => {
                assert_eq!(msg, "bad proto version");
            }
            other => panic!("expected InvalidFormat(bad proto version), got {:?}", other),
        }
    }

    #[test]
    fn read_frame_unknown_msg_type() {
        let mut header = [0u8; 8];
        header[0] = PROTO_VERSION;
        header[1] = 0xFF; // unknown type
        header[2] = 0;
        header[3] = 0;
        header[4..8].copy_from_slice(&0u32.to_be_bytes()); // body len = 0

        let mut cursor = IoCursor::new(header.to_vec());
        let res = read_frame(&mut cursor, MAX_BODY_LEN);

        match res {
            Err(FrameError::Proto(ProtoError::UnknownType(0xFF))) => {}
            other => panic!("expected UnknownType(0xFF), got {:?}", other),
        }
    }

    #[test]
    fn read_frame_too_large() {
        // Build a valid Ping frame via write_frame, then read with a tiny max_body
        let msg = Msg::Ping { nonce: 42 };
        let (ty, body) = encode_msg(&msg).unwrap();

        let mut buf = IoCursor::new(Vec::<u8>::new());
        write_frame(&mut buf, ty, &body).unwrap();
        buf.set_position(0);

        let res = read_frame(&mut buf, 1); // smaller than body.len()

        match res {
            Err(FrameError::Proto(ProtoError::TooLarge)) => {}
            other => panic!("expected TooLarge, got {:?}", other),
        }
    }
}
