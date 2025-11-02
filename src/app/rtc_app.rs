use super::{
    conn_state::ConnState,
    gui_error::GuiError,
    utils::{show_camera_in_ui, update_camera_texture},
};
use crate::{
    core::{engine::Engine, events::EngineEvent},
    media_agent::video_frame::VideoFrame,
};
use eframe::{App, Frame, egui};
use std::collections::VecDeque;

pub struct RtcApp {
    // UI text areas
    remote_sdp_text: String,
    local_sdp_text: String,

    status_line: String,

    // New orchestrator
    engine: Engine,

    // local UI flags
    has_remote_description: bool,
    has_local_description: bool,
    is_local_offerer: bool,
    conn_state: ConnState,

    // UI log plumbing
    ui_logs: VecDeque<String>,

    local_camera_texture: Option<egui::TextureHandle>,
    remote_camera_texture: Option<egui::TextureHandle>,
}

impl RtcApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            remote_sdp_text: String::new(),
            local_sdp_text: String::new(),
            status_line: "Ready.".into(),
            engine: Engine::new(),
            has_remote_description: false,
            has_local_description: false,
            is_local_offerer: false,
            conn_state: ConnState::Idle,
            ui_logs: VecDeque::with_capacity(256),
            local_camera_texture: None,
            remote_camera_texture: None,
        }
    }

    fn push_log<T: Into<String>>(&mut self, s: T) {
        if self.ui_logs.len() == 256 {
            self.ui_logs.pop_front();
        }
        self.ui_logs.push_back(s.into());
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
}

impl App for RtcApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        // Poll engine events
        for ev in self.engine.poll() {
            match ev {
                EngineEvent::Status(s) | EngineEvent::Log(s) => self.push_log(s),
                EngineEvent::Established => {
                    self.conn_state = ConnState::Running;
                    self.status_line = "Established.".into();
                }
                EngineEvent::Closing { graceful: _ } => {
                    self.conn_state = ConnState::Stopped;
                }
                EngineEvent::Closed => {
                    self.conn_state = ConnState::Stopped;
                    self.status_line = "Closed.".into();
                }
                EngineEvent::Payload(s) => self.push_log(format!("[RECV] {s}")),
                EngineEvent::RtpIn(r) => {
                    self.push_log(format!(
                        "[RTP] received {} [B] payload (PT={})",
                        r.payload.len(),
                        r.pt
                    ));
                }
                EngineEvent::RtpMedia { pt, bytes } => {
                    self.push_log(format!("[RTP] received {} bytes (PT={})", bytes.len(), pt));
                }
                EngineEvent::Error(e) => {
                    self.status_line = format!("Error: {e}");
                    self.push_log(e);
                }
                EngineEvent::IceNominated { local, remote } => {
                    self.status_line = "ICE nominated. Press Start.".into();
                    self.push_log(format!("[ICE] nominated local={local} remote={remote}"));
                }
            }
        }
        let (local_frame, remote_frame) = self.engine.snapshot_frames();

        if let Some(local_frame) = &local_frame {
            update_camera_texture(ctx, local_frame, &mut self.local_camera_texture);
        }

        if let Some(remote_frame) = &remote_frame {
            update_camera_texture(ctx, remote_frame, &mut self.remote_camera_texture);
        }

        // Mostrar ventana de cámara solo si la conexión está establecida
        if matches!(self.conn_state, ConnState::Running) {
            egui::Window::new("Camera View")
                .default_size([800.0, 400.0])
                .resizable(true)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        show_camera_in_ui(ui, &self.local_camera_texture, 400.0, 400.0);
                        ui.separator();

                        show_camera_in_ui(ui, &self.remote_camera_texture, 400.0, 400.0);
                    });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("RoomRTC • SDP Messenger");
                ui.add_space(10.);
            });

            ui.separator();
            ui.label(format!(
                "Local video: {}",
                Self::summarize_frame(local_frame.as_ref())
            ));
            ui.label(format!(
                "Remote video: {}",
                Self::summarize_frame(remote_frame.as_ref())
            ));
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
                    if let Err(e) = self.engine.start() {
                        self.status_line = format!("Failed to start: {e}");
                    }
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
