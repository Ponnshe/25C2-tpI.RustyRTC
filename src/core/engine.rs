use std::{
    net::SocketAddr,
    sync::Arc,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use crate::{
    app::log_sink::LogSink,
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
    logger_sink: Arc<dyn LogSink>,
    cm: ConnectionManager,
    session: Option<Session>,
    event_tx: Sender<EngineEvent>,
    event_rx: Receiver<EngineEvent>,
    media_agent: MediaAgent,
}

impl Engine {
    pub fn new(logger_sink: Arc<dyn LogSink>) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let media_agent = MediaAgent::new(event_tx.clone(), logger_sink.clone());
        Self {
            cm: ConnectionManager::new(logger_sink.clone()),
            logger_sink,
            session: None,
            event_tx,
            event_rx,
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
                        .event_tx
                        .send(EngineEvent::Error(format!("socket.connect: {e}")));
                } else {
                    let local = sock
                        .local_addr()
                        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
                    let _ = self.event_tx.send(EngineEvent::IceNominated {
                        local,
                        remote: peer,
                    });
                    let sess = Session::new(
                        Arc::clone(&sock),
                        peer,
                        self.cm.remote_codecs().clone(),
                        self.event_tx.clone(),
                        self.logger_sink.clone(),
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
        while let Ok(ev) = self.event_rx.try_recv() {
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
