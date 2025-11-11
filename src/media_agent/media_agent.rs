use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    camera_manager::{camera_error::CameraError, camera_manager_c::CameraManager},
    core::{
        events::{EngineEvent, RtpIn},
        session::Session,
    },
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
            h264_depacketizer::H264Depacketizer, h264_packetizer::H264Packetizer,
            rtp_payload_chunk::RtpPayloadChunk,
        },
        rtp_codec::RtpCodec,
    },
    sink_log,
};

use super::constants::{BITRATE, KEYINT, TARGET_FPS};

use opencv::{
    core::{AlgorithmHint, CV_8UC3, Mat},
    imgproc,
    prelude::*,
};

pub struct MediaAgent {
    event_tx: Sender<EngineEvent>,
    rtp_tx: mpsc::SyncSender<RtpIn>,
    logger: Arc<dyn LogSink>,
    rtp_decoder_handle: Option<JoinHandle<()>>,
    payload_map: HashMap<u8, CodecDescriptor>,
    outbound_tracks: HashMap<u8, OutboundTrackHandle>,

    h264_encoder: Mutex<H264Encoder>,
    h264_packetizer: H264Packetizer,

    local_frame_rx: Option<Receiver<VideoFrame>>,
    local_frame: Arc<Mutex<Option<VideoFrame>>>,
    remote_frame: Arc<Mutex<Option<VideoFrame>>>,
    allowed_pts: Arc<RwLock<HashSet<u8>>>,

    last_local_frame_sent: Option<Instant>,
    rtp_ts: u32,
    rtp_ts_step: u32,
    sent_any_frame: bool,
    last_sent_local_ts_ms: Option<u128>,
}

impl MediaAgent {
    pub fn new(event_tx: Sender<EngineEvent>, logger: Arc<dyn LogSink>) -> Self {
        let mut payload_map = HashMap::new();
        let pt = 96;
        payload_map.insert(pt, CodecDescriptor::h264_dynamic(pt));

        let h264_encoder = Mutex::new(H264Encoder::new(TARGET_FPS, BITRATE, KEYINT));
        let h264_packetizer = H264Packetizer::new(1200);

        let remote_frame = Arc::new(Mutex::new(None));

        // Bounded channel to avoid UI stalls / unbounded growth
        let (rtp_tx, rtp_rx) = mpsc::sync_channel::<RtpIn>(2048);

        let allowed_pts = Arc::new(RwLock::new(
            payload_map.keys().copied().collect::<HashSet<u8>>(),
        ));
        let rtp_decoder_handle = Some(spawn_rtp_decoder_worker(
            Arc::clone(&logger),
            Arc::clone(&allowed_pts),
            Arc::clone(&remote_frame),
            rtp_rx,
        ));
        let (rx, status) = spawn_camera_worker(TARGET_FPS, logger.clone());
        if let Some(msg) = status {
            let _ = event_tx.send(EngineEvent::Status(format!("[MediaAgent] {msg}")));
        }

        Self {
            event_tx,
            rtp_tx,
            logger,
            rtp_decoder_handle,
            allowed_pts,
            payload_map,
            outbound_tracks: HashMap::new(),
            h264_encoder,
            h264_packetizer,
            local_frame_rx: Some(rx),
            local_frame: Arc::new(Mutex::new(None)),
            remote_frame,
            last_local_frame_sent: None,
            rtp_ts: rand::random::<u32>(),
            rtp_ts_step: 90_000 / TARGET_FPS,
            sent_any_frame: false,
            last_sent_local_ts_ms: None,
        }
    }

