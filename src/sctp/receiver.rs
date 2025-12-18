use crate::log::log_sink::LogSink;
use crate::sctp::events::SctpEvents;
use crate::sctp::stream::SctpStream;
use crate::{sink_debug, sink_error, sink_info, sink_trace, sink_warn};
use bytes::Bytes;
use sctp_proto::{
    Association, AssociationHandle, DatagramEvent, Endpoint, Event, Payload, StreamEvent,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub struct SctpReceiver {
    pub log_sink: Arc<dyn LogSink>,
    pub tx: Sender<SctpEvents>,
    pub rx: Receiver<SctpEvents>,
    pub streams: Arc<RwLock<HashMap<u32, SctpStream>>>,
    pub endpoint: Arc<Mutex<Endpoint>>,
    pub association: Arc<Mutex<Option<Association>>>,
    pub association_handle: Arc<Mutex<Option<AssociationHandle>>>,
}

impl SctpReceiver {
    pub fn new(
        log_sink: Arc<dyn LogSink>,
        tx: Sender<SctpEvents>,
        rx: Receiver<SctpEvents>,
        streams: Arc<RwLock<HashMap<u32, SctpStream>>>,
        endpoint: Arc<Mutex<Endpoint>>,
        association: Arc<Mutex<Option<Association>>>,
        association_handle: Arc<Mutex<Option<AssociationHandle>>>,
    ) -> Self {
        Self {
            log_sink,
            tx,
            rx,
            streams,
            endpoint,
            association,
            association_handle,
        }
    }

    #[allow(clippy::expect_used)]
    pub fn run(&self) {
        loop {
            // Determine timeout for sctp
            let timeout = {
                let mut assoc_guard = self.association.lock().expect("association lock poisoned");
                if let Some(assoc) = assoc_guard.as_mut() {
                    assoc
                        .poll_timeout()
                        .map(|inst| inst.saturating_duration_since(Instant::now()))
                } else {
                    None
                }
            };

            // Wait for event or timeout
            // Use a small timeout if sctp doesn't need immediate attention, to check stream timeouts
            let wait_duration = timeout.unwrap_or(Duration::from_millis(100));

            sink_trace!(
                self.log_sink,
                "[SCTP_RECEIVER] wait_duration before: {:?}",
                wait_duration
            );

            // Cap wait duration to check stream timeouts frequently (e.g. every 1 sec)
            let wait_duration = wait_duration.min(Duration::from_secs(1));

            let event = self.rx.recv_timeout(wait_duration);

            match event {
                Ok(SctpEvents::ReadableSctpPacket { sctp_packet }) => {
                    let start = Instant::now();
                    self.handle_packet(sctp_packet);
                    sink_trace!(
                        self.log_sink,
                        "[SCTP_RECEIVER] Processed ReadableSctpPacket in {:?}",
                        start.elapsed()
                    );
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    sink_trace!(self.log_sink, "[SCTP_RECEIVER] Timeout",);
                    // Handle SCTP timeout if needed
                    let mut assoc_guard =
                        self.association.lock().expect("association lock poisoned");
                    if let Some(assoc) = assoc_guard.as_mut() {
                        // Check if it was really an SCTP timeout or just our loop cap
                        if let Some(next_timeout) = assoc.poll_timeout()
                            && Instant::now() >= next_timeout
                        {
                            assoc.handle_timeout(Instant::now());
                        }
                    }
                    drop(assoc_guard);
                    self.poll_association();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
                _ => {}
            }

            self.check_stream_timeouts();
        }
    }

    #[allow(clippy::expect_used)]
    fn check_stream_timeouts(&self) {
        let mut timed_out_ids = Vec::new();
        {
            let streams = self.streams.read().expect("streams lock poisoned");
            for (id, stream) in streams.iter() {
                if stream.is_timed_out() {
                    timed_out_ids.push(*id);
                }
            }
        }

        for id in timed_out_ids {
            sink_warn!(
                self.log_sink,
                "[SCTP_RECEIVER] Stream {} timed out, sending Cancel",
                id
            );
            let _ = self.tx.send(SctpEvents::SendCancel { id });
        }
    }

    #[allow(clippy::expect_used)]
    fn handle_packet(&self, packet: Vec<u8>) {
        let start = Instant::now();
        sink_trace!(
            self.log_sink,
            "[SCTP_RECEIVER] Handling incoming SCTP packet of size {}",
            packet.len()
        );
        sink_debug!(
            self.log_sink,
            "[SCTP_RECEIVER] SCTP bytes received via DTLS: {}",
            packet.len()
        );
        let mut endpoint = self.endpoint.lock().expect("Failed to lock endpoint");
        let now = Instant::now();
        // Use a dummy address as we are tunneling over DTLS
        let remote: SocketAddr = "127.0.0.1:5000".parse().expect("Invalid dummy IP address");

        let bytes = Bytes::from(packet);

        match endpoint.handle(now, remote, None, None, bytes) {
            Some((handle, DatagramEvent::NewAssociation(assoc))) => {
                sink_info!(
                    self.log_sink,
                    "[SCTP_RECEIVER] New SCTP Association created"
                );
                {
                    let mut my_assoc = self.association.lock().expect("association lock poisoned");
                    *my_assoc = Some(assoc);
                    let mut my_handle = self
                        .association_handle
                        .lock()
                        .expect("association handle lock poisoned");
                    *my_handle = Some(handle);
                }

                // Poll the new association immediately
            }
            Some((_handle, DatagramEvent::AssociationEvent(event))) => {
                let mut my_assoc_guard =
                    self.association.lock().expect("association lock poisoned");
                if let Some(assoc) = my_assoc_guard.as_mut() {
                    assoc.handle_event(event);
                } else {
                    sink_warn!(
                        self.log_sink,
                        "[SCTP_RECEIVER] Received AssociationEvent but no association exists"
                    );
                }
                drop(my_assoc_guard); // unlock to poll
            }
            None => {
                // Packet consumed, no event.
            }
        }
        self.poll_association();
        sink_trace!(
            self.log_sink,
            "[SCTP_RECEIVER] handle_packet took {:?}",
            start.elapsed()
        );
    }

    #[allow(clippy::expect_used)]
    fn poll_association(&self) {
        let start = Instant::now();
        let mut assoc_guard = self.association.lock().expect("association lock poisoned");
        if let Some(assoc) = assoc_guard.as_mut() {
            let now = Instant::now();

            // Poll transmit
            while let Some(transmit) = assoc.poll_transmit(now) {
                if let Payload::RawEncode(bytes_vec) = transmit.payload {
                    let mut payload = Vec::new();
                    for b in bytes_vec {
                        payload.extend_from_slice(&b);
                    }
                    let _ = self.tx.send(SctpEvents::TransmitSctpPacket { payload });
                }
            }

            // Poll events
            while let Some(event) = assoc.poll() {
                match event {
                    Event::Connected => {
                        sink_info!(self.log_sink, "[SCTP_RECEIVER] SCTP Association connected");
                        let _ = self.tx.send(SctpEvents::SctpConnected);
                    }
                    Event::AssociationLost { reason } => {
                        sink_error!(
                            self.log_sink,
                            "[SCTP_RECEIVER] SCTP Association lost: {:?}",
                            reason
                        );
                    }
                    Event::Stream(StreamEvent::Readable { id }) => {
                        // Read from stream
                        if let Ok(mut stream) = assoc.stream(id) {
                            match stream.read_sctp() {
                                Ok(Some(chunks)) => {
                                    let mut buf = vec![0u8; 65535];
                                    match chunks.read(&mut buf) {
                                        Ok(len) => {
                                            sink_trace!(
                                                self.log_sink,
                                                "[SCTP_RECEIVER] Stream {} readable. Read {} bytes.",
                                                id,
                                                len
                                            );
                                            let data = Bytes::copy_from_slice(&buf[..len]);
                                            self.handle_chunk_data(data);
                                        }
                                        Err(e) => {
                                            sink_warn!(
                                                self.log_sink,
                                                "[SCTP_RECEIVER] Error reading chunks: {:?}",
                                                e
                                            );
                                        }
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    sink_warn!(
                                        self.log_sink,
                                        "[SCTP_RECEIVER] Error reading from stream {}: {:?}",
                                        id,
                                        e
                                    );
                                }
                            }
                        } else {
                            sink_warn!(
                                self.log_sink,
                                "[SCTP_RECEIVER] Stream {} readable but failed to get stream handle",
                                id
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        let elapsed = start.elapsed();
        if elapsed.as_micros() > 100 {
            sink_trace!(
                self.log_sink,
                "[SCTP_RECEIVER] poll_association took {:?}",
                elapsed
            );
        }
    }

    #[allow(clippy::expect_used)]
    fn handle_chunk_data(&self, data: Bytes) {
        use crate::sctp::protocol::SctpProtocolMessage;

        match SctpProtocolMessage::deserialize(&data) {
            Ok(msg) => {
                sink_trace!(
                    self.log_sink,
                    "[SCTP_RECEIVER] Deserialized message: {:?}",
                    msg
                );
                match msg {
                    SctpProtocolMessage::Offer {
                        id,
                        filename,
                        file_size,
                    } => {
                        sink_trace!(
                            self.log_sink,
                            "[SCTP_RECEIVER] Received Offer for file_id: {}",
                            id
                        );
                        let props = crate::sctp::events::SctpFileProperties {
                            file_name: filename,
                            file_size,
                            transaction_id: id,
                        };
                        let _ = self.tx.send(SctpEvents::ReceivedOffer {
                            file_properties: props,
                        });
                    }
                    SctpProtocolMessage::Accept { id } => {
                        sink_trace!(
                            self.log_sink,
                            "[SCTP_RECEIVER] Received Accept for file_id: {}",
                            id
                        );
                        let _ = self.tx.send(SctpEvents::ReceivedAccept { id });
                    }
                    SctpProtocolMessage::Reject { id } => {
                        sink_trace!(
                            self.log_sink,
                            "[SCTP_RECEIVER] Received Reject for file_id: {}",
                            id
                        );
                        let _ = self.tx.send(SctpEvents::ReceivedReject { id });
                    }
                    SctpProtocolMessage::Cancel { id } => {
                        sink_trace!(
                            self.log_sink,
                            "[SCTP_RECEIVER] Received Cancel for file_id: {}",
                            id
                        );
                        let _ = self.tx.send(SctpEvents::ReceivedCancel { id });
                    }
                    SctpProtocolMessage::Chunk { id, seq, payload } => {
                        sink_trace!(
                            self.log_sink,
                            "[SCTP_RECEIVER] Received Chunk for file_id: {} seq: {}",
                            id,
                            seq
                        );
                        sink_debug!(
                            self.log_sink,
                            "[SCTP_RECEIVER] File bytes received: {}",
                            payload.len()
                        );
                        {
                            let mut streams = self.streams.write().expect("streams lock poisoned");
                            if let Some(stream) = streams.get_mut(&id) {
                                stream.update_activity();
                            }
                        }
                        let _ = self.tx.send(SctpEvents::ReceivedChunk {
                            id,
                            seq: seq as u32,
                            payload,
                        });
                    }
                    SctpProtocolMessage::EndFile { id } => {
                        sink_trace!(
                            self.log_sink,
                            "[SCTP_RECEIVER] Received EndFile for file_id: {}",
                            id
                        );
                        let _ = self.tx.send(SctpEvents::ReceivedEndFile { id });
                    }
                }
            }
            Err(e) => {
                sink_warn!(
                    self.log_sink,
                    "[SCTP_RECEIVER] Failed to deserialize SCTP message: {:?}",
                    e
                );
            }
        }
    }
}
