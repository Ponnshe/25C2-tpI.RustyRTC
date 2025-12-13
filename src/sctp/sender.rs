use crate::log::log_sink::LogSink;
use crate::sctp::events::SctpEvents;
use crate::sctp::protocol::SctpProtocolMessage;
use crate::sctp::stream::SctpStream;
use crate::{sink_error, sink_warn};
use sctp_proto::{Association, Payload};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use bytes::Bytes;

pub struct SctpSender {
    pub log_sink: Arc<dyn LogSink>,
    pub tx: Sender<SctpEvents>,
    pub rx: Receiver<SctpEvents>,
    pub association: Arc<Mutex<Option<Association>>>,
    pub streams: Arc<RwLock<HashMap<u32, SctpStream>>>,
}

impl SctpSender {
    pub fn new(
        log_sink: Arc<dyn LogSink>,
        tx: Sender<SctpEvents>,
        rx: Receiver<SctpEvents>,
        association: Arc<Mutex<Option<Association>>>,
        streams: Arc<RwLock<HashMap<u32, SctpStream>>>,
    ) -> Self {
        Self {
            log_sink,
            tx,
            rx,
            association,
            streams,
        }
    }

    pub fn run(&self) {
        while let Ok(event) = self.rx.recv() {
            match event {
                SctpEvents::SendOffer { file_properties } => {
                    let msg = SctpProtocolMessage::Offer {
                        id: file_properties.transaction_id,
                        filename: file_properties.file_name,
                        file_size: file_properties.file_size,
                    };
                    self.send_message(msg);
                }
                SctpEvents::SendAccept { id } => {
                     // Create Stream
                     {
                         let props = crate::sctp::events::SctpFileProperties {
                             file_name: "".to_string(),
                             file_size: 0,
                             transaction_id: id,
                         };
                         let stream = SctpStream::new(props);
                         let mut streams = self.streams.write().unwrap();
                         streams.insert(id, stream);
                     }
                     self.send_message(SctpProtocolMessage::Accept { id });
                }
                SctpEvents::ReceivedAccept { id } => {
                     let props = crate::sctp::events::SctpFileProperties {
                         file_name: "".to_string(),
                         file_size: 0,
                         transaction_id: id,
                     };
                     let stream = SctpStream::new(props);
                     let mut streams = self.streams.write().unwrap();
                     streams.insert(id, stream);
                }
                SctpEvents::SendReject { id } => {
                     self.send_message(SctpProtocolMessage::Reject { id });
                }
                SctpEvents::SendCancel { id } => {
                     {
                         let mut streams = self.streams.write().unwrap();
                         streams.remove(&id);
                     }
                     self.send_message(SctpProtocolMessage::Cancel { id });
                }
                SctpEvents::SendChunk { file_id, payload } => {
                     let seq = {
                         let mut streams = self.streams.write().unwrap();
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
                         self.send_message(SctpProtocolMessage::Chunk {
                             id: file_id,
                             seq: s,
                             payload,
                         });
                     } else {
                         sink_warn!(self.log_sink, "Attempted to send chunk for unknown stream {}", file_id);
                     }
                }
                _ => {}
            }
        }
    }

    fn send_message(&self, msg: SctpProtocolMessage) {
        let payload = match msg.serialize() {
            Ok(p) => p,
            Err(e) => {
                sink_error!(self.log_sink, "Failed to serialize SCTP message: {:?}", e);
                return;
            }
        };

        let mut assoc_guard = self.association.lock().unwrap();
        if let Some(assoc) = assoc_guard.as_mut() {
            // Use Stream 0 for all messages. 
            let stream_id = 0; 
            
            let bytes = Bytes::from(payload);
            
            if let Ok(mut stream) = assoc.stream(stream_id) {
                 if let Err(e) = stream.write(&bytes) {
                     sink_warn!(self.log_sink, "Error writing to SCTP stream: {:?}", e);
                 }
            } else {
                 sink_warn!(self.log_sink, "Failed to get SCTP stream handle for stream {}", stream_id);
            }
            
            // Poll transmit to send the packet
            let now = Instant::now();
            while let Some(transmit) = assoc.poll_transmit(now) {
                 if let Payload::RawEncode(bytes_vec) = transmit.payload {
                     let mut payload = Vec::new();
                     for b in bytes_vec {
                         payload.extend_from_slice(&b);
                     }
                     let _ = self.tx.send(SctpEvents::TransmitSctpPacket { payload });
                 }
            }
        } else {
            sink_warn!(self.log_sink, "Attempted to send message but no SCTP association exists");
        }
    }
}