pub type Result<T> = std::result::Result<T, MediaAgentError>;
#[derive(Debug)]
pub enum MediaAgentError {
    Camera(String),
    Codec(String),
    Send(String),
}
