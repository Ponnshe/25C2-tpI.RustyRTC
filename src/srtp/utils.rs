pub(super) type HmacSha1 = Hmac<Sha1>;
pub(super) type Aes128Ctr = Ctr128BE<Aes128>;

use aes::Aes128;
use aes::cipher::{KeyIvInit, StreamCipher};
use byteorder::{BigEndian, ByteOrder};
use ctr::Ctr128BE;
use hmac::Hmac;
use sha1::Sha1;

use crate::{
    srtp::SrtpEndpointKeys,
    srtp::{
        constants::{
            SESSION_AUTH_LEN, SESSION_KEY_LEN, SESSION_SALT_LEN, SRTP_LABEL_AUTH,
            SRTP_LABEL_ENCRYPTION, SRTP_LABEL_SALT,
        },
        session_keys::SessionKeys,
    },
};

/// Simple constant-time comparison to avoid timing attacks.
/// (Standard in crypto impls to avoid leaking where the first byte mismatch occurred)
pub(super) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

pub(super) fn derive_session_keys(master: &SrtpEndpointKeys) -> SessionKeys {
    let mut enc_key = [0u8; SESSION_KEY_LEN];
    let mut auth_key = [0u8; SESSION_AUTH_LEN];
    let mut salt = [0u8; SESSION_SALT_LEN];

    let mut salt_pad = [0u8; 16];
    if master.master_salt.len() >= 14 {
        salt_pad[..14].copy_from_slice(&master.master_salt[..14]);
    } else {
        salt_pad[..master.master_salt.len()].copy_from_slice(&master.master_salt);
    }

    aes_cm_prf(
        &master.master_key,
        &salt_pad,
        SRTP_LABEL_ENCRYPTION,
        &mut enc_key,
    );
    aes_cm_prf(
        &master.master_key,
        &salt_pad,
        SRTP_LABEL_AUTH,
        &mut auth_key,
    );
    aes_cm_prf(&master.master_key, &salt_pad, SRTP_LABEL_SALT, &mut salt);

    SessionKeys {
        enc_key,
        auth_key,
        salt,
    }
}

pub(super) fn aes_cm_prf(
    master_key: &[u8],
    master_salt_padded: &[u8; 16],
    label: u8,
    out: &mut [u8],
) {
    let mut iv = [0u8; 16];
    iv.copy_from_slice(master_salt_padded);
    iv[7] ^= label;

    let mut cipher = Aes128Ctr::new(master_key.into(), &iv.into());
    out.fill(0);
    cipher.apply_keystream(out);
}

pub(super) fn compute_iv(session_salt: &[u8; 14], ssrc: u32, index: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[..14].copy_from_slice(session_salt);

    let ssrc_bytes = ssrc.to_be_bytes();
    for i in 0..4 {
        iv[4 + i] ^= ssrc_bytes[i];
    }

    let idx_full = index.to_be_bytes();
    for i in 0..6 {
        iv[8 + i] ^= idx_full[2 + i];
    }
    iv
}

pub(super) fn get_rtp_header_len(packet: &[u8]) -> Result<usize, String> {
    if packet.len() < 12 {
        return Err("Too short".into());
    }
    let v_p_x_cc = packet[0];
    let cc = v_p_x_cc & 0x0F;
    let x = (v_p_x_cc & 0x10) != 0;

    let mut len = 12 + (cc as usize * 4);

    if x {
        if packet.len() < len + 4 {
            return Err("Too short for Ext header".into());
        }
        let ext_len = BigEndian::read_u16(&packet[len + 2..len + 4]);
        len += 4 + (ext_len as usize * 4);
    }

    if packet.len() < len {
        return Err("Packet smaller than header calc".into());
    }
    Ok(len)
}
