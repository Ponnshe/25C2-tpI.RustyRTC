use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;
use std::io::Seek;

/// Parses a raw SCTP packet and returns a summary string (TSN, SSN, etc.).
/// This is used for debug logging.
pub fn parse_sctp_packet_summary(packet: &[u8]) -> String {
    let mut cursor = Cursor::new(packet);

    // Skip Common Header (Source Port(2) + Dest Port(2) + Verification Tag(4) + Checksum(4))
    if packet.len() < 12 {
        return "Invalid SCTP Packet (too short)".to_string();
    }
    cursor.set_position(12);

    let mut summary = String::new();
    let mut chunk_count = 0;

    // Parse Chunks
    while cursor.position() < packet.len() as u64 {
        // Chunk Header: Type(1) + Flags(1) + Length(2)
        let Ok(chunk_type) = cursor.read_u8() else {
            break;
        };
        let Ok(_chunk_flags) = cursor.read_u8() else {
            break;
        };
        let Ok(chunk_length) = cursor.read_u16::<BigEndian>() else {
            break;
        };

        if chunk_length < 4 {
            summary.push_str(&format!("[BadChunkLen:{}]", chunk_length));
            break;
        }

        // DATA Chunk Type is 0
        if chunk_type == 0 {
            // DATA Chunk:
            // TSN(4) + Stream ID(2) + SSN(2) + PPI(4) + Payload Data...
            if chunk_length < 16 {
                summary.push_str("[BadDataChunk]");
            } else {
                if let Ok(tsn) = cursor.read_u32::<BigEndian>() {
                    if let Ok(sid) = cursor.read_u16::<BigEndian>() {
                        if let Ok(ssn) = cursor.read_u16::<BigEndian>() {
                            // Skipping PPI(4) to just advance
                            let _ = cursor.read_u32::<BigEndian>(); // PPI
                            summary
                                .push_str(&format!("[DATA:TSN={},SID={},SSN={}]", tsn, sid, ssn));
                        }
                    }
                }
            }
        } else if chunk_type == 6 {
            summary.push_str("[ABORT]");
        } else if chunk_type == 7 {
            summary.push_str("[SHUTDOWN]");
        } else if chunk_type == 14 {
            summary.push_str("[SHUTDOWN_ACK]");
        } else if chunk_type == 1 {
            summary.push_str("[INIT]");
        } else if chunk_type == 2 {
            summary.push_str("[INIT_ACK]");
        } else if chunk_type == 3 {
            summary.push_str("[SACK]");
        } else if chunk_type == 4 {
            summary.push_str("[HEARTBEAT]");
        } else if chunk_type == 5 {
            summary.push_str("[HEARTBEAT_ACK]");
        } else {
            summary.push_str(&format!("[Type:{}]", chunk_type));
        }

        chunk_count += 1;

        // Move to next chunk (padded to 4 bytes boundary)
        let current_pos = cursor.position();

        let bytes_read_in_body = if chunk_type == 0 { 12 } else { 0 };
        let remaining_in_chunk = (chunk_length as i64) - 4 - bytes_read_in_body;

        if remaining_in_chunk < 0 {
            // Should not happen if length check passed, but safety first
            break;
        }

        if let Err(_) = cursor.seek(std::io::SeekFrom::Current(remaining_in_chunk)) {
            break;
        }

        // Padding
        let padding = (4 - (chunk_length % 4)) % 4;
        if padding > 0 {
            if let Err(_) = cursor.seek(std::io::SeekFrom::Current(padding as i64)) {
                break;
            }
        }
    }

    if summary.is_empty() {
        summary = "Empty/NoChunks".to_string();
    }

    format!("Count:{} Details:{}", chunk_count, summary)
}
