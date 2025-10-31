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

/// A single RTP payload chunk plus whether it carries the end-of-frame marker.
#[derive(Debug, Clone)]
pub struct RtpPayloadChunk {
    pub bytes: Vec<u8>,
    /// true only for the *last* chunk of the access unit (frame)
    pub marker: bool,
}

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
        let nalus = split_annexb_nalus(annexb_frame);
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

/// Extract NALU slices from an Annex-B access unit.
///
/// Returns a `Vec<&[u8]>` where each slice is *the NAL unit without start codes*.
/// Accepts both 3-byte (00 00 01) and 4-byte (00 00 00 01) start codes.
/// If no start code is found, the whole buffer is treated as a single NALU.
fn split_annexb_nalus(data: &[u8]) -> Vec<&[u8]> {
    let mut indices = Vec::new();
    let mut i = 0usize;
    let n = data.len();

    while i + 3 <= n {
        if let Some(sc_len) = start_code_len_at(data, i) {
            indices.push(i + sc_len);
            i += sc_len;
            continue;
        }
        i += 1;
    }

    if indices.is_empty() {
        // Not Annex-B? Treat the whole buffer as one NALU.
        return if !data.is_empty() {
            vec![data]
        } else {
            Vec::new()
        };
    }

    let mut nalus = Vec::with_capacity(indices.len());
    for (k, &start) in indices.iter().enumerate() {
        let end = if k + 1 < indices.len() {
            // The next start code begins somewhere after some zeros; find its start code offset.
            // We need to walk backward from indices[k+1] to strip trailing zeros.
            let mut j = indices[k + 1] - 1;
            // Walk left until we hit a non-zero (stop at >= start to avoid usize underflow)
            while j >= start && data[j] == 0 {
                if j == 0 {
                    break;
                }
                j -= 1;
            }
            // Exclusive end is j+1, but ensure >= start
            (j + 1).max(start)
        } else {
            // Last NALU: from start to end of buffer, stripping trailing zeros
            let mut j = n;
            while j > start && data[j - 1] == 0 {
                j -= 1;
            }
            j
        };

        if end > start {
            nalus.push(&data[start..end]);
        }
    }

    nalus
}

/// If a start code begins at `i`, return its length (3 or 4). Else `None`.
#[inline]
fn start_code_len_at(data: &[u8], i: usize) -> Option<usize> {
    let n = data.len();
    // 4-byte: 00 00 00 01
    if i + 4 <= n && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 0 && data[i + 3] == 1 {
        return Some(4);
    }
    // 3-byte: 00 00 01
    if i + 3 <= n && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
        return Some(3);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
