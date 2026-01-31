use crate::log::log_sink::LogSink;
use crate::sctp::events::SctpEvents;
use crate::sctp::protocol::SctpProtocolMessage;
use crate::sctp::stream::SctpStream;
use crate::{sink_debug, sink_error, sink_info, sink_trace, sink_warn};
use bytes::Bytes;
use sctp_proto::{
    Association, AssociationHandle, ClientConfig, Endpoint, Error, Payload,
    PayloadProtocolIdentifier,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

pub struct SctpSender {
    pub log_sink: Arc<dyn LogSink>,
    pub tx: Sender<SctpEvents>,
    pub rx: Receiver<SctpEvents>,
    pub association: Arc<Mutex<Option<Association>>>,
    pub association_handle: Arc<Mutex<Option<AssociationHandle>>>,
    pub streams: Arc<RwLock<HashMap<u32, SctpStream>>>,
    pub endpoint: Arc<Mutex<Endpoint>>,
    pub is_client: bool,
}

impl SctpSender {
    pub fn new(
        log_sink: Arc<dyn LogSink>,
        tx: Sender<SctpEvents>,
        rx: Receiver<SctpEvents>,
        association: Arc<Mutex<Option<Association>>>,
        association_handle: Arc<Mutex<Option<AssociationHandle>>>,
        streams: Arc<RwLock<HashMap<u32, SctpStream>>>,
        endpoint: Arc<Mutex<Endpoint>>,
        is_client: bool,
    ) -> Self {
        Self {
            log_sink,
            tx,
            rx,
            association,
            association_handle,
            streams,
            endpoint,
            is_client,
        }
    }

    #[allow(clippy::expect_used)]
    pub fn run(&self) {
        let mut pending_messages = Vec::new();
        use std::time::Duration;

        // Ensure SCTP association is started immediately
        self.ensure_connection();

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
            let wait_duration = timeout.unwrap_or(Duration::from_millis(100));
            // Cap wait duration
            let wait_duration = wait_duration.min(Duration::from_secs(1));

            let event = self.rx.recv_timeout(wait_duration);

            match event {
                Ok(SctpEvents::SendOffer { file_properties }) => {
                    sink_trace!(
                        self.log_sink,
                        "[SCTP_SENDER] Processing SendOffer for id: {}",
                        file_properties.transaction_id
                    );
                    // Create Stream entry for tracking chunks
                    {
                        let stream = SctpStream::new(file_properties.clone());
                        let mut streams = self.streams.write().expect("streams lock poisoned");
                        streams.insert(file_properties.transaction_id, stream);
                    }

                    let msg = SctpProtocolMessage::Offer {
                        id: file_properties.transaction_id,
                        filename: file_properties.file_name,
                        file_size: file_properties.file_size,
                    };
                    self.send_message(msg, &mut pending_messages);
                }
                Ok(SctpEvents::SendAccept { id }) => {
                    sink_trace!(
                        self.log_sink,
                        "[SCTP_SENDER] Processing SendAccept for id: {}",
                        id
                    );
                    // Create Stream
                    {
                        let props = crate::sctp::events::SctpFileProperties {
                            file_name: "".to_string(),
                            file_size: 0,
                            transaction_id: id,
                        };
                        let stream = SctpStream::new(props);
                        let mut streams = self.streams.write().expect("streams lock poisoned");
                        streams.insert(id, stream);
                    }
                    self.send_message(SctpProtocolMessage::Accept { id }, &mut pending_messages);
                }
                Ok(SctpEvents::ReceivedAccept { id }) => {
                    sink_trace!(
                        self.log_sink,
                        "[SCTP_SENDER] Processing ReceivedAccept for id: {}",
                        id
                    );
                    let props = crate::sctp::events::SctpFileProperties {
                        file_name: "".to_string(),
                        file_size: 0,
                        transaction_id: id,
                    };
                    let stream = SctpStream::new(props);
                    let mut streams = self.streams.write().expect("streams lock poisoned");
                    streams.insert(id, stream);
                }
                Ok(SctpEvents::SendReject { id }) => {
                    sink_trace!(
                        self.log_sink,
                        "[SCTP_SENDER] Processing SendReject for id: {}",
                        id
                    );
                    self.send_message(SctpProtocolMessage::Reject { id }, &mut pending_messages);
                }
                Ok(SctpEvents::SendCancel { id }) => {
                    sink_trace!(
                        self.log_sink,
                        "[SCTP_SENDER] Processing SendCancel for id: {}",
                        id
                    );
                    {
                        let mut streams = self.streams.write().expect("streams lock poisoned");
                        streams.remove(&id);
                    }
                    self.send_message(SctpProtocolMessage::Cancel { id }, &mut pending_messages);
                }
                Ok(SctpEvents::KickSender) => {
                    sink_trace!(
                        self.log_sink,
                        "[SCTP_SENDER] KickSender received, waking up"
                    );
                }
                Ok(SctpEvents::SendChunk { file_id, payload }) => {
                    let start_chunk = Instant::now();
                    let seq = {
                        let mut streams = self.streams.write().expect("streams lock poisoned");
                        if let Some(stream) = streams.get_mut(&file_id) {
                            let s = stream.next_seq;
                            stream.next_seq += 1;
                            stream.update_activity();
                            Some(s)
                        } else {
                            None
                        }
                    };

                    if let Some(s) = seq {
                        sink_trace!(
                            self.log_sink,
                            "[SCTP_SENDER] Sending Chunk seq {} for file_id: {}",
                            s,
                            file_id
                        );
                        let payload_len = payload.len();
                        crate::sctp_log!(
                            self.log_sink,
                            "SendChunk: FileID:{} Seq:{} Size:{}",
                            file_id,
                            s,
                            payload_len
                        );
                        self.send_message(
                            SctpProtocolMessage::Chunk {
                                id: file_id,
                                seq: s,
                                payload,
                            },
                            &mut pending_messages,
                        );
                        sink_debug!(
                            self.log_sink,
                            "[SCTP_SENDER] File bytes sent to SCTP: {}",
                            payload_len
                        );
                        sink_trace!(
                            self.log_sink,
                            "[SCTP_SENDER] Processed SendChunk in {:?}",
                            start_chunk.elapsed()
                        );
                    } else {
                        sink_warn!(
                            self.log_sink,
                            "[SCTP_SENDER] Attempted to send chunk for unknown stream {}",
                            file_id
                        );
                    }
                }
                Ok(SctpEvents::SendEndFile { id }) => {
                    sink_trace!(
                        self.log_sink,
                        "[SCTP_SENDER] Processing SendEndFile for id: {}",
                        id
                    );
                    {
                        let mut streams = self.streams.write().expect("streams lock poisoned");
                        streams.remove(&id);
                    }
                    self.send_message(SctpProtocolMessage::EndFile { id }, &mut pending_messages);
                }
                Ok(SctpEvents::SctpConnected) => {
                    sink_info!(
                        self.log_sink,
                        "[SCTP_SENDER] SCTP Connected, flushing {} pending messages",
                        pending_messages.len()
                    );
                    let messages_to_send = std::mem::take(&mut pending_messages);
                    for msg in messages_to_send {
                        self.send_message(msg, &mut pending_messages);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Timeout expired, poll association below
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
                _ => {}
            }

            // Periodic poll for timeouts and retransmissions
            {
                let mut assoc_guard = self.association.lock().expect("association lock poisoned");
                if let Some(assoc) = assoc_guard.as_mut() {
                    let now = Instant::now();

                    // Handle timeouts
                    if let Some(next_timeout) = assoc.poll_timeout()
                        && now >= next_timeout
                    {
                        assoc.handle_timeout(now);
                    }

                    // Poll transmit
                    let start_poll = Instant::now();
                    while let Some(transmit) = assoc.poll_transmit(now) {
                        if let Payload::RawEncode(bytes_vec) = transmit.payload {
                            for b in bytes_vec {
                                let payload = b.to_vec();
                                crate::sctp_log!(
                                    self.log_sink,
                                    "SCTP_PACKET_OUT: {}",
                                    crate::sctp::debug_utils::parse_sctp_packet_summary(&payload)
                                );
                                let _ = self.tx.send(SctpEvents::TransmitSctpPacket { payload });
                            }
                        }
                    }
                    let elapsed_poll = start_poll.elapsed();
                    if elapsed_poll.as_micros() > 100 {
                        sink_trace!(
                            self.log_sink,
                            "[SCTP_SENDER] poll_transmit took {:?}",
                            elapsed_poll
                        );
                    }
                }
            }
        }
    }

    #[allow(clippy::expect_used)]
    fn ensure_connection(&self) {
        let mut assoc_guard = self.association.lock().expect("association lock poisoned");
        if assoc_guard.is_none() {
            if !self.is_client {
                // If we are server, we wait for incoming connection (handled by Receiver)
                return;
            }
            sink_info!(
                self.log_sink,
                "[SCTP_SENDER] Initiating SCTP association (ensure_connection)..."
            );
            let mut endpoint = self.endpoint.lock().expect("endpoint lock poisoned");
            let remote: SocketAddr = "192.168.1.1:5000"
                .parse()
                .expect("Invalid dummy IP address");
            let mut config = ClientConfig::default();
            match endpoint.connect(config, remote) {
                Ok((handle, assoc)) => {
                    *assoc_guard = Some(assoc);
                    let mut handle_guard = self
                        .association_handle
                        .lock()
                        .expect("association handle lock poisoned");
                    *handle_guard = Some(handle);
                }
                Err(e) => {
                    sink_error!(
                        self.log_sink,
                        "[SCTP_SENDER] Failed to initiate SCTP association: {:?}",
                        e
                    );
                }
            }
        }
    }

    #[allow(clippy::expect_used)]
    fn send_message(&self, msg: SctpProtocolMessage, pending: &mut Vec<SctpProtocolMessage>) {
        let start = Instant::now();
        let payload = match msg.serialize() {
            Ok(p) => p,
            Err(e) => {
                sink_error!(
                    self.log_sink,
                    "[SCTP_SENDER] Failed to serialize SCTP message: {:?}",
                    e
                );
                return;
            }
        };

        self.ensure_connection();

        let mut assoc_guard = self.association.lock().expect("association lock poisoned");
        if let Some(assoc) = assoc_guard.as_mut() {
            // Use Stream 0 for all messages.
            let stream_id = 0;

            let bytes = Bytes::from(payload);

            // Try to get stream, if not, open it
            let stream_handle = match assoc.stream(stream_id) {
                Ok(s) => Ok(s),
                Err(_) => assoc.open_stream(stream_id, PayloadProtocolIdentifier::Binary),
            };

            if let Ok(mut stream) = stream_handle {
                if let Err(e) = stream.write(&bytes) {
                    if e == Error::ErrPayloadDataStateNotExist {
                        sink_info!(
                            self.log_sink,
                            "[SCTP_SENDER] Connection not ready, queuing message"
                        );
                        pending.push(msg);
                    } else {
                        sink_warn!(
                            self.log_sink,
                            "[SCTP_SENDER] Error writing to SCTP stream: {:?}",
                            e
                        );
                    }
                }
            } else {
                sink_warn!(
                    self.log_sink,
                    "[SCTP_SENDER] Failed to get or open SCTP stream {}",
                    stream_id
                );
            }

            // Poll transmit to send the packet
            let now = Instant::now();
            while let Some(transmit) = assoc.poll_transmit(now) {
                if let Payload::RawEncode(bytes_vec) = transmit.payload {
                    for b in bytes_vec {
                        let payload = b.to_vec();
                        sink_debug!(
                            self.log_sink,
                            "[SCTP_SENDER] SCTP bytes sent to DTLS: {}",
                            payload.len()
                        );
                        crate::sctp_log!(
                            self.log_sink,
                            "SCTP_PACKET_OUT: {}",
                            crate::sctp::debug_utils::parse_sctp_packet_summary(&payload)
                        );
                        let _ = self.tx.send(SctpEvents::TransmitSctpPacket { payload });
                    }
                }
            }
        } else {
            sink_warn!(
                self.log_sink,
                "[SCTP_SENDER] Attempted to send message but no SCTP association exists"
            );
            pending.push(msg);
        }
        sink_trace!(
            self.log_sink,
            "[SCTP_SENDER] send_message took {:?}",
            start.elapsed()
        );
    }
}
