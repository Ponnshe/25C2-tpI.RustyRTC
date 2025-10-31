use openh264::{
    OpenH264API,
    encoder::{
        BitRate, Encoder, EncoderConfig, FrameRate, RateControlMode, SpsPpsStrategy, UsageType,
    },
    formats::{RgbSliceU8, YUVBuffer},
};

use crate::media_agent::{
    frame_format::FrameFormat, media_agent_error::MediaAgentError, video_frame::VideoFrame,
};

pub struct H264Encoder {
    enc: Option<Encoder>,

    // simple knobs you can expose/tweak
    target_fps: u32,
    target_bps: u32,
    keyint: u32,
}

impl H264Encoder {
    pub fn new(target_fps: u32, target_bps: u32, keyint: u32) -> Self {
        let mut me = Self {
            enc: None,
            target_fps,
            target_bps,
            keyint,
        };
        me.init_encoder();
        me
    }

    fn init_encoder(&mut self) {
        // Build config via builder methods (fields are private in 0.9)
        let cfg = EncoderConfig::new()
            .usage_type(UsageType::CameraVideoRealTime)
            .max_frame_rate(FrameRate::from_hz(self.target_fps as f32))
            .bitrate(BitRate::from_bps(self.target_bps))
            .rate_control_mode(RateControlMode::Bitrate)
            // Emit SPS/PPS with keyframes so new decoders can join mid-stream:
            .sps_pps_strategy(SpsPpsStrategy::InAccessUnit)
            // Periodic IDR every `keyint` frames (0 disables periodic IDR):
            .intra_frame_period(self.keyint.into());

        // Create preconfigured encoder
        let api = OpenH264API::default();
        self.enc = Encoder::with_api_config(api, cfg).ok();
    }

    /// Encode an RGB frame to an H.264 bytestream (Annex-B–compatible NAL sequence).
    pub fn encode_frame_to_h264(&mut self, frame: &VideoFrame) -> Result<Vec<u8>, MediaAgentError> {
        match frame.format {
            FrameFormat::Rgb => {}
            FrameFormat::Yuv420 => {
                // (Optional) you could add a fast-path using YUVBuffer::new and set_y/u/v planes.
            }
        }

        {
            let Some(enc) = self.enc.as_mut() else {
                return Err(MediaAgentError::Codec(
                    "openh264 encoder unavailable".into(),
                ));
            };

            // Wrap the raw RGB as a slice and let the crate convert to I420.
            let w = frame.width as usize;
            let h = frame.height as usize;
            let rgb = RgbSliceU8::new(frame.bytes.as_slice(), (w, h));
            let yuv = YUVBuffer::from_rgb_source(rgb);

            // Encode → EncodedBitStream → bytes
            let bitstream = enc
                .encode(&yuv)
                .map_err(|e| MediaAgentError::Codec(e.to_string()))?;
            let bytes = bitstream.to_vec(); // 0.9 exposes to_vec()

            Ok(bytes)
        }
    }

    /// Ask encoder to generate a keyframe on the next encode.
    pub fn request_keyframe(&mut self) {
        if let Some(enc) = self.enc.as_mut() {
            let _ = enc.request_keyframe();
        }
    }
}
