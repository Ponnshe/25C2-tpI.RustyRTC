use rustyrtc::config::Config;
use rustyrtc::log::log_sink::LogSink;
use rustyrtc::log::logger::Logger;
use rustyrtc::signaling::run::run_signaling_server_with_log;
use std::sync::Arc;
use std::{env, process};

fn main() -> std::io::Result<()> {
    // --- Load config file --------------------------------------------------
    let config = Config::load("roomrtc.conf")
        .or_else(|_| Config::load("default.conf"))
        .unwrap_or_else(|e| {
            eprintln!("Error loading config: {e}. Using empty config.");
            Config::empty()
        });
    let config = Arc::new(config);

    // --- Parse CLI args ----------------------------------------------------
    let args: Vec<String> = env::args().collect();

    let addr = match args.len() {
        1 => config
            .get_or_default("Signaling", "listen_address", "0.0.0.0:5000")
            .to_string(),
        2 => args[1].clone(),
        3 => format!("{}:{}", args[1], args[2]),
        _ => {
            eprintln!("Usage:");
            eprintln!("  {}                # listen on 0.0.0.0:5000", args[0]);
            eprintln!("  {} [ADDR]         # e.g. 0.0.0.0:6000", args[0]);
            eprintln!("  {} [IP] [PORT]    # e.g. 127.0.0.1 6000", args[0]);
            eprintln!();
            eprintln!("When using cargo:");
            eprintln!("  cargo run --bin signaling_server");
            eprintln!("  cargo run --bin signaling_server -- 0.0.0.0:6000");
            eprintln!("  cargo run --bin signaling_server -- 127.0.0.1 6000");
            process::exit(1);
        }
    };

    // --- Start process logger ----------------------------------------------
    let logger = Logger::start_default("signaling_server", 1024, 128, 10);
    let handle = logger.handle();
    let log_sink: Arc<dyn LogSink> = Arc::new(handle);

    eprintln!("[signaling_server] starting on {}", addr);

    // --- Run signaling server (blocks) -------------------------------------
    run_signaling_server_with_log(&addr, log_sink, Arc::clone(&config))
}

