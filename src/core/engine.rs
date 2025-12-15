//! Core WebRTC Engine module.
//!
//! The [`Engine`] struct is the main entry point for managing a WebRTC session,
//! orchestrating signaling, ICE, DTLS, and media transport.

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    config::Config,
    congestion_controller::CongestionController,
    connection_manager::{ConnectionManager, OutboundSdp, connection_error::ConnectionError},
    core::{
        events::EngineEvent,
        session::{Session, SessionConfig, SessionInitArgs},
    },
    dtls::{self, DtlsRole},
    file_handler::{FileHandler, events::FileHandlerEvents},
    ice::type_ice::ice_agent::IceRole,
    log::log_sink::LogSink,
    media_agent::video_frame::VideoFrame,
    media_transport::{MediaTransport, media_transport_event::MediaTransportEvent},
    sctp::events::SctpEvents,
    sink_debug, sink_error, sink_info, sink_trace,
};

use super::constants::{MAX_BITRATE, MIN_BITRATE};
use crate::connection_manager::ice_and_sdp::ICEAndSDP;

/// The central orchestrator for a WebRTC peer connection.
///
/// Manages ICE, SDP negotiation, DTLS handshake, and media transport.
pub struct Engine {
    logger_sink: Arc<dyn LogSink>,
    cm: ConnectionManager,
    session: Arc<Mutex<Option<Session>>>,
    event_tx: Sender<EngineEvent>,
    ui_rx: Receiver<EngineEvent>,
    media_transport: MediaTransport,
    congestion_controller: CongestionController,
    config: Arc<Config>,
    file_handler: Arc<Mutex<Option<Arc<FileHandler>>>>,
    sending_files: Arc<AtomicBool>,
    receiving_files: Arc<AtomicBool>,
}

