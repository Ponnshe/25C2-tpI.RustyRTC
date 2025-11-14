use std::{
    io,
    net::{self, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use rand::{RngCore, rngs::OsRng};

use crate::rtp_session::{
    outbound_track_handle::OutboundTrackHandle, rtp_codec::RtpCodec,
    rtp_recv_config::RtpRecvConfig, rtp_session::RtpSession,
};
use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    core::{
        events::EngineEvent,
        protocol::{self, AppMsg},
    },
    media_transport::payload::rtp_payload_chunk::RtpPayloadChunk,
    sink_log,
};

#[derive(Clone, Copy)]
/// Configuration for a `Session`.
pub struct SessionConfig {
    /// The duration after which a handshake attempt will time out.
    pub handshake_timeout: Duration,
    /// The duration after which a message will be resent if no acknowledgment is received.
    pub resend_every: Duration,
    /// The duration after which a close attempt will time out.
    pub close_timeout: Duration,
    /// The duration after which a close message will be resent if no acknowledgment is received.
    pub close_resend_every: Duration,
}

/// Represents a single WebRTC session, managing the handshake, media transport,
/// and session closing.
pub struct Session {
    /// The UDP socket used for communication.
    sock: Arc<UdpSocket>,
    /// The peer's socket address.
    peer: net::SocketAddr,
    /// List of remote RTP codecs.
    pub remote_codecs: Vec<RtpCodec>,

    /// Flag to control the main run loop of the session.
    run_flag: Arc<AtomicBool>,
    /// Flag indicating if the session is established.
    established: Arc<AtomicBool>,

    /// Local session token.
    token_local: u64,
    /// Peer's session token.
    token_peer: Arc<AtomicU64>,

    /// Flag indicating if we initiated the close.
    we_initiated_close: Arc<AtomicBool>,
    /// Flag indicating if the peer initiated the close.
    peer_initiated_close: Arc<AtomicBool>,
    /// Flag indicating if the close process is complete.
    close_done: Arc<AtomicBool>,

    /// Sender for engine events.
    tx_evt: Sender<EngineEvent>,
    /// Logger for the session.
    logger: Arc<dyn LogSink>,
    /// Session configuration.
    cfg: SessionConfig,

    #[allow(clippy::struct_field_names)]
    /// The RTP session for media transport.
    rtp_session: Arc<Mutex<Option<RtpSession>>>,
    /// Sender for RTP media.
    rtp_media_tx: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
}

impl Session {
    /// Creates a new `Session` instance.
    ///
    /// # Arguments
    ///
    /// * `sock` - The UDP socket to use for communication.
    /// * `peer` - The address of the remote peer.
    /// * `remote_codecs` - A list of RTP codecs supported by the remote peer.
    /// * `event_tx` - A sender for `EngineEvent`s to communicate with the engine.
    /// * `logger` - A logger instance for logging session events.
    /// * `cfg` - The session configuration.
    ///
    /// # Returns
    ///
    /// A new `Session` instance.
    pub fn new(
        sock: Arc<UdpSocket>,
        peer: std::net::SocketAddr,
        remote_codecs: Vec<RtpCodec>,
        event_tx: Sender<EngineEvent>,
        logger: Arc<dyn LogSink>,
        cfg: SessionConfig,
    ) -> Self {
        Self {
            sock,
            peer,
            remote_codecs,
            run_flag: Arc::new(AtomicBool::new(false)),
            established: Arc::new(AtomicBool::new(false)),
            token_local: 0,
            token_peer: Arc::new(AtomicU64::new(0)),
            we_initiated_close: Arc::new(AtomicBool::new(false)),
            peer_initiated_close: Arc::new(AtomicBool::new(false)),
            close_done: Arc::new(AtomicBool::new(false)),
            tx_evt: event_tx,
            logger,
            cfg,
            rtp_session: Arc::new(Mutex::new(None)),
            rtp_media_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Starts the session, initiating the handshake process and media transport.
    pub fn start(&mut self) {
        // fresh tokens/flags
        self.token_local = OsRng.next_u64();
        self.token_peer.store(0, Ordering::SeqCst);
        self.established.store(false, Ordering::SeqCst);
        self.we_initiated_close.store(false, Ordering::SeqCst);
        self.peer_initiated_close.store(false, Ordering::SeqCst);
        self.close_done.store(false, Ordering::SeqCst);

        // socket setup & clear any stale data
        let _ = self.sock.set_nonblocking(true);
        let _ = self.sock.set_read_timeout(Some(Duration::from_millis(500)));
        while self.sock.recv(&mut [0; 1500]).is_ok() {}

        self.run_flag.store(true, Ordering::SeqCst);

        // reset RTP plumbing before starting
        self.teardown_rtp();

        let initial_recv: Vec<_> = self
            .remote_codecs
            .clone()
            .into_iter()
            .map(|codec| RtpRecvConfig::new(codec, None))
            .collect();

        let (tx_media, rx_media) = mpsc::channel();
        let rtp_result = RtpSession::new(
            Arc::clone(&self.sock),
            self.peer,
            self.tx_evt.clone(),
            self.logger.clone(),
            rx_media,
            initial_recv,
            Vec::new(),
        )
        .and_then(|mut rtp| {
            if let Err(e) = rtp.start() {
                Err(e)
            } else {
                Ok(rtp)
            }
        });

        match rtp_result {
            Ok(rtp) => {
                if let Ok(mut guard) = self.rtp_session.lock() {
                    *guard = Some(rtp);
                }
                if let Ok(mut guard) = self.rtp_media_tx.lock() {
                    *guard = Some(tx_media.clone());
                }
                sink_log!(&self.logger, LogLevel::Debug, "[RTP] session started");
            }
            Err(e) => {
                sink_log!(
                    &self.logger,
                    LogLevel::Error,
                    "Failed to start RTP session: {e}"
                );
                let _ = self.tx_evt.send(EngineEvent::Error(format!(
                    "Failed to start RTP session: {e}"
                )));
            }
        }

        self.spawn_receiver_thread();
        self.spawn_handshake_driver_thread();
    }

    /// Spawns a thread to receive and process incoming application messages.
    fn spawn_receiver_thread(&self) {
        let rx_run = Arc::clone(&self.run_flag);
        let rx_sock = Arc::clone(&self.sock);
        let rx_tok_peer = Arc::clone(&self.token_peer);
        let rx_est = Arc::clone(&self.established);
        let rx_close_done = Arc::clone(&self.close_done);
        let rx_peer_init = Arc::clone(&self.peer_initiated_close);
        let local_token = self.token_local;
        let tx = self.tx_evt.clone();
        let logger = self.logger.clone();
        let rtp_media_tx = Arc::clone(&self.rtp_media_tx);
        let rtp_session_handle = Arc::clone(&self.rtp_session);

        thread::spawn(move || {
            let mut buf = [0u8; 1500];
            while rx_run.load(Ordering::SeqCst) {
                match rx_sock.recv(&mut buf) {
                    Ok(n) => {
                        let msg = protocol::parse_app_msg(&buf[..n]);
                        let args = HandleAppMsgArgs {
                            msg,
                            rx_sock: &rx_sock,
                            rx_tok_peer: &rx_tok_peer,
                            rx_est: &rx_est,
                            rx_close_done: &rx_close_done,
                            rx_peer_init: &rx_peer_init,
                            local_token,
                            tx: &tx,
                            logger: &logger,
                            rtp_media_tx: &rtp_media_tx,
                            rtp_session_handle: &rtp_session_handle,
                        };
                        handle_app_msg(args);
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => {
                        sink_log!(&logger, LogLevel::Error, "recv error: {e}");
                        let _ = tx.send(EngineEvent::Error(format!("recv error: {e}")));
                        break;
                    }
                }
            }
        });
    }

    /// Spawns a thread to drive the handshake process, sending SYN messages and retransmitting as needed.
    fn spawn_handshake_driver_thread(&self) {
        let hs_run = Arc::clone(&self.run_flag);
        let hs_est = Arc::clone(&self.established);
        let hs_sock = Arc::clone(&self.sock);
        let hs_peer_tok = Arc::clone(&self.token_peer);
        let tx2 = self.tx_evt.clone();
        let logger2 = self.logger.clone();
        let cfg = self.cfg;
        let local_token2 = self.token_local;

        thread::spawn(move || {
            sink_log!(
                &logger2,
                LogLevel::Debug,
                " [HS] start (local={local_token2:016x})"
            );
            let started_at = Instant::now();
            let mut last_tx = Instant::now()
                .checked_sub(cfg.resend_every)
                .unwrap_or_else(Instant::now);

            while hs_run.load(Ordering::SeqCst) && !hs_est.load(Ordering::SeqCst) {
                if started_at.elapsed() >= cfg.handshake_timeout {
                    let _ = tx2.send(EngineEvent::Error("handshake timeout".into()));
                    break;
                }
                if last_tx.elapsed() >= cfg.resend_every {
                    let syn = protocol::encode_syn(local_token2);
                    let _ = hs_sock.send(syn.as_bytes());

                    sink_log!(&logger2, LogLevel::Debug, "[HS] send SYN");

                    let their = hs_peer_tok.load(Ordering::SeqCst);
                    if their != 0 {
                        let synack = protocol::encode_synack(their, local_token2);
                        let ack = protocol::encode_ack(their);
                        let _ = hs_sock.send(synack.as_bytes());
                        let _ = hs_sock.send(ack.as_bytes());

                        sink_log!(&logger2, LogLevel::Debug, "[HS] send SYN-ACK + ACK");
                    }
                    last_tx = Instant::now();
                }
                thread::sleep(Duration::from_millis(40));
            }
            sink_log!(&logger2, LogLevel::Debug, "[HS] driver done");
        });
    }

    /// Sends a raw payload over the UDP socket if the session is established.
    ///
    /// # Arguments
    ///
    /// * `bytes` - The payload to send.
    ///
    /// # Returns
    ///
    /// A `Result` indicating the number of bytes sent or an `io::Error`.
    ///
    /// # Errors
    /// Returns an `Err(io::Error)` if the underlying UDP socket fails to send the data.
    /// In the non-established case the function returns `Ok(0)` (no send attempted).
    pub fn send_payload(&self, bytes: &[u8]) -> io::Result<usize> {
        if self.established.load(Ordering::SeqCst) {
            self.sock.send(bytes)
        } else {
            Ok(0)
        }
    }

    /// Initiates the session closing process.
    pub fn request_close(&mut self) {
        self.we_initiated_close.store(true, Ordering::SeqCst);
        self.established.store(false, Ordering::SeqCst);

        let io_flag = Arc::clone(&self.run_flag);
        let close_done = Arc::clone(&self.close_done);
        let peer_tok = Arc::clone(&self.token_peer);
        let tx = self.tx_evt.clone();
        let logger = self.logger.clone();
        let sock = Arc::clone(&self.sock);
        let cfg = self.cfg;
        let local_tok = self.token_local;

        stop_rtp_session(&self.rtp_session, &self.rtp_media_tx);

        thread::spawn(move || {
            sink_log!(
                &logger,
                LogLevel::Debug,
                "[CLOSE] driver start (local={local_tok:016x})"
            );
            let started_at = Instant::now();
            let mut last_tx = Instant::now()
                .checked_sub(cfg.close_resend_every)
                .unwrap_or_else(Instant::now);

            while io_flag.load(Ordering::SeqCst) && !close_done.load(Ordering::SeqCst) {
                if started_at.elapsed() >= cfg.close_timeout {
                    sink_log!(&logger, LogLevel::Debug, "[CLOSE] timeout → forcing stop");
                    break;
                }
                if last_tx.elapsed() >= cfg.close_resend_every {
                    let fin = protocol::encode_fin(local_tok);
                    let _ = sock.send(fin.as_bytes());

                    sink_log!(&logger, LogLevel::Debug, "[CLOSE] send FIN");
                    let their = peer_tok.load(Ordering::SeqCst);
                    if their != 0 {
                        let finack = protocol::encode_finack(their, local_tok);
                        let _ = sock.send(finack.as_bytes());
                        sink_log!(&logger, LogLevel::Debug, "[CLOSE] send FIN-ACK");
                    }
                    last_tx = Instant::now();
                }
                thread::sleep(Duration::from_millis(40));
            }
            // stop all
            io_flag.store(false, Ordering::SeqCst);
            sink_log!(&logger, LogLevel::Debug, "[CLOSE] driver done");
            let _ = tx.send(EngineEvent::Closed);
        });
    }

    /// # Errors
    /// Returns an error if the rtp session is not running or the lock is poisoned.
    pub fn register_outbound_track(&self, codec: RtpCodec) -> Result<OutboundTrackHandle, String> {
        let guard = self
            .rtp_session
            .lock()
            .map_err(|_| "rtp session lock poisoned".to_string())?;
        let rtp_sesh = guard
            .as_ref()
            .ok_or_else(|| "rtp session not running".to_string())?;
        rtp_sesh
            .register_outbound_track(codec)
            .map_err(|e| e.to_string())
    }

    /// Method for legacy. Should not be used preferably anymore.
    /// # Errors
    /// Returns an error if the rtp session is not running or the lock is poisoned.
    pub fn send_media_frame(
        &self,
        handle: &OutboundTrackHandle,
        payload: &[u8],
    ) -> Result<(), String> {
        let guard = self
            .rtp_session
            .lock()
            .map_err(|_| "rtp session lock poisoned".to_string())?;
        let session = guard
            .as_ref()
            .ok_or_else(|| "rtp session not running".to_string())?;
        session
            .send_frame(handle.local_ssrc, payload)
            .map_err(|e| e.to_string())
    }

    /// Method for legacy. Should not be used preferably anymore.
    /// # Errors
    /// Returns an error if the rtp session is not running or the lock is poisoned.
    pub fn send_rtp_payload(
        &self,
        handle: &OutboundTrackHandle,
        payload: &[u8],
        timestamp: u32,
        marker: bool,
    ) -> Result<(), String> {
        let guard = self
            .rtp_session
            .lock()
            .map_err(|_| "rtp session lock poisoned".to_string())?;
        let rtp = guard
            .as_ref()
            .ok_or_else(|| "rtp session not running".to_string())?;
        rtp.send_rtp_payload(handle.local_ssrc, payload, timestamp, marker)
            .map_err(|e| e.to_string())
    }

    /// # Errors
    /// Returns an error if the rtp session is not running or the lock is poisoned.
    pub fn send_rtp_payloads_for_frame(
        &self,
        handle: &OutboundTrackHandle,
        chunks: &[(&[u8], bool)],
        timestamp: u32,
    ) -> Result<(), String> {
        let guard = self
            .rtp_session
            .lock()
            .map_err(|_| "rtp session lock poisoned".to_string())?;
        let rtp = guard
            .as_ref()
            .ok_or_else(|| "rtp session not running".to_string())?;
        rtp.send_rtp_payloads_for_frame(handle.local_ssrc, chunks, timestamp)
            .map_err(|e| e.to_string())
    }

    /// # Errors
    /// Returns an error if the rtp session is not running or the lock is poisoned.
    pub fn send_rtp_chunks_for_frame(
        &self,
        local_ssrc: u32,
        chunks: &[RtpPayloadChunk],
        timestamp: u32,
    ) -> Result<(), String> {
        let guard = self
            .rtp_session
            .lock()
            .map_err(|_| "rtp session lock poisoned".to_string())?;
        let rtp = guard
            .as_ref()
            .ok_or_else(|| "rtp session not running".to_string())?;
        rtp.send_rtp_chunks_for_frame(local_ssrc, chunks, timestamp)
            .map_err(|e| e.to_string())
    }

    /// Tears down the RTP session.
    fn teardown_rtp(&self) {
        stop_rtp_session(&self.rtp_session, &self.rtp_media_tx);
    }
}

/// Helper struct to pass arguments to the `handle_app_msg` function.
struct HandleAppMsgArgs<'a> {
    /// The application message to handle.
    msg: AppMsg,
    /// The UDP socket for sending responses.
    rx_sock: &'a Arc<UdpSocket>,
    /// The peer's token.
    rx_tok_peer: &'a Arc<AtomicU64>,
    /// Flag indicating if the session is established.
    rx_est: &'a Arc<AtomicBool>,
    /// Flag indicating if the close process is done.
    rx_close_done: &'a Arc<AtomicBool>,
    /// Flag indicating if the peer initiated the close.
    rx_peer_init: &'a Arc<AtomicBool>,
    /// The local token.
    local_token: u64,
    /// Sender for engine events.
    tx: &'a Sender<EngineEvent>,
    /// Logger for the session.
    logger: &'a Arc<dyn LogSink>,
    /// Sender for RTP media.
    rtp_media_tx: &'a Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    /// Handle to the RTP session.
    rtp_session_handle: &'a Arc<Mutex<Option<RtpSession>>>,
}

/// Handles incoming application messages.
fn handle_app_msg(args: HandleAppMsgArgs) {
    match args.msg {
        AppMsg::Syn { token: their } => {
            sink_log!(args.logger, LogLevel::Debug, "[HS] recv SYN({their:016x})");
            args.rx_tok_peer.store(their, Ordering::SeqCst);
            let synack = protocol::encode_synack(their, args.local_token);
            let _ = args.rx_sock.send(synack.as_bytes());
            sink_log!(
                args.logger,
                LogLevel::Debug,
                "[HS] send SYN-ACK({their:016x},{local_token:016x})",
                local_token = args.local_token
            );
        }
        AppMsg::SynAck { your, mine } => {
            if your == args.local_token {
                args.rx_tok_peer.store(mine, Ordering::SeqCst);
                let ack = protocol::encode_ack(mine);
                let _ = args.rx_sock.send(ack.as_bytes());

                sink_log!(
                    args.logger,
                    LogLevel::Debug,
                    "[HS] recv SYN-ACK ok → send ACK({mine:016x})"
                );
            } else {
                // ignore glare/mismatch quietly to avoid log spam
            }
        }
        AppMsg::Ack { your } => {
            if your == args.local_token {
                args.rx_est.store(true, Ordering::SeqCst);
                let _ = args.tx.send(EngineEvent::Established);
                sink_log!(args.logger, LogLevel::Debug, "[HS] ESTABLISHED");
            }
        }
        AppMsg::Fin { token: their } => {
            args.rx_peer_init.store(true, Ordering::SeqCst);
            args.rx_est.store(false, Ordering::SeqCst);
            args.rx_tok_peer.store(their, Ordering::SeqCst);
            let finack = protocol::encode_finack(their, args.local_token);
            let _ = args.rx_sock.send(finack.as_bytes());
            stop_rtp_session(args.rtp_session_handle, args.rtp_media_tx);
            sink_log!(
                args.logger,
                LogLevel::Debug,
                "[CLOSE] recv FIN({their:016x}) → send FIN-ACK({their:016x},{local_token:016x})",
                local_token = args.local_token
            );
        }
        AppMsg::FinAck { your, mine } => {
            let peer_tok_now = args.rx_tok_peer.load(Ordering::SeqCst);
            if your == args.local_token {
                // they echoed our FIN → finish their side
                let finack2 = protocol::encode_finack2(mine);
                let _ = args.rx_sock.send(finack2.as_bytes());
                sink_log!(
                    args.logger,
                    LogLevel::Debug,
                    "[CLOSE] recv FIN-ACK ok → send FIN-ACK2({mine:016x})"
                );
            } else if peer_tok_now != 0 && your == peer_tok_now {
                // idempotent echo related to their-initiated close; ignore quietly
            }
        }
        AppMsg::FinAck2 { your } => {
            if your == args.local_token {
                args.rx_est.store(false, Ordering::SeqCst);
                args.rx_close_done.store(true, Ordering::SeqCst);
                let _ = args.tx.send(EngineEvent::Closing { graceful: true });
                let _ = args.tx.send(EngineEvent::Closed);
                stop_rtp_session(args.rtp_session_handle, args.rtp_media_tx);
                sink_log!(
                    args.logger,
                    LogLevel::Info,
                    "[CLOSE] graceful close complete",
                );
            }
        }
        AppMsg::Other(pkt) => {
            if args.rx_est.load(Ordering::SeqCst) {
                let maybe_tx = args
                    .rtp_media_tx
                    .lock()
                    .ok()
                    .and_then(|guard| guard.as_ref().cloned());
                if let Some(tx_media) = maybe_tx {
                    let _ = tx_media.send(pkt);
                }
            }
        }
    }
}

/// Stops the RTP session and clears the media sender.
fn stop_rtp_session(
    rtp_session: &Arc<Mutex<Option<RtpSession>>>,
    rtp_media_tx: &Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
) {
    if let Ok(mut guard) = rtp_session.lock() {
        if let Some(rtp) = guard.as_ref() {
            rtp.stop();
        }
        guard.take();
    }
    if let Ok(mut guard) = rtp_media_tx.lock() {
        guard.take();
    }
}
