use crate::connection_manager::{ConnectionManager, OutboundSdp};
use eframe::{App, Frame, egui};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnState {
    Idle,
    Connecting,
    Running,
    Stopped,
}

#[derive(Debug)]
pub enum GuiError {
    ConnectionError(String),
}

pub struct RtcApp {
    // Raw text areas for user I/O
    remote_sdp: String,
    local_sdp: String,

    status: String,
    conn_manager: ConnectionManager,

    // Flags for simple gating
    has_remote: bool,
    has_local: bool,
    i_am_offerer: bool, // useful tag for logs
    conn_state: ConnState,

    // logging from background threads
    log_tx: Sender<String>,
    log_rx: Receiver<String>,
    logs: VecDeque<String>,

    // stop signal for threads
    run_flag: Arc<AtomicBool>,
}

impl RtcApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        Self {
            remote_sdp: String::new(),
            local_sdp: String::new(),
            status: "Ready.".into(),
            conn_manager: ConnectionManager::new(),

            has_remote: false,
            has_local: false,
            i_am_offerer: false,
            conn_state: ConnState::Idle,

            log_tx: tx,
            log_rx: rx,
            logs: VecDeque::with_capacity(256),
            run_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create (or re-)negotiate: UI calls into ConnectionManager which decides.
    fn create_or_renegotiate_local_sdp(&mut self) -> Result<(), GuiError> {
        match self
            .conn_manager
            .negotiate()
            .map_err(|e| GuiError::ConnectionError(format!("negotiate: {e}")))?
        {
            OutboundSdp::Offer(offer) => {
                self.local_sdp = offer.encode();
                self.has_local = true;
                self.i_am_offerer = true;
                self.status = "Local OFFER created. Share it with the peer.".into();
            }
            OutboundSdp::Answer(ans) => {
                // Unlikely path for negotiate(); still handle gracefully.
                self.local_sdp = ans.encode();
                self.has_local = true;
                self.i_am_offerer = false;
                self.status = "Local ANSWER created.".into();
            }
            OutboundSdp::None => {
                self.status = "Negotiation already in progress (have-local-offer).".into();
            }
        }
        Ok(())
    }

    /// Paste any remote SDP (offer or answer). Manager decides and returns action.
    fn set_remote_sdp(&mut self, sdp_str: &str) -> Result<(), GuiError> {
        match self
            .conn_manager
            .apply_remote_sdp(sdp_str)
            .map_err(|e| GuiError::ConnectionError(format!("apply_remote_sdp: {e}")))?
        {
            OutboundSdp::Answer(answer) => {
                // We received a remote OFFER while stable; we produced an ANSWER to send back.
                self.local_sdp = answer.encode();
                self.has_local = true;
                self.i_am_offerer = false;
                self.status = "Remote OFFER set → Local ANSWER created. Share it back.".into();
            }
            OutboundSdp::Offer(offer) => {
                // We normally won't produce an offer here, but handle defensively.
                self.local_sdp = offer.encode();
                self.has_local = true;
                self.i_am_offerer = true;
                self.status = "Local OFFER produced after remote SDP (edge case).".into();
            }
            OutboundSdp::None => {
                // This is the typical path when we had a local offer and just received their ANSWER.
                self.status = "Remote ANSWER set.".into();
            }
        }
        self.has_remote = true;
        Ok(())
    }

