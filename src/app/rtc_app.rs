use super::{
    conn_state::ConnState, gpu_yuv_renderer::GpuYuvRenderer, gui_error::GuiError,
    utils::show_camera_in_ui,
};
use crate::{
    core::{
        engine::Engine,
        events::EngineEvent::{
            self, Closed, Closing, Error, Established, IceNominated, Log, RtpIn, Status,
        },
    },
    log::{log_level::LogLevel, log_sink::LogSink, logger::Logger},
    media_agent::video_frame::{VideoFrame, VideoFrameData},
    signaling::protocol::SignalingMsg,
    signaling_client::{SignalingClient, SignalingEvent},
    sink_debug,
};
use eframe::{App, Frame, egui, egui_wgpu::RenderState};
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, mpsc::TrySendError},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalingScreen {
    Connect,
    Login,
    Home,
}

#[derive(Debug, Clone)]
enum CallFlow {
    Idle,
    Dialing {
        peer: String,
        txn_id: u64,
    },
    Incoming {
        from: String,
        txn_id: u64,
        sdp: String,
    },
    Active {
        peer: String,
    },
}

pub struct RtcApp {
    // UI text areas
    remote_sdp_text: String,
    local_sdp_text: String,
    pending_remote_sdp: Option<String>,

    status_line: String,

    // orchestrator
    engine: Engine,

    // JSEP state
    has_remote_description: bool,
    has_local_description: bool,
    is_local_offerer: bool,
    conn_state: ConnState,

    // UI log
    logger: Logger,
    ui_logs: VecDeque<String>,
    bg_dropped: usize,

    // RTP summaries
    rtp_pkts: u64,
    rtp_bytes: u64,
    rtp_last_report: Instant,

    //Siganling setup
    signaling_client: Option<SignalingClient>,
    signaling_screen: SignalingScreen,
    server_addr_input: String,
    login_username: String,
    login_password: String,
    register_username: String,
    register_password: String,
    peers_online: Vec<String>,
    current_username: Option<String>,
    signaling_error: Option<String>,
    call_flow: CallFlow,
    next_txn_id: u64,

    // Renderers and textures
    local_camera_texture: Option<(egui::TextureId, (u32, u32))>,
    remote_camera_texture: Option<(egui::TextureId, (u32, u32))>,

    local_yuv_renderer: Option<GpuYuvRenderer>,
    remote_yuv_renderer: Option<GpuYuvRenderer>,
}

impl RtcApp {
    const HEADER_TITLE: &str = "RoomRTC • SDP Messenger";
    const CAMERAS_WINDOW_WIDTH: f32 = 800.0;
    const CAMERAS_WINDOW_HEIGHT: f32 = 400.0;
    const LOCAL_CAMERA_SIZE: f32 = 400.0;
    const REMOTE_CAMERA_SIZE: f32 = 400.0;

    const SIGNALING_SERVER_ADDR: &str = "192.168.0.12:6000";
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let logger = Logger::start_default("roomrtc", 4096, 256, 50);
        let logger_handle = Arc::new(logger.handle());

        let (local_yuv_renderer, remote_yuv_renderer) =
            if let Some(render_state) = cc.wgpu_render_state.as_ref() {
                let local = GpuYuvRenderer::new(
                    &render_state.device,
                    render_state.target_format,
                    logger_handle.clone(),
                );
                let remote = GpuYuvRenderer::new(
                    &render_state.device,
                    render_state.target_format,
                    logger_handle.clone(),
                );
                (Some(local), Some(remote))
            } else {
                (None, None)
            };

