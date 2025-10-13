use crate::sdp::media;
pub struct MediaSpec {
    pub mid: String, // "0"
    pub kind: media::MediaKind,
    pub direction: &'static str,  // "sendrecv" | "sendonly" | ...
    pub payload_type: u8,         // 96
    pub codec_name: &'static str, // "VP8"
    pub clock_rate: u32,          // 90000
    pub fmtp: Option<String>,     // None for VP8
    pub header_exts: Vec<(u8, &'static str)>, // [(1, "urn:ietf:params:rtp-hdrext:sdes:mid")]
    pub ssrc: u32,                // random
    pub cname: String,            // random token
    pub stream_id: String,        // e.g., "stream0"
    pub track_id: String,         // e.g., "video0"
}
