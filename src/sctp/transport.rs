use crate::dtls::buffered_udp_channel::BufferedUdpChannel;
use crate::log::log_sink::LogSink;
use crate::sctp::events::SctpEvents;
use crate::{sink_debug, sink_error, sink_trace};
use openssl::ssl::SslStream;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

pub struct SctpTransport {
    ssl_stream: SslStream<BufferedUdpChannel>,
    log_sink: Arc<dyn LogSink>,
    router_tx: Sender<SctpEvents>,
    rx: Receiver<SctpEvents>,
}

impl SctpTransport {
    pub fn new(
        ssl_stream: SslStream<BufferedUdpChannel>,
        log_sink: Arc<dyn LogSink>,
        router_tx: Sender<SctpEvents>,
        rx: Receiver<SctpEvents>,
    ) -> Self {
        // Set manual mode on the channel so we don't race with Session's socket reading
        let mut stream = ssl_stream;
        stream.get_mut().set_manual_mode(true);
        Self {
            ssl_stream: stream,
            log_sink,
            router_tx,
            rx,
        }
    }

    pub fn run(mut self) {
        sink_debug!(self.log_sink, "[SctpTransport] Started");
        let mut buf = [0u8; 65535];

        while let Ok(first_event) = self.rx.recv() {
            let mut batch = Vec::with_capacity(16);
            batch.push(first_event);
            batch.extend(self.rx.try_iter());

            // Bulk Injection & Processing
            for event in batch {
                match event {
                    SctpEvents::IncomingSctpPacket { sctp_packet } => {
                        // Packet from UDP socket (via Session)
                        sink_trace!(
                            self.log_sink,
                            "[SctpTransport] Received IncomingSctpPacket len={}",
                            sctp_packet.len()
                        );
                        // Push to internal queue (Bulk Injection)
                        self.ssl_stream.get_mut().push_incoming(sctp_packet);
                    }
                    SctpEvents::TransmitSctpPacket { payload } => {
                        // Encrypt and send
                        if let Err(e) = self.ssl_stream.write_all(&payload) {
                            sink_error!(self.log_sink, "[SctpTransport] DTLS write error: {}", e);
                        }
                    }
                    _ => {}
                }
            }

            // Optimized Read Loop
            loop {
                let start = std::time::Instant::now();
                match self.ssl_stream.read(&mut buf) {
                    Ok(n) => {
                        let elapsed = start.elapsed();
                        if n > 0 {
                            sink_trace!(
                                self.log_sink,
                                "[SCTP_TRANSPORT] DTLS decryption time: {:?} (decrypted {} bytes)",
                                elapsed,
                                n
                            );
                            let decrypted = buf[..n].to_vec();
                            // Send to Router
                            let _ = self.router_tx.send(SctpEvents::ReadableSctpPacket {
                                sctp_packet: decrypted,
                            });
                        } else {
                            break;
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(e) => {
                        sink_error!(
                            self.log_sink,
                            "[SctpTransport] DTLS read error: {}",
                            e
                        );
                        break;
                    }
                }
            }
        }
        sink_debug!(self.log_sink, "[SctpTransport] Stopped");
    }
}
