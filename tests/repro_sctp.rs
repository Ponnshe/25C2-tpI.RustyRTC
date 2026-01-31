use byteorder::{BigEndian, WriteBytesExt};
use rustyrtc::sctp::debug_utils::parse_sctp_packet_summary;
use std::io::Write;

#[test]
fn test_repro_missing_padding_theory() {
    let mut packet = Vec::new();

    // 1. Common Header (12 bytes)
    packet.write_u16::<BigEndian>(5000).unwrap();
    packet.write_u16::<BigEndian>(5000).unwrap();
    packet.write_u32::<BigEndian>(0).unwrap();
    packet.write_u32::<BigEndian>(0).unwrap();

    // 2. Chunk 1: DATA
    // We want Length % 4 == 1, so padding is 3.
    // Let's say payload data is 5 bytes.
    // Header(4) + TSN(4)+SID(2)+SSN(2)+PPI(4) + Data(5) = 21 bytes.
    // Length field = 21 (0x0015).
    // 21 % 4 = 1. Padding needed = 3.
    let chunk_len = 21;
    packet.write_u8(0).unwrap(); // Type DATA
    packet.write_u8(0).unwrap(); // Flags
    packet.write_u16::<BigEndian>(chunk_len).unwrap(); // Length

    packet.write_u32::<BigEndian>(2092334635).unwrap(); // TSN from log
    packet.write_u16::<BigEndian>(0).unwrap(); // SID
    packet.write_u16::<BigEndian>(1).unwrap(); // SSN
    packet.write_u32::<BigEndian>(0).unwrap(); // PPI

    packet.write_all(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]).unwrap(); // 5 bytes data

    // CRITICAL: DO NOT WRITE PADDING.
    // Spec requires 3 bytes of padding here. We skip it to simulate the bug.

    // 3. Chunk 2: DATA (Simulated)
    // We want Header bytes to be misread.
    // Expected Header: Type(0) Flags(0) Len(1043=0x0413) ...
    // Bytes: 00 00 04 13 ...
    // Parser seeks 3 bytes (thinking they are padding).
    // Parser skips 00 00 04.
    // Parser reads 13 as Type. 13 hex = 19 decimal.
    packet.write_u8(0).unwrap(); // Type
    packet.write_u8(0).unwrap(); // Flags
    packet.write_u16::<BigEndian>(1043).unwrap(); // Length 0x0413
    packet.write_u32::<BigEndian>(123456).unwrap(); // TSN
    packet.write_u16::<BigEndian>(0).unwrap(); // SID
    packet.write_u16::<BigEndian>(2).unwrap(); // SSN
    packet.write_u32::<BigEndian>(0).unwrap(); // PPI
    // Payload...
    packet.write_all(&vec![0u8; 1000]).unwrap();

    let summary = parse_sctp_packet_summary(&packet);
    println!("Summary: {}", summary);

    // Verify if we reproduced the Type:19 error
    assert!(
        summary.contains("Type:19"),
        "Did not reproduce Type:19. Got: {}",
        summary
    );
    assert!(
        summary.contains("DATA:TSN=2092334635"),
        "First chunk not parsed?"
    );
}
