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
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.header.csrcs.len() * 4 + self.payload.len() + 4);

        let cc = (self.header.csrcs.len() & 0x0F) as u8;
        let vpxcc = (self.header.version & 0b11) << 6
            | (self.header.padding as u8) << 5
            | (self.header.extension as u8) << 4
            | cc;

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
            let len_words = ((ext.data.len() + 3) / 4) as u16;
            out.extend_from_slice(&ext.profile.to_be_bytes());
            out.extend_from_slice(&len_words.to_be_bytes());
            out.extend_from_slice(&ext.data);

            // pad to 32-bit boundary with zero bytes
            let pad = (4 - (ext.data.len() % 4)) % 4;
            if pad != 0 {
                out.extend(std::iter::repeat(0u8).take(pad));
            }
        }

        // For encode(), we *do not* add RTP padding by default because the
        // session layer should decide this. If header.padding is true and
        // padding_bytes > 0, we append that many zero octets and set P bit.
        out.extend_from_slice(&self.payload);

        if self.header.padding && self.padding_bytes > 0 {
            // Add (padding_bytes - 1) zeros and end with padding_bytes count
            if self.padding_bytes > 1 {
                out.extend(std::iter::repeat(0u8).take((self.padding_bytes - 1) as usize));
            }
            out.push(self.padding_bytes);
        }

        out
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

        let sequence_number = u16::from_be_bytes(buf[2..4].try_into().unwrap());
        let timestamp = u32::from_be_bytes(buf[4..8].try_into().unwrap());
        let ssrc = u32::from_be_bytes(buf[8..12].try_into().unwrap());

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
            let csrc = u32::from_be_bytes(buf[idx..idx + 4].try_into().unwrap());
            csrcs.push(csrc);
            idx += 4;
        }

        // Header extension (generic 3550)
        let mut header_extension: Option<RtpHeaderExtension> = None;
        if extension {
            if buf.len() < idx + 4 {
                return Err(RtpError::HeaderExtensionTooShort);
            }
            let profile = u16::from_be_bytes(buf[idx..idx + 2].try_into().unwrap());
            let length_words = u16::from_be_bytes(buf[idx + 2..idx + 4].try_into().unwrap());
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