        Self {
            remote_sdp_text: String::new(),
            local_sdp_text: String::new(),
            pending_remote_sdp: None,
            status_line: "Ready.".into(),
            engine: Engine::new(logger_handle),
            has_remote_description: false,
            has_local_description: false,
            is_local_offerer: false,
            conn_state: ConnState::Idle,
            logger,
            ui_logs: VecDeque::with_capacity(256),
            bg_dropped: 0,
            rtp_pkts: 0,
            rtp_bytes: 0,
            rtp_last_report: Instant::now(),
            local_camera_texture: None,
            remote_camera_texture: None,
            signaling_client: None,
            signaling_screen: SignalingScreen::Connect,
            server_addr_input: Self::SIGNALING_SERVER_ADDR.into(),
            login_username: String::new(),
            login_password: String::new(),
            register_username: String::new(),
            register_password: String::new(),
            peers_online: Vec::new(),
            current_username: None,
            signaling_error: None,
            call_flow: CallFlow::Idle,
            next_txn_id: 1,
            local_yuv_renderer,
            remote_yuv_renderer,
        }
    }

    fn push_ui_log<T: Into<String>>(&mut self, s: T) {
        // Only keep a small tail in the UI
        if self.ui_logs.len() == 256 {
            self.ui_logs.pop_front();
        }
        self.ui_logs.push_back(s.into());
    }

    fn background_log<L: Into<String>>(&mut self, level: LogLevel, text: L) {
        let target: &'static str = module_path!();
        match self.logger.try_log(level, text, target) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                self.bg_dropped += 1;
                if self.bg_dropped.is_multiple_of(100) {
                    self.push_ui_log(format!(
                        "(logger) dropped {} background log lines",
                        self.bg_dropped
                    ));
                }
            }
        }
    }

    fn drain_ui_log_tap(&mut self) {
        while let Some(line) = self.logger.try_recv_ui() {
            self.push_ui_log(line);
        }
    }
    fn summarize_frame(frame: Option<&VideoFrame>) -> String {
        match frame {
            Some(f) => {
                let (w, h) = (f.width, f.height);
                let format = f.format;
                let bytes = match &f.data {
                    VideoFrameData::Rgb(d) => d.len(),
                    VideoFrameData::Yuv420 { y, u, v, .. } => y.len() + u.len() + v.len(),
                };
                if w > 0 && h > 0 {
                    format!("{w}x{h} ({format:?}) • {bytes} bytes")
                } else {
                    format!("{bytes} bytes (pending decode)")
                }
            }
            None => "no frame".into(),
        }
    }

    fn connect_to_signaling(&mut self) {
        let log_sink = Arc::new(self.logger.handle());

        // Trim and basic sanity check
        let addr = self.server_addr_input.trim();
        if addr.is_empty() {
            let msg = "Please enter a signaling server address (host:port)".to_string();
            self.signaling_error = Some(msg.clone());
            self.push_ui_log(msg);
            return;
        }

        // TLS SNI / certificate name.
        // This MUST match the mkcert-generated certificate (signal.internal).
        // We keep it fixed for now, even if the user types 127.0.0.1:6000.
        let domain = "signal.internal";

        // Build TLS config + connect over TLS, handling errors explicitly (no `?`).
        let res: io::Result<SignalingClient> =
            SignalingClient::default_tls_config().and_then(|tls_cfg| {
                // `addr` is "host:port", `domain` is the bare host for SNI
                SignalingClient::connect_tls(addr, domain, tls_cfg, log_sink.clone())
            });

        match res {
            Ok(client) => {
                self.signaling_client = Some(client);
                self.signaling_screen = SignalingScreen::Login;
                self.signaling_error = None;
                self.status_line = format!("Connecting to {}…", addr);
            }
            Err(e) => {
                let msg = format!("Failed to connect to signaling server: {e}");
                self.signaling_error = Some(msg.clone());
                self.push_ui_log(msg);
            }
        }
    }

    fn disconnect_from_signaling(&mut self) {
        if let Some(client) = &self.signaling_client {
            client.disconnect();
        }
        self.clear_signaling_state();
        self.status_line = "Disconnected from signaling server.".into();
    }

    fn clear_signaling_state(&mut self) {
        self.signaling_client = None;
        self.signaling_screen = SignalingScreen::Connect;
        self.current_username = None;
        self.peers_online.clear();
        self.call_flow = CallFlow::Idle;
    }

    fn poll_signaling_events(&mut self) {
        if self.signaling_client.is_none() {
            return;
        }
        let mut events = Vec::new();
        if let Some(client) = self.signaling_client.as_ref() {
            while let Some(ev) = client.try_recv() {
                events.push(ev);
            }
        }
        for ev in events {
            self.handle_signaling_event(ev);
        }
    }

    fn handle_signaling_event(&mut self, event: SignalingEvent) {
        match event {
            SignalingEvent::Connected => {
                self.status_line = "Connected to signaling server.".into();
            }
            SignalingEvent::Disconnected => {
                self.push_ui_log("Signaling server disconnected.");
                self.clear_signaling_state();
            }
            SignalingEvent::Error(err) => {
                self.signaling_error = Some(err.clone());
                self.push_ui_log(format!("Signaling error: {err}"));
            }
            SignalingEvent::ServerMsg(msg) => self.handle_signaling_server_msg(msg),
        }
    }

    fn handle_signaling_server_msg(&mut self, msg: SignalingMsg) {
        match msg {
            SignalingMsg::LoginOk { username } => {
                self.current_username = Some(username.clone());
                self.signaling_screen = SignalingScreen::Home;
                self.status_line = format!("Logged in as {username}");
                self.login_password.clear();
                self.request_peer_list();
            }
            SignalingMsg::LoginErr { code } => {
                let msg = format!("Login failed with code {}", code);
                self.signaling_error = Some(msg.clone());
                self.push_ui_log(msg);
            }
            SignalingMsg::RegisterOk { username } => {
                self.status_line = format!("Registered {username}. You can now log in.");
                self.login_username = username;
            }
            SignalingMsg::RegisterErr { code } => {
                let msg = format!("Registration failed with code {}", code);
                self.signaling_error = Some(msg.clone());
                self.push_ui_log(msg);
            }
            SignalingMsg::PeersOnline { peers } => {
                self.peers_online = peers;
            }
            SignalingMsg::Offer {
                from, txn_id, sdp, ..
            } => match String::from_utf8(sdp) {
                Ok(body) => {
                    self.remote_sdp_text = body.clone();
                    self.call_flow = CallFlow::Incoming {
                        from: from.clone(),
                        txn_id,
                        sdp: body,
                    };
                    self.status_line = format!("Incoming call from {from}");
                    let _ = self.send_signaling(SignalingMsg::Ack {
                        from: self.current_username.clone().unwrap_or_default(),
                        to: from.clone(),
                        txn_id,
                    });
                }
                Err(e) => {
                    self.push_ui_log(format!("Invalid SDP from {from}: {e}"));
                }
            },
            SignalingMsg::Answer {
                from, txn_id, sdp, ..
            } => match String::from_utf8(sdp) {
                Ok(body) => {
                    self.remote_sdp_text = body.clone();
                    self.pending_remote_sdp = Some(body);
                    self.call_flow = CallFlow::Active { peer: from.clone() };
                    self.status_line = format!("Received answer from {from}");
                    // Acknowledge receipt so the sender can stop retries if they add reliability.
                    let _ = self.send_signaling(SignalingMsg::Ack {
                        from: self.current_username.clone().unwrap_or_default(),
                        to: from.clone(),
                        txn_id,
                    });
                }
                Err(e) => self.push_ui_log(format!("Invalid answer from {from}: {e}")),
            },
            SignalingMsg::Candidate { from, cand, .. } => match String::from_utf8(cand) {
                Ok(line) => match self.engine.apply_remote_candidate(&line) {
                    Ok(()) => {
                        self.push_ui_log(format!("Applied ICE candidate from {from}"));
                    }
                    Err(e) => {
                        let msg = format!("Failed to apply ICE candidate from {from}: {e}");
                        self.signaling_error = Some(msg.clone());
                        self.push_ui_log(msg);
                    }
                },
                Err(e) => {
                    self.push_ui_log(format!("Invalid ICE candidate from {from}: {e}"));
                }
            },
            SignalingMsg::Ping { nonce } => {
                let _ = self.send_signaling(SignalingMsg::Pong { nonce });
            }
            SignalingMsg::Bye { from, reason, .. } => {
                self.push_ui_log(format!("Peer {from} ended call: {:?}", reason));
                // Remote already sent BYE; don't echo it back.
                self.teardown_call(reason, false);
            }
            SignalingMsg::Ack { txn_id, from, .. } => {
                self.push_ui_log(format!("Received ACK from {from} for txn_id={txn_id}"));
            }
            other => {
                self.background_log(
                    LogLevel::Debug,
                    format!("Unhandled signaling message: {:?}", other),
                );
            }
        }
    }

    fn request_peer_list(&mut self) {
        let _ = self.send_signaling(SignalingMsg::ListPeers);
    }

    fn send_signaling(&mut self, msg: SignalingMsg) -> Result<(), ()> {
        if let Some(client) = self.signaling_client.as_ref() {
            if let Err(e) = client.send(msg) {
                let err = format!("Failed to send signaling message: {e}");
                self.signaling_error = Some(err.clone());
                self.push_ui_log(err);
                return Err(());
            }
            Ok(())
        } else {
            let err = "Not connected to signaling server.".to_string();
            self.signaling_error = Some(err.clone());
            self.push_ui_log(err);
            Err(())
        }
    }

    fn send_local_candidates(&mut self, peer: &str) {
        let Some(user) = self.current_username.clone() else {
            self.signaling_error = Some("Please login before sending candidates.".into());
            return;
        };
        let candidates = self.engine.local_candidates_as_sdp_lines();
        if candidates.is_empty() {
            return;
        }
        for cand_line in candidates {
            let msg = SignalingMsg::Candidate {
                from: user.clone(),
                to: peer.to_string(),
                mid: "0".into(),
                mline_index: 0,
                cand: cand_line.into_bytes(),
            };
            let _ = self.send_signaling(msg);
        }
    }

    fn send_bye(&mut self, peer: &str, reason: Option<String>) {
        let Some(user) = self.current_username.clone() else {
            return;
        };
        let msg = SignalingMsg::Bye {
            from: user,
            to: peer.to_string(),
            reason,
        };
        let _ = self.send_signaling(msg);
    }

    fn start_outgoing_call(&mut self, peer: &str) {
        if !matches!(self.call_flow, CallFlow::Idle) {
            self.status_line = "Finish or cancel the current call first.".into();
            return;
        }
        if self.current_username.is_none() {
            self.signaling_error = Some("Please login before calling.".into());
            return;
        }
        if let Err(e) = self.create_or_renegotiate_local_sdp() {
            self.status_line = format!("Failed to create local SDP: {e:?}");
            return;
        }
        if self.local_sdp_text.trim().is_empty() {
            self.status_line = "Local SDP is empty.".into();
            return;
        }
        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;
        let from = self.current_username.clone().unwrap_or_default();
        let msg = SignalingMsg::Offer {
            txn_id,
            from: from.clone(),
            to: peer.to_string(),
            sdp: self.local_sdp_text.as_bytes().to_vec(),
        };
        if self.send_signaling(msg).is_ok() {
            self.call_flow = CallFlow::Dialing {
                peer: peer.to_string(),
                txn_id,
            };
            self.status_line = format!("Sent offer to {peer}");
            self.send_local_candidates(peer);
        }
    }

    fn accept_incoming_call(&mut self) {
        let CallFlow::Incoming { from, txn_id, sdp } = self.call_flow.clone() else {
            return;
        };
        match self.set_remote_sdp(&sdp) {
            Ok(()) => {
                if self.local_sdp_text.trim().is_empty() {
                    self.status_line = "Answer not generated.".into();
                    return;
                }
                let msg = SignalingMsg::Answer {
                    txn_id,
                    from: self.current_username.clone().unwrap_or_default(),
                    to: from.clone(),
                    sdp: self.local_sdp_text.as_bytes().to_vec(),
                };
                if self.send_signaling(msg).is_ok() {
                    self.call_flow = CallFlow::Active { peer: from.clone() };
                    self.status_line = format!("Sent answer to {from}");
                    self.send_local_candidates(&from);
                }
            }
            Err(e) => {
                self.status_line = format!("Failed to accept call: {e:?}");
            }
        }
    }

    fn decline_incoming_call(&mut self) {
        self.teardown_call(Some("declined".into()), true);
    }

    fn reset_call_flow(&mut self) {
        self.call_flow = CallFlow::Idle;
        self.pending_remote_sdp = None;
        self.engine.stop();
    }

    fn create_or_renegotiate_local_sdp(&mut self) -> Result<(), GuiError> {
        match self
            .engine
            .negotiate()
            .map_err(|e| GuiError::Connection(format!("negotiate: {e}").into()))?
        {
            Some(s) => {
                self.local_sdp_text = s;
                self.has_local_description = true;
                self.is_local_offerer = true;
                self.status_line = "Local OFFER created. Share it with the peer.".into();
            }
            None => {
                self.status_line = "Negotiation already in progress (have-local-offer).".into();
            }
        }
        Ok(())
    }

    fn set_remote_sdp(&mut self, sdp_str: &str) -> Result<(), GuiError> {
        match self
            .engine
            .apply_remote_sdp(sdp_str)
            .map_err(|e| GuiError::Connection(format!("apply_remote_sdp: {e}").into()))?
        {
            Some(answer) => {
                self.local_sdp_text = answer;
                self.has_local_description = true;
                self.is_local_offerer = false;
                self.status_line = "Remote OFFER set → Local ANSWER created. Share it back.".into();
            }
            None => {
                self.status_line = "Remote ANSWER set.".into();
            }
        }
        self.has_remote_description = true;
        Ok(())
    }

    fn poll_engine_events(&mut self) {
        // Poll engine events
        for ev in self.engine.poll() {
            match ev {
                Log(m) => {
                    // Send to background file logger with original level
                    self.background_log(m.level, format!("{} | {}", m.target, m.text));
                    // UI echo: only warn/error or occasional samples if you want
                    if matches!(m.level, LogLevel::Warn | LogLevel::Error) {
                        self.push_ui_log(format!("[{:?}] {} — {}", m.level, m.target, m.text));
                    }
                }
                Status(s) => {
                    self.background_log(LogLevel::Info, &s);
                    // keep a small echo in UI:
                    self.push_ui_log(&s);
                }
                Established => {
                    self.conn_state = ConnState::Running;
                    self.status_line = "Established.".into();
                    self.engine.start_media_transport();
                }
                Closing { graceful: _ } => {
                    self.conn_state = ConnState::Stopped;
                    self.call_flow = CallFlow::Idle;
                }
                Closed => {
                    self.conn_state = ConnState::Stopped;
                    self.status_line = "Closed.".into();
                    self.engine.close_session();
                    self.call_flow = CallFlow::Idle;
                }
                RtpIn(r) => {
                    self.rtp_pkts += 1;
                    self.rtp_bytes += r.payload.len() as u64;
                    self.background_log(
                        LogLevel::Debug,
                        format!("[RTP] {} bytes PT={}", r.payload.len(), r.pt),
                    );
                }
                Error(e) => {
                    self.status_line = format!("Error: {e}");
                    self.background_log(LogLevel::Error, &e);
                    self.push_ui_log(e);
                }
                IceNominated { local, remote } => {
                    self.status_line = "ICE nominated. Press Start.".into();
                    self.background_log(
                        LogLevel::Info,
                        format!("[ICE] nominated local={local} remote={remote}"),
                    );
                }
                EngineEvent::NetworkMetrics(_) | EngineEvent::UpdateBitrate(_) => {
                    // These are handled by the engine internally, no UI action needed.
                }
            }
        }
    }

    fn render_camera_view(
        &mut self,
        ctx: &egui::Context,
        _local_frame: Option<&VideoFrame>,
        _remote_frame: Option<&VideoFrame>,
    ) {
        sink_debug!(
            self.logger.handle(),
            "[UI] remote_camera_texture exists? {} (id={:?})",
            self.remote_camera_texture.is_some(),
            self.remote_camera_texture.map(|(id, _)| id)
        );
        // show the window if we are running OR we already have any texture
        let have_any_texture =
            self.local_camera_texture.is_some() || self.remote_camera_texture.is_some();

        if matches!(self.conn_state, ConnState::Running) || have_any_texture {
            egui::Window::new("Camera View")
                .default_size([Self::CAMERAS_WINDOW_WIDTH, Self::CAMERAS_WINDOW_HEIGHT])
                .resizable(true)
                .show(ctx, |ui| {
                    // If only remote is present → give it full real estate
                    let only_remote =
                        self.local_camera_texture.is_none() && self.remote_camera_texture.is_some();

                    if only_remote {
                        show_camera_in_ui(
                            ui,
                            self.remote_camera_texture,
                            Self::CAMERAS_WINDOW_WIDTH - 16.0,
                            Self::CAMERAS_WINDOW_HEIGHT - 16.0,
                        );
                    } else {
                        ui.horizontal(|ui| {
                            show_camera_in_ui(
                                ui,
                                self.local_camera_texture,
                                Self::LOCAL_CAMERA_SIZE,
                                Self::LOCAL_CAMERA_SIZE,
                            );
                            ui.separator();
                            show_camera_in_ui(
                                ui,
                                self.remote_camera_texture,
                                Self::REMOTE_CAMERA_SIZE,
                                Self::REMOTE_CAMERA_SIZE,
                            );
                        });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Call controls:");
                        if ui.button(egui::RichText::new("Hang up").strong()).clicked() {
                            self.teardown_call(Some("hangup".into()), true);
                        }
                    });
                });
        }
    }
    const fn can_start(&self) -> bool {
        self.has_remote_description
            && self.has_local_description
            && matches!(self.conn_state, ConnState::Idle | ConnState::Stopped)
    }

    fn render_header(ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.heading(Self::HEADER_TITLE);
            ui.add_space(10.);
        });
    }

    fn render_video_summary(
        ui: &mut egui::Ui,
        local_frame: Option<&VideoFrame>,
        remote_frame: Option<&VideoFrame>,
    ) {
        ui.separator();
        ui.label(format!(
            "Local video: {}",
            Self::summarize_frame(local_frame)
        ));
        ui.label(format!(
            "Remote video: {}",
            Self::summarize_frame(remote_frame)
        ));
    }

    fn render_signaling_panel(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Signaling");
        match self.signaling_screen {
            SignalingScreen::Connect => self.render_connect_screen(ui),
            SignalingScreen::Login => self.render_login_screen(ui),
            SignalingScreen::Home => self.render_home_screen(ui),
        }
        if let Some(err) = &self.signaling_error {
            ui.colored_label(egui::Color32::LIGHT_RED, err);
        }
    }

    fn render_connect_screen(&mut self, ui: &mut egui::Ui) {
        ui.label("Server address:");
        ui.text_edit_singleline(&mut self.server_addr_input);
        if ui.button("Connect").clicked() {
            self.connect_to_signaling();
        }
    }

    fn render_login_screen(&mut self, ui: &mut egui::Ui) {
        ui.label("Login");
        ui.horizontal(|ui| {
            ui.label("Username");
            ui.text_edit_singleline(&mut self.login_username);
        });
        ui.horizontal(|ui| {
            ui.label("Password");
            ui.add(egui::TextEdit::singleline(&mut self.login_password).password(true));
        });
        if ui.button("Login").clicked() {
            let _ = self.send_signaling(SignalingMsg::Login {
                username: self.login_username.clone(),
                password: self.login_password.clone(),
            });
        }
        ui.separator();
        ui.label("Register");
        ui.horizontal(|ui| {
            ui.label("Username");
            ui.text_edit_singleline(&mut self.register_username);
        });
        ui.horizontal(|ui| {
            ui.label("Password");
            ui.add(egui::TextEdit::singleline(&mut self.register_password).password(true));
        });
        if ui.button("Register").clicked() {
            let _ = self.send_signaling(SignalingMsg::Register {
                username: self.register_username.clone(),
                password: self.register_password.clone(),
            });
        }
        if ui.button("Disconnect").clicked() {
            self.disconnect_from_signaling();
        }
    }

    fn render_home_screen(&mut self, ui: &mut egui::Ui) {
        if let Some(user) = &self.current_username {
            ui.label(format!("Logged in as {user}"));
        }
        ui.horizontal(|ui| {
            if ui.button("Refresh peers").clicked() {
                self.request_peer_list();
            }
            if ui.button("Disconnect").clicked() {
                self.disconnect_from_signaling();
            }
        });
        ui.separator();
        ui.label("Available peers:");
        if self.peers_online.is_empty() {
            ui.label("No peers online.");
        } else {
            let peers: Vec<String> = self.peers_online.clone();
            for peer in peers {
                ui.horizontal(|ui| {
                    ui.label(&peer);
                    let busy = matches!(
                        self.call_flow,
                        CallFlow::Dialing { .. } | CallFlow::Active { .. }
                    );
                    if ui
                        .add_enabled(!busy, egui::Button::new(format!("Call {peer}")))
                        .clicked()
                    {
                        self.start_outgoing_call(&peer);
                    }
                });
            }
        }
        self.render_call_flow_ui(ui);
    }

    fn render_call_flow_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        match self.call_flow.clone() {
            CallFlow::Idle => {
                ui.label("No active calls.");
            }
            CallFlow::Dialing { peer, .. } => {
                ui.label(format!("Calling {peer}…"));
                if ui.button("Cancel outgoing call").clicked() {
                    self.teardown_call(Some("cancelled".into()), true);
                }
            }
            CallFlow::Incoming { from, .. } => {
                ui.label(format!("Incoming call from {from}"));
                ui.horizontal(|ui| {
                    if ui.button("Accept").clicked() {
                        self.accept_incoming_call();
                    }
                    if ui.button("Decline").clicked() {
                        self.decline_incoming_call();
                    }
                });
            }
            CallFlow::Active { peer } => {
                ui.label(format!("In call with {peer}"));
                if ui.button("Hang up").clicked() {
                    self.teardown_call(Some("hangup".into()), true);
                }
            }
        }
    }

    fn render_remote_sdp_input(&mut self, ui: &mut egui::Ui) {
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
                self.pending_remote_sdp = Some(self.remote_sdp_text.trim().to_owned());
            }
            if ui.button("Clear").clicked() {
                self.remote_sdp_text.clear();
            }
        });
    }

    fn render_local_sdp_output(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("2) Create local SDP and share it (offer/renegotiation):");
        ui.horizontal(|ui| {
            if ui.button("Create SDP message").clicked() {
                if let Err(e) = self.create_or_renegotiate_local_sdp() {
                    self.status_line = format!("Failed to create local SDP: {e:?}");
                } else {
                    self.status_line = String::from("Local SDP generated.");
                }
            }
            if ui.button("Copy to clipboard").clicked() {
                ui.output_mut(|o| o.copied_text = String::from(&self.local_sdp_text));
                self.status_line = String::from("Copied local SDP to clipboard.");
            }
        });
        ui.add(
            egui::TextEdit::multiline(&mut self.local_sdp_text)
                .desired_rows(15)
                .desired_width(f32::INFINITY)
                .hint_text("Your local SDP (Offer/Answer) will appear here…"),
        );
    }

    fn render_connection_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.can_start(), egui::Button::new("Start Connection"))
                .clicked()
                && let Err(e) = self.engine.start()
            {
                self.status_line = format!("Failed to start: {e}");
            }
            if ui
                .add_enabled(
                    matches!(self.conn_state, ConnState::Running),
                    egui::Button::new("End call"),
                )
                .clicked()
            {
                self.teardown_call(Some("stopped".into()), true);
            }
            ui.label(format!("State: {:?}", self.conn_state));
        });
    }

    fn render_log_section(&self, ui: &mut egui::Ui) {
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
    }

    fn render_status_line(&self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(&self.status_line);
    }
    fn debug_frame_alias_and_size(
        &mut self,
        local: Option<&VideoFrame>,
        remote: Option<&VideoFrame>,
    ) {
        if let (Some(l), Some(r)) = (local, remote) {
            if let (VideoFrameData::Rgb(l_buf), VideoFrameData::Rgb(r_buf)) = (&l.data, &r.data) {
                // 1) Same underlying buffer?
                let lp = l_buf.as_ptr() as usize;
                let rp = r_buf.as_ptr() as usize;
                if !l_buf.is_empty() && lp == rp {
                    self.background_log(
                        LogLevel::Error,
                        "⚠️ Local & Remote share the SAME pixel buffer (0x{lp:x}).",
                    );
                }

                // 2) Basic size checks (RGB24 expected)
                let l_need = (l.width as usize) * (l.height as usize) * 3;
                let r_need = (r.width as usize) * (r.height as usize) * 3;
                if l_buf.len() != l_need {
                    self.background_log(
                        LogLevel::Error,
                        format!("⚠️ Local bad len: {} vs {}", l_buf.len(), l_need),
                    );
                }
                if r_buf.len() != r_need {
                    self.background_log(
                        LogLevel::Error,
                        format!("⚠️ Remote bad len: {} vs {}", r_buf.len(), r_need),
                    );
                }
            } else {
                self.background_log(LogLevel::Debug, "Skipping debug checks for non-RGB frames");
            }
        }
    }
    fn current_peer(&self) -> Option<String> {
        match &self.call_flow {
            CallFlow::Dialing { peer, .. } => Some(peer.clone()),
            CallFlow::Incoming { from, .. } => Some(from.clone()),
            CallFlow::Active { peer } => Some(peer.clone()),
            CallFlow::Idle => None,
        }
    }

    fn teardown_call(&mut self, reason: Option<String>, send_bye: bool) {
        // 1) Conditionally send Bye Singaling Message
        if send_bye {
            if let Some(peer) = self.current_peer() {
                self.send_bye(&peer, reason.clone());
            }
        }

        // 2) Tear down media (safe to call even if session never started)
        self.engine.stop();

        // 3) Reset call-related state
        self.call_flow = CallFlow::Idle;
        self.pending_remote_sdp = None;
        self.has_local_description = false;
        self.has_remote_description = false;

        // This ensures 'have_any_texture' becomes false, closing the window.
        self.local_camera_texture = None;
        self.remote_camera_texture = None;

        if let Some(r) = reason {
            self.status_line = format!("Call ended: {r}");
        } else {
            self.status_line = "Call ended.".into();
        }
    }
}

