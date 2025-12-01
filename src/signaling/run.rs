use crate::config::Config;
use crate::log::log_sink::LogSink;
use crate::signaling::SignalingServer;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

/// Run the signaling server on `addr` using the given log sink.
///
/// Uses FileUserStore at `RUSTYRTC_USERS_PATH` or `users.db` by default.
pub fn run_signaling_server_with_log(
    addr: &str,
    log_sink: Arc<dyn LogSink>,
    config: Arc<Config>,
) -> io::Result<()> {
    let users_path = user_store_path(config.clone());

    let server = SignalingServer::with_file_store(addr.to_string(), log_sink, users_path, config)?;
    server.run()
}

/// Convenience: run signaling server with a `NoopLogSink` (no logging),
/// still using FileUserStore at the configured path.
pub fn run_signaling_server(addr: &str, config: Arc<Config>) -> io::Result<()> {
    let users_path = user_store_path(config.clone());

    let server = SignalingServer::with_file_store_no_log(addr.to_string(), users_path, config)?;
    server.run()
}

fn user_store_path(config: Arc<Config>) -> PathBuf {
    if let Some(path) = config.get_non_empty("Signaling", "database_path") {
        return PathBuf::from(path);
    }

    if let Ok(p) = std::env::var("RUSTYRTC_USERS_PATH")
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }

    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("users.db")))
        .unwrap_or_else(|| PathBuf::from("users.db"))
}
