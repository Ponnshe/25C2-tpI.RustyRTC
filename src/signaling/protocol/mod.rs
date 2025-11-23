use std::io::{self, Read, Write};

/// Protocol
/// ----------- Header -----------------
/// Version (1B) - Msg Type (1B) - Flags (2B)
/// Body Length (2B)
/// ----------- Body -------------------
/// Payload (1MiB)
/// Protocol version (first byte in the frame header).
pub const PROTO_VERSION: u8 = 1;

/// Maximum allowed body size for a frame (to avoid OOM).
pub const MAX_BODY_LEN: usize = 1_048_576; // 1 MiB

// ---- Basic types ----------------------------------------------------------

pub type UserName = String;
pub type SessionId = String;
pub type SessionCode = String;
pub type TxnId = u64; // for offer/answer reliability
