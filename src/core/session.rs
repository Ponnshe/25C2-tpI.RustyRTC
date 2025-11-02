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
    rtp_session::payload::rtp_payload_chunk::RtpPayloadChunk,
    sink_log,
};

#[derive(Clone, Copy)]
pub struct SessionConfig {
    pub handshake_timeout: Duration,
    pub resend_every: Duration,
    pub close_timeout: Duration,
    pub close_resend_every: Duration,
}

pub struct Session {
    sock: Arc<UdpSocket>,
    peer: net::SocketAddr,
    remote_codecs: Vec<RtpCodec>,

    run_flag: Arc<AtomicBool>,
    established: Arc<AtomicBool>,

    token_local: u64,
    token_peer: Arc<AtomicU64>,

    we_initiated_close: Arc<AtomicBool>,
    peer_initiated_close: Arc<AtomicBool>,
    close_done: Arc<AtomicBool>,

    tx_evt: Sender<EngineEvent>,
    logger: Arc<dyn LogSink>,
    cfg: SessionConfig,

    rtp_session: Arc<Mutex<Option<RtpSession>>>,
    rtp_media_tx: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
}

impl Session {
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
        let mut junk = [0u8; 1500];
        while let Ok(_) = self.sock.recv(&mut junk) {}

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

        // === Receiver (parse+respond) ===
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
                    Ok(n) => match protocol::parse_app_msg(&buf[..n]) {
                        AppMsg::Syn { token: their } => {
                            sink_log!(&logger, LogLevel::Debug, "[HS] recv SYN({their:016x})");
                            rx_tok_peer.store(their, Ordering::SeqCst);
                            let synack = protocol::encode_synack(their, local_token);
                            let _ = rx_sock.send(synack.as_bytes());
                            sink_log!(
                                &logger,
                                LogLevel::Debug,
                                "[HS] send SYN-ACK({their:016x},{local_token:016x})"
                            );
                        }
                        AppMsg::SynAck { your, mine } => {
                            if your == local_token {
                                rx_tok_peer.store(mine, Ordering::SeqCst);
                                let ack = protocol::encode_ack(mine);
                                let _ = rx_sock.send(ack.as_bytes());

                                sink_log!(
                                    &logger,
                                    LogLevel::Debug,
                                    "[HS] recv SYN-ACK ok → send ACK({mine:016x})"
                                );
                            } else {
                                // ignore glare/mismatch quietly to avoid log spam
                            }
                        }
                        AppMsg::Ack { your } => {
                            if your == local_token {
                                rx_est.store(true, Ordering::SeqCst);
                                let _ = tx.send(EngineEvent::Established);
                                sink_log!(&logger, LogLevel::Debug, "[HS] ESTABLISHED");
                            }
                        }
                        AppMsg::Fin { token: their } => {
                            rx_peer_init.store(true, Ordering::SeqCst);
                            rx_est.store(false, Ordering::SeqCst);
                            rx_tok_peer.store(their, Ordering::SeqCst);
                            let finack = protocol::encode_finack(their, local_token);
                            let _ = rx_sock.send(finack.as_bytes());
                            stop_rtp_session(&rtp_session_handle, &rtp_media_tx);
                            sink_log!(
                                &logger,
                                LogLevel::Debug,
                                "[CLOSE] recv FIN({their:016x}) → send FIN-ACK({their:016x},{local_token:016x})"
                            );
                        }
                        AppMsg::FinAck { your, mine } => {
                            let peer_tok_now = rx_tok_peer.load(Ordering::SeqCst);
                            if your == local_token {
                                // they echoed our FIN → finish their side
                                let finack2 = protocol::encode_finack2(mine);
                                let _ = rx_sock.send(finack2.as_bytes());
                                sink_log!(
                                    &logger,
                                    LogLevel::Debug,
                                    "[CLOSE] recv FIN-ACK ok → send FIN-ACK2({mine:016x})"
                                );
                            } else if peer_tok_now != 0 && your == peer_tok_now {
                                // idempotent echo related to their-initiated close; ignore quietly
                            } else {
                                // unrelated; ignore
                            }
                        }
                        AppMsg::FinAck2 { your } => {
                            if your == local_token {
                                rx_est.store(false, Ordering::SeqCst);
                                rx_close_done.store(true, Ordering::SeqCst);
                                let _ = tx.send(EngineEvent::Closing { graceful: true });
                                let _ = tx.send(EngineEvent::Closed);
                                stop_rtp_session(&rtp_session_handle, &rtp_media_tx);
                                sink_log!(
                                    &logger,
                                    LogLevel::Info,
                                    "[CLOSE] graceful close complete",
                                );
                            }
                        }
                        AppMsg::Other(pkt) => {
                            if rx_est.load(Ordering::SeqCst) {
                                let maybe_tx = rtp_media_tx
                                    .lock()
                                    .ok()
                                    .and_then(|guard| guard.as_ref().cloned());
                                if let Some(tx_media) = maybe_tx {
                                    let _ = tx_media.send(pkt);
                                    continue;
                                }
                            }
                        }
                    },
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

        // === Handshake driver ===
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
            let mut last_tx = Instant::now() - cfg.resend_every;

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

    pub fn send_payload(&self, bytes: &[u8]) -> io::Result<usize> {
        if self.established.load(Ordering::SeqCst) {
            self.sock.send(bytes)
        } else {
            Ok(0)
        }
    }

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
            let mut last_tx = Instant::now() - cfg.close_resend_every;

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

    pub fn send_rtp_chunks_for_frame(
        &self,
        handle: &OutboundTrackHandle,
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
        rtp.send_rtp_chunks_for_frame(handle.local_ssrc, chunks, timestamp)
            .map_err(|e| e.to_string())
    }

    fn teardown_rtp(&self) {
        stop_rtp_session(&self.rtp_session, &self.rtp_media_tx);
    }
}

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
