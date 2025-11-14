use std::{
    net::SocketAddr,
    sync::Arc,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

use crate::{
    app::log_sink::LogSink,
    congestion_controller::congestion_controller::CongestionController,
    connection_manager::{connection_error::ConnectionError, ConnectionManager, OutboundSdp},
    core::{
        events::EngineEvent,
        session::{Session, SessionConfig},
    },
    media_agent::video_frame::VideoFrame,
    media_transport::media_transport::MediaTransport,
};

use super::constants::{MAX_BITRATE, MIN_BITRATE};

pub struct Engine {
    logger_sink: Arc<dyn LogSink>,
    cm: ConnectionManager,
    session: Option<Session>,
    event_tx: Sender<EngineEvent>,
    event_rx: Receiver<EngineEvent>,
    media_transport: MediaTransport,
    congestion_controller: CongestionController,
}

impl Engine {
    pub fn new(logger_sink: Arc<dyn LogSink>) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let media_transport = MediaTransport::new(event_tx.clone(), logger_sink.clone());
        let initial_bitrate = crate::media_agent::constants::BITRATE;
        let congestion_controller = CongestionController::new(
            initial_bitrate,
            MIN_BITRATE,
            MAX_BITRATE,
            logger_sink.clone(),
            event_tx.clone(),
        );
        Self {
            cm: ConnectionManager::new(logger_sink.clone()),
            logger_sink,
            session: None,
            event_tx,
            event_rx,
            media_transport,
            congestion_controller,
        }
    }

    pub fn negotiate(&mut self) -> Result<Option<String>, ConnectionError> {
        self.cm
            .set_local_rtp_codecs(self.media_transport.codec_descriptors());
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
            .set_local_rtp_codecs(self.media_transport.codec_descriptors());
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

        if let (None, Ok((sock, peer))) = (
            self.session.as_ref(),
            self.cm.ice_agent.get_data_channel_socket(),
        ) {
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

        let mut out = Vec::new();
        let start = Instant::now();
        let max_events = 500;
        let max_time = Duration::from_millis(4);

        let mut processed = 0;
        loop {
            if processed >= max_events || start.elapsed() >= max_time {
                break;
            }
            match self.event_rx.try_recv() {
                Ok(ev) => {
                    match &ev {
                        EngineEvent::NetworkMetrics(metrics) => {
                            self.congestion_controller.on_network_metrics(metrics.clone());
                        }
                        EngineEvent::UpdateBitrate(new_bitrate) => {
                            self.media_transport.set_bitrate(*new_bitrate);
                        }
                        _ => {
                            self.media_transport
                                .handle_engine_event(&ev, self.session.as_ref());
                        }
                    }
                    out.push(ev);
                    processed += 1;
                }
                Err(_) => break,
            }
        }

        self.media_transport.tick(self.session.as_ref());
        out
    }

    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        self.media_transport.snapshot_frames()
    }
}
