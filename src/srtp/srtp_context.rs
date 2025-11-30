use crate::log::log_sink::LogSink;
use crate::srtp::SrtpEndpointKeys;
use crate::srtp::constants::AUTH_TAG_LEN;
use crate::srtp::replay_window::ReplayWindow;
use crate::srtp::session_keys::SessionKeys;
use crate::srtp::utils::{
    Aes128Ctr, HmacSha1, compute_iv, constant_time_eq, derive_session_keys, get_rtp_header_len,
};
use crate::{sink_debug, sink_error, sink_trace, sink_warn};
use aes::cipher::{KeyIvInit, StreamCipher};
use byteorder::{BigEndian, ByteOrder};
use hmac::Mac;
use std::collections::HashMap;
use std::sync::Arc;

pub struct SrtpContext {
    pub logger: Arc<dyn LogSink>,
    pub session_keys: SessionKeys,
    pub rocs: HashMap<u32, u32>,
    pub last_seqs: HashMap<u32, u16>,
    pub replay_windows: HashMap<u32, ReplayWindow>,
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
