use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use crate::log::log_sink::LogSink;
use crate::signaling::protocol::{FrameError, SignalingMsg};
use crate::signaling::protocol::{read_msg as proto_read_msg, write_msg as proto_write_msg};
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
    pub fn new(id: ClientId, stream: S) -> Self {
        Self {
            client_id: id,
            stream,
        }
    }

    pub fn recv(&mut self) -> Result<SignalingMsg, FrameError> {
        proto_read_msg(&mut self.stream)
    }

    pub fn send(&mut self, msg: &SignalingMsg) -> Result<(), FrameError> {
        proto_write_msg(&mut self.stream, msg)
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
) -> io::Result<()> {
    let (to_client_tx, to_client_rx) = mpsc::channel::<SignalingMsg>();

    // Register client with the central server loop.
    server_tx
        .send(ServerEvent::RegisterClient {
            client_id,
            to_client: to_client_tx.clone(),
        })
        .expect("server loop should be alive");

    let log_for_thread = log.clone();

    thread::spawn(move || {
        let mut conn = Connection::new(client_id, stream);

        loop {
            // 1) Drain outgoing messages from server → client.
            loop {
                match to_client_rx.try_recv() {
                    Ok(msg) => {
                        if let Err(e) = conn.send(&msg) {
                            sink_error!(
                                log_for_thread,
                                "[conn {}] error sending TLS msg: {:?}",
                                client_id,
                                e
                            );
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
                        log_for_thread,
                        "[conn {}] IO error in TLS reader: {:?} (kind={:?})",
                        client_id,
                        e,
                        e.kind()
                    );
                    let _ = server_tx.send(ServerEvent::Disconnected { client_id });
                    return;
                }
                // Protocol/framig error: also disconnect.
                Err(other) => {
                    sink_error!(
                        log_for_thread,
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

    Ok(())
}

/// Spawn reader + writer threads for a single TcpStream client.
///
/// `server_tx` is the Sender<ServerEvent> that talks to the central server loop.
#[allow(clippy::expect_used)]
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
            to_client: to_client_tx.clone(),
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

    let log_for_write = log.clone();
    {
        let server_tx = server_tx.clone();
        thread::spawn(move || {
            let mut conn = Connection::new(client_id, write_stream);

            while let Ok(msg) = to_client_rx.recv() {
                if let Err(e) = conn.send(&msg) {
                    sink_error!(
                        log_for_write,
                        "[conn {}] error sending msg: {:?}",
                        client_id,
                        e
                    );
                    let _ = server_tx.send(ServerEvent::Disconnected { client_id });
                    break;
                }
            }
        });
    }

    Ok(())
}
