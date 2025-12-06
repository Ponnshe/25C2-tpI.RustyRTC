use std::sync::mpsc::channel;
use crate::audio_poc::{recorder::AudioRecorder, player::AudioPlayer};
use crate::audio_poc::{udp_sender::UdpSender, udp_receiver::start_udp_receiver};
use anyhow::Result;

pub fn start_audio_poc() -> Result<()> {
    println!("Starting AUDIO POC...");

    let (tx_audio, rx_audio) = channel::<Vec<i16>>();

    // UDP Receiver (puerto donde recibo audio)
    start_udp_receiver(tx_audio, 50010)?;

    // UDP Sender (enviar audio a otro cliente / a mí mismo)
    let sender = UdpSender::new("127.0.0.1:50010")?;

    // Player
    let _player_stream = AudioPlayer::start(rx_audio)?;

    // Recorder → enviado por UDP
    let _rec_stream = AudioRecorder::start(move |frame| {
        sender.send(frame).ok();
    })?;

    println!("AUDIO POC RUNNING. Speak into your microphone.");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}
