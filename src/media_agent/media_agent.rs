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

use crate::{camera_manager::camera_error::CameraError, core::events::RtpIn};
use crate::{
    camera_manager::camera_manager_c::CameraManager,
    core::{events::EngineEvent, session::Session},
    media_agent::{
        codec_descriptor::CodecDescriptor,
        frame_format::FrameFormat,
        h264_decoder::H264Decoder,
        h264_encoder::H264Encoder,
        media_agent_error::{MediaAgentError, Result},
        utils::now_millis,
        video_frame::VideoFrame,
    },
    rtp_session::{
        outbound_track_handle::OutboundTrackHandle,
        payload::{
            h264_depacketizer::{AccessUnit, H264Depacketizer},
            h264_packetizer::H264Packetizer,
            rtp_payload_chunk::RtpPayloadChunk,
        },
        rtp_codec::RtpCodec,
    },
};

use opencv::{
    core::{AlgorithmHint, CV_8UC3, Mat},
    imgproc,
    prelude::*,
};

pub struct MediaAgent {
    event_tx: Sender<EngineEvent>,
    payload_map: HashMap<u8, CodecDescriptor>,
    outbound_tracks: HashMap<u8, OutboundTrackHandle>,
    h264_decoder: Mutex<H264Decoder>,
    h264_encoder: Mutex<H264Encoder>,
    h264_packetizer: H264Packetizer,
    h264_depacketizer: H264Depacketizer,
    local_frame_rx: Option<Receiver<VideoFrame>>,
    local_frame: Arc<Mutex<Option<VideoFrame>>>,
    remote_frame: Arc<Mutex<Option<VideoFrame>>>,
    last_local_frame_sent: Option<Instant>,
    rtp_ts: u32,
    rtp_ts_step: u32, // 90_000 / fps
    sent_any_frame: bool,
    last_sent_local_ts_ms: Option<u128>,
}

