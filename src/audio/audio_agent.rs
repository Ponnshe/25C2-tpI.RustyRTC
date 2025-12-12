use std::sync::mpsc::{Sender, Receiver, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use crate::{sink_info, sink_warn};
use crate::log::log_sink::LogSink;

use crate::audio::audio_io::{AudioIo, AudioResult};
use crate::audio::types::{AudioConfig, AudioFrame};

/// Estado del agente de audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioAgentState {
    Stopped,
    Running,
}

/// Represents whether the audio is currently enabled or muted.
#[derive(Debug, PartialEq, Eq)]
pub enum AudioState {
    Enabled,
    Muted,
}

pub struct AudioAgent {
    config: AudioConfig,
    io: Box<dyn AudioIo>,
    state: AudioAgentState,
    start_instant: Option<Instant>,

    /// Audio local capturado hacia RTP.
    pub tx_to_rtp: Sender<AudioFrame>,

    /// RTP → agente (sender para que RTP pueda enviar audio).
    pub tx_from_rtp: Sender<AudioFrame>,

    /// RTP → agente (receiver interno para playback).
    rx_from_rtp: Receiver<AudioFrame>,

    rx_to_rtp: Receiver<AudioFrame>,

    worker_handle: Option<JoinHandle<()>>,

    logger: Option<Arc<dyn LogSink>>,

    /// Handles the audio flow: mute/unmute state, audio pipeline control, etc.
    mute_flag: Arc<AtomicBool>,
}

impl AudioAgent {
    pub fn new(
        config: AudioConfig,
        io: Box<dyn AudioIo>,
        logger: Option<Arc<dyn LogSink>>,
    ) -> Self {
        let (tx_to_rtp, rx_to_rtp) = channel::<AudioFrame>();

        // Canal bidireccional para RTP → agente
        let (tx_from_rtp, rx_from_rtp) = channel::<AudioFrame>();

        Self {
            config,
            io,
            state: AudioAgentState::Stopped,
            start_instant: None,
            tx_to_rtp,
            tx_from_rtp,
            rx_from_rtp,
            worker_handle: None,
            logger,
            mute_flag: Arc::new(AtomicBool::new(false)),
            rx_to_rtp
        }
    }

    /// Devuelve el Receiver que entrega los frames de audio
    /// capturados y listos para enviar por RTP.
    ///
    /// Se "consume" el receiver interno, por lo que sólo debe llamarse una vez.
    pub fn take_uplink_receiver(&mut self) -> Receiver<AudioFrame> {
        // Creamos un canal dummy para dejar algo en `self.rx_to_rtp`
        let (_dummy_tx, dummy_rx) = std::sync::mpsc::channel();
        std::mem::replace(&mut self.rx_to_rtp, dummy_rx)
    }

    /// Devuelve un handle compartido al flag de mute.
    ///
    /// Permite que otros hilos consulten el estado de mute sin
    /// tomar un lock sobre `AudioAgent`.
    pub fn mute_handle(&self) -> Arc<AtomicBool> {
        self.mute_flag.clone()
    }

    pub fn config(&self) -> &AudioConfig {
        &self.config
    }

    pub fn state(&self) -> AudioAgentState {
        self.state
    }

    pub fn start(&mut self) -> AudioResult<()> {
        if self.state == AudioAgentState::Running {
            return Err("AudioAgent ya está en estado Running".into());
        }

        self.start_instant = Some(Instant::now());

        // Canales internos para IO
        let (tx_capture_to_agent, rx_capture_to_agent) = channel::<AudioFrame>();
        let (tx_agent_to_playback, rx_agent_to_playback) = channel::<AudioFrame>();

        // Iniciar captura y playback
        self.io.start_capture(tx_capture_to_agent)?;
        self.io.start_playback(rx_agent_to_playback)?;

        let tx_to_rtp = self.tx_to_rtp.clone();

        let rx_from_rtp = std::mem::replace(&mut self.rx_from_rtp,
                                            channel::<AudioFrame>().1);

        let logger = self.logger.clone();

        let handle = std::thread::spawn(move || {
            loop {
                match rx_capture_to_agent.recv_timeout(Duration::from_millis(10)) {
                    Ok(frame) => {
                        if tx_to_rtp.send(frame).is_err() {
                            if let Some(l) = &logger {
                                sink_warn!(l, "[AudioAgent] tx_to_rtp desconectado");
                            }
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        if let Some(l) = &logger {
                            sink_warn!(l, "[AudioAgent] rx_capture_to_agent desconectado");
                        }
                        break;
                    }
                }

                match rx_from_rtp.recv_timeout(Duration::from_millis(1)) {
                    Ok(frame) => {
                        if tx_agent_to_playback.send(frame).is_err() {
                            if let Some(l) = &logger {
                                sink_warn!(l, "[AudioAgent] tx_agent_to_playback desconectado");
                            }
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
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

    pub fn stop(&mut self) -> AudioResult<()> {
        if self.state == AudioAgentState::Stopped {
            return Ok(());
        }

        self.state = AudioAgentState::Stopped;

        if let Some(handle) = self.worker_handle.take() {
            if handle.join().is_err() {
                return Err("No se pudo unir el hilo interno de AudioAgent".into());
            }
        }

        if let Some(l) = &self.logger {
            sink_info!(l, "[AudioAgent] detenido");
        }

        Ok(())
    }

    /// Mutes the audio stream.
    pub fn mute(&self) {
        self.mute_flag.store(true, Ordering::Relaxed);
    }

    /// Unmutes the audio stream.
    pub fn unmute(&self) {
        self.mute_flag.store(false, Ordering::Relaxed);
    }

    /// Returns the current audio state.
    pub fn audio_state(&self) -> AudioState {
        if self.mute_flag.load(Ordering::Relaxed) {
            AudioState::Muted
        } else {
            AudioState::Enabled
        }
    }

    /// Returns true if audio is muted.
    pub fn is_muted(&self) -> bool {
        self.mute_flag.load(Ordering::Relaxed)
    }

    /// Called by the audio pipeline before sending RTP.
    ///
    /// If muted, the audio is **not** sent.
    pub fn should_send_audio(&self) -> bool {
        !self.mute_flag.load(Ordering::Relaxed)
    }

    pub fn downlink_sender(&self) -> Option<Sender<AudioFrame>> {
        Some(self.tx_from_rtp.clone())
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
