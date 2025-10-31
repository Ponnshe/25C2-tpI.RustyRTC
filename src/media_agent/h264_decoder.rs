use std::sync::Arc;

use openh264::{
    OpenH264API,
    decoder::{Decoder as ODecoder, DecoderConfig},
    formats::YUVSource, // for .dimensions(), .rgb8_len(), .write_rgb8()
};

use crate::media_agent::{
    frame_format::FrameFormat,
    media_agent_error::{MediaAgentError, Result},
    utils::now_millis,
    video_frame::VideoFrame,
};

pub struct H264Decoder {
    inner: Option<ODecoder>,
}

impl H264Decoder {
    pub fn new() -> Self {
        // Use the explicit API constructor (works without enabling the crate's "source" feature).
        let api = OpenH264API::from_source();
        let inner = ODecoder::with_api_config(api, DecoderConfig::new()).ok();
        Self { inner }
    }

    pub fn decode(&mut self, payload: &[u8]) -> Result<VideoFrame> {
        if let Some(decoder) = self.inner.as_mut() {
            match decoder.decode(payload) {
                // 0.9 returns Result<Option<DecodedYUV>>, not a struct with `.image`
                Ok(Some(yuv)) => {
                    let (w, h) = yuv.dimensions();
                    let mut rgb = vec![0u8; yuv.rgb8_len()];
                    yuv.write_rgb8(&mut rgb);
                    return Ok(VideoFrame {
                        width: w as u32,
                        height: h as u32,
                        format: FrameFormat::Rgb,
                        bytes: Arc::new(rgb),
                        timestamp_ms: now_millis(),
                    });
                }
                Ok(None) => {
                    // no picture yet — fall through to your existing “empty frame” behavior
                }
                Err(e) => {
                    return Err(MediaAgentError::Codec(format!(
                        "openh264 decode error: {e}"
                    )));
                }
            }
        }

        Ok(VideoFrame {
            width: 0,
            height: 0,
            format: FrameFormat::Rgb,
            bytes: Arc::new(payload.to_vec()),
            timestamp_ms: now_millis(),
        })
    }
}