impl App for RtcApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut Frame) {
        // repaint policy: if connection is running OR any texture is alive, tick ~60 fps
        let any_video = self.local_camera_texture.is_some() || self.remote_camera_texture.is_some();
        if matches!(self.conn_state, ConnState::Running) || any_video {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }

        if let Some(sdp) = self.pending_remote_sdp.take() {
            match self.set_remote_sdp(&sdp) {
                Ok(()) => self.status_line = String::from("Remote SDP processed."),
                Err(e) => self.status_line = format!("Failed to set remote SDP: {e:?}"),
            }
        }

        self.poll_engine_events();
        self.poll_signaling_events();
        self.drain_ui_log_tap();

        // If we hung up (CallFlow::Idle), force frames to None.
        // This prevents the "last frame" from resurrecting the textures
        // while the Engine is busy closing gracefully in the background.
        let (local_frame, remote_frame) = if !matches!(self.call_flow, CallFlow::Idle) {
            self.engine.snapshot_frames()
        } else {
            (None, None)
        };

        self.debug_frame_alias_and_size(local_frame.as_ref(), remote_frame.as_ref());

        let logger_handle = Arc::new(self.logger.handle());

        // Inlined texture update logic
        if let Some(render_state) = frame.wgpu_render_state() {
            if let Some(f) = local_frame.as_ref() {
                update_texture_from_frame(
                    ctx,
                    f,
                    &mut self.local_camera_texture,
                    &mut self.local_yuv_renderer,
                    Some(render_state),
                    "camera/local",
                    logger_handle.clone(),
                );
            }
            if let Some(f) = remote_frame.as_ref() {
                update_texture_from_frame(
                    ctx,
                    f,
                    &mut self.remote_camera_texture,
                    &mut self.remote_yuv_renderer,
                    Some(render_state),
                    "camera/remote",
                    logger_handle.clone(),
                );
            }
        }