impl Engine {
    /// Creates a new `Engine` instance.
    pub fn new(
        logger_sink: Arc<dyn LogSink>,
        config: Arc<Config>,
        sending_files: Arc<AtomicBool>,
        receiving_files: Arc<AtomicBool>,
    ) -> Self {
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
            file_handler: Arc::new(Mutex::new(None)),
            sending_files,
            receiving_files,
        }
    }

    /// Initiates an SDP negotiation as an offerer.
    ///
    /// # Errors
    ///
    /// Returns `ConnectionError` if the negotiation fails.
    pub fn negotiate(&mut self) -> Result<Option<String>, ConnectionError> {
        self.cm
            .set_local_rtp_codecs(self.media_transport.codec_descriptors());
        match self.cm.negotiate()? {
            OutboundSdp::Offer(o) => Ok(Some(o.encode())),
            OutboundSdp::Answer(a) => Ok(Some(a.encode())),
            OutboundSdp::None => Ok(None),
        }
    }

    /// Applies a remote SDP (offer or answer) received from the peer.
    ///
    /// # Errors
    ///
    /// Returns `ConnectionError` if applying the remote SDP fails.
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

    /// Applies a remote ICE candidate.
    ///
    /// # Errors
    ///
    /// Returns `ConnectionError` if applying the candidate fails.
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

    /// Starts the WebRTC session.
    ///
    /// # Errors
    ///
    /// Returns a `String` error if no nominated ICE pair is available.
    ///
    /// # Panics
    ///
    /// Panics if the internal session lock is poisoned.
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

    /// Stops the WebRTC session.
    ///
    /// # Panics
    ///
    /// Panics if the internal session lock is poisoned.
    #[allow(clippy::expect_used)]
    pub fn stop(&mut self) {
        if let Some(sess) = self.session.lock().expect("session lock poisoned").as_mut() {
            sess.request_close();
        }
        self.media_transport.stop();
        // Stop file handler
        if let Ok(mut fh_guard) = self.file_handler.lock() {
            if let Some(fh) = fh_guard.as_ref() {
                // Reset flags
                self.sending_files.store(false, Ordering::SeqCst);
                self.receiving_files.store(false, Ordering::SeqCst);
                // Shutdown
                fh.shutdown();
            }
            *fh_guard = None;
        }
    }
    /// Closes the WebRTC session and resets the connection manager.
    ///
    /// # Panics
    ///
    /// Panics if the internal session lock is poisoned.
    #[allow(clippy::expect_used)]
    pub fn close_session(&mut self) {
        let mut guard = self.session.lock().expect("session lock poisoned");
        *guard = None;
        self.cm.reset();
        sink_debug!(
            self.logger_sink,
            "[Engine] Session closed and ConnectionManager reset."
        );
        // Reset file handler
        if let Ok(mut fh) = self.file_handler.lock() {
            *fh = None;
        }
    }

    pub fn send_file(&self, path: String, id: u32) {
        println!(
            "[CLI DEBUG] Engine::send_file called path={} id={}",
            path, id
        );
        sink_info!(
            self.logger_sink,
            "[Engine] send_file called for path: {} (id: {})",
            path,
            id
        );
        if let Ok(fh_guard) = self.file_handler.lock() {
            if let Some(fh) = fh_guard.as_ref() {
                sink_info!(
                    self.logger_sink,
                    "[Engine] FileHandler found, sending ReadFile event"
                );
                self.sending_files.store(true, Ordering::SeqCst);
                if let Err(e) = fh.send(FileHandlerEvents::ReadFile { path, id }) {
                    sink_error!(
                        self.logger_sink,
                        "[Engine] Failed to send ReadFile event to FileHandler: {}",
                        e
                    );
                }
            } else {
                sink_error!(
                    self.logger_sink,
                    "[Engine] FileHandler is None in send_file!"
                );
            }
        } else {
            sink_error!(
                self.logger_sink,
                "[Engine] Failed to lock FileHandler in send_file"
            );
        }
    }

    pub fn accept_file(&self, id: u32, filename: String) {
        if let Ok(sess_guard) = self.session.lock()
            && let Some(sess) = sess_guard.as_ref()
        {
            // We are receiving a file
            self.receiving_files.store(true, Ordering::SeqCst);
            sess.send_sctp_event(SctpEvents::SendAccept { id });
        }
        // Notify local FileHandler to start writing
        if let Ok(fh_guard) = self.file_handler.lock()
            && let Some(fh) = fh_guard.as_ref()
        {
            let _ = fh.send(FileHandlerEvents::WriteFile { filename, id });
        }
    }

    pub fn reject_file(&self, id: u32) {
        if let Ok(sess_guard) = self.session.lock()
            && let Some(sess) = sess_guard.as_ref()
        {
            sess.send_sctp_event(SctpEvents::SendReject { id });
        }
    }

    pub fn cancel_file(&self, id: u32) {
        // Cancel can be local sender cancelling, or local receiver cancelling
        // Notify Session to send Cancel msg
        if let Ok(sess_guard) = self.session.lock()
            && let Some(sess) = sess_guard.as_ref()
        {
            sess.send_sctp_event(SctpEvents::SendCancel { id });
        }
        // Also notify local FileHandler to stop
        if let Ok(fh_guard) = self.file_handler.lock()
            && let Some(fh) = fh_guard.as_ref()
        {
            let _ = fh.send(FileHandlerEvents::Cancel(id));
        }
    }

    pub fn set_audio_mute(&mut self, mute: bool) {
        self.media_transport.set_audio_mute(mute);
    }

    /// Polls for `EngineEvent`s and processes them.
    /// This method is called repeatedly to drive the engine's state.
    ///
    /// # Panics
    ///
    /// Panics if the internal session lock or file handler lock is poisoned.
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

                self.cm.stop_ice_worker();

                // --- IceRole -> DtlsRole ---
                let dtls_role = match self.cm.ice_agent.role {
                    IceRole::Controlling => DtlsRole::Server,
                    IceRole::Controlled => DtlsRole::Client,
                };

                // Retrieve the remote fingerprint stored in CM
                let remote_fp = self.cm.remote_fingerprint.clone();

                // --- blocking DTLS handshake ---
                // Modified to destructure the tuple
                match dtls::run_dtls_handshake(
                    Arc::clone(&sock),
                    peer,
                    dtls_role,
                    self.logger_sink.clone(),
                    Duration::from_secs_f32(5.0),
                    remote_fp,
                    self.config.clone(),
                ) {
                    Ok((srtp_cfg, ssl_stream)) => {
                        // Create FileHandler
                        let fh = Arc::new(FileHandler::new(
                            self.config.clone(),
                            self.logger_sink.clone(),
                            self.event_tx.clone(),
                        ));
                        *self.file_handler.lock().expect("fh lock") = Some(fh.clone());

                        // Spawn DrainChunks thread
                        let sending_files_clone = self.sending_files.clone();
                        let fh_weak = Arc::downgrade(&fh);
                        // Interval from config or default
                        let drain_interval_ms = self
                            .config
                            .get("file_handler", "drain_interval_ms")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(100);
                        let drain_interval = Duration::from_millis(drain_interval_ms);

                        thread::spawn(move || {
                            loop {
                                thread::sleep(drain_interval);
                                if sending_files_clone.load(Ordering::SeqCst) {
                                    if let Some(fh) = fh_weak.upgrade() {
                                        if fh.send(FileHandlerEvents::DrainChunks).is_err() {
                                            break;
                                        }
                                    } else {
                                        break;
                                    }
                                } else if fh_weak.strong_count() == 0 {
                                    break;
                                }
                            }
                        });

                        let sess = Session::new(SessionInitArgs {
                            sock: Arc::clone(&sock),
                            peer,
                            remote_codecs: self.cm.remote_codecs().clone(),
                            event_tx: self.event_tx.clone(),
                            logger: self.logger_sink.clone(),
                            cfg: SessionConfig {
                                handshake_timeout: Duration::from_secs(10),
                                resend_every: Duration::from_millis(250),
                                close_timeout: Duration::from_secs(5),
                                close_resend_every: Duration::from_millis(250),
                            },
                            srtp_cfg: Some(srtp_cfg),
                            ssl_stream,
                        });
                        *self.session.lock().expect("session lock poisoned") = Some(sess);
                    }
                    Err(e) => {
                        let _ = self
                            .event_tx
                            .send(EngineEvent::Error(format!("DTLS handshake failed: {e}")));
                    }
                };
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
                Ok(ev) => match ev {
                    EngineEvent::NetworkMetrics(m) => {
                        self.congestion_controller.on_network_metrics(m.clone());
                        processed += 1;
                        out.push(EngineEvent::NetworkMetrics(m.clone()));
                    }

                    EngineEvent::UpdateBitrate(br) => {
                        if let Some(media_transport_tx) =
                            self.media_transport.media_transport_event_tx()
                        {
                            let _ = media_transport_tx.send(MediaTransportEvent::UpdateBitrate(br));
                        }
                        processed += 1;
                        out.push(EngineEvent::UpdateBitrate(br));
                    }

                    EngineEvent::SendFileOffer(props) => {
                        if let Ok(sess_guard) = self.session.lock()
                            && let Some(sess) = sess_guard.as_ref()
                        {
                            sess.send_sctp_event(SctpEvents::SendOffer {
                                file_properties: props,
                            });
                        }
                    }
                    EngineEvent::SendFileChunk(id, payload) => {
                        if let Ok(sess_guard) = self.session.lock()
                            && let Some(sess) = sess_guard.as_ref()
                        {
                            sess.send_sctp_event(SctpEvents::SendChunk {
                                file_id: id,
                                payload,
                            });
                        }
                    }
                    EngineEvent::SendFileEnd(id) => {
                        if let Ok(sess_guard) = self.session.lock()
                            && let Some(sess) = sess_guard.as_ref()
                        {
                            sess.send_sctp_event(SctpEvents::SendEndFile { id });
                        }
                        // Reset sending flag if no other files? For now simple reset.
                        self.sending_files.store(false, Ordering::SeqCst);
                    }
                    EngineEvent::ReceivedFileChunk(id, _seq, payload) => {
                        // Don't expose to UI, send to FileHandler
                        if let Ok(fh_guard) = self.file_handler.lock()
                            && let Some(fh) = fh_guard.as_ref()
                        {
                            let _ = fh.send(FileHandlerEvents::WriteChunk { id, payload });
                        }
                    }
                    EngineEvent::ReceivedFileEnd(id) => {
                        self.receiving_files.store(false, Ordering::SeqCst);
                        out.push(EngineEvent::ReceivedFileEnd(id));
                        processed += 1;
                    }
                    EngineEvent::ReceivedFileOffer(props) => {
                        out.push(EngineEvent::ReceivedFileOffer(props));
                        processed += 1;
                    }
                    EngineEvent::ReceivedFileAccept(id) => {
                        // Peer accepted our file. Notify FileHandler to start sending.
                        if let Ok(fh_guard) = self.file_handler.lock()
                            && let Some(fh) = fh_guard.as_ref()
                        {
                            let _ = fh.send(FileHandlerEvents::RemoteAccepted(id));
                        }
                        out.push(EngineEvent::ReceivedFileAccept(id));
                        processed += 1;
                    }
                    EngineEvent::ToggleAudio(mute) => {
                        self.media_transport.set_audio_mute(mute);
                        // We push it out so the UI can update its state if the event came from elsewhere
                        out.push(EngineEvent::ToggleAudio(mute));
                        processed += 1;
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

    /// Returns a snapshot of the local and remote video frames.
    #[must_use]
    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        self.media_transport.snapshot_frames()
    }

    /// Starts the media transport event loops.
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
