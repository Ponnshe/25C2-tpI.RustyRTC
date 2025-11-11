use std::{
    net::SocketAddr,
    sync::Arc,
    sync::mpsc::{self, Receiver, Sender},
};

use crate::{
    app::log_sink::LogSink,
    connection_manager::{ConnectionManager, OutboundSdp, connection_error::ConnectionError},
    media_agent::media_agent::MediaAgent,
};
use crate::{
    congestion_controller::congestion_controller::CongestionController,
    core::{
        events::EngineEvent,
        session::{Session, SessionConfig},
    },
    media_agent::video_frame::VideoFrame,
};

use super::constants::{MAX_BITRATE, MIN_BITRATE};

/// The `Engine` is the core component of the WebRTC implementation.
///
/// It orchestrates the entire lifecycle of a WebRTC session, from signaling and
/// ICE negotiation to media transport and session management. The `Engine` integrates
/// several key modules:
///
/// - `ConnectionManager`: Manages the SDP offer/answer exchange and ICE candidate
///   gathering and connectivity checks.
/// - `Session`: Implements the application-level handshake (`SYN`, `SYN-ACK`, `ACK`)
///   and manages the established connection.
/// - `MediaAgent`: Handles media processing, including encoding and decoding video frames.
///
/// The `Engine` communicates with the application's UI layer through a system of
/// `EngineEvent`s, allowing for an event-driven architecture. The `poll` method
/// should be called periodically by the application to drive the engine's state machine.
pub struct Engine {
    logger_sink: Arc<dyn LogSink>,
    cm: ConnectionManager,
    session: Option<Session>,
    event_tx: Sender<EngineEvent>,
    event_rx: Receiver<EngineEvent>,
    media_agent: MediaAgent,
    congestion_controller: CongestionController,
}

impl Engine {
    /// Creates a new `Engine` instance.
    ///
    /// Initializes the `ConnectionManager`, `MediaAgent`, and the event channel
    /// used for internal communication.
    ///
    /// # Arguments
    ///
    /// * `logger_sink` - A thread-safe sink for logging messages.
    pub fn new(logger_sink: Arc<dyn LogSink>) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let media_agent = MediaAgent::new(event_tx.clone(), logger_sink.clone());
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
            media_agent,
            congestion_controller,
        }
    }

    /// Initiates or continues the SDP negotiation process.
    ///
    /// This method drives the `ConnectionManager` to produce an SDP offer or answer.
    /// It ensures that the local RTP codecs from the `MediaAgent` are included in
    /// the negotiation.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(String))` - If an SDP offer or answer was successfully generated.
    /// * `Ok(None)` - If the negotiation is complete and no further SDP message is needed.
    ///
    /// # Errors
    ///
    /// This function will return an error if the underlying `ConnectionManager` fails
    /// to generate or process an SDP message.
    pub fn negotiate(&mut self) -> Result<Option<String>, ConnectionError> {
        self.cm
            .set_local_rtp_codecs(self.media_agent.codec_descriptors());
        match self.cm.negotiate()? {
            OutboundSdp::Offer(o) => Ok(Some(o.encode())),
            OutboundSdp::Answer(a) => Ok(Some(a.encode())),
            OutboundSdp::None => Ok(None),
        }
    }

    /// Applies a remote SDP received from the peer.
    ///
    /// This method passes the peer's SDP to the `ConnectionManager` to advance the
    /// negotiation state. It may result in the generation of a local SDP answer.
    ///
    /// # Arguments
    ///
    /// * `remote_sdp` - The SDP string received from the remote peer.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(String))` - If an SDP answer was generated in response.
    /// * `Ok(None)` - If no SDP response is required.
    ///
    /// # Errors
    ///
    /// This function will return an error if the remote SDP is invalid or an
    /// error occurs while processing it.
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

    /// Starts the application-level session handshake.
    ///
    /// This should be called after ICE negotiation has nominated a candidate pair.
    /// It activates the `Session` to begin the `SYN/ACK` handshake.
    ///
    /// # Errors
    ///
    /// Returns an error if no session is available to start (e.g., ICE not complete).
    pub fn start(&mut self) -> Result<(), String> {
        if self.session.is_none() {
            return Err("no nominated pair yet".into());
        }
        if let Some(sess) = &mut self.session {
            sess.start();
        }
        Ok(())
    }

    /// Requests a graceful shutdown of the session.
    ///
    /// This initiates the `FIN` message sequence to terminate the connection cleanly.
    pub fn stop(&mut self) {
        if let Some(sess) = &mut self.session {
            sess.request_close();
        }
    }

    /// Polls the engine to drive its internal state and process events.
    ///
    /// This method should be called periodically by the application (e.g., on every
    /// UI frame). It performs several key tasks:
    ///
    /// 1. Drains ICE events from the `ConnectionManager`.
    /// 2. Checks for ICE nomination and creates a `Session` if a data channel is ready.
    /// 3. Receives and processes internal `EngineEvent`s from the event channel.
    /// 4. Ticks the `MediaAgent` to handle media processing.
    ///
    /// # Returns
    ///
    /// A `Vec<EngineEvent>` containing all events that occurred during the poll,
    /// which the UI can use to update its state.
    pub fn poll(&mut self) -> Vec<EngineEvent> {
        // keep ICE reactive
        self.cm.drain_ice_events();

        if let (None, Ok((sock, peer))) = (
            self.session.as_ref(),
            self.cm.ice_agent.get_data_channel_socket(),
        ) {
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

        let mut out = Vec::new();

        use std::time::{Duration, Instant};
        let start = Instant::now();
        let max_events = 500; // tune: 200–1000 is typical
        let max_time = Duration::from_millis(4); // or 2–6 ms

        let mut processed = 0;
        loop {
            if processed >= max_events || start.elapsed() >= max_time {
                break;
            }
            match self.event_rx.try_recv() {
                Ok(ev) => {
                    match ev {
                        EngineEvent::NetworkMetrics(metrics) => {
                            self.congestion_controller.on_network_metrics(metrics);
                        }
                        EngineEvent::UpdateBitrate(new_bitrate) => {
                            self.media_agent.set_bitrate(new_bitrate);
                        }
                        _ => {
                            self.media_agent
                                .handle_engine_event(&ev, self.session.as_ref());
                            out.push(ev);
                        }
                    }
                    processed += 1;
                }
                Err(_) => break,
            }
        }

        self.media_agent.tick(self.session.as_ref());
        out
    }

    /// Retrieves the most recent video frames for local and remote feeds.
    ///
    /// This is used by the UI to get the latest video data for rendering.
    ///
    /// # Returns
    ///
    /// A tuple `(Option<VideoFrame>, Option<VideoFrame>)` where the first element
    /// is the local video frame (from the camera) and the second is the remote
    /// video frame (from the peer).
    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        self.media_agent.snapshot_frames()
    }
}
