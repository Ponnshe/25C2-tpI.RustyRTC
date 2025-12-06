use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{Receiver};
use anyhow::Result;

pub struct AudioPlayer;

impl AudioPlayer {
    pub fn start(incoming: Receiver<Vec<i16>>) -> Result<cpal::Stream> {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("No output device");

        let config = device.default_output_config()?.into();
        
        let stream = device.build_output_stream(
            &config,
            move |output: &mut [i16], _| {
                if let Ok(mut packet) = incoming.try_recv() {
                    let len = packet.len().min(output.len());
                    output[..len].copy_from_slice(&packet[..len]);
                } else {
                    for sample in output.iter_mut() { *sample = 0; }
                }
            },
            move |err| {
                eprintln!("Audio output error: {:?}", err);
            },
        )?;

        stream.play()?;
        Ok(stream)
    }
}
