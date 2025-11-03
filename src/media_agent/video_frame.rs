use std::sync::Arc;

use crate::media_agent::{frame_format::FrameFormat, utils::now_millis};

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub timestamp_ms: u128,
    pub format: FrameFormat,
    pub bytes: Arc<Vec<u8>>,
}

impl VideoFrame {
    pub fn synthetic(width: u32, height: u32, tick: u8) -> Self {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let r = x as u8 ^ tick;
                let g = y as u8 ^ tick;
                let b = (x.wrapping_add(y)) as u8 ^ tick;
                data.push(r);
                data.push(g);
                data.push(b);
            }
        }
        Self {
            width,
            height,
            format: FrameFormat::Rgb,
            bytes: Arc::new(data),
            timestamp_ms: now_millis(),
        }
    }
}
