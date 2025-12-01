use std::fmt;

/// A specialized `Result` type for MediaAgent operations.
///
/// This alias simplifies function signatures throughout the module by pre-filling
/// the `Err` variant with [`MediaAgentError`].
pub type Result<T> = std::result::Result<T, MediaAgentError>;

/// Represents all possible failures within the Media Agent subsystem.
///
/// This enum consolidates errors from various sub-components (Camera, Codecs, Threading)
/// into a single type for unified error handling across the pipeline.
#[derive(Debug)]
pub enum MediaAgentError {
    /// Errors originating from the video capture device or camera manager.
    /// Examples: Camera not found, initialization failed, or frame capture timeout.
    Camera(String),

    /// Errors occurring during video encoding (H.264) or decoding.
    /// Examples: OpenH264 library missing, invalid NAL unit, or configuration failure.
    Codec(String),

    /// Errors related to internal message passing (channels).
    /// Examples: Receiver disconnected, channel full (backpressure), or send timeout.
    Send(String),

    /// Low-level I/O errors.
    /// Examples: File system permission denied, raw buffer access failures, or OS-level pipe errors.
    Io(String),

    /// Specific failure when spawning the background encoder thread.
    /// Usually indicates system resource exhaustion (OS failed to create thread).
    EncoderSpawn(String),
}

impl fmt::Display for MediaAgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MediaAgentError::*;
        match self {
            Camera(e) => write!(f, "Camera error: {e}"),
            Codec(e) => write!(f, "Codec error: {e}"),
            Send(e) => write!(f, "Send error: {e}"),
            Io(e) => write!(f, "Io error: {e}"),
            EncoderSpawn(e) => write!(f, "Encoder Spawn error: {e}"),
        }
    }
}

// Implement `std::error::Error` to allow compatibility with the standard error ecosystem
// (e.g., `?` operator, `anyhow`, `Box<dyn Error>`).
impl std::error::Error for MediaAgentError {}
