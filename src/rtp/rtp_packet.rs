//! Minimal RTP packet model + encode/decode per RFC 3550.
//! This module has **no** session logic (no jitter calc, no RTX, etc.).
//! It focuses on immutable packet structs and safe serialization.
#![allow(dead_code)]

use super::{
    config::RTP_VERSION, rtp_error::RtpError, rtp_header::RtpHeader,
    rtp_header_extension::RtpHeaderExtension,
};
use std::convert::TryInto;

/// Complete RTP packet (header + payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpPacket {
    pub header: RtpHeader,
    /// Payload without any trailing padding bytes. If padding was present,
    /// use `padding_bytes` to know how much was removed during decode.
    pub payload: Vec<u8>,
    /// Count of padding bytes (from the last byte) if the P bit was set.
    pub padding_bytes: u8,
}

impl RtpPacket {
    pub fn new(header: RtpHeader, payload: Vec<u8>) -> Self {
        Self {
            header,
            payload,
            padding_bytes: 0,
        }
    }

    /// Convenience constructor.
    pub fn simple(
        payload_type: u8,
        marker: bool,
        seq: u16,
        ts: u32,
        ssrc: u32,
        payload: Vec<u8>,
    ) -> Self {
        let header = RtpHeader::new(payload_type, seq, ts, ssrc).with_marker(marker);
        Self::new(header, payload)
    }

    /// Encode into a fresh Vec<u8> (network byte order).
    pub fn encode(&self) -> Result<Vec<u8>, RtpError> {
        let mut out = Vec::with_capacity(12 + self.header.csrcs.len() * 4 + self.payload.len() + 4);

        let cc = (self.header.csrcs.len() & 0x0F) as u8;
        let has_ext = self.header.header_extension.is_some();
        let has_pad = self.padding_bytes > 0;
        let vpxcc =
            (self.header.version & 0b11) << 6 | (has_pad as u8) << 5 | (has_ext as u8) << 4 | cc;

        let m_pt = ((self.header.marker as u8) << 7) | (self.header.payload_type & 0x7F);

        out.push(vpxcc);
        out.push(m_pt);
        out.extend_from_slice(&self.header.sequence_number.to_be_bytes());
        out.extend_from_slice(&self.header.timestamp.to_be_bytes());
        out.extend_from_slice(&self.header.ssrc.to_be_bytes());

        for csrc in &self.header.csrcs {
            out.extend_from_slice(&csrc.to_be_bytes());
        }

        if let Some(ext) = &self.header.header_extension {
            // RFC3550: 16-bit profile, 16-bit length in 32-bit words
            let words = ext.data.len().div_ceil(4) as u32;
            if words > u16::MAX as u32 {
                return Err(RtpError::HeaderExtensionTooLong);
            }
            let len_words = words as u16;
            out.extend_from_slice(&ext.profile.to_be_bytes());
            out.extend_from_slice(&len_words.to_be_bytes());
            out.extend_from_slice(&ext.data);

            // pad to 32-bit boundary with zero bytes
            let pad = (4 - (ext.data.len() % 4)) % 4;
            if pad != 0 {
                out.extend(std::iter::repeat_n(0u8, pad));
            }
        }

        // For encode(), we *do not* add RTP padding by default because the
        // session layer should decide this. If header.padding is true and
        // padding_bytes > 0, we append that many zero octets and set P bit.
        out.extend_from_slice(&self.payload);

        if has_pad {
            // Add (padding_bytes - 1) filler bytes (any value is legal; use 0) and end with the pad count
            if self.padding_bytes > 1 {
                out.extend(std::iter::repeat_n(0u8, (self.padding_bytes - 1) as usize));
            }
            out.push(self.padding_bytes);
        }

        Ok(out)
    }

