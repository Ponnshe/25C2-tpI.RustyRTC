use openh264::{
    decoder::{DecodedYUV, Decoder as ODecoder},
    formats::YUVSource,
};
use std::sync::Arc;

use crate::{app::log_sink::LogSink, media_agent::{
    frame_format::FrameFormat,
    media_agent_error::{MediaAgentError, Result},
    utils::now_millis,
    video_frame::{
        VideoFrame,
        VideoFrameData,
    },
}, sink_info};

pub struct H264Decoder {
    inner: Option<ODecoder>,
    logger: Arc<dyn LogSink>
}

impl H264Decoder {
    pub fn new(logger: Arc<dyn LogSink>) -> Self {
        Self {
            logger,
            inner: openh264::decoder::Decoder::new().ok(),
        }
    }

    pub fn decode_frame(&mut self, bytes: &[u8], frame_format: FrameFormat) -> Result<Option<VideoFrame>> {
        let Some(dec) = self.inner.as_mut() else {
            return Err(MediaAgentError::Codec(
                "openh264 decoder unavailable".into(),
            ));
        };

        let t0 = std::time::Instant::now();
        let res = dec.decode(bytes);
        let t_decode = t0.elapsed();

        match res {
            Ok(Some(yuv)) => {
                let t1 = std::time::Instant::now();
                let frame = yuv_to_videoframe(&yuv, frame_format);
                let t_conv = t1.elapsed();
                sink_info!(self.logger, "[Decoder timing] decode: {:?}, yuv_convertion: {:?}", t_decode, t_conv);
                Ok(Some(frame))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                // Reinitialize the decoder on error to clear its internal state
                self.inner = openh264::decoder::Decoder::new().ok();
                Err(MediaAgentError::Codec(format!(
                    "openh264 decode error: {e}"
                )))
            }
        }
    }
}

fn yuv_to_videoframe(yuv: &DecodedYUV<'_>, frame_format: FrameFormat) -> VideoFrame {
    match frame_format {
        FrameFormat::Rgb => yuv_to_rgbframe(yuv),
        FrameFormat::Yuv420 => yuv_to_yuv420frame(yuv),
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

    let data = VideoFrameData::Rgb(Arc::new(rgb));

    VideoFrame {
        width: w as u32,
        height: h as u32,
        format: FrameFormat::Rgb,
        data,
        timestamp_ms: ts,
    }
}

fn yuv_to_yuv420frame(yuv: &DecodedYUV<'_>) -> VideoFrame {
    let (w, h) = yuv.dimensions();
    let w = w as usize;
    let h = h as usize;

    let (y_stride_orig, u_stride_orig, v_stride_orig) = yuv.strides();

    // New strides for wgpu
    let y_stride_new = aligned_stride(w);
    let uv_w = (w + 1) / 2;
    let u_stride_new = aligned_stride(uv_w);
    let v_stride_new = aligned_stride(uv_w);
    
    let uv_h = (h + 1) / 2;

    let mut y_plane = vec![0u8; y_stride_new * h];
    let mut u_plane = vec![0u8; u_stride_new * uv_h];
    let mut v_plane = vec![0u8; v_stride_new * uv_h];

    // Copy Y plane
    let src_y = yuv.y();
    for row in 0..h {
        let src_start = row * y_stride_orig;
        let dst_start = row * y_stride_new;
        y_plane[dst_start..dst_start + w].copy_from_slice(&src_y[src_start..src_start + w]);
    }

    // Copy U plane
    let src_u = yuv.u();
    for row in 0..uv_h {
        let src_start = row * u_stride_orig;
        let dst_start = row * u_stride_new;
        u_plane[dst_start..dst_start + uv_w].copy_from_slice(&src_u[src_start..src_start + uv_w]);
    }

    // Copy V plane
    let src_v = yuv.v();
    for row in 0..uv_h {
        let src_start = row * v_stride_orig;
        let dst_start = row * v_stride_new;
        v_plane[dst_start..dst_start + uv_w].copy_from_slice(&src_v[src_start..src_start + uv_w]);
    }

    let ts_raw = yuv.timestamp().as_millis() as u128;
    let ts = if ts_raw == 0 { now_millis() } else { ts_raw };

    VideoFrame {
        width: w as u32,
        height: h as u32,
        timestamp_ms: ts,
        format: FrameFormat::Yuv420,
        data: VideoFrameData::Yuv420 {
            y: Arc::new(y_plane),
            u: Arc::new(u_plane),
            v: Arc::new(v_plane),
            y_stride: y_stride_new,
            u_stride: u_stride_new,
            v_stride: v_stride_new,
        },
    }
}

fn aligned_stride(width: usize) -> usize {
    const ALIGNMENT: usize = 256; // wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
    (width + ALIGNMENT - 1) / ALIGNMENT * ALIGNMENT
}
