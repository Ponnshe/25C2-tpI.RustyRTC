use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::audio_io::{AudioIo, AudioResult};
use crate::audio::types::{AudioConfig, AudioFrame};

/// Implementación de AudioIo usando la biblioteca CPAL.
///
/// - Captura audio desde el micrófono.
/// - Reproduce audio hacia los parlantes.
/// - Convierte buffers CPAL <-> AudioFrame.
///
/// Este módulo NO realiza:
/// - Codificación Opus
/// - Jitter buffering
/// - Sincronización A/V
///
/// Eso vendrá más adelante.
pub struct CpalAudioIo {
    config: AudioConfig,
    host: cpal::Host,
    input_device: cpal::Device,
    output_device: cpal::Device,
}

impl CpalAudioIo {
    /// Crea una instancia nueva de CpalAudioIo buscando los dispositivos por defecto.
    pub fn new(config: AudioConfig) -> AudioResult<Self> {
        let host = cpal::default_host();

        let input_device = host
            .default_input_device()
            .ok_or_else(|| "No se encontró un dispositivo de entrada".to_string())?;

        let output_device = host
            .default_output_device()
            .ok_or_else(|| "No se encontró un dispositivo de salida".to_string())?;

        Ok(Self {
            config,
            host,
            input_device,
            output_device,
        })
    }

    /// Helper interno para determinar el formato de CPAL adecuado.
    fn select_format(&self, device: &cpal::Device, channels: u16) -> AudioResult<cpal::StreamConfig> {
        let supported = device
            .default_input_config()
            .map_err(|e| format!("Error consultando formato CPAL: {e}"))?;

        let sample_rate = cpal::SampleRate(self.config.sample_rate_hz);

        let cfg = cpal::StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        Ok(cfg)
    }
}

impl AudioIo for CpalAudioIo {
    fn config(&self) -> &AudioConfig {
        &self.config
    }

    fn start_capture(&mut self, tx: Sender<AudioFrame>) -> AudioResult<()> {
        let cfg = self.select_format(&self.input_device, self.config.channels)?;

        let frame_duration = self.config.frame_duration;

        let mut last_frame = Instant::now();

        let tx_clone = tx;

        let stream = self
            .input_device
            .build_input_stream(
                &cfg,
                move |data: &[f32], _| {
                    // 20 ms → frame completo
                    if last_frame.elapsed() >= frame_duration {
                        let samples: Vec<i16> = data
                            .iter()
                            .map(|f| (*f * i16::MAX as f32) as i16)
                            .collect();

                        let frame = AudioFrame {
                            timestamp: last_frame.elapsed(),
                            samples,
                            channels: cfg.channels,
                        };

                        let _ = tx_clone.send(frame);

                        last_frame = Instant::now();
                    }
                },
                move |err| {
                    eprintln!("Error en captura CPAL: {err}");
                },
                None,
            )
            .map_err(|e| format!("No se pudo crear stream de entrada: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Error iniciando stream de entrada: {e}"))?;

        // No guardamos el stream porque CPAL lo mantiene vivo internamente.
        Ok(())
    }

    fn start_playback(&mut self, rx: Receiver<AudioFrame>) -> AudioResult<()> {
        let cfg = self.select_format(&self.output_device, self.config.channels)?;

        let rx_clone = rx;

        let stream = self
            .output_device
            .build_output_stream(
                &cfg,
                move |output: &mut [f32], _| {
                    if let Ok(frame) = rx_clone.try_recv() {
                        for (i, sample) in frame.samples.iter().enumerate().take(output.len()) {
                            output[i] = *sample as f32 / i16::MAX as f32;
                        }
                    } else {
                        // Rellenamos silencio si no hay frame
                        for s in output.iter_mut() {
                            *s = 0.0;
                        }
                    }
                },
                move |err| {
                    eprintln!("Error en reproducción CPAL: {err}");
                },
                None,
            )
            .map_err(|e| format!("No se pudo crear stream de salida: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Error iniciando stream de salida: {e}"))?;

        Ok(())
    }
}
