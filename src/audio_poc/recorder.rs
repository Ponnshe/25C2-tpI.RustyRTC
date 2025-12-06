use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use anyhow::Result;

//Este código captura audio en PCM y 
//llama a un callback que te permite enviarlo por UDP:
pub struct AudioRecorder;

impl AudioRecorder {
    pub fn start<F>(mut callback: F) -> Result<cpal::Stream>
    where
        F: FnMut(&[i16]) + Send + 'static,
    {
        let host = cpal::default_host();
        let device = host.default_input_device().expect("No input device");
        
        let config = device.default_input_config()?.into();
        let stream = device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                callback(data);
            },
            move |err| {
                eprintln!("Audio input error: {:?}", err);
            },
        )?;

        stream.play()?;
        Ok(stream)
    }
}
