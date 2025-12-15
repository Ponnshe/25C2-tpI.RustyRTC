use openh264::{
    decoder::{DecodedYUV, Decoder as ODecoder},
    formats::YUVSource,
};
use std::sync::Arc;

use crate::{
    log::log_sink::LogSink,
    media_agent::{
        frame_format::FrameFormat,
        media_agent_error::{MediaAgentError, Result},
        utils::now_millis,
        video_frame::{VideoFrame, VideoFrameData},
    },
    sink_debug,
};

/// A wrapper around the OpenH264 software decoder.
///
/// This struct manages the lifecycle of the underlying `openh264` decoder instance
/// and handles the conversion of decoded raw YUV data into the application's
/// `VideoFrame` format.
///
/// # Key Features
/// * **Automatic Recovery**: If the internal decoder fails, it attempts to re-initialize itself.
/// * **GPU Alignment**: When outputting YUV420, it automatically pads the data strides to
///   meet `wgpu` buffer alignment requirements (256 bytes).
pub struct H264Decoder {
    /// The underlying OpenH264 decoder. wrapped in Option to handle initialization failures.
    inner: Option<ODecoder>,
    logger: Arc<dyn LogSink>,
}

impl H264Decoder {
    /// Creates a new H264 decoder instance.
    ///
    /// Tries to initialize the native OpenH264 library. If initialization fails
    /// (e.g., library missing), `inner` will be `None`, and subsequent calls to
    /// `decode_frame` will return an error.
    pub fn new(logger: Arc<dyn LogSink>) -> Self {
        Self {
            logger,
            inner: openh264::decoder::Decoder::new().ok(),
        }
    }

    /// Decodes a raw H.264 byte slice (NAL unit) into a video frame.
    ///
    /// # Arguments
    ///
    /// * `bytes` - The encoded H.264 data (Annex B format).
    /// * `frame_format` - The desired output format (RGB or YUV420).
    ///
    /// # Returns
    ///
    /// * `Ok(Some(VideoFrame))` - If a complete frame was produced.
    /// * `Ok(None)` - If the decoder consumed the data but needs more input to produce a frame.
    /// * `Err(MediaAgentError)` - If decoding failed or the library is unavailable.
    ///
    /// # Error Handling
    ///
    /// If the underlying decoder returns an error, this method **drops and re-creates**
    /// the decoder instance. This "hard reset" strategy helps recover from corrupted
    /// streams or invalid state without crashing the worker thread.
    pub fn decode_frame(
        &mut self,
        bytes: &[u8],
        frame_format: FrameFormat,
    ) -> Result<Option<VideoFrame>> {
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

                sink_debug!(
                    self.logger,
                    "[Decoder timing] decode: {:?}, yuv_convertion: {:?}",
                    t_decode,
                    t_conv
                );
                Ok(Some(frame))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                // Reinitialize the decoder on error to clear its internal state
                // and attempt to recover for future frames.
                self.inner = openh264::decoder::Decoder::new().ok();
                Err(MediaAgentError::Codec(format!(
                    "openh264 decode error: {e}"
                )))
            }
        }
    }
}

/// Dispatches the YUV conversion based on the requested format.
fn yuv_to_videoframe(yuv: &DecodedYUV<'_>, frame_format: FrameFormat) -> VideoFrame {
    match frame_format {
        FrameFormat::Rgb => yuv_to_rgbframe(yuv),
        FrameFormat::Yuv420 => yuv_to_yuv420frame(yuv),
    }
}

/// Converts decoded YUV planar data to a packed RGB frame.
///
/// Uses `openh264`'s internal high-performance YUV->RGB converter.
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

/// Converts decoded YUV planar data to a stride-aligned YUV420 frame.
///
/// # Stride Alignment
///
/// This function performs a deep copy of the planes. It adjusts the "stride" (width in bytes per row)
/// to be a multiple of 256. This alignment is **required by wgpu** (WebGPU) for buffer copies.
///
/// The transform is: `OpenH264 Packed YUV` -> `Aligned YUV (256-byte aligned rows)`.
fn yuv_to_yuv420frame(yuv: &DecodedYUV<'_>) -> VideoFrame {
    let (w, h) = yuv.dimensions();

    let (y_stride_orig, u_stride_orig, v_stride_orig) = yuv.strides();

    // New strides calculated for wgpu compatibility (COPY_BYTES_PER_ROW_ALIGNMENT)
    let y_stride_new = aligned_stride(w);
    let uv_w = w.div_ceil(2);
    let u_stride_new = aligned_stride(uv_w);
    let v_stride_new = aligned_stride(uv_w);

    let uv_h = h.div_ceil(2);

    // Allocate aligned buffers
    let mut y_plane = vec![0u8; y_stride_new * h];
    let mut u_plane = vec![0u8; u_stride_new * uv_h];
    let mut v_plane = vec![0u8; v_stride_new * uv_h];

    // Copy Y plane (Row by Row)
    let src_y = yuv.y();
    for row in 0..h {
        let src_start = row * y_stride_orig;
        let dst_start = row * y_stride_new;
        // Copy only valid data, leave padding bytes (at the end of row) untouched/zeroed
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

/// Calculates the byte stride required to meet wgpu alignment standards.
///
/// Current standard: `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` is 256 bytes.
fn aligned_stride(width: usize) -> usize {
    const ALIGNMENT: usize = 256;
    width.div_ceil(ALIGNMENT) * ALIGNMENT
}
