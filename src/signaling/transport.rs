use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Sender};
use std::thread;

use crate::signaling::protocol::{FrameError, Msg};
use crate::signaling::protocol::{read_msg as proto_read_msg, write_msg as proto_write_msg};
use crate::signaling::server_event::ServerEvent;
use crate::signaling::types::ClientId;

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

    pub fn recv(&mut self) -> Result<Msg, FrameError> {
        proto_read_msg(&mut self.stream)
    }

    pub fn send(&mut self, msg: &Msg) -> Result<(), FrameError> {
        proto_write_msg(&mut self.stream, msg)
    }
}

/// Spawn reader + writer threads for a single TcpStream client.
///
/// `server_tx` is the Sender<ServerEvent> that talks to the central server loop.
pub fn spawn_connection_threads(
    client_id: ClientId,
    stream: TcpStream,
    server_tx: Sender<ServerEvent>,
) -> std::io::Result<()> {
    let (to_client_tx, to_client_rx) = mpsc::channel::<Msg>();

    // Register client with server
    server_tx
        .send(ServerEvent::RegisterClient {
            client_id,
            to_client: to_client_tx.clone(),
        })
        .expect("server loop should be alive");

    let read_stream = stream.try_clone()?;
    let write_stream = stream;

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
                                eprintln!(
                                    "[conn {}] IO error in reader: {:?} (kind={:?})",
                                    client_id,
                                    io_e,
                                    io_e.kind()
                                );
                            }
                            other => {
                                eprintln!(
                                    "[conn {}] frame error in reader: {:?}",
                                    client_id, other
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
        let server_tx = server_tx.clone();
        thread::spawn(move || {
            let mut conn = Connection::new(client_id, write_stream);

            while let Ok(msg) = to_client_rx.recv() {
                if let Err(e) = conn.send(&msg) {
                    eprintln!("[conn {}] error sending msg: {:?}", client_id, e);
                    let _ = server_tx.send(ServerEvent::Disconnected { client_id });
                    break;
                }
            }
        });
    }

    Ok(())
}
