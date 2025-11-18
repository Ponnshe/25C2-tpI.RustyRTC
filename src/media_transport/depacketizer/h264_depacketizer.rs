//! RFC 6184 H.264 <- RTP depacketizer (Single NALU + FU-A).
//!
//! Input : a stream of RTP payloads with the same timestamp, ending with M=1.
//! Output: an Annex-B access unit (frame) as bytes, or None if more packets are needed.
//!
//! Scope : non-interleaved, packetization-mode=1. STAP-A is ignored (not used by your packetizer).

#[derive(Debug, Clone)]
struct FuState {
    nalu_header: u8, // reconstructed: F|NRI|Type
    buf: Vec<u8>,    // complete NAL content: [nalu_header, ...payload...]
}

#[derive(Debug, Default, Clone)]
pub struct H264Depacketizer {
    cur_ts: Option<u32>,
    expected_seq: Option<u16>,
    nalus: Vec<Vec<u8>>, // NAL units collected for the current frame (without start codes)
    fua: Option<FuState>, // ongoing FU-A reassembly
    frame_corrupted: bool, // set if we detect loss or malformed FU-A; drop frame on M=1
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
        // timestamp & sequence handling unchanged...
        if let Some(ts) = self.cur_ts {
            if timestamp != ts {
                self.reset_for_new_ts(timestamp);
            }
        } else {
            self.cur_ts = Some(timestamp);
        }

        if let Some(expect) = self.expected_seq {
            if seq != expect {
                self.frame_corrupted = true;
            }
        }
        self.expected_seq = Some(seq.wrapping_add(1));

        if payload.is_empty() {
            self.frame_corrupted = true;
            return self.finish_if_marker(marker);
        }

        let nalu_header = payload[0];
        let nalu_type = nalu_header & 0x1F;

        match nalu_type {
            1..=23 => {
                if self.fua.is_some() {
                    self.frame_corrupted = true;
                    self.fua = None;
                }
                // *** de-dupe single-NAL additions ***
                self.push_slice_if_new(payload);
            }
            28 => {
                if payload.len() < 2 {
                    self.frame_corrupted = true;
                    return self.finish_if_marker(marker);
                }
                let fu_indicator = nalu_header;
                let fu_header = payload[1];
                let start = fu_header & 0x80 != 0;
                let end = fu_header & 0x40 != 0;
                let ttype = fu_header & 0x1F;

                let orig_hdr = (fu_indicator & 0xE0) | ttype;

                if start {
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
                    st.buf.extend_from_slice(&payload[2..]);
                } else {
                    self.frame_corrupted = true;
                }

                if end {
                    if let Some(st) = self.fua.take() {
                        // *** de-dupe FU-A completions too ***
                        self.push_vec_if_new(st.buf);
                    } else {
                        self.frame_corrupted = true;
                    }
                }
            }
            24 => { /* ignore STAP-A as before */ }
            _ => {
                self.frame_corrupted = true;
            }
        }

