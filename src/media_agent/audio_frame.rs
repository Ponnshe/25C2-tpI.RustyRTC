use std::sync::Arc;

/// Represents a single audio frame with associated metadata.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// The raw audio samples (mono, f32).
    pub data: Arc<Vec<f32>>,
    /// Number of samples in this frame.
    pub samples: usize,
    /// Sample rate in Hz (e.g., 48000).
    pub sample_rate: u32,
    /// Number of channels (e.g., 1).
    pub channels: u16,
    /// Timestamp of capture in milliseconds.
    pub timestamp_ms: u128,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_frame_creation() {
        let data = vec![0.0; 960];
        let frame = AudioFrame {
            data: Arc::new(data.clone()),
            samples: 960,
            sample_rate: 48000,
            channels: 1,
            timestamp_ms: 123456789,
        };

        assert_eq!(frame.samples, 960);
        assert_eq!(frame.sample_rate, 48000);
        assert_eq!(frame.channels, 1);
        assert_eq!(frame.timestamp_ms, 123456789);
        assert_eq!(frame.data.len(), 960);
    }
}
