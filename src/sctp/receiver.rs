use crate::log::log_sink::LogSink;
use crate::sctp::events::SctpEvents;
use crate::sctp::stream::SctpStream;
use crate::{sink_error, sink_info, sink_warn};
use sctp_proto::{Association, AssociationHandle, DatagramEvent, Endpoint, Event, Payload, StreamEvent};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use bytes::Bytes;

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

    pub fn run(&self) {
        loop {
             // Determine timeout for sctp
             let timeout = {
                let mut assoc_guard = self.association.lock().unwrap();
                if let Some(assoc) = assoc_guard.as_mut() {
                    assoc.poll_timeout().map(|inst| inst.saturating_duration_since(Instant::now()))
                } else {
                    None
                }
            };
            
            // Wait for event or timeout
            // Use a small timeout if sctp doesn't need immediate attention, to check stream timeouts
            let wait_duration = timeout.unwrap_or(Duration::from_millis(100));
            
            // Cap wait duration to check stream timeouts frequently (e.g. every 1 sec)
            let wait_duration = wait_duration.min(Duration::from_secs(1));

            let event = self.rx.recv_timeout(wait_duration);
            
            match event {
                Ok(SctpEvents::IncomingSctpPacket { sctp_packet }) => {
                    self.handle_packet(sctp_packet);
                },
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Handle SCTP timeout if needed
                     let mut assoc_guard = self.association.lock().unwrap();
                     if let Some(assoc) = assoc_guard.as_mut() {
                         // Check if it was really an SCTP timeout or just our loop cap
                         if let Some(next_timeout) = assoc.poll_timeout() {
                             if Instant::now() >= next_timeout {
                                 assoc.handle_timeout(Instant::now());
                             }
                         }
                     }
                     drop(assoc_guard);
                     self.poll_association();
                },
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
                _ => {}
            }
            
            self.check_stream_timeouts();
        }
    }

    fn check_stream_timeouts(&self) {
        let mut timed_out_ids = Vec::new();
        {
            let streams = self.streams.read().unwrap();
            for (id, stream) in streams.iter() {
                if stream.is_timed_out() {
                    timed_out_ids.push(*id);
                }
            }
        }
        
        for id in timed_out_ids {
            sink_warn!(self.log_sink, "Stream {} timed out, sending Cancel", id);
            let _ = self.tx.send(SctpEvents::SendCancel { id });
        }
    }

    fn handle_packet(&self, packet: Vec<u8>) {
        let mut endpoint = self.endpoint.lock().expect("Failed to lock endpoint");
        let now = Instant::now();
        // Use a dummy address as we are tunneling over DTLS
        let remote: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        
        let bytes = Bytes::from(packet);
        
        match endpoint.handle(now, remote, None, None, bytes) {
            Some((handle, DatagramEvent::NewAssociation(assoc))) => {
                sink_info!(self.log_sink, "New SCTP Association created");
                {
                    let mut my_assoc = self.association.lock().unwrap();
                    *my_assoc = Some(assoc);
                    let mut my_handle = self.association_handle.lock().unwrap();
                    *my_handle = Some(handle);
                }
                
                // Poll the new association immediately
                self.poll_association();
            }
            Some((_handle, DatagramEvent::AssociationEvent(event))) => {
                 let mut my_assoc_guard = self.association.lock().unwrap();
                 if let Some(assoc) = my_assoc_guard.as_mut() {
                     assoc.handle_event(event);
                 } else {
                     sink_warn!(self.log_sink, "Received AssociationEvent but no association exists");
                 }
                 drop(my_assoc_guard); // unlock to poll
                 self.poll_association();
            }
            None => {
                // Packet consumed, no event.
            }
        }
    }
    
    fn poll_association(&self) {
        let mut assoc_guard = self.association.lock().unwrap();
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
                        sink_info!(self.log_sink, "SCTP Association connected");
                    }
                    Event::AssociationLost { reason } => {
                        sink_error!(self.log_sink, "SCTP Association lost: {:?}", reason);
                    }
                    Event::Stream(StreamEvent::Readable { id }) => {
                        // Read from stream
                        if let Ok(mut stream) = assoc.stream(id) {
                            match stream.read_sctp() {
                                Ok(Some(chunks)) => {
                                    let mut buf = vec![0u8; 65535]; 
                                    match chunks.read(&mut buf) {
                                        Ok(len) => {
                                            let data = Bytes::copy_from_slice(&buf[..len]);
                                            self.handle_chunk_data(data);
                                        }
                                        Err(e) => {
                                            sink_warn!(self.log_sink, "Error reading chunks: {:?}", e);
                                        }
                                    }
                                }
                                Ok(None) => {} 
                                Err(e) => {
                                    sink_warn!(self.log_sink, "Error reading from stream {}: {:?}", id, e);
                                }
                            }
                        } else {
                             sink_warn!(self.log_sink, "Stream {} readable but failed to get stream handle", id);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    
    fn handle_chunk_data(&self, data: Bytes) {
         use crate::sctp::protocol::SctpProtocolMessage;
         
         match SctpProtocolMessage::deserialize(&data) {
             Ok(msg) => {
                 match msg {
                     SctpProtocolMessage::Offer { id, filename, file_size } => {
                          let props = crate::sctp::events::SctpFileProperties {
                              file_name: filename,
                              file_size,
                              transaction_id: id,
                          };
                          let _ = self.tx.send(SctpEvents::ReceivedOffer { file_properties: props });
                     }
                     SctpProtocolMessage::Accept { id } => {
                          let _ = self.tx.send(SctpEvents::ReceivedAccept { id });
                     }
                     SctpProtocolMessage::Reject { id } => {
                          let _ = self.tx.send(SctpEvents::ReceivedReject { id });
                     }
                     SctpProtocolMessage::Cancel { id } => {
                          let _ = self.tx.send(SctpEvents::ReceivedCancel { id });
                     }
                     SctpProtocolMessage::Chunk { id, seq, payload } => {
                          {
                              let mut streams = self.streams.write().unwrap();
                              if let Some(stream) = streams.get_mut(&id) {
                                  stream.update_activity();
                              }
                          }
                          let _ = self.tx.send(SctpEvents::ReceivedChunk { id, seq: seq as u32, payload });
                     }
                     SctpProtocolMessage::EndFile { id: _ } => {
                     }
                 }
             }
             Err(e) => {
                 sink_warn!(self.log_sink, "Failed to deserialize SCTP message: {:?}", e);
             }
         }
    }
}
