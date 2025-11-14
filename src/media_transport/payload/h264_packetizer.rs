//! RFC 6184 H.264 -> RTP packetizer (Single NALU + FU-A).
//!
//! Input  : one Annex-B "access unit" (frame) as a byte slice (may contain multiple NAL units).
//! Output : a vector of RTP payload chunks; each chunk is ready to become an RTP payload.
//!          We also expose a helper to build full `RtpPacket`s if you prefer.
//!
//! Scope  : non-interleaved mode (packetization-mode=1). We support:
//!          - Single NAL Unit packets (no start codes in payload)
//!          - FU-A fragmentation for large NALUs
//!          (STAP-A aggregation is optional and omitted to keep v1 simple).
//!
//! Marker : The `marker` flag is set to true ONLY on the *last* payload chunk of the frame.
//!
//! Notes  : - The packetizer removes Annex-B start codes from the wire format.
//!          - Choose `mtu` (and `rtp_overhead`) so `max_payload = mtu - overhead`
//!            leaves room for headers and possible SRTP auth tags.
//!
//! Typical use (payloads only):
//!   let p = H264Packetizer::new(1200);
//!   let chunks = p.packetize_annexb_to_payloads(annexb_frame);
//!   for (i, ch) in chunks.iter().enumerate() {
//!       let is_last = i + 1 == chunks.len();
//!       // build/send your RTP packet with same timestamp, M = is_last
//!   }
//!
//! Or build full RTP packets (keeps `seq` locally and returns the next value):
//!   let (pkts, next_seq) = p.packetize_annexb_to_rtp(annexb_frame, pt, ts, ssrc, seq_start);

use crate::rtp::rtp_packet::RtpPacket;

use super::rtp_payload_chunk::RtpPayloadChunk;

/// H.264 (RFC 6184) packetizer.
#[derive(Debug, Clone)]
pub struct H264Packetizer {
    mtu: usize,
    /// Bytes reserved for RTP (and friends) that are *not* part of the payload:
    /// - RTP header (12 B)
    /// - any extensions, SRTP tag, etc.
    rtp_overhead: usize,
}

impl H264Packetizer {
    /// Create a packetizer with a target MTU (e.g., 1200) and default RTP overhead of 12 bytes.
    pub fn new(mtu: usize) -> Self {
        Self {
            mtu,
            rtp_overhead: 12,
        }
    }

    /// Override the assumed RTP overhead (header + extensions + SRTP tag if any).
    pub fn with_overhead(mut self, overhead: usize) -> Self {
        self.rtp_overhead = overhead;
        self
    }

    #[inline]
    fn max_payload(&self) -> usize {
        self.mtu.saturating_sub(self.rtp_overhead)
    }

    /// Split an Annex-B access unit (frame) into RTP payload chunks.
    ///
    /// - Removes Annex-B start codes.
    /// - Uses Single-NALU if nal.len() <= max_payload, else FU-A.
    /// - The `marker` flag is true on the *last* returned chunk only.
    pub fn packetize_annexb_to_payloads(&self, annexb_frame: &[u8]) -> Vec<RtpPayloadChunk> {
        let mut out = Vec::new();
        let nalus = split_annexb_nalus_preserve_last_zeros(annexb_frame);
        if nalus.is_empty() {
            return out; // nothing to send
        }
        let max_payload = self.max_payload();

        for (ni, nalu) in nalus.iter().enumerate() {
            if nalu.is_empty() {
                continue;
            }

            if nalu.len() <= max_payload {
                // Single NALU packet: payload is the NALU bytes (no start code).
                out.push(RtpPayloadChunk {
                    bytes: nalu.to_vec(),
                    marker: false, // we'll fix the last one after the loop
                });
            } else {
                // FU-A fragmentation
                // Original header
                let nalu_header = nalu[0];
                let f_bit = nalu_header & 0x80; // usually 0
                let nri = nalu_header & 0x60;
                let ntype = nalu_header & 0x1F;

                // FU Indicator: F | NRI | 28 (FU-A)
                let fu_indicator = f_bit | nri | 28;
                // FU Header base: S/E bits will be set per-fragment; type is original type
                let fu_header_base = ntype;

                // Each FU-A payload reserves 2 bytes for (FU-Ind, FU-Hdr)
                let frag_budget = max_payload.saturating_sub(2);
                if frag_budget == 0 {
                    // Degenerate config; avoid infinite loop
                    continue;
                }

                let mut offset = 1; // skip original NALU header
                let n = nalu.len();

                while offset < n {
                    let remaining = n - offset;
                    let take = remaining.min(frag_budget);

                    let s_bit = if offset == 1 { 0x80 } else { 0x00 };
                    let e_bit = if offset + take == n { 0x40 } else { 0x00 };
                    let fu_header = s_bit | e_bit | fu_header_base;

                    let mut payload = Vec::with_capacity(2 + take);
                    payload.push(fu_indicator);
                    payload.push(fu_header);
                    payload.extend_from_slice(&nalu[offset..offset + take]);

                    out.push(RtpPayloadChunk {
                        bytes: payload,
                        marker: false, // fixed after loop
                    });

                    offset += take;
                }
            }

            // If this NALU was the last NALU of the AU and we already pushed at least one chunk,
            // mark the last emitted chunk as marker=true (end of frame).
            if ni + 1 == nalus.len() {
                if let Some(last) = out.last_mut() {
                    last.marker = true;
                }
            }
        }

        out
    }

