use crate::audio::types::{AudioConfig, AudioFrame};
use std::sync::mpsc::{Receiver, Sender};

/// Resultado genérico para las operaciones de audio.
pub type AudioResult<T> = Result<T, String>;

/// Abstracción de entrada/salida de audio.
///
/// Implementaciones típicas:
/// - `CpalAudioIo`: usa la placa de sonido real.
/// - `FakeAudioIo`: para tests (no toca hardware).
pub trait AudioIo: Send {
    /// Configuración efectiva de audio que esta implementación usa.
    fn config(&self) -> &AudioConfig;

    /// Inicia la captura desde micrófono o fuente de entrada.
    ///
    /// Los frames capturados se envían por `tx`.
    ///
    /// # Errores
    /// Retorna `Err(String)` si la configuración no es soportada
    /// o si no se puede acceder al dispositivo.
    fn start_capture(&mut self, tx: Sender<AudioFrame>) -> AudioResult<()>;

    /// Inicia la reproducción en el dispositivo de salida.
    ///
    /// Consume frames de `rx`. Si no hay frames, la implementación
    /// puede reproducir silencio.
    ///
    /// # Errores
    /// Retorna `Err(String)` si el dispositivo de salida falla.
    fn start_playback(&mut self, rx: Receiver<AudioFrame>) -> AudioResult<()>;
}
