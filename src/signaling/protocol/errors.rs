use std::io;

/// Protocol-level errors (body parsing/format issues, etc.).
#[derive(Debug)]
pub enum ProtoError {
    UnknownType(u8),
    Truncated,
    InvalidUtf8,
    TooLarge,
    InvalidFormat(&'static str),
    StringTooLong { max: usize, actual: usize },
}

/// Frame-level error wrapper: IO vs protocol.
#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    Proto(ProtoError),
}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ProtoError> for FrameError {
    fn from(e: ProtoError) -> Self {
        Self::Proto(e)
    }
}