    /// Decode a single RTP packet from `buf`.
    pub fn decode(buf: &[u8]) -> Result<Self, RtpError> {
        if buf.len() < 12 {
            return Err(RtpError::TooShort);
        }

        let vpxcc = buf[0];
        let m_pt = buf[1];

        let version = (vpxcc >> 6) & 0b11;
        if version != RTP_VERSION {
            return Err(RtpError::BadVersion(version));
        }
        let padding = ((vpxcc >> 5) & 1) != 0;
        let extension = ((vpxcc >> 4) & 1) != 0;
        let cc = (vpxcc & 0x0F) as usize;

        let marker = (m_pt >> 7) != 0;
        let payload_type = m_pt & 0x7F;

        let sequence_number =
            u16::from_be_bytes(buf[2..4].try_into().map_err(|_| RtpError::Invalid)?);
        let timestamp = u32::from_be_bytes(buf[4..8].try_into().map_err(|_| RtpError::Invalid)?);
        let ssrc = u32::from_be_bytes(buf[8..12].try_into().map_err(|_| RtpError::Invalid)?);

        let mut idx = 12usize;

        // CSRCs
        if buf.len() < idx + cc * 4 {
            return Err(RtpError::CsrcCountMismatch {
                expected: cc,
                buf_left: buf.len().saturating_sub(idx),
            });
        }
        let mut csrcs = Vec::with_capacity(cc);
        for _ in 0..cc {
            let csrc = u32::from_be_bytes(
                buf[idx..idx + 4]
                    .try_into()
                    .map_err(|_| RtpError::Invalid)?,
            );
            csrcs.push(csrc);
            idx += 4;
        }

        // Header extension (generic 3550)
        let mut header_extension: Option<RtpHeaderExtension> = None;
        if extension {
            if buf.len() < idx + 4 {
                return Err(RtpError::HeaderExtensionTooShort);
            }
            let profile = u16::from_be_bytes(
                buf[idx..idx + 2]
                    .try_into()
                    .map_err(|_| RtpError::Invalid)?,
            );
            let length_words = u16::from_be_bytes(
                buf[idx + 2..idx + 4]
                    .try_into()
                    .map_err(|_| RtpError::Invalid)?,
            );
            idx += 4;

            let ext_len = (length_words as usize) * 4;
            if buf.len() < idx + ext_len {
                return Err(RtpError::HeaderExtensionTooShort);
            }
            let data = buf[idx..idx + ext_len].to_vec();
            idx += ext_len;

            header_extension = Some(RtpHeaderExtension { profile, data });
        }

        if buf.len() < idx {
            return Err(RtpError::TooShort);
        }

        // Determine payload region (handle P bit)
        let mut payload_end = buf.len();
        let mut padding_bytes = 0u8;

        if padding {
            // Last byte is padding count; must be >= 1 and <= payload length
            if payload_end == idx {
                return Err(RtpError::PaddingTooShort);
            }
            let pad = buf[payload_end - 1];
            if pad == 0 {
                return Err(RtpError::PaddingTooShort);
            }
            if pad as usize > payload_end - idx {
                return Err(RtpError::PaddingTooShort);
            }
            padding_bytes = pad;
            payload_end -= pad as usize;
        }

        let payload = if payload_end >= idx {
            buf[idx..payload_end].to_vec()
        } else {
            return Err(RtpError::Invalid);
        };

        let header = RtpHeader {
            version,
            padding,
            extension,
            marker,
            payload_type,
            sequence_number,
            timestamp,
            ssrc,
            csrcs,
            header_extension,
        };

        Ok(RtpPacket {
            header,
            payload,
            padding_bytes,
        })
    }