    fn start_connection(&mut self) -> Result<(), GuiError> {
        if !(self.has_remote && self.has_local) {
            return Err(GuiError::ConnectionError("SDP not complete".into()));
        }
        if !matches!(self.conn_state, ConnState::Idle | ConnState::Stopped) {
            return Ok(()); // already running/connecting
        }

        // ---- Socket plumbing (adapt to your structs) ----
        let local_candidate = self
            .conn_manager
            .ice_agent
            .local_candidates
            .get_mut(0)
            .ok_or_else(|| GuiError::ConnectionError("No local candidate".into()))?;

        let remote_candidate = self
            .conn_manager
            .ice_agent
            .remote_candidates
            .get(0)
            .ok_or_else(|| GuiError::ConnectionError("No remote candidate".into()))?;

        // Move the socket out so threads own it
        let socket = local_candidate
            .socket
            .take()
            .ok_or_else(|| GuiError::ConnectionError("Local socket not initialized".into()))?;

        let remote_addr = remote_candidate.address;

        socket
            .connect(remote_addr)
            .map_err(|e| GuiError::ConnectionError(format!("connect: {e}")))?;

        let local_addr = socket
            .local_addr()
            .map_err(|e| GuiError::ConnectionError(format!("local_addr: {e}")))?;
        let peer_addr = socket
            .peer_addr()
            .map_err(|e| GuiError::ConnectionError(format!("peer_addr: {e}")))?;

        let tag = if self.i_am_offerer {
            "OFFERER"
        } else {
            "ANSWERER"
        };

        let tx = self.log_tx.clone();

        // one clone per thread:
        let run_send = self.run_flag.clone();
        let run_recv = self.run_flag.clone();

        self.run_flag.store(true, Ordering::SeqCst);
        self.conn_state = ConnState::Running;

        // Sender thread (1 msg/sec)
        let send_sock = socket
            .try_clone()
            .map_err(|e| GuiError::ConnectionError(format!("try_clone (send): {e}")))?;
        thread::spawn(move || {
            let _ = tx.send(format!(
                "[INFO] Connected. local={local_addr} peer={peer_addr}"
            ));
            let mut seq: u64 = 0;
            while run_send.load(std::sync::atomic::Ordering::SeqCst) {
                let msg = format!("{tag}:{seq}");
                if let Err(e) = send_sock.send(msg.as_bytes()) {
                    let _ = tx.send(format!("[SEND ERROR] {e}"));
                    break;
                }
                let _ = tx.send(format!("[SEND] {msg}"));
                seq = seq.wrapping_add(1);
                std::thread::sleep(Duration::from_secs(1));
            }
            let _ = tx.send("[INFO] Sender stopped.".into());
        });

        // Receiver thread (blocking recv)
        let tx2 = self.log_tx.clone();

        // optional: let the blocking recv wake up periodically after stop()
        let _ = socket.set_read_timeout(Some(Duration::from_millis(500)));

        std::thread::spawn(move || {
            let mut buf = [0u8; 1500];
            loop {
                if !run_recv.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                match socket.recv(&mut buf) {
                    Ok(n) => {
                        let s = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = tx2.send(format!("[RECV] {s}"));
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        continue;
                    }
                    Err(e) => {
                        let _ = tx2.send(format!("[RECV ERROR] {e}"));
                        break;
                    }
                }
            }
            let _ = tx2.send("[INFO] Receiver stopped.".into());
        });
        Ok(())
    }

    fn stop_connection(&mut self) {
        self.run_flag.store(false, Ordering::SeqCst);
        self.conn_state = ConnState::Stopped;
        self.status = "Connection stopping…".into();
    }
}

impl App for RtcApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        while let Ok(line) = self.log_rx.try_recv() {
            if self.logs.len() == 256 {
                self.logs.pop_front();
            }
            self.logs.push_back(line);
            ctx.request_repaint(); // keep UI lively while messages arrive
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("RoomRTC • SDP Messenger");
                ui.add_space(10.);
            });

            ui.separator();
            ui.label("1) Paste remote SDP (offer or answer):");
            ui.add(
                egui::TextEdit::multiline(&mut self.remote_sdp)
                    .desired_rows(15)
                    .desired_width(f32::INFINITY)
                    .hint_text("Paste remote SDP here…")
                    .lock_focus(true),
            );
            ui.horizontal(|ui| {
                let can_set = !self.remote_sdp.trim().is_empty();

                if ui
                    .add_enabled(can_set, egui::Button::new("Enter SDP message (Set Remote)"))
                    .clicked()
                {
                    let sdp = self.remote_sdp.trim().to_owned();
                    match self.set_remote_sdp(sdp.as_str()) {
                        Ok(_) => self.status = "Remote SDP processed.".to_owned(),
                        Err(e) => self.status = format!("Failed to set remote SDP: {e:?}"),
                    }
                }

                if ui.button("Clear").clicked() {
                    self.remote_sdp.clear();
                }
            });

            ui.separator();
            ui.label("2) Create local SDP and share it (offer/renegotiation):");
            ui.horizontal(|ui| {
                if ui.button("Create SDP message").clicked() {
                    if let Err(e) = self.create_or_renegotiate_local_sdp() {
                        self.status = format!("Failed to create local SDP: {e:?}");
                    } else {
                        self.status = "Local SDP generated.".to_owned();
                    }
                }
                if ui.button("Copy to clipboard").clicked() {
                    ui.output_mut(|o| o.copied_text = self.local_sdp.clone());
                    self.status = "Copied local SDP to clipboard.".to_owned();
                }
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.local_sdp)
                    .desired_rows(15)
                    .desired_width(f32::INFINITY)
                    .hint_text("Your local SDP (Offer/Answer) will appear here…"),
            );

            ui.separator();
            ui.label(&self.status);
            ui.separator();

            let can_start = self.has_remote
                && self.has_local
                && matches!(self.conn_state, ConnState::Idle | ConnState::Stopped);

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_start, egui::Button::new("Start Connection"))
                    .clicked()
                {
                    if let Err(e) = self.start_connection() {
                        self.status = format!("Failed to start: {e:?}");
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
                    for line in &self.logs {
                        ui.monospace(line);
                    }
                });

            ui.separator();
            ui.label(&self.status);
        });
    }
}
