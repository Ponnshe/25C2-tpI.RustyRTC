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

        loop {
            // Determine if we need to busy-wait (poll) or block
            // Note: We check buffered_udp_channel's queue.
            // ssl_stream -> BufferedUdpChannel
            let has_pending = self.ssl_stream.get_mut().has_pending_writes();

            let event_result = if has_pending {
                // If we have pending writes, we don't want to block forever.
                // We use a small timeout to allow flushing retries.
                self.rx.recv_timeout(std::time::Duration::from_millis(1))
            } else {
                // No pending writes, we can block until new events arrive
                self.rx
                    .recv()
                    .map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected)
            };

            let first_event = match event_result {
                Ok(ev) => Some(ev),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };

            if let Some(event) = first_event {
                let mut batch = Vec::with_capacity(16);
                batch.push(event);
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
                            let start_write = std::time::Instant::now();
                            if let Err(e) = self.ssl_stream.write_all(&payload) {
                                sink_error!(
                                    self.log_sink,
                                    "[SctpTransport] DTLS write error: {}",
                                    e
                                );
                            }
                            sink_trace!(
                                self.log_sink,
                                "[SCTP_TRANSPORT] DTLS write time: {:?}",
                                start_write.elapsed()
                            );
                            crate::sctp_log!(
                                self.log_sink,
                                "DTLS_ENCRYPT/SEND_START: {}",
                                payload.len()
                            );
                        }
                        _ => {}
                    }
                }
            }

            // Always try to read/flush after event processing (or timeout)
            // Optimized Read Loop & Flush
            let mut read_count = 0;
            loop {
                // Try to flush outgoing queue first
                if let Err(e) = self.ssl_stream.get_mut().flush() {
                    sink_error!(self.log_sink, "[SctpTransport] Flush error: {}", e);
                }

                if read_count >= 20 {
                    // Yield to event loop to allow sending responses (SACKs)
                    break;
                }

                let start = std::time::Instant::now();
                match self.ssl_stream.read(&mut buf) {
                    Ok(n) => {
                        read_count += 1;
                        let elapsed = start.elapsed();
                        if n > 0 {
                            sink_trace!(
                                self.log_sink,
                                "[SCTP_TRANSPORT] DTLS decryption time: {:?} (decrypted {} bytes)",
                                elapsed,
                                n
                            );
                            crate::sctp_log!(
                                self.log_sink,
                                "DTLS_DECRYPT/RECV_END: {} (Time: {:?})",
                                n,
                                elapsed
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
                        sink_error!(self.log_sink, "[SctpTransport] DTLS read error: {}", e);
                        break;
                    }
                }
            }
        }
        sink_debug!(self.log_sink, "[SctpTransport] Stopped");
    }
}
