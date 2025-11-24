use std::net::TcpListener;
use std::sync::{Arc, mpsc};
use std::{io, thread};

use crate::app::log_sink::{LogSink, NoopLogSink};
use crate::signaling::router::Router;
use crate::signaling::runtime::run_server_loop;
use crate::signaling::server_event::ServerEvent;
use crate::signaling::transport::spawn_connection_threads;
use crate::signaling::types::ClientId;
use crate::signaling::{AuthBackend, FileUserStore};

/// Run the signaling server on `addr` using the given log sink.
pub fn run_signaling_server_with_log(addr: &str, log_sink: Arc<dyn LogSink>) -> io::Result<()> {
    let listener = TcpListener::bind(addr)?;

    // ---- Open user DB (FileUserStore) ----
    let user_store_path =
        std::env::var("RUSTYRTC_USERS_PATH").unwrap_or_else(|_| "users.db".to_string());
    let file_store = FileUserStore::open(&user_store_path)?;
    let auth_backend: Box<dyn AuthBackend> = Box::new(file_store);

    // Events from all connections → central server loop
    let (server_tx, server_rx) = mpsc::channel::<ServerEvent>();

    // Central Router + Server loop in its own thread
    {
        let log_for_loop = log_sink.clone();
        let log_for_router = log_sink.clone();
        let user_store_path_for_log = user_store_path.clone();
        thread::spawn(move || {
            eprintln!(
                "[signaling/run] server loop started; user DB at {}",
                user_store_path_for_log
            );
            // Use our FileUserStore-backed auth
            let router = Router::with_log_and_auth(log_for_router, auth_backend);
            run_server_loop(router, log_for_loop, server_rx);
        });
    }

    let mut next_client_id: ClientId = 1;

    eprintln!(
        "[signaling/run] listening on {} with user DB at {}",
        addr, user_store_path
    );

    for stream in listener.incoming() {
        let stream = stream?;

        let client_id = next_client_id;
        next_client_id += 1;

        let server_tx_clone = server_tx.clone();

        if let Err(e) = spawn_connection_threads(client_id, stream, server_tx_clone) {
            // transport-level failure; stderr is fine
            eprintln!(
                "[signaling/run] failed to spawn connection threads for client {}: {:?}",
                client_id, e
            );
        }
    }

    Ok(())
}

/// Convenience: run signaling server with a `NoopLogSink` (no logging).
pub fn run_signaling_server(addr: &str) -> io::Result<()> {
    run_signaling_server_with_log(addr, Arc::new(NoopLogSink))
}
