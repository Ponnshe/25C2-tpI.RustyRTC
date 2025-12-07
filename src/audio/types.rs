use std::time::Duration;

/// Configuración básica de audio.
///
/// Por ahora la mantenemos simple.
/// Más adelante podemos alinearla con Opus/RTP
/// (sample rate fijo, tamaño de frame, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioConfig {
    /// Frecuencia de muestreo (ej. 48000 Hz).
    pub sample_rate_hz: u32,
    /// Cantidad de canales (1 = mono, 2 = estéreo).
    pub channels: u16,
    /// Tamaño de frame en milisegundos (para pacing).
    pub frame_duration: Duration,
}

/// Frame de audio PCM en memoria.
///
/// Esta es la unidad lógica que el AudioAgent intercambia
/// con el resto del sistema (RTP, jitter buffer, etc.).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    /// Timestamp relativo al inicio de la llamada.
    ///
    /// Más adelante lo usaremos para sincronizar con video.
    pub timestamp: Duration,
    /// Muestras PCM intercaladas por canal (i16).
    pub samples: Vec<i16>,
    /// Cantidad de canales (copiado de la config en el momento de captura).
    pub channels: u16,
}

impl AudioConfig {
    /// Crea una configuración de audio segura por defecto.
    ///
    /// - 48 kHz
    /// - Mono
    /// - Frames de 20 ms (típico para VoIP/Opus)
    #[must_use]
    pub fn default_voice() -> Self {
        Self {
            sample_rate_hz: 48_000,
            channels: 1,
            frame_duration: Duration::from_millis(20),
        }
    }
}
