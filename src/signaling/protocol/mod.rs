use std::io::{self, Read, Write};

/// Protocol
/// ----------- Header -----------------
/// Version (1B) - Msg Type (1B) - Flags (2B)
/// Body Length (2B)
/// ----------- Body -------------------
/// Payload (1MiB)
/// Protocol version (first byte in the frame header).
pub const PROTO_VERSION: u8 = 1;

/// Maximum allowed body size for a frame (to avoid OOM).
pub const MAX_BODY_LEN: usize = 1_048_576; // 1 MiB

// ---- Basic types ----------------------------------------------------------

pub type UserName = String;
pub type SessionId = String;
pub type SessionCode = String;
pub type TxnId = u64; // for offer/answer reliability

// ---- Message type byte ----------------------------------------------------

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MsgType {
    Hello = 0x01,
    Login = 0x02,
    LoginOk = 0x03,
    LoginErr = 0x04,

    CreateSession = 0x10,
    Created = 0x11,
    Join = 0x12,
    JoinOk = 0x13,
    JoinErr = 0x14,

    Offer = 0x20,
    Answer = 0x21,
    Candidate = 0x22,
    Ack = 0x23,
    Bye = 0x24,

    Ping = 0x30,
    Pong = 0x31,
}

impl MsgType {
    pub fn from_u8(v: u8) -> Result<MsgType, ProtoError> {
        use MsgType::*;
        match v {
            0x01 => Ok(Hello),
            0x02 => Ok(Login),
            0x03 => Ok(LoginOk),
            0x04 => Ok(LoginErr),
            0x10 => Ok(CreateSession),
            0x11 => Ok(Created),
            0x12 => Ok(Join),
            0x13 => Ok(JoinOk),
            0x14 => Ok(JoinErr),
            0x20 => Ok(Offer),
            0x21 => Ok(Answer),
            0x22 => Ok(Candidate),
            0x23 => Ok(Ack),
            0x24 => Ok(Bye),
            0x30 => Ok(Ping),
            0x31 => Ok(Pong),
            other => Err(ProtoError::UnknownType(other)),
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

// ---- Error Type ----------------------------------------

#[derive(Debug)]
pub enum ProtoError {
    UnknownType(u8),
    Truncated,
    InvalidUtf8,
    TooLarge,
    InvalidFormat(&'static str),
    StringTooLong { max: usize, actual: usize },
}

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    Proto(ProtoError),
}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> Self {
        FrameError::Io(e)
    }
}

impl From<ProtoError> for FrameError {
    fn from(e: ProtoError) -> Self {
        FrameError::Proto(e)
    }
}

// ---- Public message enum --------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum Msg {
    // Handshake / auth
    Hello {
        client_version: String,
    },
    Login {
        username: UserName,
        password: String, // plain text, but sent over TLS
    },
    LoginOk {
        username: UserName,
    },
    LoginErr {
        code: u16, // map to our AuthErrorCode later
    },

    // Session management
    CreateSession {
        capacity: u8,
    },
    Created {
        session_id: SessionId,
        session_code: SessionCode,
    },
    Join {
        session_code: SessionCode,
    },
    JoinOk {
        session_id: SessionId,
    },
    JoinErr {
        code: u16, // map to JoinErrorCode
    },

    // Signaling
    Offer {
        txn_id: TxnId,
        to: UserName, // for now, PeerId = username
        sdp: Vec<u8>, // raw UTF-8 text
    },
    Answer {
        txn_id: TxnId,
        to: UserName,
        sdp: Vec<u8>,
    },
    Candidate {
        to: UserName,
        mid: String,
        mline_index: u16,
        cand: Vec<u8>, // raw UTF-8 text
    },
    Ack {
        txn_id: TxnId,
    },
    Bye {
        reason: Option<String>,
    },

    // Keepalive
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
}

/// High-level: write a full framed Msg to the wire.
pub fn write_msg<W: Write>(w: &mut W, msg: &Msg) -> Result<(), FrameError> {
    let (msg_type, body) = encode_msg(msg)?;
    write_frame(w, msg_type, &body)?;
    Ok(())
}

/// High-level: read a full framed Msg from the wire.
pub fn read_msg<R: Read>(r: &mut R) -> Result<Msg, FrameError> {
    let (ty, body) = read_frame(r, MAX_BODY_LEN)?;
    let msg = decode_msg(ty, &body)?;
    Ok(msg)
}

// ---- Encode to body bytes -------------------------------------------------
pub fn encode_msg(msg: &Msg) -> Result<(MsgType, Vec<u8>), ProtoError> {
    use Msg::*;
    let mut body = Vec::new();

    let msg_type = match msg {
        Hello { client_version } => {
            put_str16(&mut body, client_version)?;
            MsgType::Hello
        }
        Login { username, password } => {
            put_str16(&mut body, username)?;
            put_str16(&mut body, password)?;
            MsgType::Login
        }
        LoginOk { username } => {
            put_str16(&mut body, username)?;
            MsgType::LoginOk
        }
        LoginErr { code } => {
            put_u16(&mut body, *code);
            MsgType::LoginErr
        }

        CreateSession { capacity } => {
            put_u8(&mut body, *capacity);
            MsgType::CreateSession
        }
        Created {
            session_id,
            session_code,
        } => {
            put_str16(&mut body, session_id)?;
            put_str16(&mut body, session_code)?;
            MsgType::Created
        }
        Join { session_code } => {
            put_str16(&mut body, session_code)?;
            MsgType::Join
        }
        JoinOk { session_id } => {
            put_str16(&mut body, session_id)?;
            MsgType::JoinOk
        }
        JoinErr { code } => {
            put_u16(&mut body, *code);
            MsgType::JoinErr
        }

        Offer { txn_id, to, sdp } => {
            put_u64(&mut body, *txn_id);
            put_str16(&mut body, to)?;
            put_u32(&mut body, sdp.len() as u32);
            body.extend_from_slice(sdp);
            MsgType::Offer
        }
        Answer { txn_id, to, sdp } => {
            put_u64(&mut body, *txn_id);
            put_str16(&mut body, to)?;
            put_u32(&mut body, sdp.len() as u32);
            body.extend_from_slice(sdp);
            MsgType::Answer
        }
        Candidate {
            to,
            mid,
            mline_index,
            cand,
        } => {
            put_str16(&mut body, to)?;
            put_str16(&mut body, mid)?;
            put_u16(&mut body, *mline_index);
            put_u32(&mut body, cand.len() as u32);
            body.extend_from_slice(cand);
            MsgType::Candidate
        }
        Ack { txn_id } => {
            put_u64(&mut body, *txn_id);
            MsgType::Ack
        }
        Bye { reason } => {
            match reason {
                Some(s) => put_str16(&mut body, s)?,
                None => put_u16(&mut body, 0), // len=0 string
            }
            MsgType::Bye
        }
        Ping { nonce } => {
            put_u64(&mut body, *nonce);
            MsgType::Ping
        }
        Pong { nonce } => {
            put_u64(&mut body, *nonce);
            MsgType::Pong
        }
    };

    Ok((msg_type, body))
}

// ---- Decode from body bytes ----------------------------------------------
pub fn decode_msg(msg_type: MsgType, body: &[u8]) -> Result<Msg, ProtoError> {
    use Msg::*;
    let mut cursor = Cursor::new(body);

    let msg = match msg_type {
        MsgType::Hello => {
            let v = cursor.get_str16()?.to_owned();
            Hello { client_version: v }
        }
        MsgType::Login => {
            let u = cursor.get_str16()?.to_owned();
            let pw = cursor.get_str16()?.to_owned();
            Login {
                username: u,
                password: pw,
            }
        }
        MsgType::LoginOk => {
            let u = cursor.get_str16()?.to_owned();
            LoginOk { username: u }
        }
        MsgType::LoginErr => {
            let code = cursor.get_u16()?;
            LoginErr { code }
        }

        MsgType::CreateSession => {
            let cap = cursor.get_u8()?;
            CreateSession { capacity: cap }
        }
        MsgType::Created => {
            let sid = cursor.get_str16()?.to_owned();
            let scode = cursor.get_str16()?.to_owned();
            Created {
                session_id: sid,
                session_code: scode,
            }
        }
        MsgType::Join => {
            let scode = cursor.get_str16()?.to_owned();
            Join {
                session_code: scode,
            }
        }
        MsgType::JoinOk => {
            let sid = cursor.get_str16()?.to_owned();
            JoinOk { session_id: sid }
        }
        MsgType::JoinErr => {
            let code = cursor.get_u16()?;
            JoinErr { code }
        }

        MsgType::Offer => {
            let txn_id = cursor.get_u64()?;
            let to = cursor.get_str16()?.to_owned();
            let len = cursor.get_u32()? as usize;
            let sdp = cursor.get_bytes(len)?.to_vec();
            Offer { txn_id, to, sdp }
        }
        MsgType::Answer => {
            let txn_id = cursor.get_u64()?;
            let to = cursor.get_str16()?.to_owned();
            let len = cursor.get_u32()? as usize;
            let sdp = cursor.get_bytes(len)?.to_vec();
            Answer { txn_id, to, sdp }
        }
        MsgType::Candidate => {
            let to = cursor.get_str16()?.to_owned();
            let mid = cursor.get_str16()?.to_owned();
            let mline_index = cursor.get_u16()?;
            let len = cursor.get_u32()? as usize;
            let cand = cursor.get_bytes(len)?.to_vec();
            Candidate {
                to,
                mid,
                mline_index,
                cand,
            }
        }
        MsgType::Ack => {
            let txn_id = cursor.get_u64()?;
            Ack { txn_id }
        }
        MsgType::Bye => {
            let s = cursor.get_str16()?.to_owned();
            let reason = if s.is_empty() { None } else { Some(s) };
            Bye { reason }
        }
        MsgType::Ping => {
            let nonce = cursor.get_u64()?;
            Ping { nonce }
        }
        MsgType::Pong => {
            let nonce = cursor.get_u64()?;
            Pong { nonce }
        }
    };

    // Optional strictness: fail if there are unexpected trailing bytes
    cursor.finish()?;

    Ok(msg)
}

// ---- Frame IO: version + type + body_len + body --------------------------

/// Write a single frame: [ver][type][reserved u16=0][len u32][body...]
pub fn write_frame<W: Write>(w: &mut W, msg_type: MsgType, body: &[u8]) -> io::Result<()> {
    if body.len() > u32::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "body too large",
        ));
    }
    let len = body.len() as u32;
    let mut header = [0u8; 8];
    header[0] = PROTO_VERSION;
    header[1] = msg_type.as_u8();
    header[2] = 0;
    header[3] = 0;
    header[4..8].copy_from_slice(&len.to_be_bytes());
    w.write_all(&header)?;
    w.write_all(body)?;
    w.flush()?;
    Ok(())
}

