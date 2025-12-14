use core::fmt;
use std::{
    collections::VecDeque,
    io::Write,
    io::{self, Cursor, Read},
    net::{SocketAddr, UdpSocket},
    sync::Arc,
};

use crate::{log::log_sink::LogSink, sink_trace, sink_warn};

// Struct modificado para incluir logger
#[derive(Clone)]
pub struct BufferedUdpChannel {
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
    reader: Cursor<Vec<u8>>,
    recv_buf: Vec<u8>,
    incoming_queue: VecDeque<u8>,
    manual_mode: bool,
    logger: Arc<dyn LogSink>,
}

impl fmt::Debug for BufferedUdpChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BufferedUdpChannel")
            .field("peer", &self.peer)
            .field("manual_mode", &self.manual_mode)
            .finish()
    }
}

impl BufferedUdpChannel {
    pub fn new(sock: Arc<UdpSocket>, peer: SocketAddr, logger: Arc<dyn LogSink>) -> Self {
        Self {
            sock,
            peer,
            reader: Cursor::new(Vec::new()),
            recv_buf: vec![0u8; 4096],
            incoming_queue: VecDeque::new(),
            manual_mode: false,
            logger,
        }
    }

    pub fn set_manual_mode(&mut self, manual: bool) {
        self.manual_mode = manual;
    }

    pub fn push_incoming(&mut self, data: Vec<u8>) {
        self.incoming_queue.extend(data);
    }
}

impl Read for BufferedUdpChannel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // 1. Consume what's in the cursor first (leftovers from previous packet)
        let pos = self.reader.position();
        if pos < self.reader.get_ref().len() as u64 {
            return self.reader.read(buf);
        }

        if self.manual_mode {
            // In manual mode, we only read from incoming_queue
            if self.incoming_queue.is_empty() {
                return Err(io::Error::from(io::ErrorKind::WouldBlock));
            }
            let amt = std::cmp::min(buf.len(), self.incoming_queue.len());
            for (i, b) in self.incoming_queue.drain(..amt).enumerate() {
                buf[i] = b;
            }
            return Ok(amt);
        }

        // 2. Normal socket mode
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
