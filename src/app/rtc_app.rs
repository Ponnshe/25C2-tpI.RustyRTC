use super::{
    conn_state::ConnState,
    gui_error::GuiError,
    utils::{show_camera_in_ui, update_camera_texture},
};
use crate::{
    app::{log_level::LogLevel, logger::Logger},
    core::{
        engine::Engine,
        events::EngineEvent::{
            Closed, Closing, Error, Established, IceNominated, Log, RtpIn, Status,
        },
    },
    media_agent::video_frame::VideoFrame,
};
use eframe::{App, Frame, egui};
use std::{
    collections::VecDeque,
    sync::{Arc, mpsc::TrySendError},
    time::Instant,
};

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

    local_camera_texture: Option<egui::TextureHandle>,
    remote_camera_texture: Option<egui::TextureHandle>,
}

impl RtcApp {
    const HEADER_TITLE: &str = "RoomRTC • SDP Messenger";
    const CAMERAS_WINDOW_WIDTH: f32 = 800.0;
    const CAMERAS_WINDOW_HEIGHT: f32 = 400.0;
    const LOCAL_CAMERA_SIZE: f32 = 400.0;
    const REMOTE_CAMERA_SIZE: f32 = 400.0;

    #[must_use]
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let logger = Logger::start_default("roomrtc", 4096, 256, 50);
        Self {
            remote_sdp_text: String::new(),
            local_sdp_text: String::new(),
            pending_remote_sdp: None,
            status_line: "Ready.".into(),
            engine: Engine::new(Arc::new(logger.handle())),
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
            Some(f) if f.width > 0 && f.height > 0 => {
                format!("{}x{} • {} bytes", f.width, f.height, f.bytes.len())
            }
            Some(f) => format!("{} bytes (pending decode)", f.bytes.len()),
            None => "no frame".into(),
        }
    }

    fn push_kbps_log_from_rtp_packets(&mut self) {
        let now = std::time::Instant::now();
        if now.duration_since(self.rtp_last_report).as_millis() >= 500 {
            let kbps = (self.rtp_bytes as f64 * 8.0) / 1000.0 * 2.0; // rough since 0.5s window
            self.push_ui_log(format!(
                "RTP: {} pkts, {:.1} kbps (last 0.5s)",
                self.rtp_pkts, kbps
            ));
            self.rtp_pkts = 0;
            self.rtp_bytes = 0;
            self.rtp_last_report = now;
        }
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
                }
                Closing { graceful: _ } => {
                    self.conn_state = ConnState::Stopped;
                }
                Closed => {
                    self.conn_state = ConnState::Stopped;
                    self.status_line = "Closed.".into();
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
            }
        }
    }

    fn render_camera_view(
        &mut self,
        ctx: &egui::Context,
        local_frame: Option<&VideoFrame>,
        remote_frame: Option<&VideoFrame>,
    ) {
        // update textures first, independently of connection state
        if let Some(f) = local_frame {
            update_camera_texture(ctx, f, &mut self.local_camera_texture, "camera/local");
        }
        if let Some(f) = remote_frame {
            update_camera_texture(ctx, f, &mut self.remote_camera_texture, "camera/remote");
        }

        // show the window if we are running OR we already have any texture
        let have_any_texture =
            self.local_camera_texture.is_some() || self.remote_camera_texture.is_some();

        if matches!(self.conn_state, ConnState::Running) {
            self.push_kbps_log_from_rtp_packets();
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
                            self.remote_camera_texture.as_ref(),
                            Self::CAMERAS_WINDOW_WIDTH - 16.0,
                            Self::CAMERAS_WINDOW_HEIGHT - 16.0,
                        );
                    } else {
                        ui.horizontal(|ui| {
                            show_camera_in_ui(
                                ui,
                                self.local_camera_texture.as_ref(),
                                Self::LOCAL_CAMERA_SIZE,
                                Self::LOCAL_CAMERA_SIZE,
                            );
                            ui.separator();
                            show_camera_in_ui(
                                ui,
                                self.remote_camera_texture.as_ref(),
                                Self::REMOTE_CAMERA_SIZE,
                                Self::REMOTE_CAMERA_SIZE,
                            );
                        });
                    }
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
                    egui::Button::new("Stop"),
                )
                .clicked()
            {
                self.engine.stop();
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
            // 1) Same underlying buffer?
            let lp = l.bytes.as_ptr() as usize;
            let rp = r.bytes.as_ptr() as usize;
            if !l.bytes.is_empty() && lp == rp {
                self.background_log(
                    LogLevel::Error,
                    "⚠️ Local & Remote share the SAME pixel buffer (0x{lp:x}).",
                );
            }

            // 2) Basic size checks (RGB24 expected)
            let l_need = (l.width as usize) * (l.height as usize) * 3;
            let r_need = (r.width as usize) * (r.height as usize) * 3;
            if l.bytes.len() != l_need {
                self.background_log(
                    LogLevel::Error,
                    format!("⚠️ Local bad len: {} vs {}", l.bytes.len(), l_need),
                );
            }
            if r.bytes.len() != r_need {
                self.background_log(
                    LogLevel::Error,
                    format!("⚠️ Remote bad len: {} vs {}", r.bytes.len(), r_need),
                );
            }
        }
    }
}

impl App for RtcApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
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
        self.drain_ui_log_tap();

        let (local_frame, remote_frame) = self.engine.snapshot_frames();
        self.debug_frame_alias_and_size(local_frame.as_ref(), remote_frame.as_ref());

        self.render_camera_view(ctx, local_frame.as_ref(), remote_frame.as_ref());

        egui::CentralPanel::default().show(ctx, |ui| {
            Self::render_header(ui);
            Self::render_video_summary(ui, local_frame.as_ref(), remote_frame.as_ref());
            self.render_remote_sdp_input(ui);
            self.render_local_sdp_output(ui);
            self.render_connection_controls(ui);
            self.render_status_line(ui);
            self.render_log_section(ui);
        });
    }
}
