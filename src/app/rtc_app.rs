use super::app_proto::{
    AppMsg, encode_ack, encode_fin, encode_finack, encode_finack2, encode_syn, encode_synack,
    parse_app_msg,
};
use super::{conn_state::ConnState, gui_error::GuiError};
use crate::connection_manager::connection_error::ConnectionError;
use crate::connection_manager::{ConnectionManager, OutboundSdp};
use eframe::{App, Frame, egui};
use rand::{RngCore, rngs::OsRng};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_RESEND_EVERY: Duration = Duration::from_millis(250);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const CLOSE_RESEND_EVERY: Duration = Duration::from_millis(250);

pub struct RtcApp {
    // UI text areas
    remote_sdp_text: String,
    local_sdp_text: String,

    status_line: String,
    conn_manager: ConnectionManager,

    // Negotiation flags
    has_remote_description: bool,
    has_local_description: bool,
    is_local_offerer: bool,
    conn_state: ConnState,

    // UI log plumbing
    ui_log_tx: Sender<String>,
    ui_log_rx: Receiver<String>,
    ui_logs: VecDeque<String>,

    // Shared run flag for all I/O threads
    io_threads_running: Arc<AtomicBool>,

    // App-level handshake state
    // Gate payload until BOTH users press Start AND the token-echo handshake completes
    app_local_start_pressed: Arc<AtomicBool>,
    peer_syn_seen_flag: Arc<AtomicBool>,
    app_payload_allowed: Arc<AtomicBool>,
    app_handshake_failed: Arc<AtomicBool>,

    // Handshake tokens (0 means "unset")
    handshake_token_local: u64,
    handshake_token_remote: Arc<AtomicU64>,

    closing_requested_flag: Arc<AtomicBool>, // user pressed Stop (graceful)
    close_completed_flag: Arc<AtomicBool>,   // FIN handshake done (both sides agree)
    close_failed_flag: Arc<AtomicBool>,      // timeout/failure
}

impl RtcApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        Self {
            remote_sdp_text: String::new(),
            local_sdp_text: String::new(),
            status_line: "Ready.".into(),
            conn_manager: ConnectionManager::new(),

            has_remote_description: false,
            has_local_description: false,
            is_local_offerer: false,
            conn_state: ConnState::Idle,

            ui_log_tx: tx,
            ui_log_rx: rx,
            ui_logs: VecDeque::with_capacity(256),

            io_threads_running: Arc::new(AtomicBool::new(false)),

            app_local_start_pressed: Arc::new(AtomicBool::new(false)),
            peer_syn_seen_flag: Arc::new(AtomicBool::new(false)),
            app_payload_allowed: Arc::new(AtomicBool::new(false)),
            app_handshake_failed: Arc::new(AtomicBool::new(false)),

            handshake_token_local: 0,
            handshake_token_remote: Arc::new(AtomicU64::new(0)),

