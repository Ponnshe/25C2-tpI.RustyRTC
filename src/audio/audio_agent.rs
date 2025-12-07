use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use crate::{sink_debug, sink_error, sink_info, sink_warn};
use crate::log::log_sink::LogSink;

use crate::audio::audio_io::{AudioIo, AudioResult};
use crate::audio::types::{AudioConfig, AudioFrame};

/// Representa el estado de ejecución del AudioAgent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioAgentState {
    /// El agente aún no fue iniciado.
    Stopped,
    /// El agente está corriendo (captura y/o reproducción activos).
    Running,
}

/// Orquestador de la lógica de audio de un peer.
///
/// - Arranca captura de audio desde `AudioIo`.
/// - Arranca reproducción.
/// - Expone canales para que RTP / SRTP envíen y reciban `AudioFrame`.
pub struct AudioAgent {
    /// Configuración de audio usada por el agente.
    config: AudioConfig,
    /// Implementación concreta de IO (CPAL, fake, etc.).
    io: Box<dyn AudioIo>,
    /// Estado actual del agente.
    state: AudioAgentState,
    /// Momento de inicio, para generar timestamps relativos.
    start_instant: Option<Instant>,

    /// Canal hacia RTP: frames capturados locales → stack de red.
    pub tx_to_rtp: Sender<AudioFrame>,
    /// Canal desde RTP: frames recibidos desde la red → playback local.
    pub rx_from_rtp: Receiver<AudioFrame>,

    /// Handle de hilo opcional para tareas internas futuras (ej. sync).
    worker_handle: Option<JoinHandle<()>>,

    /// Logger opcional. Podés reemplazarlo con tu `Arc<dyn LogSink>`.
    logger: Option<Arc<dyn LogSink>>,
}

impl AudioAgent {
    /// Crea un nuevo `AudioAgent` con la implementación de IO y logger dados.
    ///
    /// No inicia aún la captura ni reproducción; eso se hace con `start`.
    #[must_use]
    pub fn new(
        config: AudioConfig,
        io: Box<dyn AudioIo>,
        logger: Option<Arc<dyn LogSink>>,
    ) -> Self {
        let (tx_to_rtp, rx_to_rtp) = channel::<AudioFrame>();
        let (tx_from_rtp, rx_from_rtp) = channel::<AudioFrame>();

        // Por ahora rx_to_rtp y tx_from_rtp no se usan internamente;
        // pero nos dejan el “gancho” para conectar con RTP.
        let _ = rx_to_rtp;
        let _ = tx_from_rtp;

        Self {
            config,
            io,
            state: AudioAgentState::Stopped,
            start_instant: None,
            tx_to_rtp,
            rx_from_rtp,
            worker_handle: None,
            logger,
        }
    }

    /// Devuelve la configuración de audio actual.
    #[must_use]
    pub fn config(&self) -> &AudioConfig {
        &self.config
    }

    /// Indica si el agente está en ejecución.
    #[must_use]
    pub fn state(&self) -> AudioAgentState {
        self.state
    }

