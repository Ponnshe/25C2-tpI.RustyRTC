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

pub struct H264Encoder {
    enc: Option<Encoder>,
    target_fps: u32,
    target_bps: u32,
    keyint: u32,
}

impl H264Encoder {
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

    fn init_encoder(&mut self) {
        // Build config via builder methods (0.9)
        let cfg = EncoderConfig::new()
            .usage_type(UsageType::CameraVideoRealTime)
            .max_frame_rate(FrameRate::from_hz(self.target_fps as f32))
            .bitrate(BitRate::from_bps(self.target_bps))
            .rate_control_mode(RateControlMode::Bitrate)
            // Valid variants in 0.9 (pick one; default is ConstantId)
            .sps_pps_strategy(SpsPpsStrategy::ConstantId)
            // 0.9 needs an explicit constructor, not `.into()`
            .intra_frame_period(IntraFramePeriod::from_num_frames(self.keyint));

        // In 0.9 there is no `OpenH264API::default()`
        let api = OpenH264API::from_source();
        // Use the config-aware constructor
        self.enc = Encoder::with_api_config(api, cfg).ok();
    }

    /// Encode an RGB frame to an H.264 bytestream (Annex-B style NAL sequence).
    pub fn encode_frame_to_h264(&mut self, frame: &VideoFrame) -> Result<Vec<u8>, MediaAgentError> {
        // (If you add a YUV fast path later, keep this match.)
        match frame.format {
            FrameFormat::Rgb => {}
            FrameFormat::Yuv420 => {
                // TODO: use YUVBuffer::new(...) if you already have planar YUV.
            }
        }

        let Some(enc) = self.enc.as_mut() else {
            return Err(MediaAgentError::Codec(
                "openh264 encoder unavailable".into(),
            ));
        };

        let w = frame.width as usize;
        let h = frame.height as usize;
        let rgb = RgbSliceU8::new(frame.bytes.as_slice(), (w, h));
        let yuv = YUVBuffer::from_rgb_source(rgb);

        let bitstream = enc
            .encode(&yuv)
            .map_err(|e| MediaAgentError::Codec(e.to_string()))?;

        // EncodedBitStream in 0.9 exposes `to_vec()`
        Ok(bitstream.to_vec())
    }

    /// Ask the encoder to produce a keyframe next.
    pub fn request_keyframe(&mut self) {
        if let Some(enc) = self.enc.as_mut() {
            // 0.9 uses `force_intra_frame`, not `request_keyframe`
            let _ = enc.force_intra_frame();
        }
    }

    pub fn target_fps(&self) -> u32 {
        self.target_fps
    }

    pub fn target_bps(&self) -> u32 {
        self.target_bps
    }

    pub fn keyint(&self) -> u32 {
        self.keyint
    }

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

        // Re-init returns the new encoder via Encoder::with_api_config, which can fail.
        // We need to handle that result and potentially bubble it up.
        self.init_encoder();

        if self.enc.is_none() {
            Err(MediaAgentError::Codec(
                "Failed to re-initialize H264 encoder with new config".into(),
            ))
        } else {
            Ok(true)
        }
    }

    fn should_skip_update(&self, new_fps: u32, new_bitrate: u32, new_keyint: u32) -> bool {
        new_fps == self.target_fps && new_bitrate == self.target_bps && new_keyint == self.keyint
    }
}
