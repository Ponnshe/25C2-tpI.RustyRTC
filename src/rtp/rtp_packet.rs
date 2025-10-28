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
}
