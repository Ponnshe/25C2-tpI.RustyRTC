use std::{
    net::SocketAddr,
    sync::Arc,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use crate::connection_manager::{ConnectionManager, OutboundSdp};
use crate::core::{
    events::EngineEvent,
    session::{Session, SessionConfig},
};

pub struct Engine {
    cm: ConnectionManager,
    session: Option<Session>,
    tx_evt: Sender<EngineEvent>,
    rx_evt: Receiver<EngineEvent>,
}

impl Engine {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            cm: ConnectionManager::new(),
            session: None,
            tx_evt: tx,
            rx_evt: rx,
        }
    }

    pub fn negotiate(
        &mut self,
    ) -> Result<Option<String>, crate::connection_manager::connection_error::ConnectionError> {
        match self.cm.negotiate()? {
            OutboundSdp::Offer(o) => Ok(Some(o.encode())),
            OutboundSdp::Answer(a) => Ok(Some(a.encode())),
            OutboundSdp::None => Ok(None),
        }
    }

    pub fn apply_remote_sdp(
        &mut self,
        s: &str,
    ) -> Result<Option<String>, crate::connection_manager::connection_error::ConnectionError> {
        match self.cm.apply_remote_sdp(s)? {
            OutboundSdp::Answer(a) => Ok(Some(a.encode())),
            OutboundSdp::Offer(o) => Ok(Some(o.encode())),
            OutboundSdp::None => Ok(None),
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.session.is_none() {
            return Err("no nominated pair yet".into());
        }
        if let Some(sess) = &mut self.session {
            sess.start();
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(sess) = &mut self.session {
            sess.request_close();
        }
    }

    pub fn poll(&mut self) -> Vec<EngineEvent> {
        // keep ICE reactive
        self.cm.drain_ice_events();

        // if not yet created a session, try to set one up after nomination
        if self.session.is_none() {
            if let Some((sock, peer)) = self.cm.ice_agent.get_data_channel_socket().ok() {
                // connect, then create session (but do NOT start until UI says so)
                if let Err(e) = sock.connect(peer) {
                    let _ = self
                        .tx_evt
                        .send(EngineEvent::Error(format!("socket.connect: {e}")));
                } else {
                    let local = sock
                        .local_addr()
                        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
                    let _ = self.tx_evt.send(EngineEvent::IceNominated {
                        local,
                        remote: peer,
                    });
                    let sess = Session::new(
                        Arc::clone(&sock),
                        peer,
                        self.tx_evt.clone(),
                        SessionConfig {
                            handshake_timeout: Duration::from_secs(5),
                            resend_every: Duration::from_millis(250),
                            close_timeout: Duration::from_secs(5),
                            close_resend_every: Duration::from_millis(250),
                        },
                    );
                    self.session = Some(sess);
                }
            }
        }

        let mut out = Vec::new();
        while let Ok(ev) = self.rx_evt.try_recv() {
            out.push(ev);
        }
        out
    }
}