            closing_requested_flag: Arc::new(AtomicBool::new(false)),
            close_completed_flag: Arc::new(AtomicBool::new(false)),
            close_failed_flag: Arc::new(AtomicBool::new(false)),
        }
    }
    /// Create (or re-)negotiate: UI calls into ConnectionManager which decides.
    fn create_or_renegotiate_local_sdp(&mut self) -> Result<(), GuiError> {
        match self
            .conn_manager
            .negotiate()
            .map_err(|e| GuiError::Connection(format!("negotiate: {e}").into()))?
        {
            OutboundSdp::Offer(offer) => {
                self.local_sdp_text = offer.encode();
                self.has_local_description = true;
                self.is_local_offerer = true;
                self.status_line = "Local OFFER created. Share it with the peer.".into();
            }
            OutboundSdp::Answer(ans) => {
                // Unlikely path for negotiate(); still handle gracefully.
                self.local_sdp_text = ans.encode();
                self.has_local_description = true;
                self.is_local_offerer = false;
                self.status_line = "Local ANSWER created.".into();
            }
            OutboundSdp::None => {
                self.status_line = "Negotiation already in progress (have-local-offer).".into();
            }
        }
        Ok(())
    }

    /// Paste any remote SDP (offer or answer). Manager decides and returns action.
    fn set_remote_sdp(&mut self, sdp_str: &str) -> Result<(), GuiError> {
        match self
            .conn_manager
            .apply_remote_sdp(sdp_str)
            .map_err(|e| GuiError::Connection(format!("apply_remote_sdp: {e}").into()))?
        {
            OutboundSdp::Answer(answer) => {
                // We received a remote OFFER while stable; we produced an ANSWER to send back.
                self.local_sdp_text = answer.encode();
                self.has_local_description = true;
                self.is_local_offerer = false;
                self.status_line = "Remote OFFER set → Local ANSWER created. Share it back.".into();
            }
            OutboundSdp::Offer(offer) => {
                // We normally won't produce an offer here, but handle defensively.
                self.local_sdp_text = offer.encode();
                self.has_local_description = true;
                self.is_local_offerer = true;
                self.status_line = "Local OFFER produced after remote SDP (edge case).".into();
            }
            OutboundSdp::None => {
                // Typical path when we had a local offer and received their ANSWER.
                self.status_line = "Remote ANSWER set.".into();
            }
        }
        self.has_remote_description = true;
        Ok(())
    }

    // ...

    fn start_connection(&mut self) -> Result<(), GuiError> {
        if self.conn_manager.ice_agent.nominated_pair.is_none() {
            self.status_line = "Waiting for ICE nomination…".into();
            return Ok(());
        }

        let (data_socket, peer_addr) = self
            .conn_manager
            .ice_agent
            .get_data_channel_socket()
            .map_err(|e| GuiError::Connection(format!("nominated socket: {e}").into()))?;

        data_socket
            .connect(peer_addr)
            .map_err(|e| GuiError::Connection(format!("socket.connect: {e}").into()))?;

        // === Handshake state (fresh tokens each Start) ===========================
        self.handshake_token_local = OsRng.next_u64();
        self.handshake_token_remote.store(0, Ordering::SeqCst);

        self.app_local_start_pressed.store(true, Ordering::SeqCst);
        self.peer_syn_seen_flag.store(false, Ordering::SeqCst);
        self.app_payload_allowed.store(false, Ordering::SeqCst);
        self.app_handshake_failed.store(false, Ordering::SeqCst);

        // Optional: discard any stale UDP packets sitting in kernel buffers
        {
            let _ = data_socket.set_nonblocking(true);
            let mut junk = [0u8; 1500];
            while let Ok(_n) = data_socket.recv(&mut junk) {}
            let _ = data_socket.set_read_timeout(Some(Duration::from_millis(500)));
        }

        // === Shared state clones =================================================
        let ui_log_tx = self.ui_log_tx.clone();
        let io_threads_running = Arc::clone(&self.io_threads_running);
        let app_payload_allowed = Arc::clone(&self.app_payload_allowed);
        let app_handshake_failed = Arc::clone(&self.app_handshake_failed);

        let peer_token_atomic = Arc::clone(&self.handshake_token_remote);
        let local_token = self.handshake_token_local;

        // Start I/O threads
        self.io_threads_running.store(true, Ordering::SeqCst);
        self.conn_state = ConnState::Running;

        // === Receiver thread: parse & respond (SYN / SYN-ACK / ACK) ==============
        let rx_log = self.ui_log_tx.clone();
        let rx_run = Arc::clone(&self.io_threads_running);
        let rx_sock = Arc::clone(&data_socket);
        let rx_peer_token = Arc::clone(&self.handshake_token_remote);
        let rx_payload_allowed = Arc::clone(&self.app_payload_allowed);
        let rx_syn_seen = Arc::clone(&self.peer_syn_seen_flag);
        let rx_close_done = Arc::clone(&self.close_completed_flag);

        std::thread::spawn(move || {
            let mut buf = [0u8; 1500];
            while rx_run.load(Ordering::SeqCst) {
                match rx_sock.recv(&mut buf) {
                    Ok(n) => match parse_app_msg(&buf[..n]) {
                        AppMsg::Syn { token: their } => {
                            rx_peer_token.store(their, Ordering::SeqCst);
                            rx_syn_seen.store(true, Ordering::SeqCst);
                            let synack = encode_synack(their, local_token);
                            let _ = rx_sock.send(synack.as_bytes());
                            let _ = rx_log.send(format!(
                            "[HS] recv SYN({their:016x}) → send SYN-ACK({their:016x},{local_token:016x})"
                        ));
                        }
                        AppMsg::SynAck { your, mine } => {
                            if your == local_token {
                                rx_peer_token.store(mine, Ordering::SeqCst);
                                let ack = encode_ack(mine);
                                let _ = rx_sock.send(ack.as_bytes());
                                let _ = rx_log.send(format!(
                                "[HS] recv SYN-ACK (your==local ok) mine={mine:016x} → send ACK({mine:016x})"
                            ));
                                // We will mark ESTABLISHED only after we receive ACK(local_token)
                            } else {
                                let _ = rx_log
                                    .send("[HS] recv SYN-ACK (ignored - token mismatch)".into());
                            }
                        }
                        AppMsg::Ack { your } => {
                            if your == local_token {
                                rx_payload_allowed.store(true, Ordering::SeqCst);
                                let _ = rx_log.send("[HS] recv ACK(local) → ESTABLISHED".into());
                            } else {
                                let _ =
                                    rx_log.send("[HS] recv ACK (ignored - token mismatch)".into());
                            }
                        }
                        // === NEW: graceful close ===
                        AppMsg::Fin { token: their } => {
                            // peer requests close; stop sending payload immediately
                            rx_payload_allowed.store(false, Ordering::SeqCst);
                            rx_peer_token.store(their, Ordering::SeqCst);

                            // reply FIN-ACK(their, local_token)
                            let finack = encode_finack(their, local_token);
                            let _ = rx_sock.send(finack.as_bytes());
                            let _ = rx_log.send(format!(
            "[CLOSE] recv FIN({their:016x}) → send FIN-ACK({their:016x},{local_token:016x})"
        ));
                        }
                        AppMsg::FinAck { your, mine } => {
                            // peer echoed our local token? then send FIN-ACK2 to finish their side
                            if your == local_token {
                                let finack2 = encode_finack2(mine); // mine==their local token
                                let _ = rx_sock.send(finack2.as_bytes());
                                let _ = rx_log.send(format!(
                "[CLOSE] recv FIN-ACK(your==local ok) mine={mine:016x} → send FIN-ACK2({mine:016x})"
            ));
                            } else {
                                let _ = rx_log
                                    .send("[CLOSE] recv FIN-ACK (ignored - token mismatch)".into());
                            }
                        }
                        AppMsg::FinAck2 { your } => {
                            // This is the final ACK for our close request
                            if your == local_token {
                                rx_payload_allowed.store(false, Ordering::SeqCst);
                                rx_close_done.store(true, Ordering::SeqCst);
                                // mark graceful close finished
                                // (driver thread will see this and stop io)
                                let _ = rx_log.send(
                                    "[CLOSE] recv FIN-ACK2(local) → graceful close complete".into(),
                                );
                            } else {
                                let _ = rx_log.send(
                                    "[CLOSE] recv FIN-ACK2 (ignored - token mismatch)".into(),
                                );
                            }
                        }

                        // === payload or other ===
                        AppMsg::Other(pkt) => {
                            if rx_payload_allowed.load(Ordering::SeqCst) {
                                let s = String::from_utf8_lossy(&pkt).to_string();
                                let _ = rx_log.send(format!("[RECV] {s}"));
                            } else {
                                let _ =
                                    rx_log.send("[RECV] (ignored - not started/closing)".into());
                            }
                        }
                    },
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        continue;
                    }
                    Err(e) => {
                        let _ = rx_log.send(format!("[RECV ERROR] {e}"));
                        break;
                    }
                }
            }
            let _ = rx_log.send("[INFO] Receiver stopped.".into());
        });

        // === Handshake driver: retransmit until established/timeout ==============
        let hs_sock = Arc::clone(&data_socket);
        std::thread::spawn(move || {
            let _ = ui_log_tx.send(format!("[HS] start (local={local_token:016x})"));
            let started_at = Instant::now();
            let mut last_tx = Instant::now() - HANDSHAKE_RESEND_EVERY;

            while io_threads_running.load(Ordering::SeqCst)
                && !app_payload_allowed.load(Ordering::SeqCst)
            {
                if started_at.elapsed() >= HANDSHAKE_TIMEOUT {
                    let _ = ui_log_tx.send("[HS] timeout (60s). Stopping I/O.".into());
                    app_handshake_failed.store(true, Ordering::SeqCst);
                    io_threads_running.store(false, Ordering::SeqCst);
                    break;
                }

                if last_tx.elapsed() >= HANDSHAKE_RESEND_EVERY {
                    // Always (re)send SYN for this attempt
                    let syn = encode_syn(local_token);
                    let _ = hs_sock.send(syn.as_bytes());
                    let _ = ui_log_tx.send("[HS] send SYN".into());

                    // If we have a peer token, (re)send SYN-ACK and ACK (idempotent)
                    let their = peer_token_atomic.load(Ordering::SeqCst);
                    if their != 0 {
                        let synack = encode_synack(their, local_token);
                        let ack = encode_ack(their);
                        let _ = hs_sock.send(synack.as_bytes());
                        let _ = hs_sock.send(ack.as_bytes());
                        let _ = ui_log_tx.send("[HS] send SYN-ACK + ACK".into());
                    }

                    last_tx = Instant::now();
                }

                std::thread::sleep(Duration::from_millis(40));
            }
            let _ = ui_log_tx.send("[HS] driver done".into());
        });

        // === Sender thread (gated by app_payload_allowed) ========================
        let tx_sendlog = self.ui_log_tx.clone();
        let send_run = Arc::clone(&self.io_threads_running);
        let may_send = Arc::clone(&self.app_payload_allowed);
        let send_sock = Arc::clone(&data_socket);
        let role_tag = if self.is_local_offerer {
            "OFFERER"
        } else {
            "ANSWERER"
        };

        std::thread::spawn(move || {
            let local_addr = send_sock.local_addr();
            let _ = tx_sendlog.send(format!(
                "[INFO] Connected. local={local_addr:?} peer={peer_addr}"
            ));

            let mut seq: u64 = 0;
            while send_run.load(Ordering::SeqCst) {
                if !may_send.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(40));
                    continue;
                }
                let msg = format!("{role_tag}:{seq}");
                if let Err(e) = send_sock.send(msg.as_bytes()) {
                    let _ = tx_sendlog.send(format!("[SEND ERROR] {e}"));
                    break;
                }
                let _ = tx_sendlog.send(format!("[SEND] {msg}"));
                seq = seq.wrapping_add(1);
                std::thread::sleep(Duration::from_secs(1));
            }
            let _ = tx_sendlog.send("[INFO] Sender stopped.".into());
        });

        Ok(())
    }

    fn stop_connection(&mut self) {
        // If there is no nominated socket or we're not running, just hard stop:
        if !self.io_threads_running.load(Ordering::SeqCst) {
            self.conn_state = ConnState::Stopped;
            self.status_line = "Connection stopped.".into();
            return;
        }

        // Gate payload immediately
        self.app_payload_allowed.store(false, Ordering::SeqCst);

        // Flags
        self.closing_requested_flag.store(true, Ordering::SeqCst);
        self.close_completed_flag.store(false, Ordering::SeqCst);
        self.close_failed_flag.store(false, Ordering::SeqCst);

        // We need the nominated socket again to send FIN frames
        let Ok((data_socket, _peer_addr)) = self
            .conn_manager
            .ice_agent
            .get_data_channel_socket()
            .map_err(|e| {
                self.status_line = format!("Stop: {e}");
                ConnectionError::ClosingProt(e)
            })
        else {
            // If we can't get it, force stop
            self.io_threads_running.store(false, Ordering::SeqCst);
            self.conn_state = ConnState::Stopped;
            self.status_line = "Connection stopped (no socket).".into();
            return;
        };

        let ui_log_tx = self.ui_log_tx.clone();
        let io_flag = Arc::clone(&self.io_threads_running);
        let close_done = Arc::clone(&self.close_completed_flag);
        let close_fail = Arc::clone(&self.close_failed_flag);
        let peer_tok = Arc::clone(&self.handshake_token_remote);
        let local_tok = self.handshake_token_local; // token for this session

        // Close driver: FIN / FIN-ACK resend until FIN-ACK2 arrives or timeout
        std::thread::spawn(move || {
            let _ = ui_log_tx.send(format!("[CLOSE] driver start (local={local_tok:016x})"));
            let started_at = Instant::now();
            let mut last_tx = Instant::now() - CLOSE_RESEND_EVERY;

            // keep resending until receiver thread observes FIN-ACK2(local)
            while io_flag.load(Ordering::SeqCst) && !close_done.load(Ordering::SeqCst) {
                if started_at.elapsed() >= CLOSE_TIMEOUT {
                    let _ = ui_log_tx.send("[CLOSE] timeout → forcing stop".into());
                    close_fail.store(true, Ordering::SeqCst);
                    break;
                }

                if last_tx.elapsed() >= CLOSE_RESEND_EVERY {
                    // Always send FIN(local)
                    let fin = encode_fin(local_tok);
                    let _ = data_socket.send(fin.as_bytes());
                    let _ = ui_log_tx.send("[CLOSE] send FIN".into());

                    // If we know peer token (likely true in ESTABLISHED), also send FIN-ACK(peer, local)
                    let their = peer_tok.load(Ordering::SeqCst);
                    if their != 0 {
                        let finack = encode_finack(their, local_tok);
                        let _ = data_socket.send(finack.as_bytes());
                        let _ = ui_log_tx.send("[CLOSE] send FIN-ACK".into());
                    }

                    last_tx = Instant::now();
                }

                std::thread::sleep(Duration::from_millis(40));
            }

            // Stop all loops; update state
            io_flag.store(false, Ordering::SeqCst);
            let _ = ui_log_tx.send("[CLOSE] driver done".into());
        });

        self.conn_state = ConnState::Stopped; // UI state
        self.status_line = "Closing gracefully…".into();
    }
}

