use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    core::{events::EngineEvent, session::Session},
    media_agent::{
        codec_descriptor::CodecDescriptor,
        h264_decoder::H264Decoder,
        h264_encoder::H264Encoder,
        media_agent_error::{MediaAgentError, Result},
        video_frame::VideoFrame,
    },
    rtp_session::{
        outbound_track_handle::OutboundTrackHandle,
        payload::{h264_packetizer::H264Packetizer, rtp_payload_chunk::RtpPayloadChunk},
        rtp_codec::RtpCodec,
    },
};

pub struct MediaAgent {
    tx_evt: Sender<EngineEvent>,
    payload_map: HashMap<u8, CodecDescriptor>,
    outbound_tracks: HashMap<u8, OutboundTrackHandle>,
    h264_decoder: Mutex<H264Decoder>,
    h264_encoder: Mutex<H264Encoder>,
    h264_packetizer: H264Packetizer,
    local_frame_rx: Option<Receiver<VideoFrame>>,
    local_frame: Arc<Mutex<Option<VideoFrame>>>,
    remote_frame: Arc<Mutex<Option<VideoFrame>>>,
    last_local_frame_sent: Option<Instant>,
}

impl MediaAgent {
    pub fn new(tx_evt: Sender<EngineEvent>) -> Self {
        let mut payload_map = HashMap::new();
        let pt = 96;
        payload_map.insert(pt, CodecDescriptor::h264_dynamic(pt));

        let (rx, status) = spawn_camera_worker();
        if let Some(msg) = status {
            let _ = tx_evt.send(EngineEvent::Status(format!("[MediaAgent] {msg}")));
        }
        // Recommended WebRTC-safe MTU for UDP path: ~1200 total bytes.
        // Overhead defaults to 12 (RTP header); bump if we add SRTP/DTLS/exts.
        let h264_packetizer = H264Packetizer::new(1200); // .with_overhead(12) is default
        let h264_encoder = H264Encoder::new(30, 800_000, 60);

        Self {
            tx_evt,
            payload_map,
            outbound_tracks: HashMap::new(),
            h264_decoder: Mutex::new(H264Decoder::new()),
            h264_encoder: Mutex::new(h264_encoder),
            local_frame_rx: Some(rx),
            local_frame: Arc::new(Mutex::new(None)),
            remote_frame: Arc::new(Mutex::new(None)),
            last_local_frame_sent: None,
            h264_packetizer,
        }
    }

    pub fn payload_mapping(&self) -> &HashMap<u8, CodecDescriptor> {
        &self.payload_map
    }

    pub fn local_rtp_codecs(&self) -> Vec<RtpCodec> {
        self.payload_map.values().map(|c| c.rtp.clone()).collect()
    }

    pub fn codec_descriptors(&self) -> Vec<CodecDescriptor> {
        self.payload_map.values().cloned().collect()
    }

    pub fn handle_engine_event(&mut self, evt: &EngineEvent, session: Option<&Session>) {
        match evt {
            EngineEvent::Established => {
                if let Some(sess) = session {
                    if let Err(e) = self.ensure_outbound_tracks(sess) {
                        let _ = self
                            .tx_evt
                            .send(EngineEvent::Error(format!("media tracks: {e:?}")));
                    }
                }
            }
            EngineEvent::Closed | EngineEvent::Closing { .. } => {
                self.outbound_tracks.clear();
                self.last_local_frame_sent = None;
            }
            EngineEvent::RtpMedia { pt, bytes } => self.handle_remote_rtp(*pt, bytes),
            _ => {}
        }
    }

