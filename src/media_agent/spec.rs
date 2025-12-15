#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaType {
    Video,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecSpec {
    H264,
    G711U,
}

impl CodecSpec {
    pub fn media_type(&self) -> MediaType {
        match self {
            CodecSpec::H264 => MediaType::Video,
            CodecSpec::G711U => MediaType::Audio,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaSpec {
    pub media_type: MediaType,
    pub codec_spec: CodecSpec,
}