impl App for RtcApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        // Keep ICE reactive
        self.conn_manager.drain_ice_events();

        if self.conn_manager.ice_agent.nominated_pair.is_some()
            && matches!(self.conn_state, ConnState::Idle | ConnState::Stopped)
        {
            self.status_line = "ICE nominated a pair. You can Start Connection now.".into();
        }

        if self.app_handshake_failed.load(Ordering::SeqCst) {
            if matches!(self.conn_state, ConnState::Running) {
                self.stop_connection();
            }
            self.status_line = "Handshake timed out (60s). Peer didn’t press Start.".into();
        }

        // Drain UI logs
        while let Ok(line) = self.ui_log_rx.try_recv() {
            if self.ui_logs.len() == 256 {
                self.ui_logs.pop_front();
            }
            self.ui_logs.push_back(line);
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("RoomRTC • SDP Messenger");
                ui.add_space(10.);
            });

            ui.separator();
            ui.label("1) Paste remote SDP (offer or answer):");
            ui.add(
                egui::TextEdit::multiline(&mut self.remote_sdp_text)
                    .desired_rows(15)
                    .desired_width(f32::INFINITY)
                    .hint_text("Paste remote SDP here…")
                    .lock_focus(true),
            );
            ui.horizontal(|ui| {
                let can_set = !self.remote_sdp_text.trim().is_empty();
                if ui
                    .add_enabled(can_set, egui::Button::new("Enter SDP message (Set Remote)"))
                    .clicked()
                {
                    let sdp = self.remote_sdp_text.trim().to_owned();
                    match self.set_remote_sdp(sdp.as_str()) {
                        Ok(_) => self.status_line = "Remote SDP processed.".to_owned(),
                        Err(e) => self.status_line = format!("Failed to set remote SDP: {e:?}"),
                    }
                }
                if ui.button("Clear").clicked() {
                    self.remote_sdp_text.clear();
                }
            });

            ui.separator();
            ui.label("2) Create local SDP and share it (offer/renegotiation):");
            ui.horizontal(|ui| {
                if ui.button("Create SDP message").clicked() {
                    if let Err(e) = self.create_or_renegotiate_local_sdp() {
                        self.status_line = format!("Failed to create local SDP: {e:?}");
                    } else {
                        self.status_line = "Local SDP generated.".to_owned();
                    }
                }
                if ui.button("Copy to clipboard").clicked() {
                    ui.output_mut(|o| o.copied_text = self.local_sdp_text.clone());
                    self.status_line = "Copied local SDP to clipboard.".to_owned();
                }
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.local_sdp_text)
                    .desired_rows(15)
                    .desired_width(f32::INFINITY)
                    .hint_text("Your local SDP (Offer/Answer) will appear here…"),
            );

            ui.separator();
            ui.label(&self.status_line);
            ui.separator();

            let can_start = self.has_remote_description
                && self.has_local_description
                && matches!(self.conn_state, ConnState::Idle | ConnState::Stopped);

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_start, egui::Button::new("Start Connection"))
                    .clicked()
                {
                    if let Err(e) = self.start_connection() {
                        self.status_line = format!("Failed to start: {e:?}");
                    }
                }
                if ui
                    .add_enabled(
                        matches!(self.conn_state, ConnState::Running),
                        egui::Button::new("Stop"),
                    )
                    .clicked()
                {
                    self.stop_connection();
                }
                ui.label(format!("State: {:?}", self.conn_state));
            });

            ui.separator();
            ui.label("Logs:");
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .max_height(180.0)
                .show(ui, |ui| {
                    for line in &self.ui_logs {
                        ui.monospace(line);
                    }
                });

            ui.separator();
            ui.label(&self.status_line);
        });
    }
}