    /// Inicia captura y reproducción de audio.
    ///
    /// - Inicializa `start_instant`.
    /// - Pide a `AudioIo` que arranque captura y playback.
    ///
    /// # Errores
    /// - Si el agente ya estaba corriendo.
    /// - Si la implementación `AudioIo` falla al inicializar los dispositivos.
    pub fn start(&mut self) -> AudioResult<()> {
        if self.state == AudioAgentState::Running {
            return Err(String::from("AudioAgent ya se encuentra en estado Running"));
        }

        let now = Instant::now();
        self.start_instant = Some(now);

        // Canales internos para IO
        let (tx_capture_to_agent, rx_capture_to_agent) = channel::<AudioFrame>();
        let (tx_agent_to_playback, rx_agent_to_playback) = channel::<AudioFrame>();

        // 1) Arrancamos captura en IO (micrófono → tx_capture_to_agent)
        self.io.start_capture(tx_capture_to_agent)?;

        // 2) Arrancamos playback en IO (rx_agent_to_playback → parlantes)
        self.io.start_playback(rx_agent_to_playback)?;

        // 3) Hilo de “router” interno:
        //    - de captura → tx_to_rtp (para red)
        //    - de rx_from_rtp → tx_agent_to_playback (para parlantes)
        let tx_to_rtp = self.tx_to_rtp.clone();
        let rx_from_rtp = self.rx_from_rtp.clone();
        let logger = self.logger.clone();

        let handle = std::thread::spawn(move || {
            loop {
                // 3.a) Ruteo de audio local capturado hacia RTP
                match rx_capture_to_agent.recv_timeout(Duration::from_millis(10)) {
                    Ok(mut frame) => {
                        // (en el futuro podríamos ajustar timestamp / sync acá)
                        if tx_to_rtp.send(frame).is_err() {
                            if let Some(l) = &logger {
                                sink_warn!(l, "[AudioAgent] tx_to_rtp desconectado");
                            }
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        // nada que hacer, seguimos
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        if let Some(l) = &logger {
                            sink_warn!(l, "[AudioAgent] rx_capture_to_agent desconectado");
                        }
                        break;
                    }
                }

                // 3.b) Ruteo de audio recibido por red hacia playback
                match rx_from_rtp.recv_timeout(Duration::from_millis(1)) {
                    Ok(frame) => {
                        if tx_agent_to_playback.send(frame).is_err() {
                            if let Some(l) = &logger {
                                sink_warn!(l, "[AudioAgent] tx_agent_to_playback desconectado");
                            }
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        // nada por ahora
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        if let Some(l) = &logger {
                            sink_warn!(l, "[AudioAgent] rx_from_rtp desconectado");
                        }
                        break;
                    }
                }
            }
        });

        self.worker_handle = Some(handle);
        self.state = AudioAgentState::Running;

        if let Some(l) = &self.logger {
            sink_info!(l, "[AudioAgent] iniciado");
        }

        Ok(())
    }

    /// Intenta detener el agente de audio.
    ///
    /// - Marca el estado como `Stopped`.
    /// - Intenta hacer `join` al hilo interno, si existe.
    pub fn stop(&mut self) -> AudioResult<()> {
        if self.state == AudioAgentState::Stopped {
            return Ok(());
        }

        self.state = AudioAgentState::Stopped;

        if let Some(handle) = self.worker_handle.take() {
            // No tenemos una señal explícita de cancelación todavía,
            // por lo que confiamos en que la desconexión de canales
            // haga que el hilo termine. En una versión más avanzada
            // podés introducir un flag de `running: AtomicBool`.
            if handle.join().is_err() {
                return Err(String::from(
                    "No se pudo unir el hilo interno de AudioAgent",
                ));
            }
        }

        if let Some(l) = &self.logger {
            sink_info!(l, "[AudioAgent] detenido");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::types::AudioConfig;
    use crate::audio::audio_io::{AudioIo, AudioResult};
    use std::sync::mpsc::{Sender, Receiver, channel};

    /// Implementación fake de AudioIo para tests.
    struct FakeAudioIo {
        config: AudioConfig,
        started_capture: bool,
        started_playback: bool,
    }

    impl FakeAudioIo {
        fn new(config: AudioConfig) -> Self {
            Self {
                config,
                started_capture: false,
                started_playback: false,
            }
        }
    }

    impl AudioIo for FakeAudioIo {
        fn config(&self) -> &AudioConfig {
            &self.config
        }

        fn start_capture(&mut self, _tx: Sender<AudioFrame>) -> AudioResult<()> {
            self.started_capture = true;
            Ok(())
        }

        fn start_playback(&mut self, _rx: Receiver<AudioFrame>) -> AudioResult<()> {
            self.started_playback = true;
            Ok(())
        }
    }

    fn make_agent() -> AudioAgent {
        let cfg = AudioConfig::default_voice();
        let io: Box<dyn AudioIo> = Box::new(FakeAudioIo::new(cfg.clone()));
        AudioAgent::new(cfg, io, None)
    }

    #[test]
    fn agent_starts_and_stops() {
        let mut agent = make_agent();

        assert_eq!(agent.state(), AudioAgentState::Stopped);

        let start_res = agent.start();
        assert!(start_res.is_ok());
        assert_eq!(agent.state(), AudioAgentState::Running);

        let stop_res = agent.stop();
        assert!(stop_res.is_ok());
        assert_eq!(agent.state(), AudioAgentState::Stopped);
    }

    #[test]
    fn start_twice_fails() {
        let mut agent = make_agent();
        assert!(agent.start().is_ok());
        let second = agent.start();
        assert!(second.is_err());
    }
}
