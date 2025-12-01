use core::fmt;
use std::{
    io::Write,
    io::{self, Cursor, Read},
    net::{SocketAddr, UdpSocket},
    sync::Arc,
};

use crate::{log::log_sink::LogSink, sink_trace, sink_warn};

// Struct modificado para incluir logger
#[derive(Clone)]
pub(crate) struct BufferedUdpChannel {
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
    reader: Cursor<Vec<u8>>,
    recv_buf: Vec<u8>,
    logger: Arc<dyn LogSink>,
}

impl fmt::Debug for BufferedUdpChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BufferedUdpChannel")
            .field("peer", &self.peer)
            .finish()
    }
}

impl BufferedUdpChannel {
    pub(crate) fn new(sock: Arc<UdpSocket>, peer: SocketAddr, logger: Arc<dyn LogSink>) -> Self {
        Self {
            sock,
            peer,
            reader: Cursor::new(Vec::new()),
            recv_buf: vec![0u8; 4096],
            logger,
        }
    }
}

impl Read for BufferedUdpChannel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // entrega datos pendientes
        let pos = self.reader.position();
        if pos < self.reader.get_ref().len() as u64 {
            return self.reader.read(buf);
        }

        // buffer vacío: leer del socket
        loop {
            match self.sock.recv_from(&mut self.recv_buf) {
                Ok((n, from)) => {
                    if from != self.peer {
                        sink_warn!(
                            &self.logger,
                            "[DTLS IO] Ignored packet from unknown peer: {} (expected {})",
                            from,
                            self.peer
                        );
                        continue;
                    }

                    sink_trace!(&self.logger, "[DTLS IO] Read {} bytes from {}", n, from);
                    self.reader = Cursor::new(self.recv_buf[..n].to_vec());
                    return self.reader.read(buf);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Err(io::Error::from(io::ErrorKind::WouldBlock));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Write for BufferedUdpChannel {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        sink_trace!(
            &self.logger,
            "[DTLS IO] Sending {} bytes to {}",
            buf.len(),
            self.peer
        );
        self.sock.send_to(buf, self.peer)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
