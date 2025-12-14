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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaSpec {
    pub media_type: MediaType,
    pub codec_spec: CodecSpec,
}
