use openh264::{
    OpenH264API,
    encoder::{
        BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode,
        SpsPpsStrategy, UsageType,
    },
    formats::{RgbSliceU8, YUVBuffer},
};

use crate::media_agent::{
    frame_format::FrameFormat, media_agent_error::MediaAgentError, video_frame::VideoFrame,
};

/// A high-level wrapper around the OpenH264 encoder.
///
/// This struct manages the configuration state (FPS, Bitrate, Keyframe Interval)
/// and handles the conversion of incoming frames into a format compatible with
/// the underlying encoder engine.
///
/// # Color Space Conversion
/// OpenH264 natively expects YUV I420 input. Since this wrapper currently accepts
/// RGB frames, it performs an internal CPU-based **RGB -> YUV** conversion for every frame.
pub struct H264Encoder {
    enc: Option<Encoder>,
    target_fps: u32,
    target_bps: u32,
    keyint: u32,
}

impl H264Encoder {
    /// Creates and initializes a new H.264 encoder.
    ///
    /// # Arguments
    ///
    /// * `frame_rate` - Target frames per second (e.g., 30).
    /// * `bit_rate` - Target bitrate in bits per second (e.g., 1_500_000).
    /// * `keyint` - Intra-frame period (keyframe interval).
    pub fn new(frame_rate: u32, bit_rate: u32, keyint: u32) -> Self {
        let mut me = Self {
            enc: None,
            target_fps: frame_rate,
            target_bps: bit_rate,
            keyint,
        };
        me.init_encoder();
        me
    }

    /// Internal helper to initialize (or re-initialize) the OpenH264 instance.
    ///
    /// Configures the encoder for real-time camera usage (`UsageType::CameraVideoRealTime`),
    /// using Constant ID strategy for SPS/PPS insertion.
    fn init_encoder(&mut self) {
        // Build config via builder methods (OpenH264 0.9 API style)
        let cfg = EncoderConfig::new()
            .usage_type(UsageType::CameraVideoRealTime)
            .max_frame_rate(FrameRate::from_hz(self.target_fps as f32))
            .bitrate(BitRate::from_bps(self.target_bps))
            .rate_control_mode(RateControlMode::Bitrate)
            // Strategy: Insert SPS/PPS with every IDR frame to ensure stream joinability.
            .sps_pps_strategy(SpsPpsStrategy::ConstantId)
            .intra_frame_period(IntraFramePeriod::from_num_frames(self.keyint));

        let api = OpenH264API::from_source();
        // Use the config-aware constructor to apply settings immediately
        self.enc = Encoder::with_api_config(api, cfg).ok();
    }

    /// Encodes a video frame into an H.264 bitstream (Annex-B format).
    ///
    /// # Process
    /// 1. Identifies the input format.
    /// 2. Converts the input data (e.g., RGB) into a `YUVBuffer`.
    /// 3. Feeds the YUV buffer to the encoder.
    /// 4. Returns the encoded NAL units.
    ///
    /// # Errors
    ///
    /// Returns `MediaAgentError::Codec` if:
    /// * The underlying encoder instance is missing (initialization failed).
    /// * The encoding operation itself fails inside OpenH264.
    ///
    /// # Panics
    ///
    /// Panics if `frame.format` is `FrameFormat::Yuv420`, as the direct YUV pass-through
    /// path is not yet implemented.
    pub fn encode_frame_to_h264(&mut self, frame: &VideoFrame) -> Result<Vec<u8>, MediaAgentError> {
        // Placeholder for future zero-copy YUV path implementation
        match frame.format {
            FrameFormat::Rgb => {}
            FrameFormat::Yuv420 => {
                // TODO: use YUVBuffer::new(...) if you already have planar YUV to avoid conversion.
            }
        }

        let Some(enc) = self.enc.as_mut() else {
            return Err(MediaAgentError::Codec(
                "openh264 encoder unavailable".into(),
            ));
        };

        let w = frame.width as usize;
        let h = frame.height as usize;

        // Prepare source slice for conversion
        let rgb_slice = match &frame.data {
            crate::media_agent::video_frame::VideoFrameData::Rgb(buf) => {
                RgbSliceU8::new(buf.as_slice(), (w, h))
            }
            crate::media_agent::video_frame::VideoFrameData::Yuv420 { .. } => {
                // This path is technically unreachable due to the match block above,
                // but serves as a reminder for future implementation.
                panic!("Direct YUV encoding not implemented yet");
            }
        };

        // Convert RGB -> YUV (CPU intensive)
        let yuv = YUVBuffer::from_rgb_source(rgb_slice);

        // Perform encoding
        let bitstream = enc
            .encode(&yuv)
            .map_err(|e| MediaAgentError::Codec(e.to_string()))?;

        // Return the raw bitstream (Annex-B)
        Ok(bitstream.to_vec())
    }

    /// Forces the generation of a Keyframe (IDR) on the next encode call.
    ///
    /// Essential for allowing new clients to subscribe to the stream or to recover
    /// from massive packet loss.
    pub fn request_keyframe(&mut self) {
        if let Some(enc) = self.enc.as_mut() {
            enc.force_intra_frame();
        }
    }

    #[allow(dead_code)]
    pub fn target_fps(&self) -> u32 {
        self.target_fps
    }

    #[allow(dead_code)]
    pub fn target_bps(&self) -> u32 {
        self.target_bps
    }

    #[allow(dead_code)]
    pub fn keyint(&self) -> u32 {
        self.keyint
    }

    /// Updates the encoder configuration dynamically.
    ///
    /// # Behavior
    /// Checks if the parameters have actually changed. If they have, it updates the internal
    /// state and **re-initializes the encoder**.
    ///
    /// **Warning**: This causes a hard reset of the encoder pipeline. The stream context
    /// is reset, and the first frame generated after this call will be an IDR frame.
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Configuration changed and encoder was re-initialized.
    /// * `Ok(false)` - Configuration was identical; no action taken.
    /// * `Err(...)` - Failed to re-initialize the encoder.
    pub fn set_config(
        &mut self,
        new_fps: u32,
        new_bitrate: u32,
        new_keyint: u32,
    ) -> Result<bool, MediaAgentError> {
        if self.should_skip_update(new_fps, new_bitrate, new_keyint) {
            return Ok(false);
        }
        self.target_fps = new_fps;
        self.target_bps = new_bitrate;
        self.keyint = new_keyint;

        // Re-init returns the new encoder via Encoder::with_api_config.
        // If it fails, we catch it here.
        self.init_encoder();

        if self.enc.is_none() {
            Err(MediaAgentError::Codec(
                "Failed to re-initialize H264 encoder with new config".into(),
            ))
        } else {
            Ok(true)
        }
    }

    /// Helper to determine if a config update is necessary.
    fn should_skip_update(&self, new_fps: u32, new_bitrate: u32, new_keyint: u32) -> bool {
        new_fps == self.target_fps && new_bitrate == self.target_bps && new_keyint == self.keyint
    }
}
