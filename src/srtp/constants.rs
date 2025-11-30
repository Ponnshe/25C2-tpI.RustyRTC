pub const SRTP_LABEL_ENCRYPTION: u8 = 0x00;
pub const SRTP_LABEL_AUTH: u8 = 0x01;
pub const SRTP_LABEL_SALT: u8 = 0x02;

// SRTP_AES128_CM_SHA1_80 constants
pub const SESSION_KEY_LEN: usize = 16; // 128 bits
pub const SESSION_AUTH_LEN: usize = 20; // 160 bits (SHA1)
pub const SESSION_SALT_LEN: usize = 14; // 112 bits
pub const AUTH_TAG_LEN: usize = 10; // 80 bits truncated

// Replay protection window size (64 packets)
pub const REPLAY_WINDOW_SIZE: u64 = 64;
