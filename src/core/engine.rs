use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

use crate::{
    config::Config,
    congestion_controller::CongestionController,
    connection_manager::{ConnectionManager, OutboundSdp, connection_error::ConnectionError},
    core::{
        events::EngineEvent,
        session::{Session, SessionConfig},
    },
    dtls::{self, DtlsRole},
    ice::type_ice::ice_agent::IceRole,
    log::log_sink::LogSink,
    media_agent::video_frame::VideoFrame,
    media_transport::{MediaTransport, media_transport_event::MediaTransportEvent},
    sink_debug, sink_info, sink_trace,
};

use super::constants::{MAX_BITRATE, MIN_BITRATE};
use crate::connection_manager::ice_and_sdp::ICEAndSDP;

pub struct Engine {
    logger_sink: Arc<dyn LogSink>,
    cm: ConnectionManager,
    session: Arc<Mutex<Option<Session>>>,
    event_tx: Sender<EngineEvent>,
    ui_rx: Receiver<EngineEvent>,
    media_transport: MediaTransport,
    congestion_controller: CongestionController,
    config: Arc<Config>,
}

impl Engine {
    pub fn new(logger_sink: Arc<dyn LogSink>, config: Arc<Config>) -> Self {
        let (ui_tx, ui_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let media_transport =
            MediaTransport::new(event_tx.clone(), logger_sink.clone(), config.clone());
        let initial_bitrate = crate::media_agent::constants::BITRATE;
        let max_bitrate = config
            .get("Media", "max_bitrate")
            .and_then(|s| s.parse().ok())
            .unwrap_or(MAX_BITRATE);

        let min_bitrate = config
            .get("Media", "min_bitrate")
            .and_then(|s| s.parse().ok())
            .unwrap_or(MIN_BITRATE);
        let congestion_controller = CongestionController::new(
            initial_bitrate,
            min_bitrate,
            max_bitrate,
            logger_sink.clone(),
            event_tx.clone(),
        );

        let logger = logger_sink.clone();

        let media_tx = media_transport.media_transport_event_tx();
        std::thread::spawn(move || {
            while let Ok(ev) = event_rx.recv() {
                match &ev {
                    EngineEvent::RtpIn(pkt) => {
                        sink_trace!(
                            logger,
                            "[Engine] Sending RTP Packet to MediaTransport::RtpIn"
                        );
                        sink_trace!(logger, "[Engine] ssrc: {} seq: {}", pkt.ssrc, pkt.seq);
                        if let Some(tx) = &media_tx {
                            let _ = tx.send(MediaTransportEvent::RtpIn(pkt.clone()));
                        }
                    }
                    _ => {
                        let _ = ui_tx.send(ev.clone());
                    }
                }
            }
        });

        Self {
            cm: ConnectionManager::new(logger_sink.clone(), config.clone()),
            logger_sink,
            session: Arc::new(Mutex::new(None)),
            event_tx,
            media_transport,
            congestion_controller,
            ui_rx,
            config,
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

    pub fn apply_remote_candidate(&mut self, candidate_line: &str) -> Result<(), ConnectionError> {
        self.cm.apply_remote_trickle_candidate(candidate_line)
    }

    /// Returns local ICE candidates encoded as SDP attribute lines (`candidate:...`).
    pub fn local_candidates_as_sdp_lines(&self) -> Vec<String> {
        self.cm
            .ice_agent
            .local_candidates
            .iter()
            .map(|c| ICEAndSDP::new(c.clone()).to_string())
            .collect()
    }

    #[allow(clippy::expect_used)]
    pub fn start(&mut self) -> Result<(), String> {
        let mut guard = self.session.lock().expect("session lock poisoned");
        if let Some(sess) = guard.as_mut() {
            sess.start();
        } else {
            return Err("no nominated pair yet".into());
        }
        Ok(())
    }

    #[allow(clippy::expect_used)]
    pub fn stop(&mut self) {
        if let Some(sess) = self.session.lock().expect("session lock poisoned").as_mut() {
            sess.request_close();
        }
        self.media_transport.stop();
    }
    #[allow(clippy::expect_used)]
    pub fn close_session(&mut self) {
        let mut guard = self.session.lock().expect("session lock poisoned");
        *guard = None;
        // This ensures cm.ice_agent.get_data_channel_socket() returns Err/None
        // in the next poll() loop, preventing the zombie DTLS handshake.
        self.cm.reset();
        sink_debug!(
            self.logger_sink,
            "[Engine] Session closed and ConnectionManager reset."
        );
    }

    #[allow(clippy::expect_used)]
    pub fn poll(&mut self) -> Vec<EngineEvent> {
        // keep ICE reactive
        self.cm.drain_ice_events();

        if self
            .session
            .lock()
            .expect("session lock poisoned")
            .is_none()
            && let Ok((sock, peer)) = self.cm.ice_agent.get_data_channel_socket()
        {
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

                // Matar al worker de ICE antes de DTLS ---
                // Esto asegura que nadie más esté leyendo del socket.
                self.cm.stop_ice_worker();

                // --- IceRole -> DtlsRole ---
                let dtls_role = match self.cm.ice_agent.role {
                    IceRole::Controlling => DtlsRole::Server,
                    IceRole::Controlled => DtlsRole::Client,
                };

                // Retrieve the remote fingerprint stored in CM
                let remote_fp = self.cm.remote_fingerprint.clone();

                // --- blocking DTLS handshake ---
                let srtp_cfg = match dtls::run_dtls_handshake(
                    Arc::clone(&sock),
                    peer,
                    dtls_role,
                    self.logger_sink.clone(),
                    Duration::from_secs_f32(5.0),
                    remote_fp,
                    self.config.clone(),
                ) {
                    Ok(cfg) => Some(cfg),
                    Err(e) => {
                        let _ = self
                            .event_tx
                            .send(EngineEvent::Error(format!("DTLS handshake failed: {e}")));
                        None // podrías también hacer `continue` para no crear sesión
                    }
                };

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
                    srtp_cfg,
                );
                *self.session.lock().expect("session lock poisoned") = Some(sess);
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
            match self.ui_rx.try_recv() {
                Ok(ev) => match &ev {
                    EngineEvent::NetworkMetrics(m) => {
                        self.congestion_controller.on_network_metrics(m.clone());
                        processed += 1;
                        out.push(EngineEvent::NetworkMetrics(m.clone()));
                    }

                    EngineEvent::UpdateBitrate(br) => {
                        if let Some(media_transport_tx) =
                            self.media_transport.media_transport_event_tx()
                        {
                            let _ =
                                media_transport_tx.send(MediaTransportEvent::UpdateBitrate(*br));
                        }
                        processed += 1;
                        out.push(EngineEvent::UpdateBitrate(*br));
                    }

                    _ => {
                        processed += 1;
                        out.push(ev);
                    }
                },
                Err(_) => break,
            }
        }

        out
    }

    #[must_use]
    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        self.media_transport.snapshot_frames()
    }

    pub fn start_media_transport(&mut self) {
        self.media_transport.start_event_loops(self.session.clone());
        sink_info!(
            self.logger_sink,
            "[Engine] Sending Established Event to Media Transport"
        );
        if let Some(media_transport_event_tx) = self.media_transport.media_transport_event_tx() {
            let _ = media_transport_event_tx.send(MediaTransportEvent::Established);
        }
    }
}
