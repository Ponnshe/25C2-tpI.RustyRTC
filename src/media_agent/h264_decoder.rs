use openh264::{
    decoder::{DecodedYUV, Decoder as ODecoder},
    formats::YUVSource,
};
use std::sync::Arc;

use crate::{
    media_agent::{
        frame_format::FrameFormat,
        media_agent_error::{MediaAgentError, Result},
        utils::now_millis,
        video_frame::VideoFrame,
    },
    rtp_session::payload::h264_depacketizer::AccessUnit,
};

pub struct H264Decoder {
    inner: Option<ODecoder>,
}

impl H264Decoder {
    pub fn new() -> Self {
        Self {
            inner: openh264::decoder::Decoder::new().ok(),
        }
    }

    /// Feed one access unit (list of NAL units) in order.
    /// Returns Some frame when OpenH264 outputs a picture, or None if it needs more NALs.
    pub fn decode_au(&mut self, au: &AccessUnit) -> Result<Option<VideoFrame>> {
        let Some(dec) = self.inner.as_mut() else {
            return Err(MediaAgentError::Codec(
                "openh264 decoder unavailable".into(),
            ));
        };

        for nalu in &au.nalus {
            match dec.decode(nalu) {
                Ok(Some(yuv)) => return Ok(Some(yuv_to_rgbframe(&yuv))),
                Ok(None) => continue,
                Err(e) => {
                    return Err(MediaAgentError::Codec(format!(
                        "openh264 decode error: {e}"
                    )));
                }
            }
        }
        Ok(None)
    }
}

fn yuv_to_rgbframe(yuv: &DecodedYUV<'_>) -> VideoFrame {
    let (w, h) = yuv.dimensions();
    let mut rgb = vec![0u8; yuv.rgb8_len()];
    yuv.write_rgb8(&mut rgb);

    // If OpenH264 didn't propagate a timestamp, fall back to wall clock.
    let ts = {
        let t = yuv.timestamp().as_millis() as u128;
        if t == 0 { now_millis() } else { t }
    };

    VideoFrame {
        width: w as u32,
        height: h as u32,
        format: FrameFormat::Rgb,
        bytes: Arc::new(rgb),
        timestamp_ms: ts,
    }
}
