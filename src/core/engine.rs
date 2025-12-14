use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    config::Config,
    congestion_controller::CongestionController,
    connection_manager::{connection_error::ConnectionError, ConnectionManager, OutboundSdp},
    core::{
        events::EngineEvent,
        session::{Session, SessionConfig},
    },
    dtls::{self, DtlsRole},
    file_handler::{events::FileHandlerEvents, FileHandler},
    ice::type_ice::ice_agent::IceRole,
    log::log_sink::LogSink,
    media_agent::video_frame::VideoFrame,
    media_transport::{media_transport_event::MediaTransportEvent, MediaTransport},
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
    #[allow(clippy::expect_used)]
    pub fn stop(&mut self) {
        if let Some(sess) = self.session.lock().expect("session lock poisoned").as_mut() {
            sess.request_close();
        }
        self.media_transport.stop();
        // Stop file handler
        if let Ok(mut fh_guard) = self.file_handler.lock() {
            if let Some(fh) = fh_guard.as_mut() {
                // If FileHandler had Arc<AtomicBool> we could update them, but they are in Engine.
                // Reset flags
                self.sending_files.store(false, Ordering::SeqCst);
                self.receiving_files.store(false, Ordering::SeqCst);
                // Shutdown
                Arc::get_mut(fh).map(|f| f.shutdown());
            }
            *fh_guard = None;
        }
    }
    /// Closes the WebRTC session and resets the connection manager.
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
        // Reset file handler
        if let Ok(mut fh) = self.file_handler.lock() {
            *fh = None;
        }
    }

    pub fn send_file(&self, path: String, id: u32) {
        println!("[CLI DEBUG] Engine::send_file called path={} id={}", path, id);
        sink_info!(self.logger_sink, "[Engine] send_file called for path: {} (id: {})", path, id);
        if let Ok(fh_guard) = self.file_handler.lock() {
            if let Some(fh) = fh_guard.as_ref() {
                sink_info!(self.logger_sink, "[Engine] FileHandler found, sending ReadFile event");
                self.sending_files.store(true, Ordering::SeqCst);
                if let Err(e) = fh.send(FileHandlerEvents::ReadFile { path, id }) {
                    sink_error!(self.logger_sink, "[Engine] Failed to send ReadFile event to FileHandler: {}", e);
                }
            } else {
                sink_error!(self.logger_sink, "[Engine] FileHandler is None in send_file!");
            }
        } else {
            sink_error!(self.logger_sink, "[Engine] Failed to lock FileHandler in send_file");
        }
    }

    pub fn accept_file(&self, id: u32, filename: String) {
        if let Ok(sess_guard) = self.session.lock() {
            if let Some(sess) = sess_guard.as_ref() {
                // We are receiving a file
                self.receiving_files.store(true, Ordering::SeqCst);
                sess.send_sctp_event(SctpEvents::SendAccept { id });
            }
        }
        // Notify local FileHandler to start writing
        if let Ok(fh_guard) = self.file_handler.lock() {
            if let Some(fh) = fh_guard.as_ref() {
                let _ = fh.send(FileHandlerEvents::WriteFile { filename, id });
            }
        }
    }

    pub fn reject_file(&self, id: u32) {
        if let Ok(sess_guard) = self.session.lock() {
            if let Some(sess) = sess_guard.as_ref() {
                sess.send_sctp_event(SctpEvents::SendReject { id });
            }
        }
    }

    pub fn cancel_file(&self, id: u32) {
        // Cancel can be local sender cancelling, or local receiver cancelling
        // Notify Session to send Cancel msg
        if let Ok(sess_guard) = self.session.lock() {
            if let Some(sess) = sess_guard.as_ref() {
                sess.send_sctp_event(SctpEvents::SendCancel { id });
            }
        }
        // Also notify local FileHandler to stop
        if let Ok(fh_guard) = self.file_handler.lock() {
            if let Some(fh) = fh_guard.as_ref() {
                let _ = fh.send(FileHandlerEvents::Cancel(id));
            }
        }
    }

    /// Polls for `EngineEvent`s and processes them.
    /// This method is called repeatedly to drive the engine's state.
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
                        let fh_clone = fh.clone();
                        // Interval from config or default
                        let drain_interval_ms = self.config.get("file_handler", "drain_interval_ms").and_then(|s| s.parse().ok()).unwrap_or(100);
                        let drain_interval = Duration::from_millis(drain_interval_ms);

                        // We need a way to stop this thread when session ends?
                        // For now it runs until fh sends error on send?
                        // Or we can rely on Weak ref if we had one.
                        // Or just let it run. If file_handler shuts down, send returns error, loop can break?
                        // But sending_files is false usually.
                        thread::spawn(move || {
                            loop {
                                thread::sleep(drain_interval);
                                // If sending_files is true
                                if sending_files_clone.load(Ordering::SeqCst) {
                                    if let Err(_) = fh_clone.send(FileHandlerEvents::DrainChunks) {
                                        break; // FileHandler dropped/closed
                                    }
                                }
                                // Check if we should exit?
                                // If fh_clone is the only strong ref, we keep it alive.
                                // Engine holds another Strong ref.
                                // If Engine drops, fh drops.
                                // But this thread holds Strong ref.
                                // Circular reference if we are not careful? No.
                            }
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
                            Some(srtp_cfg),
                            ssl_stream,
                        );
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
                            let _ =
                                media_transport_tx.send(MediaTransportEvent::UpdateBitrate(br));
                        }
                        processed += 1;
                        out.push(EngineEvent::UpdateBitrate(br));
                    }

                    EngineEvent::SendFileOffer(props) => {
                        if let Ok(sess_guard) = self.session.lock() {
                            if let Some(sess) = sess_guard.as_ref() {
                                sess.send_sctp_event(SctpEvents::SendOffer { file_properties: props });
                            }
                        }
                    }
                    EngineEvent::SendFileChunk(id, payload) => {
                        if let Ok(sess_guard) = self.session.lock() {
                            if let Some(sess) = sess_guard.as_ref() {
                                sess.send_sctp_event(SctpEvents::SendChunk { file_id: id, payload });
                            }
                        }
                    }
                    EngineEvent::SendFileEnd(id) => {
                        if let Ok(sess_guard) = self.session.lock() {
                            if let Some(sess) = sess_guard.as_ref() {
                                sess.send_sctp_event(SctpEvents::SendEndFile { id });
                            }
                        }
                        // Reset sending flag if no other files? For now simple reset.
                        self.sending_files.store(false, Ordering::SeqCst);
                    }
                    EngineEvent::ReceivedFileChunk(id, _seq, payload) => {
                        // Don't expose to UI, send to FileHandler
                        if let Ok(fh_guard) = self.file_handler.lock() {
                            if let Some(fh) = fh_guard.as_ref() {
                                let _ = fh.send(FileHandlerEvents::WriteChunk { id, payload });
                            }
                        }
                    }
                    EngineEvent::ReceivedFileEnd(id) => {
                         // Explicit end of file from peer
                         // We could notify FileHandler to force close if not already?
                         // Or just treat as success.
                         // But FileHandler already detects empty chunk?
                         // Let's rely on FileHandler's own logic for now, but update flags.
                         self.receiving_files.store(false, Ordering::SeqCst);
                         // Optional: Pass to FileHandlerEvents::RemoteEnd?
                         out.push(EngineEvent::ReceivedFileEnd(id));
                         processed += 1;
                    }
                    EngineEvent::ReceivedFileOffer(props) => {
                         // Prepare file writer?
                         // "Deberá tener soporte para eventos... ReceivedOffer"
                         // We probably want to ask UI first.
                         // So we push to UI.
                         // But we also need to initialize writer when accepted?
                         // RtcApp will call accept_file which sends Accept.
                         // And probably triggers FileHandler::WriteFile?
                         // Yes, RtcApp should call engine.init_download(...) ?
                         // Or Engine handles it?
                         // "Engine deberá tener un FileHandler... Deberá tener soporte para eventos... ReceivedOffer"
                         // I'll forward to UI. UI will call `engine.start_download`?
                         out.push(EngineEvent::ReceivedFileOffer(props));
                         processed += 1;
                    }
                    EngineEvent::ReceivedFileAccept(id) => {
                        // Peer accepted our file. Notify FileHandler to start sending.
                        if let Ok(fh_guard) = self.file_handler.lock() {
                            if let Some(fh) = fh_guard.as_ref() {
                                let _ = fh.send(FileHandlerEvents::RemoteAccepted(id));
                            }
                        }
                        out.push(EngineEvent::ReceivedFileAccept(id));
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
