use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

#[derive(Debug, Clone, PartialEq)]
pub enum SctpProtocolMessage {
    Offer {
        id: u32,
        filename: String,
        file_size: u64,
    },
    Accept {
        id: u32,
    },
    Reject {
        id: u32,
    },
    Cancel {
        id: u32,
    },
    Chunk {
        id: u32,
        seq: u64,
        payload: Vec<u8>,
    },
    EndFile {
        id: u32,
    },
}

impl SctpProtocolMessage {
    const TYPE_OFFER: u8 = 1;
    const TYPE_ACCEPT: u8 = 2;
    const TYPE_REJECT: u8 = 3;
    const TYPE_CANCEL: u8 = 4;
    const TYPE_CHUNK: u8 = 5;
    const TYPE_END_FILE: u8 = 6;

    pub fn serialize(&self) -> Result<Vec<u8>, std::io::Error> {
        let mut buf = Vec::new();
        match self {
            SctpProtocolMessage::Offer { id, filename, file_size } => {
                buf.write_u8(Self::TYPE_OFFER)?;
                buf.write_u32::<BigEndian>(*id)?;
                buf.write_u64::<BigEndian>(*file_size)?;
                let filename_bytes = filename.as_bytes();
                buf.write_u16::<BigEndian>(filename_bytes.len() as u16)?;
                buf.write_all(filename_bytes)?;
            }
            SctpProtocolMessage::Accept { id } => {
                buf.write_u8(Self::TYPE_ACCEPT)?;
                buf.write_u32::<BigEndian>(*id)?;
            }
            SctpProtocolMessage::Reject { id } => {
                buf.write_u8(Self::TYPE_REJECT)?;
                buf.write_u32::<BigEndian>(*id)?;
            }
            SctpProtocolMessage::Cancel { id } => {
                buf.write_u8(Self::TYPE_CANCEL)?;
                buf.write_u32::<BigEndian>(*id)?;
            }
            SctpProtocolMessage::Chunk { id, seq, payload } => {
                buf.write_u8(Self::TYPE_CHUNK)?;
                buf.write_u32::<BigEndian>(*id)?;
                buf.write_u64::<BigEndian>(*seq)?;
                buf.write_u32::<BigEndian>(payload.len() as u32)?;
                buf.write_all(payload)?;
            }
            SctpProtocolMessage::EndFile { id } => {
                buf.write_u8(Self::TYPE_END_FILE)?;
                buf.write_u32::<BigEndian>(*id)?;
            }
        }
        Ok(buf)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, std::io::Error> {
        // println!("[CLI DEBUG] SctpProtocolMessage::deserialize len={}", data.len());
        let mut cursor = Cursor::new(data);
        let msg_type = cursor.read_u8()?;

        match msg_type {
            Self::TYPE_OFFER => {
                let id = cursor.read_u32::<BigEndian>()?;
                let file_size = cursor.read_u64::<BigEndian>()?;
                let filename_len = cursor.read_u16::<BigEndian>()?;
                let mut filename_bytes = vec![0u8; filename_len as usize];
                cursor.read_exact(&mut filename_bytes)?;
                let filename = String::from_utf8(filename_bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                Ok(SctpProtocolMessage::Offer { id, filename, file_size })
            }
            Self::TYPE_ACCEPT => {
                let id = cursor.read_u32::<BigEndian>()?;
                Ok(SctpProtocolMessage::Accept { id })
            }
            Self::TYPE_REJECT => {
                let id = cursor.read_u32::<BigEndian>()?;
                Ok(SctpProtocolMessage::Reject { id })
            }
            Self::TYPE_CANCEL => {
                let id = cursor.read_u32::<BigEndian>()?;
                Ok(SctpProtocolMessage::Cancel { id })
            }
            Self::TYPE_CHUNK => {
                let id = cursor.read_u32::<BigEndian>()?;
                let seq = cursor.read_u64::<BigEndian>()?;
                let payload_len = cursor.read_u32::<BigEndian>()?;
                let mut payload = vec![0u8; payload_len as usize];
                cursor.read_exact(&mut payload)?;
                Ok(SctpProtocolMessage::Chunk { id, seq, payload })
            }
            Self::TYPE_END_FILE => {
                let id = cursor.read_u32::<BigEndian>()?;
                Ok(SctpProtocolMessage::EndFile { id })
            }
            unknown_type => {
                println!("[CLI DEBUG] Unknown SCTP message type: {}", unknown_type);
                Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Unknown message type: {}", unknown_type)))
            },
        }
    }
}
