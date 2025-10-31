use std::sync::Arc;

use openh264::{
    decoder::{DecodedYUV, Decoder},
    formats::YUVSource, // dimensions(), rgb8_len(), write_rgb8()
    nal_units,
};

use crate::media_agent::{
    frame_format::FrameFormat,
    media_agent_error::{MediaAgentError, Result},
    utils::now_millis,
    video_frame::VideoFrame,
};

pub struct H264Decoder {
    inner: Option<Decoder>,
}

impl H264Decoder {
    pub fn new() -> Self {
        let inner = Decoder::new().ok(); // v0.9 canonical ctor
        Self { inner }
    }

    /// Decodes either a single NAL *or* an Annex-B byte stream.
    /// Returns Ok(Some(frame)) when a picture is produced; Ok(None) when more data is needed.
    pub fn decode(&mut self, bytes: &[u8]) -> Result<Option<VideoFrame>> {
        let Some(dec) = self.inner.as_mut() else {
            return Err(MediaAgentError::Codec(
                "openh264 decoder unavailable".into(),
            ));
        };

        // If it looks like Annex-B, iterate NALs; otherwise treat `bytes` as a single NAL.
        if looks_like_annex_b(bytes) {
            for nalu in nal_units(bytes) {
                if let Some(frame) = decode_one(dec, nalu)? {
                    return Ok(Some(frame));
                }
            }
            Ok(None)
        } else {
            decode_one(dec, bytes)
        }
    }
}

fn decode_one(dec: &mut Decoder, packet: &[u8]) -> Result<Option<VideoFrame>> {
    match dec.decode(packet) {
        Ok(Some(yuv)) => Ok(Some(yuv_to_rgbframe(&yuv))),
        Ok(None) => Ok(None),
        Err(e) => Err(MediaAgentError::Codec(format!(
            "openh264 decode error: {e}"
        ))),
    }
}

fn yuv_to_rgbframe(yuv: &DecodedYUV<'_>) -> VideoFrame {
    let (w, h) = yuv.dimensions();
    let mut rgb = vec![0u8; yuv.rgb8_len()];
    yuv.write_rgb8(&mut rgb);

    // Prefer the decoder’s stream timestamp; fall back to wall-clock if needed.
    let ts = yuv.timestamp().as_millis() as u128;
    let ts = if ts == 0 { now_millis() } else { ts };

    VideoFrame {
        width: w as u32,
        height: h as u32,
        format: FrameFormat::Rgb,
        bytes: Arc::new(rgb),
        timestamp_ms: ts,
    }
}

/// Quick Annex-B check: 0x000001 or 0x00000001 start codes present?
fn looks_like_annex_b(buf: &[u8]) -> bool {
    buf.windows(3).any(|w| w == [0, 0, 1]) || buf.windows(4).any(|w| w == [0, 0, 0, 1])
}
