use crate::app::log_sink::LogSink;
use crate::dtls_srtp::SrtpEndpointKeys;
use crate::{sink_debug, sink_error, sink_trace, sink_warn};
use aes::Aes128;
use aes::cipher::{KeyIvInit, StreamCipher};
use byteorder::{BigEndian, ByteOrder};
use ctr::Ctr128BE;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::HashMap;
use std::sync::Arc;

const SRTP_LABEL_ENCRYPTION: u8 = 0x00;
const SRTP_LABEL_AUTH: u8 = 0x01;
const SRTP_LABEL_SALT: u8 = 0x02;

// SRTP_AES128_CM_SHA1_80 constants
const SESSION_KEY_LEN: usize = 16; // 128 bits
const SESSION_AUTH_LEN: usize = 20; // 160 bits (SHA1)
const SESSION_SALT_LEN: usize = 14; // 112 bits
const AUTH_TAG_LEN: usize = 10; // 80 bits truncated

type HmacSha1 = Hmac<Sha1>;
type Aes128Ctr = Ctr128BE<Aes128>;

// Replay protection window size (64 packets)
const REPLAY_WINDOW_SIZE: u64 = 64;

struct SessionKeys {
    enc_key: [u8; SESSION_KEY_LEN],
    auth_key: [u8; SESSION_AUTH_LEN],
    salt: [u8; SESSION_SALT_LEN],
}

struct ReplayWindow {
    max_index: u64,
    window: u64,
}

impl ReplayWindow {
    fn new() -> Self {
        Self {
            max_index: 0,
            window: 0,
        }
    }

    fn is_replay(&self, index: u64) -> bool {
        if index > self.max_index {
            return false;
        }
        let diff = self.max_index.saturating_sub(index);
        if diff >= REPLAY_WINDOW_SIZE {
            return true;
        }
        (self.window & (1u64 << diff)) != 0
    }

    fn record(&mut self, index: u64) {
        if index > self.max_index {
            let diff = index.saturating_sub(self.max_index);
            if diff < REPLAY_WINDOW_SIZE {
                self.window <<= diff as u32;
            } else {
                self.window = 0;
            }
            self.window |= 1;
            self.max_index = index;
        } else {
            let diff = self.max_index.saturating_sub(index);
            if diff < REPLAY_WINDOW_SIZE {
                self.window |= 1u64 << diff;
            }
        }
    }
}

pub struct SrtpContext {
    logger: Arc<dyn LogSink>,
    session_keys: SessionKeys,
    rocs: HashMap<u32, u32>,
    last_seqs: HashMap<u32, u16>,
    replay_windows: HashMap<u32, ReplayWindow>,
}

impl SrtpContext {
    pub fn new(logger: Arc<dyn LogSink>, master_keys: SrtpEndpointKeys) -> Self {
        let session_keys = derive_session_keys(&master_keys);

        // --- DEBUG LOGGING: KEYS ---
        sink_debug!(
            logger,
            "[SRTP Context] Keys derived. \n\tEnc: {:02X?}\n\tAuth: {:02X?}\n\tSalt: {:02X?}",
            &session_keys.enc_key,
            &session_keys.auth_key,
            &session_keys.salt
        );

        Self {
            logger,
            session_keys,
            rocs: HashMap::new(),
            last_seqs: HashMap::new(),
            replay_windows: HashMap::new(),
        }
    }

    pub fn protect(&mut self, ssrc: u32, packet: &mut Vec<u8>) -> Result<(), String> {
        if packet.len() < 12 {
            return Err("Packet too short for RTP header".into());
        }

        let seq = BigEndian::read_u16(&packet[2..4]);
        let roc = self.get_or_create_roc(ssrc, seq);
        let index = ((roc as u64) << 16) | (seq as u64);

        let header_len = get_rtp_header_len(packet)?;

        // --- ENCRYPTION ---
        let iv = compute_iv(&self.session_keys.salt, ssrc, index);
        let mut cipher = Aes128Ctr::new(&self.session_keys.enc_key.into(), &iv.into());
        cipher.apply_keystream(&mut packet[header_len..]);

        // --- AUTHENTICATION ---
        let mut mac = HmacSha1::new_from_slice(&self.session_keys.auth_key)
            .map_err(|_| "Invalid auth key length")?;

        mac.update(packet);
        let mut roc_bytes = [0u8; 4];
        BigEndian::write_u32(&mut roc_bytes, roc);
        mac.update(&roc_bytes);

        // Finalize gives 20 bytes (SHA1)
        let result = mac.finalize().into_bytes();
        // Truncate to 10 bytes (SRTP 80-bit tag)
        let tag = &result[..AUTH_TAG_LEN];

        packet.extend_from_slice(tag);

        sink_trace!(
            self.logger,
            "[SRTP] Protected Packet: SSRC={:#x} Seq={} ROC={} Len={} Tag={:02X?}",
            ssrc,
            seq,
            roc,
            packet.len(),
            tag
        );

        Ok(())
    }

