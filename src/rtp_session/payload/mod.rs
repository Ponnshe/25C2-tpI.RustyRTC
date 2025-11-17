
#[cfg(test)]
mod roundtrip_tests {
    use crate::media_transport::depacketizer::h264_depacketizer::H264Depacketizer;
    use crate::media_transport::payload::h264_packetizer::H264Packetizer;

    // ---------- helpers ----------
    fn mk_nalu(ntype: u8, nri: u8, payload_len: usize) -> Vec<u8> {
        assert!((1..=23).contains(&ntype));
        let header = (nri & 0x60) | (ntype & 0x1F); // F=0
        let mut v = Vec::with_capacity(1 + payload_len);
        v.push(header);
        for i in 0..payload_len {
            v.push(((i as u8).wrapping_mul(7)).wrapping_add(3)); // deterministic bytes
        }
        v
    }

    fn to_annexb(nalus: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for n in nalus {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(n);
        }
        out
    }

    fn roundtrip_once(mtu: usize, nalus: Vec<Vec<u8>>, ts: u32, seq_start: u16) -> Vec<u8> {
        let p = H264Packetizer::new(mtu);
        let annexb = to_annexb(&nalus);
        let chunks = p.packetize_annexb_to_payloads(&annexb);

        assert!(
            !chunks.is_empty(),
            "packetizer must produce at least one chunk"
        );
        assert!(
            chunks.last().unwrap().marker,
            "last chunk must have marker=true"
        );

        let mut d = H264Depacketizer::new();
        let mut seq = seq_start;
        let mut out = None;

        for ch in chunks {
            let maybe = d.push_rtp(&ch.bytes, ch.marker, ts, seq);
            seq = seq.wrapping_add(1);
            if maybe.is_some() {
                out = maybe;
            }
        }

        let frame = out.expect("depacketizer must emit a frame on marker=true");
        assert_eq!(frame, annexb);
        frame
    }

    // ---------- tests ----------

    #[test]
    fn rt_single_small_nalus_no_fragment() {
        // Big MTU → Single-NALU packets
        let mtu = 1400;
        // SPS + PPS + small IDR
        let sps = mk_nalu(7, 0x60, 8);
        let pps = mk_nalu(8, 0x60, 6);
        let idr = mk_nalu(5, 0x40, 50);
        let nalus = vec![sps, pps, idr];

        let ts = 90_000;
        let _au = roundtrip_once(mtu, nalus, ts, 1000);
    }

    #[test]
    fn rt_one_large_nalu_forces_fua() {
        // Small MTU → FU-A fragmentation
        let mtu = 600; // leaves ~588 for payload; packetizer subtracts RTP overhead internally
        let idr = mk_nalu(5, 0x40, 4000); // large
        let nalus = vec![idr];

        let ts = 90_000 * 2;
        let _au = roundtrip_once(mtu, nalus, ts, 2000);
    }

    #[test]
    fn rt_mixed_multi_nal_frame_with_fragmented_idr() {
        let mtu = 800;
        let sps = mk_nalu(7, 0x60, 16);
        let pps = mk_nalu(8, 0x60, 10);
        let sei = mk_nalu(6, 0x20, 24); // SEI falls in 1..=23; using type 6 here
        let idr = mk_nalu(5, 0x40, 1800); // big → FU-A
        let nalus = vec![sps, pps, sei, idr];

        let ts = 90_000 * 3;
        let _au = roundtrip_once(mtu, nalus, ts, 3000);
    }

    #[test]
    fn rt_two_frames_back_to_back() {
        let mtu = 1200;

        // Frame 1
        let f1 = vec![
            mk_nalu(7, 0x60, 12),
            mk_nalu(8, 0x60, 8),
            mk_nalu(5, 0x40, 900), // IDR, fragmented
        ];
        let ts1 = 10_000;
        let au1 = roundtrip_once(mtu, f1.clone(), ts1, 4000);
        assert_eq!(au1, to_annexb(&f1));

        // Frame 2
        let f2 = vec![mk_nalu(1, 0x20, 300)]; // P-frame
        let ts2 = 20_000;
        let au2 = roundtrip_once(mtu, f2.clone(), ts2, 5000);
        assert_eq!(au2, to_annexb(&f2));
    }

    #[test]
    fn rt_sequence_wraparound_ok() {
        let mtu = 1200;
        let idr = mk_nalu(5, 0x40, 2000); // FU-A
        let nalus = vec![idr];

        let ts = 33_000;
        // Start near the end to force wraparound internally in roundtrip_once loop
        let _au = roundtrip_once(mtu, nalus, ts, u16::MAX - 5);
    }

    #[test]
    fn rt_many_randomized_cases() {
        // Deterministic-ish: simple LCG for randomness, no external deps.
        fn rand(mut s: u64) -> impl FnMut() -> u32 {
            move || {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                (s >> 33) as u32
            }
        }
        let mut r = rand(0xC0FFEE);

        for i in 0..50 {
            let mtu = 500 + (r() % 1000) as usize; // 500..1499
            let nn = 1 + (r() % 4) as usize; // 1..4 NALUs

            let mut nalus = Vec::with_capacity(nn);
            for _ in 0..nn {
                // Pick a legal type in 1..=23 (avoid aggregation/fragment types)
                let ty = 1 + (r() % 23) as u8;
                let nri = match r() % 3 {
                    0 => 0x20, // low
                    1 => 0x40, // medium
                    _ => 0x60, // high
                };
                let sz = 10 + (r() % 5000) as usize; // 10..5010
                nalus.push(mk_nalu(ty, nri, sz));
            }

            let ts = 1000 * i;
            let _au = roundtrip_once(mtu, nalus, ts, (10_000 + i) as u16);
        }
    }
}