    /// Convenience: build full `RtpPacket`s.
    ///
    /// - `payload_type`: your dynamic PT (e.g., 96 for H.264)
    /// - `timestamp`: RTP 90 kHz clock for the *frame* (same for all chunks)
    /// - `ssrc`: sender SSRC
    /// - `seq_start`: first sequence number to use; returns `(packets, next_seq_after)`
    pub fn packetize_annexb_to_rtp(
        &self,
        annexb_frame: &[u8],
        payload_type: u8,
        timestamp: u32,
        ssrc: u32,
        seq_start: u16,
    ) -> (Vec<RtpPacket>, u16) {
        let chunks = self.packetize_annexb_to_payloads(annexb_frame);
        let mut packets = Vec::with_capacity(chunks.len());
        let mut seq = seq_start;

        for ch in chunks {
            let pkt = RtpPacket::simple(payload_type, ch.marker, seq, timestamp, ssrc, ch.bytes);
            packets.push(pkt);
            seq = seq.wrapping_add(1);
        }

        (packets, seq)
    }
}


fn split_annexb_nalus_preserve_last_zeros(data: &[u8]) -> Vec<&[u8]> {
    let (mut sc_pos, mut sc_len) = match find_start_code(data, 0) {
        Some(t) => t,
        None => {
            return if data.is_empty() {
                Vec::new()
            } else {
                vec![data]
            };
        }
    };

    let n = data.len();
    let mut out = Vec::new();

    loop {
        let nal_start = sc_pos + sc_len;
        let next = find_start_code(data, nal_start);
        let nal_end = match next {
            Some((next_sc_pos, _)) => next_sc_pos,
            None => n, // DO NOT trim zeros here (needed for size-based FU-A)
        };

        if nal_end > nal_start {
            out.push(&data[nal_start..nal_end]);
        }

        match next {
            Some((next_sc_pos, next_sc_len)) => {
                sc_pos = next_sc_pos;
                sc_len = next_sc_len;
            }
            None => break,
        }
    }

    out
}