    pub fn tick(&mut self, session: Option<&Session>) {
        self.drain_local_frames();
        if let Some(sess) = session {
            if let Err(e) = self.maybe_send_local_frame(sess) {
                let _ = self.tx_evt.send(EngineEvent::Error(format!(
                    "[MediaAgent] send local frame failed: {e:?}"
                )));
            }
        }
    }

    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        let local = self
            .local_frame
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().cloned());
        let remote = self
            .remote_frame
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().cloned());
        (local, remote)
    }

    fn ensure_outbound_tracks(&mut self, session: &Session) -> Result<()> {
        for (pt, codec) in &self.payload_map {
            if self.outbound_tracks.contains_key(pt) {
                continue;
            }
            let handle = session
                .register_outbound_track(codec.rtp.clone())
                .map_err(MediaAgentError::Send)?;
            self.outbound_tracks.insert(*pt, handle);
        }
        Ok(())
    }

    fn maybe_send_local_frame(&mut self, session: &Session) -> Result<()> {
        let Some(frame) = self
            .local_frame
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().cloned())
        else {
            return Ok(());
        };

        if self.outbound_tracks.is_empty() {
            return Ok(());
        }

        // simple pacing (you can later move to real vsync)
        let now = Instant::now();
        if let Some(last) = self.last_local_frame_sent {
            if now.duration_since(last) < Duration::from_millis(200) {
                return Ok(());
            }
        }

        // 1) Pick one outbound video track (H264 PT=96)
        let handle = match self.outbound_tracks.values().next() {
            Some(h) => h,
            None => return Ok(()),
        };

        // 2) Encode to an Annex-B access unit (SPS/PPS/IDR/etc.)
        let annexb_frame: Vec<u8> = {
            let mut enc = self
                .h264_encoder
                .lock()
                .map_err(|_| MediaAgentError::Codec("encoder poisoned".into()))?;
            enc.encode_frame_to_h264(&frame)?
        }; // 3) Packetize (Single NALU or FU-A fragments; marker set only on last chunk)
        let chunks: Vec<RtpPayloadChunk> = self
            .h264_packetizer
            .packetize_annexb_to_payloads(&annexb_frame);

        if chunks.is_empty() {
            // Nothing to send (e.g., degenerate MTU/overhead); just bail gracefully
            return Ok(());
        }

        // 4) Timestamp (90 kHz); keep SAME ts for all fragments of this frame
        let ts90k = (frame.timestamp_ms as u32).wrapping_mul(90);

        // 5) Send all fragments in one call (locks once inside the session)
        session
            .send_rtp_chunks_for_frame(handle, &chunks, ts90k)
            .map_err(MediaAgentError::Send)?;

        self.last_local_frame_sent = Some(now);
        Ok(())
    }

    fn drain_local_frames(&mut self) {
        if let Some(rx) = &self.local_frame_rx {
            while let Ok(frame) = rx.try_recv() {
                if let Ok(mut guard) = self.local_frame.lock() {
                    *guard = Some(frame);
                }
            }
        }
    }

    fn handle_remote_rtp(&self, payload_type: u8, bytes: &[u8]) {
        if !self.payload_map.contains_key(&payload_type) {
            let _ = self.tx_evt.send(EngineEvent::Log(format!(
                "[MediaAgent] ignoring payload type {payload_type}"
            )));
            return;
        }
        match self.decode_h264(bytes) {
            Ok(Some(frame)) => {
                if let Ok(mut guard) = self.remote_frame.lock() {
                    *guard = Some(frame);
                }
            }

            Ok(None) => {
                // No frame listo todavía: probablemente el decodificador
                // está esperando más fragmentos o slices.
                // Puedes registrar un log en nivel debug, pero no es error.
                let _ = self.tx_evt.send(EngineEvent::Log(
                    "[MediaAgent] waiting for more H264 data (incomplete frame)".into(),
                ));
            }
            Err(e) => {
                let _ = self.tx_evt.send(EngineEvent::Log(format!(
                    "[MediaAgent] decode error: {e:?}"
                )));
            }
        }
    }

    fn decode_h264(&self, bytes: &[u8]) -> Result<Option<VideoFrame>> {
        let mut guard = self
            .h264_decoder
            .lock()
            .map_err(|_| MediaAgentError::Codec("decoder poisoned".into()))?;
        guard.decode(bytes)
    }
}

fn spawn_camera_worker() -> (Receiver<VideoFrame>, Option<String>) {
    let (tx, rx) = mpsc::channel();
    let status = discover_camera_path()
        .map(|p| format!("using camera source {}", p.display()))
        .or_else(|| Some("no physical camera detected, using test pattern".into()));

    thread::Builder::new()
        .name("media-agent-camera".into())
        .spawn(move || {
            if let Err(e) = camera_loop(tx) {
                eprintln!("camera loop stopped: {e:?}");
            }
        })
        .ok();

    (rx, status)
}

fn camera_loop(tx: Sender<VideoFrame>) -> Result<()> {
    let mut phase = 0u8;
    loop {
        let frame = VideoFrame::synthetic(320, 240, phase);
        phase = phase.wrapping_add(1);
        tx.send(frame)
            .map_err(|e| MediaAgentError::Camera(e.to_string()))?;
        thread::sleep(Duration::from_millis(100));
    }
}

fn discover_camera_path() -> Option<PathBuf> {
    for idx in 0..4 {
        let path = PathBuf::from(format!("/dev/video{idx}"));
        if fs::metadata(&path).is_ok() {
            return Some(path);
        }
    }
    None
}
