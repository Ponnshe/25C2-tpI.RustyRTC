use std::sync::Arc;

use crate::media_agent::{
    frame_format::FrameFormat,
    media_agent_error::{MediaAgentError, Result},
    utils::now_millis,
    video_frame::VideoFrame,
};

use openh264::{
    decoder::{DecodedYUV, Decoder}, formats::YUVSource, nal_units
};

pub struct H264Decoder {
    inner: Option<Decoder>,
}

impl H264Decoder {
    pub fn new() -> Self {
        {
            let inner = openh264::decoder::Decoder::new().ok();
            Self { inner }
        }
    }
    
    pub fn decode(&self, bytes: &[u8]) -> Result<Option<VideoFrame>> {
        if let Some(decoder) = self.inner.as_mut(){
            for nalu in nal_units(bytes) {
                // Each decode can return Ok(Some(frame)), Ok(None), o Err(_)
                match decoder.decode(nalu) {
                    Ok(Some(yuv)) => {
                        let video_frame = decodedyuv_to_rgbframe(&yuv);
                        return Ok(Some(video_frame))
                    },
                    Ok(None) => continue,
                    Err(e) => return Err(MediaAgentError::Codec(format!("decode error: {e}").into())),
                }
            }
        }

        Ok(None)
    }
}

fn decodedyuv_to_rgbframe(yuv: &DecodedYUV) -> VideoFrame {
    let (width, height) = yuv.dimensions();
    let rgb_len = yuv.rgb8_len();
    let mut rgb = vec![0u8; rgb_len];
    yuv.write_rgb8(&mut rgb);

    VideoFrame {
        width: width as u32,
        height: height as u32,
        timestamp_ms: yuv.timestamp().as_millis() as u128,
        format: FrameFormat::Rgb,
        bytes: Arc::new(rgb),
    }
}
