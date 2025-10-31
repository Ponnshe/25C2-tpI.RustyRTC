use std::sync::Arc;

use crate::media_agent::{
    frame_format::FrameFormat,
    media_agent_error::{MediaAgentError, Result},
    utils::now_millis,
    video_frame::VideoFrame,
};

pub struct H264Decoder {
    inner: Option<openh264::decoder::Decoder>,
}

impl H264Decoder {
    pub fn new() -> Self {
        {
            let inner = openh264::decoder::Decoder::new().ok();
            Self { inner }
        }
    }

    pub fn decode(&mut self, payload: &[u8]) -> Result<VideoFrame> {
        {
            if let Some(decoder) = self.inner.as_mut() {
                match decoder.decode(payload) {
                    Ok(result) => {
                        if let Some(image) = result.image {
                            let plane = image.to_rgb();
                            return Ok(VideoFrame {
                                width: plane.width(),
                                height: plane.height(),
                                format: FrameFormat::Rgb,
                                bytes: Arc::new(plane.as_slice().to_vec()),
                                timestamp_ms: now_millis(),
                            });
                        }
                    }
                    Err(e) => {
                        return Err(MediaAgentError::Codec(format!(
                            "openh264 decode error: {e:?}"
                        )));
                    }
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
