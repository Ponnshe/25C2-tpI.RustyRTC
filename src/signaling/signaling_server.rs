use crate::config::Config;
use crate::log::NoopLogSink;
use crate::log::log_sink::LogSink;
use crate::signaling::auth::{AuthBackend, FileUserStore};
use crate::signaling::router::Router;
use crate::signaling::runtime::run_server_loop;
use crate::signaling::server_event::ServerEvent;
use crate::signaling::tls::build_signaling_server_config;
use crate::signaling::transport::spawn_tls_connection_thread;
use crate::signaling::types::ClientId;
use crate::{sink_info, sink_warn};
use rustls::{ServerConnection, StreamOwned};
use std::io;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

/// Top-level runtime object for the signaling service.
///
/// This owns:
/// - bind address
/// - logging sink
/// - auth backend (e.g. `FileUserStore`)
///   and knows how to spin up the central Router+Server loop plus per-connection threads.
pub struct SignalingServer {
    bind_addr: String,
    log: Arc<dyn LogSink>,
    auth_backend: Box<dyn AuthBackend>,
    /// Optional: kept only for nicer logging/debugging.
    user_store_path: Option<PathBuf>,
    config: Arc<Config>,
}

impl SignalingServer {
    /// Construct a server with an arbitrary auth backend (good for tests).
    pub fn with_auth<S, A>(
        bind_addr: S,
        log: Arc<dyn LogSink>,
        auth_backend: A,
        config: Arc<Config>,
    ) -> Self
    where
        S: Into<String>,
        A: AuthBackend + 'static,
    {
        Self {
            bind_addr: bind_addr.into(),
            log,
            auth_backend: Box::new(auth_backend),
            user_store_path: None,
            config,
        }
    }

    /// Construct a server that uses a `FileUserStore` at `users_path`.
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if the user store file cannot be opened.
    pub fn with_file_store<S>(
        bind_addr: S,
        log: Arc<dyn LogSink>,
        users_path: PathBuf,
        config: Arc<Config>,
    ) -> io::Result<Self>
    where
        S: Into<String>,
    {
        let store = FileUserStore::open(&users_path)?;
        Ok(Self {
            bind_addr: bind_addr.into(),
            log,
            auth_backend: Box::new(store),
            user_store_path: Some(users_path),
            config,
        })
    }

    /// Convenience: `FileUserStore` + `NoopLogSink`.
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if the user store file cannot be opened.
    pub fn with_file_store_no_log<S>(
        bind_addr: S,
        users_path: PathBuf,
        config: Arc<Config>,
    ) -> io::Result<Self>
    where
        S: Into<String>,
    {
        Self::with_file_store(bind_addr, Arc::new(NoopLogSink), users_path, config)
    }

    /// # Errors
    ///
    /// Returns an `io::Error` if the TLS configuration cannot be built or if the
    /// server fails to bind to the specified address.
    pub fn run(self) -> io::Result<()> {
        let Self {
            bind_addr,
            log,
            auth_backend,
            user_store_path,
            config,
        } = self;

        // --- TLS config (mkcert server cert + key) ---
        let tls_config = build_signaling_server_config(config)?;

        let listener = TcpListener::bind(&bind_addr)?;

        if let Some(ref path) = user_store_path {
            sink_info!(log, "using user store file at {:?}", path);
        } else {
            sink_info!(log, "running signaling server with custom auth backend");
        }

        // Events from all connections → central server loop
        let (server_tx, server_rx) = mpsc::channel::<ServerEvent>();

        // Central Router + Server loop in its own thread
        {
            let log_for_loop = log.clone();
            let log_for_router = log.clone();

            thread::spawn(move || {
                sink_info!(log_for_loop, "[signaling] server loop started");
                let router = Router::with_log_and_auth(log_for_router, auth_backend);
                run_server_loop(router, log_for_loop, server_rx);
            });
        }

        let mut next_client_id: ClientId = 1;
        sink_info!(log, "signaling server (TLS) listening on {}", bind_addr);

        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    sink_warn!(
                        log,
                        "incoming TCP accept failed: {:?} (continuing to accept)",
                        e
                    );
                    continue;
                }
            };

            // Configure underlying TCP before wrapping in TLS.
            if let Err(e) = stream.set_nodelay(true) {
                sink_warn!(log, "set_nodelay failed: {:?}", e);
            }
            if let Err(e) = stream.set_read_timeout(Some(Duration::from_millis(200))) {
                sink_warn!(log, "set_read_timeout failed: {:?}", e);
            }

            let client_id = next_client_id;
            next_client_id += 1;

            let server_tx_clone = server_tx.clone();
            let log_for_conn = log.clone();

            sink_info!(log, "accepted TLS connection as client_id={}", client_id);

            // Build a rustls ServerConnection for this client.
            let conn = match ServerConnection::new(Arc::clone(&tls_config)) {
                Ok(c) => c,
                Err(e) => {
                    sink_warn!(
                        log,
                        "failed to create TLS session for client {}: {:?}",
                        client_id,
                        e
                    );
                    continue;
                }
            };

            // Combine TLS session + TCP into a single Read+Write stream.
            let tls_stream = StreamOwned::new(conn, stream);

            spawn_tls_connection_thread(client_id, tls_stream, server_tx_clone, log_for_conn);
        }

        Ok(())
    }
}