    // Convenience getters
    pub fn payload_type(&self) -> u8 {
        self.header.payload_type
    }
    pub fn marker(&self) -> bool {
        self.header.marker
    }
    pub fn seq(&self) -> u16 {
        self.header.sequence_number
    }
    pub fn timestamp(&self) -> u32 {
        self.header.timestamp
    }
    pub fn ssrc(&self) -> u32 {
        self.header.ssrc
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::super::config::RTP_VERSION;
    use super::super::rtp_error::RtpError;
    use super::super::rtp_header::RtpHeader;
    use super::super::rtp_header_extension::RtpHeaderExtension;
    use super::RtpPacket;

    fn mk_header_bytes(
        version: u8,
        padding: bool,
        extension: bool,
        cc: u8,
        marker: bool,
        pt: u8,
        seq: u16,
        ts: u32,
        ssrc: u32,
    ) -> Vec<u8> {
        let mut b = Vec::with_capacity(12);
        let vpxcc =
            (version & 0b11) << 6 | ((padding as u8) << 5) | ((extension as u8) << 4) | (cc & 0x0F);
        let m_pt = ((marker as u8) << 7) | (pt & 0x7F);
        b.push(vpxcc);
        b.push(m_pt);
        b.extend_from_slice(&seq.to_be_bytes());
        b.extend_from_slice(&ts.to_be_bytes());
        b.extend_from_slice(&ssrc.to_be_bytes());
        b
    }

    // ---- Decode error paths -------------------------------------------------

    #[test]
    fn decode_too_short() {
        let buf = vec![0u8; 11];
        let err = RtpPacket::decode(&buf).unwrap_err();
        assert!(matches!(err, RtpError::TooShort));
    }

    #[test]
    fn decode_bad_version() {
        // Version != 2
        let mut buf = mk_header_bytes(0, false, false, 0, false, 96, 1, 2, 3);
        let err = RtpPacket::decode(&buf).unwrap_err();
        match err {
            RtpError::BadVersion(v) => assert_eq!(v, 0),
            _ => panic!("expected BadVersion, got {err:?}"),
        }

        // Also try version = 3 (still invalid)
        buf[0] = (3 << 6) | (buf[0] & 0b00_111111);
        let err = RtpPacket::decode(&buf).unwrap_err();
        match err {
            RtpError::BadVersion(v) => assert_eq!(v, 3),
            _ => panic!("expected BadVersion, got {err:?}"),
        }
    }

    #[test]
    fn decode_csrc_count_mismatch() {
        // cc = 2 but no CSRC words present
        let buf = mk_header_bytes(RTP_VERSION, false, false, 2, false, 96, 1, 2, 3);
        let err = RtpPacket::decode(&buf).unwrap_err();
        assert!(matches!(err, RtpError::CsrcCountMismatch { .. }));
    }

    #[test]
    fn decode_header_extension_too_short_header() {
        // X = 1 but less than 4 bytes available for the extension header
        let mut buf = mk_header_bytes(RTP_VERSION, false, true, 0, false, 96, 1, 2, 3);
        buf.extend_from_slice(&[0x12, 0x34]); // only 2 bytes, need 4
        let err = RtpPacket::decode(&buf).unwrap_err();
        assert!(matches!(err, RtpError::HeaderExtensionTooShort));
    }

    #[test]
    fn decode_header_extension_too_short_data() {
        // X = 1, provide 4-byte header but not enough data for length_words
        let mut buf = mk_header_bytes(RTP_VERSION, false, true, 0, false, 96, 1, 2, 3);
        // profile = 0xABCD, length_words = 2 (needs 8 bytes)
        buf.extend_from_slice(&0xABCDu16.to_be_bytes());
        buf.extend_from_slice(&2u16.to_be_bytes());
        // only 4 bytes of data given -> should error
        buf.extend_from_slice(&[1, 2, 3, 4]);
        let err = RtpPacket::decode(&buf).unwrap_err();
        assert!(matches!(err, RtpError::HeaderExtensionTooShort));
    }

    #[test]
    fn decode_padding_without_pad_byte() {
        // P = 1 but no trailing pad count byte at all (buf ends at idx)
        let buf = mk_header_bytes(RTP_VERSION, true, false, 0, false, 96, 1, 2, 3);
        let err = RtpPacket::decode(&buf).unwrap_err();
        assert!(matches!(err, RtpError::PaddingTooShort));
    }

    #[test]
    fn decode_padding_zero_count() {
        // P = 1 with pad byte = 0 (invalid)
        let mut buf = mk_header_bytes(RTP_VERSION, true, false, 0, false, 96, 1, 2, 3);
        buf.push(0); // pad count = 0
        let err = RtpPacket::decode(&buf).unwrap_err();
        assert!(matches!(err, RtpError::PaddingTooShort));
    }

    #[test]
    fn decode_padding_count_exceeds_total_payload_region() {
        // P = 1 with pad count greater than total bytes after header.
        // Here total bytes after header = 2, but pad count = 10.
        let mut buf = mk_header_bytes(RTP_VERSION, true, false, 0, false, 96, 1, 2, 3);
        buf.extend_from_slice(&[0xAA, 10]); // 1 payload byte + pad count 10 -> invalid
        let err = RtpPacket::decode(&buf).unwrap_err();
        assert!(matches!(err, RtpError::PaddingTooShort));
    }

    // ---- Basic successful decodes / roundtrips -----------------------------

    #[test]
    fn decode_ok_no_payload_no_padding() {
        // Valid minimal RTP: header only, empty payload, no padding.
        let buf = mk_header_bytes(
            RTP_VERSION,
            false,
            false,
            0,
            true,
            127,
            0x1122,
            0x33_445_566,
            0x77_889_900,
        );
        let pkt = RtpPacket::decode(&buf).expect("should decode");
        assert_eq!(pkt.header.version, RTP_VERSION);
        assert!(pkt.header.marker);
        assert_eq!(pkt.header.payload_type, 127);
        assert_eq!(pkt.header.sequence_number, 0x1_122);
        assert_eq!(pkt.header.timestamp, 0x33_445_566);
        assert_eq!(pkt.header.ssrc, 0x77_889_900);
        assert!(pkt.payload.is_empty());
        assert_eq!(pkt.padding_bytes, 0);
    }

    #[test]
    fn roundtrip_minimal() {
        let payload = b"hello".to_vec();
        let pkt = RtpPacket::simple(96, true, 42, 9_000, 0xAA_BBC_CDD, payload.clone());
        let enc = pkt.encode().expect("encode");
        let dec = RtpPacket::decode(&enc).expect("decode");
        assert_eq!(dec.header.version, RTP_VERSION);
        assert_eq!(dec.header.payload_type, 96);
        assert!(dec.header.marker);
        assert_eq!(dec.header.sequence_number, 42);
        assert_eq!(dec.header.timestamp, 9000);
        assert_eq!(dec.header.ssrc, 0xAA_BBC_CDD);
        assert_eq!(dec.payload, payload);
        assert_eq!(dec.padding_bytes, 0);
    }

    #[test]
    fn roundtrip_with_padding_1() {
        let mut hdr = RtpHeader::new(111, 65_535, 0xDE_ADB_EEF, 0x01_020_304).with_marker(false);
        hdr.padding = true;

        let mut pkt = RtpPacket::new(hdr, b"PAYLOAD".to_vec());
        pkt.padding_bytes = 1; // only the count byte appended
        let enc = pkt.encode().expect("encode");
        assert_eq!(*enc.last().unwrap(), 1u8);

        let dec = RtpPacket::decode(&enc).expect("decode");
        assert_eq!(dec.payload, b"PAYLOAD".to_vec());
        assert_eq!(dec.padding_bytes, 1);
        assert!(dec.header.padding);
    }

    #[test]
    fn roundtrip_with_padding_4() {
        let mut hdr = RtpHeader::new(111, 7, 1234, 0xCA_FEB_ABE).with_marker(true);
        hdr.padding = true;

        let mut pkt = RtpPacket::new(hdr, vec![1, 2, 3]);
        pkt.padding_bytes = 4; // adds 3 filler bytes + final 0x04
        let enc = pkt.encode().expect("encode");
        // Total tail should end with [?, ?, ?, 4]. We don't require zero filler by spec.
        assert_eq!(enc[enc.len() - 1], 4);

        let dec = RtpPacket::decode(&enc).expect("decode");
        assert_eq!(dec.payload, vec![1, 2, 3]);
        assert_eq!(dec.padding_bytes, 4);
        assert!(dec.header.marker);
    }

    #[test]
    fn roundtrip_with_csrcs_15() {
        let mut hdr = RtpHeader::new(96, 1, 2, 3);
        let csrcs: Vec<u32> = (0..15).map(|i| 0x1111_0000 + i).collect();
        hdr = hdr.with_csrcs(csrcs.clone());
        let pkt = RtpPacket::new(hdr, vec![9, 9, 9]);
        let enc = pkt.encode().expect("encode");
        let dec = RtpPacket::decode(&enc).expect("decode");
        assert_eq!(dec.header.csrcs, csrcs);
        assert_eq!(dec.payload, vec![9, 9, 9]);
    }

    #[test]
    fn header_extension_zero_length_roundtrip() {
        let mut hdr = RtpHeader::new(100, 10, 20, 30);
        hdr = hdr.with_extension(Some(RtpHeaderExtension::new(0xBEEF, vec![])));
        let pkt = RtpPacket::new(hdr, vec![]);
        let enc = pkt.encode().expect("encode");
        let dec = RtpPacket::decode(&enc).expect("decode");
        let ext = dec.header.header_extension.expect("ext");
        assert_eq!(ext.profile, 0xBEEF);
        assert!(ext.data.is_empty());
    }

    #[test]
    fn header_extension_unaligned_data_roundtrip() {
        // data len = 6 (not multiple of 4) -> should pad to 8 on wire; decode returns 8 bytes.
        let mut hdr = RtpHeader::new(100, 10, 20, 30);
        let orig = vec![1, 2, 3, 4, 5, 6];
        hdr = hdr.with_extension(Some(RtpHeaderExtension::new(0x1234, orig.clone())));
        let pkt = RtpPacket::new(hdr, vec![0xAA]);
        let enc = pkt.encode().expect("encode");
        let dec = RtpPacket::decode(&enc).expect("decode");
        let ext = dec.header.header_extension.expect("ext");

        assert_eq!(ext.profile, 0x1234);
        // Should be 8 bytes: original 6 + 2 padding bytes
        assert_eq!(ext.data.len(), 8);
        assert_eq!(&ext.data[..6], &orig[..]);
        assert_eq!(&ext.data[6..], &[0, 0]);
        assert_eq!(dec.payload, vec![0xAA]);
    }

    // ---- Extra robustness checks -------------------------------------------

    #[test]
    fn padding_bit_follows_bytes() {
        // When padding_bytes == 0 the P bit must be 0 (even if header.padding=true).
        let mut hdr = RtpHeader::new(96, 1, 2, 3);
        hdr.padding = true; // user sets it, but encode derives from padding_bytes
        let pkt = RtpPacket::new(hdr, vec![]);
        let enc = pkt.encode().expect("encode");
        let p_bit = (enc[0] >> 5) & 1;
        assert_eq!(p_bit, 0);

        // Now with actual pad bytes:
        let mut hdr = RtpHeader::new(96, 1, 2, 3);
        hdr.padding = false;
        let mut pkt = RtpPacket::new(hdr, vec![7, 8, 9]);
        pkt.padding_bytes = 3;
        let enc = pkt.encode().expect("encode");
        let p_bit = (enc[0] >> 5) & 1;
        assert_eq!(p_bit, 1);
        let dec = RtpPacket::decode(&enc).unwrap();
        assert!(dec.header.padding);
        assert_eq!(dec.padding_bytes, 3);
    }

    #[test]
    fn zero_payload_with_padding_ok() {
        // It's legal to have zero payload and 1+ padding bytes.
        let mut hdr = RtpHeader::new(96, 10, 20, 30);
        hdr.padding = true;
        let mut pkt = RtpPacket::new(hdr, vec![]);
        pkt.padding_bytes = 1;
        let enc = pkt.encode().expect("encode");
        let dec = RtpPacket::decode(&enc).unwrap();
        assert!(dec.payload.is_empty());
        assert_eq!(dec.padding_bytes, 1);
    }

    #[test]
    fn decode_ignores_nonzero_padding_fill() {
        // RTP doesn't constrain the filler bytes (only the last count matters).
        let mut hdr = RtpHeader::new(96, 10, 20, 30);
        hdr.padding = true;
        let mut pkt = RtpPacket::new(hdr, b"ABC".to_vec());
        pkt.padding_bytes = 4;
        let mut enc = pkt.encode().expect("encode");

        // Overwrite filler bytes with non-zeros (leave last count alone).
        let n = enc.len();
        enc[n - 4] = 7;
        enc[n - 3] = 8;
        enc[n - 2] = 9;
        // enc[n-1] is the pad count (4)

        let dec = RtpPacket::decode(&enc).unwrap();
        assert_eq!(dec.payload, b"ABC");
        assert_eq!(dec.padding_bytes, 4);
    }

    #[test]
    fn header_extension_too_long_errors() {
        // ext length in words must fit u16; trigger error when it doesn't.
        let huge = vec![0u8; (u16::MAX as usize + 1) * 4];
        let mut hdr = RtpHeader::new(96, 1, 2, 3);
        hdr = hdr.with_extension(Some(RtpHeaderExtension::new(0xABCD, huge)));
        let pkt = RtpPacket::new(hdr, vec![1, 2, 3]);
        let err = pkt.encode().unwrap_err();
        assert!(matches!(err, RtpError::HeaderExtensionTooLong));
    }

    #[test]
    fn roundtrip_matrix_covering_common_axes() {
        // Sweep over a small matrix of PT/marker/CSRC count/payload sizes.
        let pts = [0u8, 96, 127];
        let markers = [false, true];
        let payload_lens = [0usize, 1, 7, 12, 13, 31];
        let csrc_counts = [0usize, 1, 15];

        for &pt in &pts {
            for &marker in &markers {
                for &cc in &csrc_counts {
                    let mut hdr =
                        RtpHeader::new(pt, 0xFFFF, 0x0123_4567, 0x89AB_CDEF).with_marker(marker);
                    let csrcs: Vec<u32> = (0..cc).map(|i| 0x1111_0000 + i as u32).collect();
                    hdr = hdr.with_csrcs(csrcs.clone());

                    for &plen in &payload_lens {
                        let payload: Vec<u8> = (0..plen as u8).collect();
                        let pkt = RtpPacket::new(hdr.clone(), payload.clone());
                        let enc = pkt.encode().expect("encode");
                        let dec = RtpPacket::decode(&enc).expect("decode");
                        assert_eq!(dec.header.payload_type, pt);
                        assert_eq!(dec.header.marker, marker);
                        assert_eq!(dec.header.csrcs, csrcs);
                        assert_eq!(dec.payload, payload);
                        assert_eq!(dec.padding_bytes, 0);
                    }
                }
            }
        }
    }

    #[test]
    fn header_extension_various_lengths_roundtrip() {
        for len in [0usize, 1, 2, 3, 4, 5, 6, 7, 8, 15, 16] {
            let data: Vec<u8> = (0..len as u8).collect();
            let mut hdr = RtpHeader::new(100, 10, 20, 30);
            hdr = hdr.with_extension(Some(RtpHeaderExtension::new(0xEE01, data.clone())));
            let pkt = RtpPacket::new(hdr, vec![0x55, 0xAA]);
            let enc = pkt.encode().expect("encode");
            let dec = RtpPacket::decode(&enc).expect("decode");
            let ext = dec.header.header_extension.expect("ext");

            assert_eq!(ext.profile, 0xEE01);
            // wire must pad to multiple of 4
            assert_eq!(ext.data.len() % 4, 0);
            assert_eq!(&ext.data[..data.len()], &data[..]);
            assert!(ext.data[data.len()..].iter().all(|&b| b == 0));
            assert_eq!(dec.payload, vec![0x55, 0xAA]);
        }
    }
}
