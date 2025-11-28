use std::sync::Arc;
use std::{env, process};

use rustyrtc::app::log_sink::LogSink;
use rustyrtc::app::logger::Logger;
use rustyrtc::signaling::run::run_signaling_server_with_log;

fn main() -> std::io::Result<()> {
    // --- Parse CLI args ----------------------------------------------------
    //
    // Supported:
    //   cargo run --bin signaling_server
    //      -> binds to 0.0.0.0:5000 (default)
    //
    //   cargo run --bin signaling_server -- 0.0.0.0:6000
    //      -> binds to 0.0.0.0:6000
    //
    //   cargo run --bin signaling_server -- 127.0.0.1 7000
    //      -> binds to 127.0.0.1:7000

    let args: Vec<String> = env::args().collect();

    let addr = match args.len() {
        // no extra args -> default listen address
        1 => "0.0.0.0:5000".to_owned(),

        // one extra arg: full addr "IP:PORT"
        2 => args[1].clone(),

        // two extra args: IP + PORT
        3 => format!("{}:{}", args[1], args[2]),

        // anything else -> usage error
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
    run_signaling_server_with_log(&addr, log_sink)
}
