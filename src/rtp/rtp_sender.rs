use super::{rtp_header::RtpHeader, rtp_packet::RtpPacket};

/// Describe la configuración RTP para un flujo específico (audio o video).
///
/// Incluye:
/// - `clock_rate_hz`: frecuencia de reloj RTP (e.g. 90k para video, 48k para audio).
/// - `payload_type`: tipo de payload RTP (según negociación SDP).
/// - `ssrc`: identificador del stream (único por flujo).
#[derive(Debug, Clone, Copy)]
pub struct RtpProfile {
    pub clock_rate_hz: u32,
    pub payload_type: u8,
    pub ssrc: u32,
}

impl RtpProfile {
    /// Perfil típico para video H264 en WebRTC.
    /// - Clock: 90 kHz (RFC 6184)
    /// - PT: 96 (dinámico, valor usual)
    #[must_use]
    pub fn video_h264(ssrc: u32) -> Self {
        Self {
            clock_rate_hz: 90_000,
            payload_type: 96,
            ssrc,
        }
    }

    /// Perfil típico para audio OPUS:
    /// - Clock: 48 kHz (RFC 7587)
    /// - PT: 111 (dinámico, valor usual en WebRTC)
    #[must_use]
    pub fn audio_opus(ssrc: u32) -> Self {
        Self {
            clock_rate_hz: 48_000,
            payload_type: 111,
            ssrc,
        }
    }
}

/// Mantiene el estado de envío RTP para un flujo (audio o video).
///
/// - Reutilizable para audio y video.
/// - Encapsula el manejo de `sequence_number` y `timestamp`.
pub struct RtpSender {
    profile: RtpProfile,
    sequence_number: u16,
    timestamp: u32,
}

impl RtpSender {
    /// Crea un nuevo `RtpSender` con un perfil ya definido.
    ///
    /// `initial_timestamp` permite arrancar en un valor aleatorio (buena práctica RTP),
    /// pero podés inyectar 0 para simplificar las pruebas.
    #[must_use]
    pub fn new(profile: RtpProfile, initial_sequence: u16, initial_timestamp: u32) -> Self {
        Self {
            profile,
            sequence_number: initial_sequence,
            timestamp: initial_timestamp,
        }
    }

    /// Atajo para construir un emisor de video H264 con parámetros por defecto.
    #[must_use]
    pub fn video_h264(ssrc: u32) -> Self {
        Self::new(RtpProfile::video_h264(ssrc), 0, 0)
    }

    /// Atajo para construir un emisor de audio OPUS con parámetros por defecto.
    #[must_use]
    pub fn audio_opus(ssrc: u32) -> Self {
        Self::new(RtpProfile::audio_opus(ssrc), 0, 0)
    }

    /// Devuelve el perfil asociado (por si lo necesitás para logs o SDP).
    #[must_use]
    pub fn profile(&self) -> RtpProfile {
        self.profile
    }

    /// Construye un `RtpPacket` para el payload dado.
    ///
    /// # Parámetros
    /// * `payload`             - Datos ya codificados (video H264 o audio Opus).
    /// * `marker`              - Bit M (último fragmento de frame, etc.).
    /// * `samples_per_frame`   - Cuántas "unidades de reloj" avanza el timestamp:
    ///     - Video: típicamente `clock_rate / fps` → ej. 90000 / 30 = 3000.
    ///     - Audio: típicamente `samples_por_frame` → ej. 960 para 20 ms a 48 kHz.
    ///
    /// # Retorno
    /// `RtpPacket` listo para ser enviado por la red.
    pub fn build_packet(&mut self, payload: Vec<u8>, marker: bool, samples_per_frame: u32) -> RtpPacket {
        // Actualizamos estado interno para el siguiente paquete.
        self.sequence_number = self.sequence_number.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(samples_per_frame);
        
        let header = RtpHeader {
            version: 2,
            padding: false,
            extension: false,
            csrcs: Vec::new(),
            marker,
            payload_type: self.profile.payload_type,
            sequence_number: self.sequence_number,
            timestamp: self.timestamp,
            ssrc: self.profile.ssrc,
            header_extension: None
        };

        RtpPacket { header, payload, padding_bytes: 0 }
    }
}
