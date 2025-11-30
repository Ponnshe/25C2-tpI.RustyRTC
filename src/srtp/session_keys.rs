use crate::srtp::constants::{SESSION_AUTH_LEN, SESSION_KEY_LEN, SESSION_SALT_LEN};

pub struct SessionKeys {
    pub(crate) enc_key: [u8; SESSION_KEY_LEN],
    pub(crate) auth_key: [u8; SESSION_AUTH_LEN],
    pub(crate) salt: [u8; SESSION_SALT_LEN],
}
