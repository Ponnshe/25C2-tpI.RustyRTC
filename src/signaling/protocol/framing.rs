use super::{FrameError, MsgType, PROTO_VERSION, ProtoError};
use std::io::{self, Read, Write};

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

    r.read_exact(&mut header)?; // io::Error -> FrameError::Io

    let ver = header[0];
    if ver != PROTO_VERSION {
        return Err(ProtoError::InvalidFormat("bad proto version").into());
    }

    let msg_type_byte = header[1];

    let msg_type = MsgType::from_u8(msg_type_byte)?; // ProtoError -> FrameError::Proto

    // flags ignored for now
    let len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if len > max_body {
        return Err(ProtoError::TooLarge.into());
    }

    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?; // io::Error -> FrameError::Io

    Ok((msg_type, body))
}
