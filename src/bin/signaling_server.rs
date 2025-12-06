//! The signaling server binary for the RoomRTC application.
//! It starts the signaling server and listens for incoming connections.

use rustyrtc::config::Config;
use rustyrtc::log::log_sink::LogSink;
use rustyrtc::log::logger::Logger;
use rustyrtc::signaling::run::run_signaling_server_with_log;
use std::sync::Arc;
use std::{env, process};

fn main() /*-> std::io::Result<()>*/ {
    audio_poc::poc_main::start_audio_poc().unwrap();
    /*let args: Vec<String> = env::args().collect();
    let config_result = if args.len() > 1 {
        let path = &args[1];
        println!("Trying to load personal config: {}", path);
        Config::load(path)
    } else {
        Config::load("server_roomrtc.conf").or_else(|_| Config::load("server_default.conf"))
    };

    let config = config_result.unwrap_or_else(|e| {
        eprintln!("Error loading config: {e}. Using empty config.");
        Config::empty()
    });

    let config = Arc::new(config);

    let Some(addr) = config.get_non_empty("Signaling", "listen_address") else {
        eprintln!("You need to set the listen_addres parameter in the config file");
        process::exit(1);
    };

    // --- Start process logger ----------------------------------------------
    let logger = Logger::start_server(1024, 128, 10, config.clone());
    let handle = logger.handle();
    let log_sink: Arc<dyn LogSink> = Arc::new(handle);

    eprintln!("[signaling_server] starting on {}", addr);

    // --- Run signaling server (blocks) -------------------------------------
    run_signaling_server_with_log(addr, log_sink, Arc::clone(&config))*/
}
