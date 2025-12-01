/// Protocol constants and header layout.
///
/// Header:
///   [ver: u8][msg_type: u8][flags: u16][body_len: u32]
/// Body:
///   [payload bytes...], up to `MAX_BODY_LEN`.
pub const PROTO_VERSION: u8 = 1;

/// Maximum allowed body size for a frame (to avoid OOM).
pub const MAX_BODY_LEN: usize = 1_048_576; // 1 MiB
