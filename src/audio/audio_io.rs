use crate::audio::types::{AudioConfig, AudioFrame};
use std::sync::mpsc::{Receiver, Sender};

/// Resultado genérico para las operaciones de audio.
pub type AudioResult<T> = Result<T, String>;

/// Abstracción de entrada/salida de audio.
pub trait AudioIo: Send {
    /// Configuración efectiva de audio que esta implementación usa.
    fn config(&self) -> &AudioConfig;

    fn start_capture(&mut self, tx: Sender<AudioFrame>) -> AudioResult<()>;
    fn start_playback(&mut self, rx: Receiver<AudioFrame>) -> AudioResult<()>;
}

/// Implementación “vacía” de AudioIo, usada cuando no se puede inicializar CPAL.
///
/// No captura ni reproduce audio, pero deja que el sistema siga funcionando.
pub struct NoopAudioIo {
    config: AudioConfig,
}

impl NoopAudioIo {
    /// Crea un NoopAudioIo con una config dada
    #[must_use]
    pub fn new(config: AudioConfig) -> Self {
        Self { config }
    }

    /// Crea un NoopAudioIo con la config de voz por defecto
    #[must_use]
    pub fn with_default_voice() -> Self {
        Self {
            config: AudioConfig::default_voice(),
        }
    }
}

impl AudioIo for NoopAudioIo {
    fn config(&self) -> &AudioConfig {
        &self.config
    }

    fn start_capture(&mut self, _tx: Sender<AudioFrame>) -> AudioResult<()> {
        Ok(())
    }

    fn start_playback(&mut self, _rx: Receiver<AudioFrame>) -> AudioResult<()> {
        Ok(())
    }
}
