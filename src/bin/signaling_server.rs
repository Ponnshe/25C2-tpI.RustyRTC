use std::sync::Arc;

use rustyrtc::app::log_sink::LogSink;
use rustyrtc::app::logger::Logger;
use rustyrtc::signaling::run::run_signaling_server_with_log;

fn main() -> std::io::Result<()> {
    // Start process logger
    let logger = Logger::start_default("signaling_server", 1024, 128, 10);
    let handle = logger.handle();
    let log_sink: Arc<dyn LogSink> = Arc::new(handle);

    // Run signaling server (blocks)
    run_signaling_server_with_log("0.0.0.0:5000", log_sink)
}
