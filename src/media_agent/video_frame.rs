use std::sync::Arc;

use crate::media_agent::{frame_format::FrameFormat, utils::now_millis};

pub type YuvPlanes<'a> = (&'a [u8], &'a [u8], &'a [u8], usize, usize, usize);

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub timestamp_ms: u128,
    pub format: FrameFormat,
    pub data: VideoFrameData,
}

#[derive(Debug, Clone)]
pub enum VideoFrameData {
    Rgb(Arc<Vec<u8>>),

    Yuv420 {
        y: Arc<Vec<u8>>,
        u: Arc<Vec<u8>>,
        v: Arc<Vec<u8>>,
        y_stride: usize,
        u_stride: usize,
        v_stride: usize,
    },
}

impl VideoFrame {
    #[must_use]
    pub fn synthetic_rgb(width: u32, height: u32, tick: u8) -> Self {
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
            timestamp_ms: now_millis(),
            data: VideoFrameData::Rgb(Arc::new(data)),
        }
    }

    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn synthetic_yuv420(width: u32, height: u32, tick: u8) -> Self {
        let w = width as usize;
        let h = height as usize;
        let y_stride = w;
        let uv_w = w.div_ceil(2);
        let uv_h = h.div_ceil(2);
        let uv_stride = uv_w;

        let mut y = vec![0u8; y_stride * h];
        let mut u = vec![128u8; uv_stride * uv_h];
        let mut v = vec![128u8; uv_stride * uv_h];

        for yy in 0..h {
            for xx in 0..w {
                // simple luminance pattern
                y[yy * y_stride + xx] = ((xx ^ yy) as u8).wrapping_add(tick);
            }
        }

        // small chroma variation
        for yy in 0..uv_h {
            for xx in 0..uv_w {
                u[yy * uv_stride + xx] = (128u8).wrapping_add(((xx + yy) as u8).wrapping_add(tick));
                v[yy * uv_stride + xx] = (128u8).wrapping_sub(((xx + yy) as u8).wrapping_add(tick));
            }
        }

        Self {
            width,
            height,
            format: FrameFormat::Yuv420,
            timestamp_ms: now_millis(),
            data: VideoFrameData::Yuv420 {
                y: Arc::new(y),
                u: Arc::new(u),
                v: Arc::new(v),
                y_stride,
                u_stride: uv_stride,
                v_stride: uv_stride,
            },
        }
    }

    pub fn as_rgb_bytes(&self) -> Option<&[u8]> {
        match &self.data {
            VideoFrameData::Rgb(buf) => Some(buf.as_ref()),
            _ => None,
        }
    }

    pub fn as_yuv_planes(&self) -> Option<YuvPlanes<'_>> {
        match &self.data {
            VideoFrameData::Yuv420 {
                y,
                u,
                v,
                y_stride,
                u_stride,
                v_stride,
            } => Some((
                y.as_ref(),
                u.as_ref(),
                v.as_ref(),
                *y_stride,
                *u_stride,
                *v_stride,
            )),
            _ => None,
        }
    }
}
