use crate::log::log_sink::LogSink;
use crate::sctp::events::SctpEvents;
use crate::sctp::receiver::SctpReceiver;
use crate::sctp::sender::SctpSender;
use crate::sctp::stream::SctpStream;
use sctp_proto::{Association, AssociationHandle, Endpoint, EndpointConfig, ServerConfig};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

pub struct SctpSession {
    pub tx: Sender<SctpEvents>,
}

impl SctpSession {
    pub fn new(log_sink: Arc<dyn LogSink>, parent_tx: Sender<SctpEvents>) -> Self {
        let (tx, rx) = channel();
        
        // Channels for internal threads
        let (tx_receiver, rx_receiver) = channel();
        let (tx_sender, rx_sender) = channel();
        
        // Shared state
        let streams = Arc::new(RwLock::new(HashMap::<u32, SctpStream>::new()));
        let association = Arc::new(Mutex::new(None::<Association>));
        let association_handle = Arc::new(Mutex::new(None::<AssociationHandle>));
        
        // Init Endpoint
        let config = EndpointConfig::default();
        let server_config = ServerConfig::default();
        // Wrap config in Arc as required by Endpoint::new
        let endpoint = Endpoint::new(Arc::new(config), Some(Arc::new(server_config)));
        let endpoint = Arc::new(Mutex::new(endpoint));
        
        // Receiver
        let receiver = SctpReceiver::new(
            log_sink.clone(),
            tx.clone(), // Receiver sends events back to Router via main tx
            rx_receiver,
            streams.clone(),
            endpoint.clone(),
            association.clone(),
            association_handle.clone(),
        );
        
        // Sender
        let sender = SctpSender::new(
            log_sink.clone(),
            tx.clone(), // Sender sends TransmitSctpPacket back to Router via main tx
            rx_sender,
            association.clone(),
            streams.clone(),
        );
        
        // Spawn threads
        thread::spawn(move || receiver.run());
        thread::spawn(move || sender.run());
        
        // Router Thread (Main)
        let tx_receiver_clone = tx_receiver.clone();
        let tx_sender_clone = tx_sender.clone();
        
        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                match event {
                    SctpEvents::IncomingSctpPacket { .. } => {
                        let _ = tx_receiver_clone.send(event);
                    }
                    SctpEvents::SendOffer { .. } | 
                    SctpEvents::SendAccept { .. } |
                    SctpEvents::SendReject { .. } |
                    SctpEvents::SendCancel { .. } |
                    SctpEvents::SendChunk { .. } => {
                        let _ = tx_sender_clone.send(event);
                    }
                    SctpEvents::ReceivedAccept { id } => {
                        // Router redirects to Sender
                        let _ = tx_sender_clone.send(SctpEvents::ReceivedAccept { id });
                        // And potentially parent?
                        let _ = parent_tx.send(SctpEvents::ReceivedAccept { id });
                    }
                    SctpEvents::ReceivedOffer { .. } |
                    SctpEvents::ReceivedReject { .. } |
                    SctpEvents::ReceivedCancel { .. } |
                    SctpEvents::ReceivedChunk { .. } |
                    SctpEvents::SctpErr(_) |
                    SctpEvents::TransmitSctpPacket { .. } => {
                        // Forward to parent
                        let _ = parent_tx.send(event);
                    }
                }
            }
        });
        
        Self { tx }
    }
}
