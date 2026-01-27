use crate::dtls::buffered_udp_channel::BufferedUdpChannel;
use crate::log::log_sink::LogSink;
use crate::sctp::events::SctpEvents;
use crate::sctp::receiver::SctpReceiver;
use crate::sctp::sender::SctpSender;
use crate::sctp::stream::SctpStream;
use crate::sctp::transport::SctpTransport;
use openssl::ssl::SslStream;
use sctp_proto::{Association, AssociationHandle, Endpoint, EndpointConfig, ServerConfig};
use std::collections::HashMap;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

pub struct SctpSession {
    pub tx: Sender<SctpEvents>,
    association: Arc<Mutex<Option<Association>>>,
}

impl SctpSession {
    pub fn new(
        log_sink: Arc<dyn LogSink>,
        parent_tx: Sender<SctpEvents>,
        ssl_stream: SslStream<BufferedUdpChannel>,
        is_client: bool,
    ) -> Self {
        let (tx, rx) = channel();

        // Channels for internal threads
        let (tx_receiver, rx_receiver) = channel();
        let (tx_sender, rx_sender) = channel();
        let (tx_transport, rx_transport) = channel();

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
            association_handle.clone(),
            streams.clone(),
            endpoint.clone(),
            is_client,
        );

        // Transport
        let transport = SctpTransport::new(
            ssl_stream,
            log_sink.clone(),
            tx.clone(), // Transport sends ReadableSctpPacket back to Router via main tx
            rx_transport,
        );

        // Spawn threads
        thread::spawn(move || receiver.run());
        thread::spawn(move || sender.run());
        thread::spawn(move || transport.run());

        // Router Thread (Main)
        let tx_receiver_clone = tx_receiver.clone();
        let tx_sender_clone = tx_sender.clone();
        let tx_transport_clone = tx_transport.clone();

        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                match event {
                    SctpEvents::SctpConnected => {
                        let _ = tx_sender_clone.send(event);
                    }
                    SctpEvents::IncomingSctpPacket { .. } => {
                        let _ = tx_transport_clone.send(event);
                    }
                    SctpEvents::ReadableSctpPacket { .. } => {
                        let _ = tx_receiver_clone.send(event);
                    }
                    SctpEvents::SendOffer { .. }
                    | SctpEvents::SendAccept { .. }
                    | SctpEvents::SendReject { .. }
                    | SctpEvents::SendCancel { .. }
                    | SctpEvents::SendChunk { .. }
                    | SctpEvents::SendEndFile { .. }
                    | SctpEvents::KickSender => {
                        let _ = tx_sender_clone.send(event);
                    }
                    SctpEvents::TransmitSctpPacket { .. } => {
                        let _ = tx_transport_clone.send(event);
                    }
                    SctpEvents::ReceivedAccept { id } => {
                        // Router redirects to Sender
                        let _ = tx_sender_clone.send(SctpEvents::ReceivedAccept { id });
                        // And potentially parent?
                        let _ = parent_tx.send(SctpEvents::ReceivedAccept { id });
                    }
                    SctpEvents::ReceivedOffer { .. }
                    | SctpEvents::ReceivedReject { .. }
                    | SctpEvents::ReceivedCancel { .. }
                    | SctpEvents::ReceivedChunk { .. }
                    | SctpEvents::ReceivedEndFile { .. }
                    | SctpEvents::SctpErr(_) => {
                        // Forward to parent
                        let _ = parent_tx.send(event);
                    }
                    SctpEvents::Shutdown => {
                        break;
                    }
                }
            }
        });

        Self {
            tx,
            association,
        }
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(SctpEvents::Shutdown);
    }

    pub fn handle_sctp_packet(&self, packet: Vec<u8>) {
        let _ = self.tx.send(SctpEvents::IncomingSctpPacket {
            sctp_packet: packet,
        });
    }

    pub fn buffered_amount(&self) -> usize {
        if let Ok(mut guard) = self.association.lock() {
            if let Some(assoc) = guard.as_mut() {
                if let Ok(stream) = assoc.stream(0) {
                    return stream.buffered_amount().unwrap_or(0);
                }
            }
        }
        0
    }
}