    pub const fn payload_mapping(&self) -> &HashMap<u8, CodecDescriptor> {
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
                if let Some(sess) = session
                    && let Err(e) = self.ensure_outbound_tracks(sess)
                {
                    let _ = self
                        .event_tx
                        .send(EngineEvent::Error(format!("media tracks: {e:?}")));
                    // if let Ok(mut w) = self.allowed_pts.write() {
                    //     w.clear();
                    //     w.extend(sess.remote_codecs.iter().map(|c| c.payload_type));
                    // }
                }
            }
            EngineEvent::Closed | EngineEvent::Closing { .. } => {
                self.outbound_tracks.clear();
                self.last_local_frame_sent = None;
            }
            EngineEvent::RtpIn(pkt) => {
                // Non-blocking send; drop if the worker is saturated
                let _ = self.rtp_tx.try_send(RtpIn {
                    payload: pkt.payload.clone(),
                    marker: pkt.marker,
                    timestamp_90khz: pkt.timestamp_90khz,
                    seq: pkt.seq,
                    pt: pkt.pt,
                    ssrc: pkt.ssrc,
                });
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, session: Option<&Session>) {
        self.drain_local_frames();
        if let Some(session_handle) = session
            && let Err(e) = self.maybe_send_local_frame(session_handle)
        {
            let _ = self.event_tx.send(EngineEvent::Error(format!(
                "[MediaAgent] send local frame failed: {e:?}"
            )));
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
        if !self.sent_any_frame
            && let Ok(mut enc) = self.h264_encoder.lock()
        {
            enc.request_keyframe();
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

    fn drain_local_frames(&self) {
        if let Some(rx) = &self.local_frame_rx {
            while let Ok(frame) = rx.try_recv() {
                if let Ok(mut guard) = self.local_frame.lock() {
                    *guard = Some(frame);
                }
            }
        }
    }

    pub fn set_bitrate(&mut self, new_bitrate: u32) {
        let new_fps;
        let new_keyint;

        if new_bitrate >= 1_500_000 {
            new_fps = 30;
            new_keyint = 60;
        } else if new_bitrate >= 800_000 {
            new_fps = 25;
            new_keyint = 90;
        } else {
            new_fps = 20;
            new_keyint = 120;
        }

        let mut updated = false;
        if let Ok(mut enc) = self.h264_encoder.lock() {
            match enc.set_config(new_fps, new_bitrate, new_keyint) {
                Ok(u) => updated = u,
                Err(e) => {
                    sink_log!(
                        self.logger.as_ref(),
                        LogLevel::Error,
                        "Failed to update H264 encoder config: {:?}",
                        e
                    );
                }
            }
        }

        if updated {
            self.rtp_ts_step = 90_000 / new_fps;
            sink_log!(
                self.logger.as_ref(),
                LogLevel::Info,
                "Reconfigured H264 encoder: bitrate={}bps, fps={}, keyint={}",
                new_bitrate,
                new_fps,
                new_keyint,
            );
        }
    }
}

fn spawn_rtp_decoder_worker(
    logger: Arc<dyn LogSink>,
    allowed_pts: Arc<RwLock<HashSet<u8>>>,
    remote_frame_slot: Arc<Mutex<Option<VideoFrame>>>,
    rtp_rx: Receiver<RtpIn>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("media-agent-rtp".into())
        .spawn(move || {
            let mut depack = H264Depacketizer::new();
            let mut decoder = H264Decoder::new();

            while let Ok(pkt) = rtp_rx.recv() {
                let ok_pt = allowed_pts
                    .read()
                    .map(|set| set.contains(&pkt.pt))
                    .unwrap_or(false);
                if !ok_pt {
                    sink_log!(
                        logger.as_ref(),
                        LogLevel::Debug,
                        "[MediaAgent] dropping RTP PT={}",
                        pkt.pt
                    );
                    continue;
                }
                if let Some(au) =
                    depack.push_rtp(&pkt.payload, pkt.marker, pkt.timestamp_90khz, pkt.seq)
                {
                    match decoder.decode_au(&au) {
                        Ok(Some(frame)) => {
                            if let Ok(mut g) = remote_frame_slot.lock() {
                                *g = Some(frame);
                            }
                        }
                        Ok(None) => {
                            sink_log!(
                                logger.as_ref(),
                                LogLevel::Debug,
                                "[MediaAgent] decoder needs more NALs for this AU"
                            );
                        }
                        Err(e) => {
                            sink_log!(
                                logger.as_ref(),
                                LogLevel::Error,
                                "[MediaAgent] decode error: {e:?}"
                            );
                        }
                    }
                }
            }
        })
        .expect("spawn media-agent-rtp")
}

fn spawn_camera_worker(
    target_fps: u32,
    logger: Arc<dyn LogSink>,
) -> (Receiver<VideoFrame>, Option<String>) {
    let (local_frame_tx, local_frame_rx) = mpsc::channel();
    let camera_manager = CameraManager::new(0, logger);

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
