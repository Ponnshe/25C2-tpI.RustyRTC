use std::{
    net::SocketAddr,
    sync::Arc,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use crate::{
    connection_manager::{ConnectionManager, OutboundSdp, connection_error::ConnectionError},
    media_agent::media_agent::MediaAgent,
};
use crate::{
    core::{
        events::EngineEvent,
        session::{Session, SessionConfig},
    },
    media_agent::video_frame::VideoFrame,
};

pub struct Engine {
    cm: ConnectionManager,
    session: Option<Session>,
    tx_evt: Sender<EngineEvent>,
    rx_evt: Receiver<EngineEvent>,
    media_agent: MediaAgent,
}

impl Engine {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let media_agent = MediaAgent::new(tx.clone());
        Self {
            cm: ConnectionManager::new(),
            session: None,
            tx_evt: tx,
            rx_evt: rx,
            media_agent,
        }
    }

    pub fn negotiate(&mut self) -> Result<Option<String>, ConnectionError> {
        self.cm
            .set_local_rtp_codecs(self.media_agent.codec_descriptors());
        match self.cm.negotiate()? {
            OutboundSdp::Offer(o) => Ok(Some(o.encode())),
            OutboundSdp::Answer(a) => Ok(Some(a.encode())),
            OutboundSdp::None => Ok(None),
        }
    }

    pub fn apply_remote_sdp(
        &mut self,
        remote_sdp: &str,
    ) -> Result<Option<String>, ConnectionError> {
        self.cm
            .set_local_rtp_codecs(self.media_agent.codec_descriptors());
        match self.cm.apply_remote_sdp(remote_sdp)? {
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
                        self.cm.remote_codecs().clone(),
                        self.tx_evt.clone(),
                        SessionConfig {
                            handshake_timeout: Duration::from_secs(10),
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
            self.media_agent
                .handle_engine_event(&ev, self.session.as_ref());
            out.push(ev);
        }
        self.media_agent.tick(self.session.as_ref());
        out
    }

    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        self.media_agent.snapshot_frames()
    }
}
