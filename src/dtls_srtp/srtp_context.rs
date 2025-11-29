use crate::dtls_srtp::SrtpEndpointKeys;
use std::collections::HashMap;

/// Mantiene el estado criptográfico de envío o recepción.
pub struct SrtpContext {
    keys: SrtpEndpointKeys,
    /// Rollover Counter (ROC) por SSRC.
    /// Necesario porque el Sequence Number (16 bits) da la vuelta frecuentemente.
    rocs: HashMap<u32, u32>,
    /// Último Sequence Number visto por SSRC (para detectar vueltas)
    last_seqs: HashMap<u32, u16>,
}

impl SrtpContext {
    pub fn new(keys: SrtpEndpointKeys) -> Self {
        Self {
            keys,
            rocs: HashMap::new(),
            last_seqs: HashMap::new(),
        }
    }

    /// Transforma un paquete RTP crudo (header + payload) en un paquete SRTP (encriptado + auth tag).
    /// Se llama justo antes de `socket.send()`.
    pub fn protect(&mut self, ssrc: u32, packet: &mut Vec<u8>) -> Result<(), String> {
        // 1. Parsear seq number del paquete crudo (bytes 2 y 3)
        if packet.len() < 12 {
            return Err("Packet too short".into());
        }
        let seq = u16::from_be_bytes([packet[2], packet[3]]);

        // 2. Actualizar ROC (lógica simplificada de sender)
        let roc = self.get_or_create_roc(ssrc, seq);

        // TODO: Implementar AES-CM-128 Encryption aquí usando self.keys + seq + roc
        // TODO: Calcular HMAC-SHA1 y hacer packet.extend_from_slice(&tag);

        // Por ahora: Passthrough (no hace nada)
        Ok(())
    }

    /// Transforma un paquete SRTP (encriptado + tag) en RTP crudo.
    /// Se llama justo después de `socket.recv()`.
    pub fn unprotect(&mut self, packet: &mut Vec<u8>) -> Result<(), String> {
        if packet.len() < 12 {
            return Err("Packet too short".into());
        }

        // 1. Extraer SSRC y Seq para buscar el ROC correcto
        let seq = u16::from_be_bytes([packet[2], packet[3]]);
        let ssrc = u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]);

        // 2. Estimar ROC del receptor (RFC 3711)
        let _roc = self.estimate_roc(ssrc, seq);

        // TODO: Verificar HMAC-SHA1. Si falla, return Err.
        // TODO: Decriptar AES-CM-128.
        // TODO: Recortar el tag: packet.truncate(len - tag_len);

        Ok(())
    }

    fn get_or_create_roc(&mut self, ssrc: u32, seq: u16) -> u32 {
        let last = *self.last_seqs.get(&ssrc).unwrap_or(&0);
        let roc = *self.rocs.get(&ssrc).unwrap_or(&0);

        // Lógica muy básica de vuelta de contador (rollover)
        // Si pasamos de un valor alto (ej 65530) a uno bajo (ej 10), incrementamos ROC.
        if last > 0xFF00 && seq < 0x00FF {
            let new_roc = roc + 1;
            self.rocs.insert(ssrc, new_roc);
            self.last_seqs.insert(ssrc, seq);
            return new_roc;
        }

        self.last_seqs.insert(ssrc, seq);
        self.rocs.insert(ssrc, roc);
        roc
    }

    fn estimate_roc(&mut self, _ssrc: u32, _seq: u16) -> u32 {
        // TODO: Implementar lógica completa de estimación ROC receiver
        0
    }
}
