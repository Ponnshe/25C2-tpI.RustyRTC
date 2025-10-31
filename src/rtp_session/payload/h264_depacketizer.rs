//! RFC 6184 H.264 <- RTP depacketizer (Single NALU + FU-A).
//!
//! Input : a stream of RTP payloads with the same timestamp, ending with M=1.
//! Output: an Annex-B access unit (frame) as bytes, or None if more packets are needed.
//!
//! Scope : non-interleaved, packetization-mode=1. STAP-A is ignored (not used by your packetizer).

#[derive(Debug, Default, Clone)]
pub struct H264Depacketizer {
    cur_ts: Option<u32>,
    expected_seq: Option<u16>,
    nalus: Vec<Vec<u8>>, // NAL units collected for the current frame (without start codes)
    fua: Option<FuState>, // ongoing FU-A reassembly
    frame_corrupted: bool, // set if we detect loss or malformed FU-A; drop frame on M=1
}

#[derive(Debug, Clone)]
struct FuState {
    nalu_header: u8, // reconstructed: F|NRI|Type
    buf: Vec<u8>,    // complete NAL content: [nalu_header, ...payload...]
}

impl H264Depacketizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push one RTP payload. Returns Some(AnnexBFrame) when the frame completes (M=1).
    ///
    /// `payload`  : RTP payload (no RTP header)
    /// `marker`   : RTP marker bit (true on last packet of the frame)
    /// `timestamp`: RTP timestamp (90 kHz clock)
    /// `seq`      : RTP sequence number (for simple loss detection)
    pub fn push_rtp(
        &mut self,
        payload: &[u8],
        marker: bool,
        timestamp: u32,
        seq: u16,
    ) -> Option<Vec<u8>> {
        // New timestamp? Flush/discard any partial frame.
        if let Some(ts) = self.cur_ts {
            if timestamp != ts {
                self.reset_for_new_ts(timestamp);
            }
        } else {
            self.cur_ts = Some(timestamp);
        }

        // Sequence tracking (best-effort). If out-of-order or lost, mark frame corrupted.
        if let Some(expect) = self.expected_seq {
            if seq != expect {
                self.frame_corrupted = true;
                // We don't abort immediately; we still drain until M=1 so the caller can keep sync.
            }
        }
        self.expected_seq = Some(seq.wrapping_add(1));

        // Empty payload? Corrupt frame.
        if payload.is_empty() {
            self.frame_corrupted = true;
            return self.finish_if_marker(marker);
        }

        let nalu_header = payload[0];
        let nalu_type = nalu_header & 0x1F;

        match nalu_type {
            1..=23 => {
                // Single NALU
                if self.fua.is_some() {
                    // mid-FU but got a single NAL => consider frame broken
                    self.frame_corrupted = true;
                    self.fua = None;
                }
                self.nalus.push(payload.to_vec());
            }
            28 => {
                // FU-A
                if payload.len() < 2 {
                    self.frame_corrupted = true;
                    return self.finish_if_marker(marker);
                }
                let fu_indicator = nalu_header; // F|NRI|28
                let fu_header = payload[1]; // S|E|R|Type
                let start = fu_header & 0x80 != 0;
                let end = fu_header & 0x40 != 0;
                let ttype = fu_header & 0x1F;

                // Reconstruct original one-byte NAL header: F|NRI|Type
                let orig_hdr = (fu_indicator & 0xE0) | ttype;

                if start {
                    // Start a new FU; reset any stale state.
                    self.fua = Some(FuState {
                        nalu_header: orig_hdr,
                        buf: {
                            let mut v = Vec::with_capacity(payload.len() + 1);
                            v.push(orig_hdr);
                            v.extend_from_slice(&payload[2..]);
                            v
                        },
                    });
                } else if let Some(st) = self.fua.as_mut() {
                    // Continuation
                    st.buf.extend_from_slice(&payload[2..]);
                } else {
                    // Got middle/end fragment without a start
                    self.frame_corrupted = true;
                }

                if end {
                    if let Some(st) = self.fua.take() {
                        self.nalus.push(st.buf);
                    } else {
                        self.frame_corrupted = true;
                    }
                }
            }
            24 => {
                // STAP-A not used by your packetizer; ignore gracefully (or mark corrupted).
                // Here we choose to ignore completely:
                // self.frame_corrupted = true; // enable this if you prefer strictness.
            }
            _ => {
                // Unsupported / reserved types (e.g., STAP-B/Multi-Times, FU-B, etc.)
                // Mark as corrupted but continue until marker.
                self.frame_corrupted = true;
            }
        }

        self.finish_if_marker(marker)
    }

    fn finish_if_marker(&mut self, marker: bool) -> Option<Vec<u8>> {
        if !marker {
            return None;
        }

        let out = if !self.frame_corrupted && !self.nalus.is_empty() {
            Some(build_annexb(&self.nalus))
        } else {
            None
        };

        // Reset for next frame (keep timestamp None until next push)
        self.cur_ts = None;
        self.expected_seq = None;
        self.nalus.clear();
        self.fua = None;
        self.frame_corrupted = false;

        out
    }

    fn reset_for_new_ts(&mut self, new_ts: u32) {
        // Drop any partial from previous timestamp.
        self.cur_ts = Some(new_ts);
        self.expected_seq = None;
        self.nalus.clear();
        self.fua = None;
        self.frame_corrupted = false;
    }
}

#[inline]
fn build_annexb(nalus: &[Vec<u8>]) -> Vec<u8> {
    // Pre-size roughly: 4 bytes start code per NAL
    let total_len: usize = nalus.iter().map(|n| n.len() + 4).sum();
    let mut out = Vec::with_capacity(total_len);
    for n in nalus {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(n);
    }
    out
}