/// Read a single frame, enforcing a max body length.
pub fn read_frame<R: Read>(r: &mut R, max_body: usize) -> Result<(MsgType, Vec<u8>), FrameError> {
    let mut header = [0u8; 8];

    r.read_exact(&mut header)?;

    let ver = header[0];
    if ver != PROTO_VERSION {
        return Err(ProtoError::InvalidFormat("bad proto version").into());
    }

    let msg_type_byte = header[1];

    let msg_type = MsgType::from_u8(msg_type_byte)?;

    let len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if len > max_body {
        return Err(ProtoError::TooLarge.into());
    }

    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;

    Ok((msg_type, body))
}

// ---- Primitive read/write helpers ----------------------------------------

fn put_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_be_bytes());
}

fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// str16 = u16 length + UTF-8 bytes
fn put_str16(buf: &mut Vec<u8>, s: &str) -> Result<(), ProtoError> {
    let bytes = s.as_bytes();
    let len = bytes.len();

    if len > u16::MAX as usize {
        return Err(ProtoError::StringTooLong {
            max: u16::MAX as usize,
            actual: len,
        });
    }

    put_u16(buf, len as u16);
    buf.extend_from_slice(bytes);
    Ok(())
}

#[derive(Debug)]
struct Cursor<'a> {
    buf: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn remaining(&self) -> usize {
        self.buf.len()
    }

    fn get_u8(&mut self) -> Result<u8, ProtoError> {
        if self.buf.is_empty() {
            return Err(ProtoError::Truncated);
        }
        let (head, rest) = self.buf.split_at(1);
        self.buf = rest;
        Ok(head[0])
    }

    fn get_u16(&mut self) -> Result<u16, ProtoError> {
        if self.buf.len() < 2 {
            return Err(ProtoError::Truncated);
        }
        let (head, rest) = self.buf.split_at(2);
        self.buf = rest;
        Ok(u16::from_be_bytes([head[0], head[1]]))
    }

    fn get_u32(&mut self) -> Result<u32, ProtoError> {
        if self.buf.len() < 4 {
            return Err(ProtoError::Truncated);
        }
        let (head, rest) = self.buf.split_at(4);
        self.buf = rest;
        Ok(u32::from_be_bytes([head[0], head[1], head[2], head[3]]))
    }

    fn get_u64(&mut self) -> Result<u64, ProtoError> {
        if self.buf.len() < 8 {
            return Err(ProtoError::Truncated);
        }
        let (head, rest) = self.buf.split_at(8);
        self.buf = rest;
        Ok(u64::from_be_bytes([
            head[0], head[1], head[2], head[3], head[4], head[5], head[6], head[7],
        ]))
    }

    fn get_bytes(&mut self, len: usize) -> Result<&'a [u8], ProtoError> {
        if self.buf.len() < len {
            return Err(ProtoError::Truncated);
        }
        let (head, rest) = self.buf.split_at(len);
        self.buf = rest;
        Ok(head)
    }

    /// Read str16 = u16 length + UTF-8 bytes
    fn get_str16(&mut self) -> Result<&'a str, ProtoError> {
        let len = self.get_u16()? as usize;
        let bytes = self.get_bytes(len)?;
        str::from_utf8(bytes).map_err(|_| ProtoError::InvalidUtf8)
    }

    /// enforce that we've consumed the whole body.
    fn finish(self) -> Result<(), ProtoError> {
        if !self.buf.is_empty() {
            Err(ProtoError::InvalidFormat("trailing bytes in message body"))
        } else {
            Ok(())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor as IoCursor;

    // ---------- Helpers ----------

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

        // Should not error
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
        put_u16(&mut body, 5);
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
        put_u16(&mut body, 1);
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
        put_u64(&mut body, 123);
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
