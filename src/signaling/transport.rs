use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use crate::log::log_sink::LogSink;
use crate::signaling::protocol::{self, FrameError, SignalingMsg};
use crate::signaling::server_event::ServerEvent;
use crate::signaling::types::ClientId;
use crate::sink_error;
use rustls::{ServerConnection, StreamOwned};

/// Thin wrapper over a blocking stream that speaks in `Msg`.
pub struct Connection<S> {
    pub client_id: ClientId,
    stream: S,
}

impl<S> Connection<S>
where
    S: Read + Write,
{
    pub const fn new(id: ClientId, stream: S) -> Self {
        Self {
            client_id: id,
            stream,
        }
    }

    /// # Errors
    /// Returns `FrameError` on I/O or protocol-level read errors.
    pub fn recv(&mut self) -> Result<SignalingMsg, FrameError> {
        protocol::read_msg(&mut self.stream)
    }

    /// # Errors
    /// Returns `FrameError` on I/O or protocol-level write errors.
    pub fn send(&mut self, msg: &SignalingMsg) -> Result<(), FrameError> {
        protocol::write_msg(&mut self.stream, msg)
    }
}

/// TLS-enabled variant: single thread that handles both reading and writing.
///
/// `stream` is a rustls `StreamOwned<ServerConnection, TcpStream>`.
#[allow(clippy::expect_used)]
pub(crate) fn spawn_tls_connection_thread(
    client_id: ClientId,
    stream: StreamOwned<ServerConnection, TcpStream>,
    server_tx: Sender<ServerEvent>,
    log: Arc<dyn LogSink>,
) {
    let (to_client_tx, to_client_rx) = mpsc::channel::<SignalingMsg>();

    // Register client with the central server loop.
    server_tx
        .send(ServerEvent::RegisterClient {
            client_id,
            to_client: to_client_tx,
        })
        .expect("server loop should be alive");

    thread::spawn(move || {
        let mut conn = Connection::new(client_id, stream);

        loop {
            // 1) Drain outgoing messages from server → client.
            loop {
                match to_client_rx.try_recv() {
                    Ok(msg) => {
                        if let Err(e) = conn.send(&msg) {
                            sink_error!(log, "[conn {}] error sending TLS msg: {:?}", client_id, e);
                            let _ = server_tx.send(ServerEvent::Disconnected { client_id });
                            return;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        let _ = server_tx.send(ServerEvent::Disconnected { client_id });
                        return;
                    }
                }
            }

            // 2) Try to read a message from client → server.
            match conn.recv() {
                Ok(msg) => {
                    if server_tx
                        .send(ServerEvent::MsgFromClient { client_id, msg })
                        .is_err()
                    {
                        // Server loop is gone.
                        return;
                    }
                }
                // Non-fatal timeouts / would-block: just no data right now.
                Err(FrameError::Io(ref e))
                    if e.kind() == io::ErrorKind::TimedOut
                        || e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::Interrupted =>
                {
                    // nothing to do, fall through
                }
                // Fatal IO error: disconnect.
                Err(FrameError::Io(e)) => {
                    sink_error!(
                        log,
                        "[conn {}] IO error in TLS reader: {:?} (kind={:?})",
                        client_id,
                        e,
                        e.kind()
                    );
                    let _ = server_tx.send(ServerEvent::Disconnected { client_id });
                    return;
                }
                // Protocol/framing error: also disconnect.
                Err(other @ FrameError::Proto(_)) => {
                    sink_error!(
                        log,
                        "[conn {}] frame error in TLS reader: {:?}",
                        client_id,
                        other
                    );
                    let _ = server_tx.send(ServerEvent::Disconnected { client_id });
                    return;
                }
            }

            // Avoid busy-spinning when idle.
            thread::sleep(Duration::from_millis(10));
        }
    });
}

/// Spawn reader + writer threads for a single `TcpStream` client.
///
/// `server_tx` is the `Sender<ServerEvent>` that talks to the central server loop.
#[allow(clippy::expect_used, clippy::needless_pass_by_value)]
#[allow(dead_code)]
pub(crate) fn spawn_connection_threads(
    client_id: ClientId,
    stream: TcpStream,
    server_tx: Sender<ServerEvent>,
    log: Arc<dyn LogSink>,
) -> std::io::Result<()> {
    let (to_client_tx, to_client_rx) = mpsc::channel::<SignalingMsg>();

    // Register client with server
    server_tx
        .send(ServerEvent::RegisterClient {
            client_id,
            to_client: to_client_tx,
        })
        .expect("server loop should be alive");

    let read_stream = stream.try_clone()?;
    let write_stream = stream;
    let log_for_read = log.clone();

    // READER THREAD: socket -> ServerEvent::MsgFromClient
    {
        let server_tx = server_tx.clone();
        thread::spawn(move || {
            let mut conn = Connection::new(client_id, read_stream);

            loop {
                match conn.recv() {
                    Ok(msg) => {
                        if server_tx
                            .send(ServerEvent::MsgFromClient { client_id, msg })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = server_tx.send(ServerEvent::Disconnected { client_id });
                        match e {
                            FrameError::Io(io_e) => {
                                sink_error!(
                                    log_for_read,
                                    "[conn {}] IO error in reader: {:?} (kind={:?})",
                                    client_id,
                                    io_e,
                                    io_e.kind()
                                );
                            }
                            other => {
                                sink_error!(
                                    log_for_read,
                                    "[conn {}] frame error in reader: {:?}",
                                    client_id,
                                    other
                                );
                            }
                        }
                        break;
                    }
                }
            }
        });
    }

    // WRITER THREAD: to_client_rx -> socket

    {
        thread::spawn(move || {
            let mut conn = Connection::new(client_id, write_stream);

            while let Ok(msg) = to_client_rx.recv() {
                if let Err(e) = conn.send(&msg) {
                    sink_error!(log, "[conn {}] error sending msg: {:?}", client_id, e);
                    // The reader thread is the one responsible for notifying the server of a disconnect.
                    break;
                }
            }
        });
    }

    Ok(())
}