        self.render_camera_view(ctx, local_frame.as_ref(), remote_frame.as_ref());

        egui::CentralPanel::default().show(ctx, |ui| {
            Self::render_header(ui);
            self.render_signaling_panel(ui);
            if !matches!(self.signaling_screen, SignalingScreen::Home) {
                ui.separator();
                ui.label("Connect and log in to place a call.");
                self.render_status_line(ui);
                self.render_log_section(ui);
                return;
            }
            Self::render_video_summary(ui, local_frame.as_ref(), remote_frame.as_ref());
            self.render_remote_sdp_input(ui);
            self.render_local_sdp_output(ui);
            self.render_connection_controls(ui);
            self.render_status_line(ui);
            self.render_log_section(ui);
        });
    }
}

fn update_texture_from_frame(
    ctx: &egui::Context,
    frame: &VideoFrame,
    texture: &mut Option<(egui::TextureId, (u32, u32))>,
    yuv_renderer: &mut Option<GpuYuvRenderer>,
    render_state: Option<&RenderState>,
    unique_name: &str,
    logger: Arc<dyn LogSink>,
) {
    let w = frame.width;
    let h = frame.height;
    if w == 0 || h == 0 {
        return;
    }

    sink_debug!(
        logger,
        "[VIDEO] {:?}: {}x{}, kind={:?}",
        unique_name,
        w,
        h,
        match &frame.data {
            VideoFrameData::Rgb(_) => "RGB",
            VideoFrameData::Yuv420 { .. } => "YUV420",
        }
    );
    match &frame.data {
        crate::media_agent::video_frame::VideoFrameData::Rgb(rgb) => {
            let image = egui::ColorImage::from_rgb([w as usize, h as usize], rgb);
            let options = egui::TextureOptions {
                magnification: egui::TextureFilter::Linear,
                minification: egui::TextureFilter::Linear,
                ..Default::default()
            };
            let tex_mngr = ctx.tex_manager();

            if let Some((id, (prev_w, prev_h))) = texture {
                if *prev_w != w || *prev_h != h {
                    tex_mngr.write().free(*id);
                    let new_id =
                        tex_mngr
                            .write()
                            .alloc(unique_name.to_owned(), image.into(), options);
                    *texture = Some((new_id, (w, h)));
                } else {
                    let delta = egui::epaint::ImageDelta {
                        image: egui::epaint::ImageData::Color(image.into()),
                        options,
                        pos: None,
                    };
                    tex_mngr.write().set(*id, delta);
                }
            } else {
                let new_id = tex_mngr
                    .write()
                    .alloc(unique_name.to_owned(), image.into(), options);
                *texture = Some((new_id, (w, h)));
            }
        }
        crate::media_agent::video_frame::VideoFrameData::Yuv420 { .. } => {
            if let (Some(renderer), Some(render_state)) = (yuv_renderer, render_state) {
                sink_debug!(&logger, "[YUV] Using renderer for {}", unique_name,);
                renderer.update_frame(
                    &render_state.device,
                    &render_state.queue,
                    frame,
                    logger.clone(),
                );

                if let Some(output_texture) = renderer.output_texture() {
                    let view =
                        output_texture.create_view(&eframe::wgpu::TextureViewDescriptor::default());
                    let filter = eframe::wgpu::FilterMode::Linear;
                    let mut wgpu_renderer = render_state.renderer.write();

                    if let Some((id, (prev_w, prev_h))) = texture {
                        if *prev_w != w || *prev_h != h {
                            wgpu_renderer.free_texture(id);
                            let new_id = wgpu_renderer.register_native_texture(
                                &render_state.device,
                                &view,
                                filter,
                            );
                            *texture = Some((new_id, (w, h)));
                        } else {
                            wgpu_renderer.update_egui_texture_from_wgpu_texture(
                                &render_state.device,
                                &view,
                                filter,
                                *id,
                            );
                        }
                    } else {
                        let new_id = wgpu_renderer.register_native_texture(
                            &render_state.device,
                            &view,
                            filter,
                        );
                        *texture = Some((new_id, (w, h)));
                    }
                }
            }
        }
    }
}
