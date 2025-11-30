use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use crate::log::log_sink::LogSink;
use crate::signaling::signaling_server::SignalingServer;

/// Run the signaling server on `addr` using the given log sink.
///
/// Uses FileUserStore at `RUSTYRTC_USERS_PATH` or `users.db` by default.
pub fn run_signaling_server_with_log(addr: &str, log_sink: Arc<dyn LogSink>) -> io::Result<()> {
    let users_path = user_store_path();

    let server = SignalingServer::with_file_store(addr.to_string(), log_sink, users_path)?;
    server.run()
}

/// Convenience: run signaling server with a `NoopLogSink` (no logging),
/// still using FileUserStore at the configured path.
pub fn run_signaling_server(addr: &str) -> io::Result<()> {
    let users_path = user_store_path();

    let server = SignalingServer::with_file_store_no_log(addr.to_string(), users_path)?;
    server.run()
}

fn user_store_path() -> PathBuf {
    if let Ok(p) = std::env::var("RUSTYRTC_USERS_PATH") {
        return PathBuf::from(p);
    }

    // Default: place users.db next to the executable so restarts pick up the same file
    // even if launched from a different working directory.
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("users.db")))
        .unwrap_or_else(|| PathBuf::from("users.db"))
}
