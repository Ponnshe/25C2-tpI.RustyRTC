use super::{Msg, MsgType, ProtoError};
use std::str;

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

    cursor.finish()?;
    Ok(msg)
}

// ---- Primitive write helpers ---------------------------------------------

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

// ---- Cursor for decoding --------------------------------------------------

#[derive(Debug)]
struct Cursor<'a> {
    buf: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    #[allow(dead_code)]
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

    /// Enforce that we've consumed the whole body.
    fn finish(self) -> Result<(), ProtoError> {
        if !self.buf.is_empty() {
            Err(ProtoError::InvalidFormat("trailing bytes in message body"))
        } else {
            Ok(())
        }
    }
}
