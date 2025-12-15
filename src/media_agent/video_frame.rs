use std::sync::Arc;

use crate::media_agent::{frame_format::FrameFormat, utils::now_millis};

/// Type alias representing the raw pointers and strides of a YUV420 planar image.
///
/// Tuple layout: `(Y_Plane, U_Plane, V_Plane, Y_Stride, U_Stride, V_Stride)`
pub type YuvPlanes<'a> = (&'a [u8], &'a [u8], &'a [u8], usize, usize, usize);

/// Represents a single video frame with associated metadata.
///
/// This struct serves as the primary unit of data passed through the media pipeline.
/// It wraps the raw pixel data (`VideoFrameData`) with dimensions, timestamp, and format info.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Width of the frame in pixels.
    pub width: u32,
    /// Height of the frame in pixels.
    pub height: u32,
    /// Timestamp of capture or generation in milliseconds.
    pub timestamp_ms: u128,
    /// The pixel format of the underlying data.
    pub format: FrameFormat,
    /// The actual pixel data storage.
    pub data: VideoFrameData,
}

/// Enum holding the underlying pixel data storage.
///
/// Uses `Arc<Vec<u8>>` to allow cheap cloning of frames. This means passing a `VideoFrame`
/// to multiple subsystems (e.g., Encoder and UI) does not require deep copying the pixel buffers.
#[derive(Debug, Clone)]
pub enum VideoFrameData {
    /// Packed RGB data (usually 24 bits per pixel: R, G, B).
    Rgb(Arc<Vec<u8>>),

    /// Planar YUV 4:2:0 data.
    ///
    /// The data is split into three separate planes. Note that U and V planes
    /// are typically subsampled (half width/height of Y).
    Yuv420 {
        y: Arc<Vec<u8>>,
        u: Arc<Vec<u8>>,
        v: Arc<Vec<u8>>,
        /// The byte width of a row in the Y plane (may include padding).
        y_stride: usize,
        /// The byte width of a row in the U plane.
        u_stride: usize,
        /// The byte width of a row in the V plane.
        v_stride: usize,
    },
}

impl VideoFrame {
    /// Generates a synthetic RGB frame with a moving test pattern.
    ///
    /// Useful for testing the pipeline when no physical camera is available.
    ///
    /// # Arguments
    /// * `tick` - A varying value (0-255) to animate the pattern over time.
    #[must_use]
    pub fn synthetic_rgb(width: u32, height: u32, tick: u8) -> Self {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                // XOR pattern generation
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

    /// Generates a synthetic YUV420 frame with a moving test pattern.
    ///
    /// Creates a luminance (Y) pattern based on coordinates and a chroma (UV)
    /// variation to produce color.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn synthetic_yuv420(width: u32, height: u32, tick: u8) -> Self {
        let w = width as usize;
        let h = height as usize;

        // Calculate dimensions and strides (tightly packed)
        let y_stride = w;
        let uv_w = w.div_ceil(2);
        let uv_h = h.div_ceil(2);
        let uv_stride = uv_w;

        let mut y = vec![0u8; y_stride * h];
        let mut u = vec![128u8; uv_stride * uv_h];
        let mut v = vec![128u8; uv_stride * uv_h];

        // Generate Luma (Y)
        for yy in 0..h {
            for xx in 0..w {
                // simple luminance pattern
                y[yy * y_stride + xx] = ((xx ^ yy) as u8).wrapping_add(tick);
            }
        }

        // Generate Chroma (UV) - small variation around 128 (neutral gray)
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

    /// Attempts to retrieve a reference to the raw bytes if the frame is RGB.
    ///
    /// # Returns
    /// * `Some(&[u8])` containing the packed RGB data if `format` is `Rgb`.
    /// * `None` if the frame is in `Yuv420` format.
    pub fn as_rgb_bytes(&self) -> Option<&[u8]> {
        match &self.data {
            VideoFrameData::Rgb(buf) => Some(buf.as_ref()),
            _ => None,
        }
    }

    /// Attempts to retrieve the raw planes and strides if the frame is YUV420.
    ///
    /// # Returns
    /// * `Some(YuvPlanes)` containing references to Y, U, V buffers and their strides.
    /// * `None` if the frame is in `Rgb` format.
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