    pub fn unprotect(&mut self, packet: &mut Vec<u8>) -> Result<(), String> {
        if packet.len() < 12 + AUTH_TAG_LEN {
            return Err("Packet too short for SRTP".into());
        }

        // 1. Separate Tag
        let tag_start = packet.len() - AUTH_TAG_LEN;
        let (content, received_tag) = packet.split_at(tag_start);

        // 2. Parse info
        if content.len() < 12 {
            return Err("Packet content too short".into());
        }
        let seq = BigEndian::read_u16(&content[2..4]);
        let ssrc = BigEndian::read_u32(&content[8..12]);

        let roc = self.estimate_roc(ssrc, seq);
        let index = ((roc as u64) << 16) | (seq as u64);

        // 3. Replay Check
        let window = self
            .replay_windows
            .entry(ssrc)
            .or_insert_with(ReplayWindow::new);

        if window.is_replay(index) {
            sink_warn!(
                self.logger,
                "[SRTP] Replay detected: SSRC={:#x} Seq={} Index={}",
                ssrc,
                seq,
                index
            );
            return Err(format!("Replay detected: ssrc={:#x} seq={}", ssrc, seq));
        }

        // 4. Verify HMAC
        let mut mac = HmacSha1::new_from_slice(&self.session_keys.auth_key)
            .map_err(|_| "Invalid auth key length")?;

        mac.update(content);
        let mut roc_bytes = [0u8; 4];
        BigEndian::write_u32(&mut roc_bytes, roc);
        mac.update(&roc_bytes);

        // --- FIX: Manual Truncation & Comparison ---
        // verify_slice fails if input len (10) != output len (20).
        let full_hash = mac.finalize().into_bytes();
        let computed_tag = &full_hash[..AUTH_TAG_LEN];

        if !constant_time_eq(computed_tag, received_tag) {
            sink_error!(
                self.logger,
                "[SRTP] Auth Fail details:\n\tSSRC: {:#x}\n\tSeq: {}\n\tROC: {}\n\tExpected Tag: {:02X?}\n\tReceived Tag: {:02X?}",
                ssrc,
                seq,
                roc,
                computed_tag,
                received_tag
            );
            return Err("SRTP Auth Tag Mismatch".into());
        }

        // 5. Decrypt
        packet.truncate(tag_start); // Remove tag

        let header_len = get_rtp_header_len(packet)?;
        let iv = compute_iv(&self.session_keys.salt, ssrc, index);

        let mut cipher = Aes128Ctr::new(&self.session_keys.enc_key.into(), &iv.into());
        cipher.apply_keystream(&mut packet[header_len..]);

        // 6. Update State
        self.rocs.insert(ssrc, roc);
        self.last_seqs.insert(ssrc, seq);
        window.record(index);

        sink_trace!(
            self.logger,
            "[SRTP] Unprotect Success: SSRC={:#x} Seq={}",
            ssrc,
            seq
        );

        Ok(())
    }

    fn get_or_create_roc(&mut self, ssrc: u32, seq: u16) -> u32 {
        if !self.last_seqs.contains_key(&ssrc) {
            self.last_seqs.insert(ssrc, seq);
            self.rocs.insert(ssrc, 0);
            return 0;
        }

        let last_seq = self.last_seqs[&ssrc];
        let mut roc = *self.rocs.get(&ssrc).unwrap_or(&0);

        if seq < last_seq {
            let diff = (last_seq as u32).wrapping_sub(seq as u32);
            if diff > 1000 {
                roc = roc.wrapping_add(1);
            }
        }

        self.last_seqs.insert(ssrc, seq);
        self.rocs.insert(ssrc, roc);
        roc
    }

    fn estimate_roc(&self, ssrc: u32, seq: u16) -> u32 {
        let last_seq = match self.last_seqs.get(&ssrc) {
            Some(&s) => s,
            None => return 0,
        };
        let last_roc = *self.rocs.get(&ssrc).unwrap_or(&0);

        let delta = (seq as i32) - (last_seq as i32);

        if delta <= -32768 {
            return last_roc.wrapping_add(1);
        }
        if delta >= 32768 {
            return last_roc.wrapping_sub(1);
        }
        last_roc
    }
}

// -----------------------------------------------------------------------------
// HELPER FUNCTIONS
// -----------------------------------------------------------------------------

/// Simple constant-time comparison to avoid timing attacks.
/// (Standard in crypto impls to avoid leaking where the first byte mismatch occurred)
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

fn derive_session_keys(master: &SrtpEndpointKeys) -> SessionKeys {
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

fn aes_cm_prf(master_key: &[u8], master_salt_padded: &[u8; 16], label: u8, out: &mut [u8]) {
    let mut iv = [0u8; 16];
    iv.copy_from_slice(master_salt_padded);
    iv[7] ^= label;

    let mut cipher = Aes128Ctr::new(master_key.into(), &iv.into());
    out.fill(0);
    cipher.apply_keystream(out);
}

fn compute_iv(session_salt: &[u8; 14], ssrc: u32, index: u64) -> [u8; 16] {
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

fn get_rtp_header_len(packet: &[u8]) -> Result<usize, String> {
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