        self.finish_if_marker(marker)
    }

    fn push_slice_if_new(&mut self, nalu: &[u8]) {
        let is_dup = self
            .nalus
            .last()
            .map(|prev| prev.as_slice() == nalu)
            .unwrap_or(false);
        if !is_dup {
            self.nalus.push(nalu.to_vec());
        }
    }

    fn push_vec_if_new(&mut self, nalu: Vec<u8>) {
        let is_dup = self
            .nalus
            .last()
            .map(|prev| prev.as_slice() == nalu.as_slice())
            .unwrap_or(false);
        if !is_dup {
            self.nalus.push(nalu);
        }
    }

    fn finish_if_marker(&mut self, marker: bool) -> Option<Vec<u8>> {
        if !marker {
            return None;
        }

        let out = if !self.frame_corrupted && !self.nalus.is_empty() {
            let mut annexb = Vec::new();
            for nalu in &self.nalus {
                annexb.extend_from_slice(&[0, 0, 0, 1]);
                annexb.extend_from_slice(nalu);
            }
            Some(annexb)
        } else {
            None
        };

        self.cur_ts = None;
        self.expected_seq = None;
        self.fua = None;
        self.frame_corrupted = false;
        self.nalus.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- helpers ----------

    fn mk_nalu(ntype: u8, nri: u8, payload_len: usize) -> Vec<u8> {
        assert!((1..=23).contains(&ntype));
        let header = (nri & 0x60) | (ntype & 0x1F); // F=0
        let mut v = Vec::with_capacity(1 + payload_len);
        v.push(header);
        // deterministic payload pattern
        for i in 0..payload_len {
            v.push((i as u8).wrapping_mul(3).wrapping_add(1));
        }
        v
    }

    /// Build FU-A fragments from a complete NALU (nalu[0] is the NAL header).
    /// `splits` are sizes of payload pieces (sum must equal nalu.len()-1).
    fn mk_fua_from_nalu(nalu: &[u8], splits: &[usize]) -> Vec<Vec<u8>> {
        assert!(!nalu.is_empty());
        let hdr = nalu[0];
        let ntype = hdr & 0x1F;
        let fu_indicator = (hdr & 0xE0) | 28; // F|NRI|28
        let payload = &nalu[1..];

        assert_eq!(splits.iter().sum::<usize>(), payload.len());

        let mut out = Vec::with_capacity(splits.len());
        let mut off = 0usize;
        for (i, &sz) in splits.iter().enumerate() {
            let s = if i == 0 { 0x80 } else { 0x00 };
            let e = if i + 1 == splits.len() { 0x40 } else { 0x00 };
            let fu_header = s | e | ntype;

            let mut pkt = Vec::with_capacity(2 + sz);
            pkt.push(fu_indicator);
            pkt.push(fu_header);
            pkt.extend_from_slice(&payload[off..off + sz]);
            out.push(pkt);

            off += sz;
        }
        out
    }

    fn push_seq(
        d: &mut H264Depacketizer,
        payload: &[u8],
        marker: bool,
        ts: u32,
        seq: &mut u16,
    ) -> Option<Vec<u8>> {
        let out = d.push_rtp(payload, marker, ts, *seq);
        *seq = seq.wrapping_add(1);
        out
    }

    // ---------- tests ----------

    #[test]
    fn single_small_nalu_emits_on_marker() {
        let mut d = H264Depacketizer::new();
        let ts = 1234;
        let mut seq = 1000;

        let nalu = mk_nalu(5, 0x40, 8); // IDR
        // Not last yet
        assert!(push_seq(&mut d, &nalu, false, ts, &mut seq).is_none());
        // Last
        let frame = push_seq(&mut d, &nalu, true, ts, &mut seq).expect("Frame expected");

        let mut expected_frame = vec![0, 0, 0, 1];
        expected_frame.extend_from_slice(&nalu);

        assert_eq!(frame, expected_frame);
    }

    #[test]
    fn two_nalus_in_one_frame() {
        let mut d = H264Depacketizer::new();
        let ts = 9999;
        let mut seq = 42;

        let sps = mk_nalu(7, 0x60, 4);
        let pps = mk_nalu(8, 0x60, 3);

        assert!(push_seq(&mut d, &sps, false, ts, &mut seq).is_none());
        let frame = push_seq(&mut d, &pps, true, ts, &mut seq).expect("Frame expected");

        let mut expected_frame = vec![0, 0, 0, 1];
        expected_frame.extend_from_slice(&sps);
        expected_frame.extend_from_slice(&[0, 0, 0, 1]);
        expected_frame.extend_from_slice(&pps);

        assert_eq!(frame, expected_frame);
    }

    #[test]
    fn fua_three_fragments_reassembles() {
        let mut d = H264Depacketizer::new();
        let ts = 321;
        let mut seq = 10;

        let idr = mk_nalu(5, 0x40, 15);
        // split payload (len 15) into 3 fragments: 5|6|4
        let frags = mk_fua_from_nalu(&idr, &[5, 6, 4]);

        assert!(push_seq(&mut d, &frags[0], false, ts, &mut seq).is_none());
        assert!(push_seq(&mut d, &frags[1], false, ts, &mut seq).is_none());
        let frame = push_seq(&mut d, &frags[2], true, ts, &mut seq).expect("Frame expected");

        let mut expected_frame = vec![0, 0, 0, 1];
        expected_frame.extend_from_slice(&idr);

        assert_eq!(frame, expected_frame);
    }

    #[test]
    fn fua_missing_middle_fragment_drops_frame() {
        let mut d = H264Depacketizer::new();
        let ts = 777;
        let mut seq = 500;

        let idr = mk_nalu(5, 0x40, 12);
        let frags = mk_fua_from_nalu(&idr, &[4, 4, 4]);

        // send start, skip the middle, then end
        assert!(push_seq(&mut d, &frags[0], false, ts, &mut seq).is_none());
        // gap here -> simulate loss by bumping seq
        seq = seq.wrapping_add(1);
        assert!(push_seq(&mut d, &frags[2], true, ts, &mut seq).is_none());
    }

    #[test]
    fn empty_payload_marks_corrupted() {
        let mut d = H264Depacketizer::new();
        let ts = 2025;
        let mut seq = 1;

        // empty payload
        assert!(push_seq(&mut d, &[], false, ts, &mut seq).is_none());
        // even with a valid NAL after, the frame is corrupted until marker
        let nalu = mk_nalu(1, 0x20, 6);
        assert!(push_seq(&mut d, &nalu, true, ts, &mut seq).is_none());
    }

    #[test]
    fn sequence_gap_drops_frame_on_marker() {
        let mut d = H264Depacketizer::new();
        let ts = 55;
        let mut seq = 10;

        let a = mk_nalu(1, 0x20, 5);
        let b = mk_nalu(1, 0x20, 5);

        assert!(push_seq(&mut d, &a, false, ts, &mut seq).is_none());
        // skip a seq -> simulate loss
        seq = seq.wrapping_add(1);
        assert!(push_seq(&mut d, &b, true, ts, &mut seq).is_none());
    }

    #[test]
    fn timestamp_switch_resets_state() {
        let mut d = H264Depacketizer::new();
        let ts1 = 1000;
        let ts2 = 2000;
        let mut seq = 300;

        let n1 = mk_nalu(1, 0x20, 5);
        let n2 = mk_nalu(1, 0x20, 6);

        // start a frame at ts1 but never finish it
        assert!(push_seq(&mut d, &n1, false, ts1, &mut seq).is_none());
        // new timestamp -> previous partial is dropped/reset
        assert!(push_seq(&mut d, &n2, false, ts2, &mut seq).is_none());
        // now finish ts2
        let frame = push_seq(&mut d, &n2, true, ts2, &mut seq).expect("Frame expected");

        let mut expected_frame = vec![0, 0, 0, 1];
        expected_frame.extend_from_slice(&n2);

        assert_eq!(frame, expected_frame);
    }

    #[test]
    fn stap_a_is_ignored_and_does_not_corrupt() {
        let mut d = H264Depacketizer::new();
        let ts = 4040;
        let mut seq = 77;

        // Minimal STAP-A payload: header only (type=24). Our depacketizer ignores it.
        let stap_a = vec![0x18]; // F=0, NRI=0, Type=24
        assert!(push_seq(&mut d, &stap_a, false, ts, &mut seq).is_none());

        // Then send a valid small NAL and finish
        let n = mk_nalu(1, 0x20, 3);
        let frame = push_seq(&mut d, &n, true, ts, &mut seq).expect("Frame expected");

        let mut expected_frame = vec![0, 0, 0, 1];
        expected_frame.extend_from_slice(&n);

        assert_eq!(frame, expected_frame);
    }

    #[test]
    fn sequence_wrap_around_ok() {
        let mut d = H264Depacketizer::new();
        let ts = 31415;
        let mut seq = u16::MAX;

        let n = mk_nalu(1, 0x20, 2);

        assert!(push_seq(&mut d, &n, false, ts, &mut seq).is_none()); // seq = 65535
        let frame = push_seq(&mut d, &n, true, ts, &mut seq).expect("Frame expected"); // seq wraps to 0

        let mut expected_frame = vec![0, 0, 0, 1];
        expected_frame.extend_from_slice(&n);

        assert_eq!(frame, expected_frame);
    }

    #[test]
    fn fua_end_without_start_drops_frame() {
        let mut d = H264Depacketizer::new();
        let ts = 7777;
        let mut seq = 900;

        let idr = mk_nalu(5, 0x40, 9);
        // craft "end" fragment without sending "start"
        let frags = mk_fua_from_nalu(&idr, &[4, 5]);
        // send only the last one (E=1)
        assert!(push_seq(&mut d, &frags[1], true, ts, &mut seq).is_none());
    }
}