#[inline]
fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    let n = data.len();
    let mut i = from;
    while i + 3 <= n {
        // Prefer 4-byte 00 00 00 01 if present
        if i + 4 <= n && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 0 && data[i + 3] == 1 {
            return Some((i, 4));
        }
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            return Some((i, 3));
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rtp::rtp_error::RtpError;
    use crate::rtp::rtp_packet::RtpPacket;

    // Helper to build Annex-B: [SC][NALU][SC][NALU]...
    fn annexb(nalus: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for n in nalus {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(n);
        }
        out
    }

    #[test]
    fn split_two_nalus() {
        let a = annexb(&[&[0x65, 1, 2, 3], &[0x41, 9, 9]]);
        let v = split_annexb_nalus(&a);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], &[0x65, 1, 2, 3]);
        assert_eq!(v[1], &[0x41, 9, 9]);
    }

    #[test]
    fn split_no_start_code_treats_all_as_one_nalu() {
        let buf = vec![0x65, 1, 2, 3, 4]; // no 00 00 01
        let v = split_annexb_nalus(&buf);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], buf.as_slice());
    }

    #[test]
    fn split_mixed_3_and_4_byte_start_codes_and_trailing_zeros() {
        // 4-byte then 3-byte start code; trailing zeros after last NALU
        let mut a = Vec::new();
        a.extend_from_slice(&[0, 0, 0, 1, 0x67, 0xAA]); // SPS-like
        a.extend_from_slice(&[0, 0, 1, 0x68, 0xBB]); // PPS-like
        a.extend_from_slice(&[0, 0]); // trailing zeros
        let v = split_annexb_nalus(&a);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], &[0x67, 0xAA]);
        assert_eq!(v[1], &[0x68, 0xBB]);
    }

    #[test]
    fn split_ignores_empty_nalus_from_back_to_back_start_codes() {
        // Back-to-back start codes produce empty NALUs; ensure they are ignored.
        let a = [
            0, 0, 1, // 3B start
            0, 0, 1, // 3B start immediately again (empty)
            0x65, 1, 2, 0, 0, 0, 1, // 4B start
            0x41, 3,
        ];
        let v = split_annexb_nalus(&a);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], &[0x65, 1, 2]);
        assert_eq!(v[1], &[0x41, 3]);
    }

    #[test]
    fn packetize_small_nalus_single() {
        let p = H264Packetizer::new(1200);
        let a = annexb(&[&[0x65, 1, 2], &[0x41, 3]]);
        let chunks = p.packetize_annexb_to_payloads(&a);
        assert_eq!(chunks.len(), 2);
        assert!(!chunks[0].marker);
        assert!(chunks[1].marker);
        assert_eq!(chunks[0].bytes, &[0x65, 1, 2]);
        assert_eq!(chunks[1].bytes, &[0x41, 3]);
    }

    #[test]
    fn packetize_exactly_at_max_payload_stays_single() {
        // max_payload = mtu - overhead = 30 - 12 = 18
        let p = H264Packetizer::new(30).with_overhead(12);
        // NALU length exactly 18 -> remains single-nalu (no FU-A)
        let mut nalu = vec![0x41];
        nalu.extend(std::iter::repeat(0u8).take(17));
        let a = annexb(&[&nalu]);
        let chunks = p.packetize_annexb_to_payloads(&a);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].bytes.len(), 18);
        // Single NALU payload should start with original header (0x41), not FU-A indicator (type 28)
        assert_ne!(chunks[0].bytes[0] & 0x1F, 28);
        assert!(chunks[0].marker);
    }

    #[test]
    fn packetize_one_over_max_payload_uses_fu_a_two_frags_min() {
        // max_payload = 18, nalu len = 19 -> FU-A of 2 fragments (since FU adds 2B overhead)
        let p = H264Packetizer::new(30).with_overhead(12);
        let mut nalu = vec![0x65]; // IDR
        nalu.extend(std::iter::repeat(0u8).take(18)); // total 19
        let a = annexb(&[&nalu]);
        let chunks = p.packetize_annexb_to_payloads(&a);
        assert!(chunks.len() >= 2);
        // Check FU-A headers
        for (i, ch) in chunks.iter().enumerate() {
            assert_eq!(ch.bytes[0] & 0x1F, 28); // FU-A
            let fu_hdr = ch.bytes[1];
            let s = fu_hdr & 0x80 != 0;
            let e = fu_hdr & 0x40 != 0;
            if i == 0 {
                assert!(s && !e);
            } else if i == chunks.len() - 1 {
                assert!(!s && e);
            } else {
                assert!(!s && !e);
            }
        }
        assert!(chunks.last().unwrap().marker);
    }

    #[test]
    fn degenerate_payload_budget_yields_no_fragments() {
        // mtu==overhead => max_payload==0; the code should skip NALU safely (no infinite loop)
        let p = H264Packetizer::new(12).with_overhead(12);
        let a = annexb(&[&[0x65, 1, 2, 3, 4, 5]]);
        let chunks = p.packetize_annexb_to_payloads(&a);
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn marker_on_last_chunk_even_if_last_nalu_is_fragmented() {
        let p = H264Packetizer::new(22).with_overhead(12); // max_payload = 10
        let small1 = vec![0x61, 1, 2]; // single
        let mut big_last = Vec::with_capacity(1 + 21);
        big_last.push(0x65);
        big_last.extend((0u8..21u8).map(|x| x.wrapping_add(1))); // will fragment
        let a = annexb(&[&small1, &big_last]);
        let chunks = p.packetize_annexb_to_payloads(&a);

        assert!(chunks.len() >= 3);
        // All but the last must have marker=false; last true
        for (i, ch) in chunks.iter().enumerate() {
            if i + 1 == chunks.len() {
                assert!(ch.marker);
            } else {
                assert!(!ch.marker);
            }
        }
    }

    #[test]
    fn packetize_to_rtp_and_decode_roundtrip() {
        let p = H264Packetizer::new(22).with_overhead(12); // max_payload=10
        let mut big = Vec::with_capacity(1 + 25);
        big.push(0x65); // IDR
        big.extend((0u8..25u8).map(|x| x.wrapping_add(1)));
        let a = annexb(&[&big]);

        let pt = 96u8;
        let ts = 123_456u32;
        let ssrc = 0x11223344;
        let seq0 = 5000u16;

        let (pkts, next_seq) = p.packetize_annexb_to_rtp(&a, pt, ts, ssrc, seq0);
        assert!(pkts.len() >= 3);
        assert_eq!(next_seq, seq0.wrapping_add(pkts.len() as u16));
        for (i, pkt) in pkts.iter().enumerate() {
            assert_eq!(pkt.header.payload_type, pt);
            assert_eq!(pkt.header.timestamp, ts);
            assert_eq!(pkt.header.ssrc, ssrc);
            let expected_marker = i + 1 == pkts.len();
            assert_eq!(pkt.header.marker, expected_marker);

            // Encode + decode sanity
            let bytes = pkt.encode().expect("encode");
            let dec = RtpPacket::decode(&bytes).expect("decode");
            assert_eq!(dec, *pkt);
        }
    }

    #[test]
    fn rtp_decode_rejects_bad_version_variants() {
        // Build a valid small RTP packet first
        let pkt = RtpPacket::simple(
            96,         // PT
            true,       // M
            1000,       // seq
            4242,       // ts
            0xAABBCCDD, // ssrc
            vec![0x65, 1, 2],
        );
        let bytes = pkt.encode().expect("encode");

        // Helper to force version in top 2 bits of first byte
        let set_version = |b: &mut [u8], v: u8| {
            b[0] = (b[0] & 0x3F) | ((v & 0b11) << 6);
        };

        for bad in [0u8, 1u8, 3u8] {
            let mut corrupted = bytes.clone();
            set_version(&mut corrupted, bad);
            match RtpPacket::decode(&corrupted) {
                Err(RtpError::BadVersion(v)) => assert_eq!(v, bad),
                other => panic!("expected BadVersion({bad}), got {:?}", other),
            }
        }

        // Sanity: version=2 should decode fine
        let mut ok = bytes.clone();
        set_version(&mut ok, 2);
        let _ = RtpPacket::decode(&ok).expect("version=2 must decode");
    }

    #[test]
    fn packetize_large_nalu_fu_a() {
        // Force fragmentation: max_payload ~ 10, nalu len ~ 1 + 25 (header + 25) → 3 fragments.
        let p = H264Packetizer::new(22).with_overhead(12); // max_payload=10
        let mut big = Vec::with_capacity(1 + 25);
        big.push(0x65); // IDR
        big.extend((0u8..25u8).map(|x| x.wrapping_add(1)));
        let a = annexb(&[&big]);

        let chunks = p.packetize_annexb_to_payloads(&a);
        assert!(chunks.len() >= 3);
        for (i, ch) in chunks.iter().enumerate() {
            // FU-A has 2-byte header
            assert!(ch.bytes.len() <= 10);
            assert_eq!(ch.bytes[0] & 0x1F, 28); // FU-A indicator type
            let fu_hdr = ch.bytes[1];
            let is_start = fu_hdr & 0x80 != 0;
            let is_end = fu_hdr & 0x40 != 0;
            if i == 0 {
                assert!(is_start);
                assert!(!is_end);
            } else if i == chunks.len() - 1 {
                assert!(!is_start);
                assert!(is_end);
            } else {
                assert!(!is_start && !is_end);
            }
        }
        assert!(chunks.last().unwrap().marker);
    }
}