impl MediaAgent {
    pub fn new(tx_evt: Sender<EngineEvent>) -> Self {
        let mut payload_map = HashMap::new();
        let pt = 96;
        let target_fps = 30;
        payload_map.insert(pt, CodecDescriptor::h264_dynamic(pt));

        let (rx, status) = spawn_camera_worker(target_fps);
        if let Some(msg) = status {
            let _ = tx_evt.send(EngineEvent::Status(format!("[MediaAgent] {msg}")));
        }
        // Recommended WebRTC-safe MTU for UDP path: ~1200 total bytes.
        // Overhead defaults to 12 (RTP header); bump if we add SRTP/DTLS/exts.
        //
        let h264_encoder = H264Encoder::new(target_fps, 800_000, 60);
        let h264_packetizer = H264Packetizer::new(1200); // .with_overhead(12) is default
        let h264_depacketizer = H264Depacketizer::new();

        Self {
            event_tx: tx_evt,
            payload_map,
            outbound_tracks: HashMap::new(),
            h264_decoder: Mutex::new(H264Decoder::new()),
            h264_encoder: Mutex::new(h264_encoder),
            local_frame_rx: Some(rx),
            local_frame: Arc::new(Mutex::new(None)),
            remote_frame: Arc::new(Mutex::new(None)),
            last_local_frame_sent: None,
            h264_packetizer,
            h264_depacketizer,
            rtp_ts: rand::random::<u32>(), // random start, per RFC 3550
            rtp_ts_step: 90_000 / target_fps, // e.g. 3000 for 30 fps
            sent_any_frame: false,
            last_sent_local_ts_ms: None,
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
                            .event_tx
                            .send(EngineEvent::Error(format!("media tracks: {e:?}")));
                    }
                }
            }
            EngineEvent::Closed | EngineEvent::Closing { .. } => {
                self.outbound_tracks.clear();
                self.last_local_frame_sent = None;
            }
            EngineEvent::RtpIn(pkt) => self.handle_remote_rtp(pkt),
            //Legacy
            EngineEvent::RtpMedia { pt, bytes } => {
                let shim = RtpIn {
                    pt: *pt,
                    marker: true,
                    timestamp_90khz: 0,
                    seq: 0,
                    ssrc: 0,
                    payload: bytes.clone(),
                };
                self.handle_remote_rtp(&shim);
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, session: Option<&Session>) {
        self.drain_local_frames();
        if let Some(session_handle) = session {
            if let Err(e) = self.maybe_send_local_frame(session_handle) {
                let _ = self.event_tx.send(EngineEvent::Error(format!(
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
        // 1) snapshot the newest local frame (set by drain_local_frames)
        let Some(frame) = self
            .local_frame
            .lock()
            .ok()
            .and_then(|g| g.as_ref().cloned())
        else {
            return Ok(());
        };

        if self.outbound_tracks.is_empty() {
            return Ok(());
        }

        // 2) avoid re-sending the same frame
        if self.last_sent_local_ts_ms == Some(frame.timestamp_ms) {
            return Ok(());
        }

        // 3) pick the outbound H.264 track
        let handle = match self.outbound_tracks.values().next() {
            Some(h) => h,
            None => return Ok(()),
        };

        // 4) ensure the very first frame is a keyframe (before encode)
        if !self.sent_any_frame {
            if let Ok(mut enc) = self.h264_encoder.lock() {
                enc.request_keyframe();
            }
        }

        // 5) encode RGB -> Annex-B H.264 (SPS/PPS/IDR…)
        let annexb_frame: Vec<u8> = {
            let mut enc = self
                .h264_encoder
                .lock()
                .map_err(|_| MediaAgentError::Codec("encoder poisoned".into()))?;
            enc.encode_frame_to_h264(&frame)?
        };

        // 6) packetize (Single NALU / FU-A); marker set on last chunk by packetizer
        let chunks: Vec<RtpPayloadChunk> = self
            .h264_packetizer
            .packetize_annexb_to_payloads(&annexb_frame);
        if chunks.is_empty() {
            return Ok(());
        }

        // 7) use stable 90kHz RTP ts (same for every chunk of this AU)
        let ts90k = self.rtp_ts;

        session
            .send_rtp_chunks_for_frame(handle, &chunks, ts90k)
            .map_err(MediaAgentError::Send)?;

        // 8) advance RTP clock and book-keeping
        self.rtp_ts = self.rtp_ts.wrapping_add(self.rtp_ts_step);
        self.sent_any_frame = true;
        self.last_sent_local_ts_ms = Some(frame.timestamp_ms);
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

    fn handle_remote_rtp(&mut self, pkt: &RtpIn) {
        // Ignore unknown payload types
        if !self.payload_map.contains_key(&pkt.pt) {
            let _ = self.event_tx.send(EngineEvent::Log(format!(
                "[MediaAgent] ignoring payload type {}",
                pkt.pt
            )));
            return;
        }

        // H.264 depacketize → AccessUnit → decode
        if let Some(au) =
            self.h264_depacketizer
                .push_rtp(&pkt.payload, pkt.marker, pkt.timestamp_90khz, pkt.seq)
        {
            match self.decode_h264(&au) {
                Ok(Some(frame)) => {
                    if let Ok(mut guard) = self.remote_frame.lock() {
                        *guard = Some(frame);
                    }
                }
                Ok(None) => {
                    let _ = self.event_tx.send(EngineEvent::Log(
                        "[MediaAgent] decoder needs more NALs for this AU".into(),
                    ));
                }
                Err(e) => {
                    let _ = self.event_tx.send(EngineEvent::Log(format!(
                        "[MediaAgent] decode error: {e:?}"
                    )));
                }
            }
        }
    }

    fn decode_h264(&self, access_unit: &AccessUnit) -> Result<Option<VideoFrame>> {
        let mut guard = self
            .h264_decoder
            .lock()
            .map_err(|_| MediaAgentError::Codec("decoder poisoned".into()))?;
        guard.decode_au(access_unit)
    }
}

fn spawn_camera_worker(target_fps: u32) -> (Receiver<VideoFrame>, Option<String>) {
    let (local_frame_tx, local_frame_rx) = mpsc::channel();
    let camera_manager = CameraManager::new(0);

    let status = match &camera_manager {
        Ok(cam) => Some(format!(
            "Using camera source with resolution {}x{}",
            cam.width(),
            cam.height()
        )),
        Err(e) => Some(format!("Camera error: {}. Using test pattern.", e)),
    };

    thread::Builder::new()
        .name("media-agent-camera".into())
        .spawn(move || {
            if let Ok(cam) = camera_manager {
                if let Err(e) = camera_loop(cam, local_frame_tx, target_fps) {
                    eprintln!("camera loop stopped: {e:?}");
                }
            }
        })
        .ok();

    (local_frame_rx, status)
}

fn synthetic_loop(tx: Sender<VideoFrame>, target_fps: u32) -> Result<()> {
    let fps = target_fps.clamp(1, 120);
    let period = Duration::from_millis(1_000 / fps as u64);
    let mut phase = 0u8;
    loop {
        let frame = VideoFrame::synthetic(320, 240, phase);
        phase = phase.wrapping_add(1);
        if tx.send(frame).is_err() {
            break;
        }
        thread::sleep(period);
    }
    Ok(())
}

fn camera_loop(
    mut cam: CameraManager,
    local_frame_tx: Sender<VideoFrame>,
    target_fps: u32,
) -> Result<()> {
    let fps = target_fps.clamp(1, 120);
    let frame_period = Duration::from_nanos(1_000_000_000u64 / fps as u64);
    let mut next_deadline = Instant::now() + frame_period;

    let (w, h) = (cam.width(), cam.height());
    let mut bgr_mat = Mat::default();
    let mut rgb_mat = Mat::default();

    loop {
        match cam.get_frame() {
            Ok(frame) => {
                bgr_mat = frame;

                imgproc::cvt_color(
                    &bgr_mat,
                    &mut rgb_mat,
                    imgproc::COLOR_BGR2RGB,
                    0,
                    AlgorithmHint::ALGO_HINT_DEFAULT,
                )
                .map_err(|e| MediaAgentError::Io(format!("cvtColor: {e}")))?;

                let bytes = tight_rgb_bytes(&rgb_mat, w, h)
                    .map_err(|e| MediaAgentError::Io(format!("pack RGB: {e}")))?;

                let vf = VideoFrame {
                    width: w,
                    height: h,
                    timestamp_ms: now_millis(), // UI/reference only; RTP uses its own clock
                    format: FrameFormat::Rgb,
                    bytes: Arc::new(bytes),
                };
                if local_frame_tx.send(vf).is_err() {
                    break;
                }
            }
            Err(err) => match err {
                CameraError::NotFrame | CameraError::CaptureFailed(_) => {
                    // Loggear y continuar, no detiene la app
                    eprintln!("Warning: camera did not return a valid frame: {}", err);
                }
                CameraError::CameraOff | CameraError::InitializationFailed(_) => {
                    // Mostrar UI o intentar reinicializar la cámara
                    eprintln!("Critical camera error: {}", err);
                    // opcional: intentar reinicializar
                    // cam.reinit()?;
                }
                CameraError::OpenCvError(e) => {
                    // Loggear y decidir si continuar o no
                    eprintln!("OpenCV error: {}", e);
                }
                _ => {
                    eprintln!("Unexpected camera error: {}", err);
                }
            },
        }

        // Pace to target FPS with drift correction
        let now = Instant::now();
        if now < next_deadline {
            thread::sleep(next_deadline - now);
            next_deadline += frame_period;
        } else {
            // we're late; skip ahead exactly one period to prevent accumulating drift
            next_deadline = now + frame_period;
        }
    }

    Ok(())
}

/// Always returns tightly packed RGB (len = width*height*3), regardless of stride/continuity.
fn tight_rgb_bytes(mat: &Mat, width: u32, height: u32) -> opencv::Result<Vec<u8>> {
    // Ensure 8UC3
    if mat.typ() != CV_8UC3 {
        let mut fixed = Mat::default();
        mat.convert_to(&mut fixed, CV_8UC3, 1.0, 0.0)?;
        return tight_rgb_bytes(&fixed, width, height);
    }

    // Force a continuous buffer if needed
    let m = if mat.is_continuous() {
        mat.try_clone()?
    } else {
        mat.clone()
    };

    let w = width as usize;
    let h = height as usize;
    let ch = m.channels() as usize; // 3
    let expected = w * h * ch;

    let data = m.data_bytes()?;

    // Fast path: already tight
    if data.len() == expected {
        return Ok(data.to_vec());
    }

    // Row-copy using actual step
    let step_elems = m.step1(0)? as usize;
    let elem_size = m.elem_size()? as usize;
    let step_bytes = step_elems * elem_size;

    let cols = m.cols() as usize;
    let rows = m.rows() as usize;
    let row_bytes = cols * ch;

    let mut out = vec![0u8; rows * row_bytes];
    for r in 0..rows {
        let src = &data[r * step_bytes..r * step_bytes + row_bytes];
        let dst = &mut out[r * row_bytes..(r + 1) * row_bytes];
        dst.copy_from_slice(src);
    }
    Ok(out)
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
