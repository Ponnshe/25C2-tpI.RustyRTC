use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use crate::app::log_sink::{LogSink, NoopLogSink};
use crate::signaling::signaling_server::SignalingServer;

/// Run the signaling server on `addr` using the given log sink.
///
/// Uses FileUserStore at `RUSTYRTC_USERS_PATH` or `users.db` by default.
pub fn run_signaling_server_with_log(addr: &str, log_sink: Arc<dyn LogSink>) -> io::Result<()> {
    let user_store_path_str =
        std::env::var("RUSTYRTC_USERS_PATH").unwrap_or_else(|_| "users.db".to_string());
    let users_path = PathBuf::from(user_store_path_str);

    let server = SignalingServer::with_file_store(addr.to_string(), log_sink, users_path)?;
    server.run()
}

/// Convenience: run signaling server with a `NoopLogSink` (no logging),
/// still using FileUserStore at the configured path.
pub fn run_signaling_server(addr: &str) -> io::Result<()> {
    let user_store_path_str =
        std::env::var("RUSTYRTC_USERS_PATH").unwrap_or_else(|_| "users.db".to_string());
    let users_path = PathBuf::from(user_store_path_str);

    let server = SignalingServer::with_file_store_no_log(addr.to_string(), users_path)?;
    server.run()
}
