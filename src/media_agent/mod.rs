use std::{
    collections::HashMap, error::Error, fs, path::PathBuf, sync::{
        mpsc::{self, Receiver, Sender}, Arc, Mutex
    }, thread, time::{Duration, Instant, SystemTime}
};

use crate::{
    core::{events::EngineEvent, session::Session},
    rtp_session::{outbound_track_handle::OutboundTrackHandle, rtp_codec::RtpCodec},
};

use openh264::{
    decoder::{DecodedYUV, Decoder}, encoder::Encoder, formats::YUVSource, nal_units
};

type Result<T> = std::result::Result<T, MediaAgentError>;

#[derive(Debug)]
pub enum MediaAgentError {
    Camera(String),
    Codec(String),
    Send(String),
}

#[derive(Debug, Clone, Copy)]
pub enum FrameFormat {
    Rgb,
    Yuv420,
}

/// Implementa YUVSource para un slice en memoria (I420)
struct YuvView<'a> {
    width: i32,
    height: i32,
    y: &'a [u8],
    u: &'a [u8],
    v: &'a [u8],
    stride_y: i32,
    stride_u: i32,
    stride_v: i32,
}

impl<'a> YUVSource for YuvView<'a> {
    fn dimensions_i32(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    fn dimensions(&self) -> (usize, usize) {
        (self.width as usize, self.height as usize)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.stride_y as usize, self.stride_u as usize, self.stride_v as usize)
    }

    fn strides_i32(&self) -> (i32, i32, i32) {
        (self.stride_y, self.stride_u, self.stride_v)
    }

    fn y(&self) -> &[u8] { self.y }
    fn u(&self) -> &[u8] { self.u }
    fn v(&self) -> &[u8] { self.v }

    // write_rgb* not required for YUVSource trait; the trait in the crate defines the above
}

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub timestamp_ms: u128,
    pub format: FrameFormat,
    pub bytes: Arc<Vec<u8>>,
}

impl YUVSource for VideoFrame {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize) {
        todo!()
    }

    fn y(&self) -> &[u8] {
        todo!()
    }

    fn u(&self) -> &[u8] {
        todo!()
    }

    fn v(&self) -> &[u8] {
        todo!()
    }
}

impl VideoFrame {
    fn synthetic(width: u32, height: u32, tick: u8) -> Self {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let r = x as u8 ^ tick;
                let g = y as u8 ^ tick;
                let b = (x.wrapping_add(y)) as u8 ^ tick;
                data.push(r);
                data.push(g);
                data.push(b);
            }
        }
        Self {
            width,
            height,
            format: FrameFormat::Rgb,
            bytes: Arc::new(data),
            timestamp_ms: now_millis(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodecDescriptor {
    pub name: &'static str,
    pub rtp: RtpCodec,
    pub fmtp: Option<String>,
}

impl CodecDescriptor {
    pub fn h264_dynamic(pt: u8) -> Self {
        Self {
            name: "H264",
            rtp: RtpCodec::with_name(pt, 90_000, "H264"),
            fmtp: Some("profile-level-id=42e01f;packetization-mode=1".into()),
        }
    }
}

pub struct MediaAgent {
    tx_evt: Sender<EngineEvent>,
    payload_map: HashMap<u8, CodecDescriptor>,
    outbound_tracks: HashMap<u8, OutboundTrackHandle>,
    h264_decoder: Mutex<Decoder>,
    h264_encoder: Mutex<Encoder>,
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

        Self {
            tx_evt,
            payload_map,
            outbound_tracks: HashMap::new(),
            h264_decoder: Mutex::new(Decoder::new()?),
            h264_encoder: Mutex::new(Encoder::new()?),
            local_frame_rx: Some(rx),
            local_frame: Arc::new(Mutex::new(None)),
            remote_frame: Arc::new(Mutex::new(None)),
            last_local_frame_sent: None,
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

        let now = Instant::now();
        if let Some(last) = self.last_local_frame_sent {
            if now.duration_since(last) < Duration::from_millis(200) {
                return Ok(());
            }
        }

        let handle = match self.outbound_tracks.values().next() {
            Some(h) => h,
            None => return Ok(()),
        };

        let payload = encode_h264(&frame);
        session
            .send_media_frame(handle, &payload)
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
        // Bloqueás el decoder porque está compartido
        let mut guard = self
            .h264_decoder
            .lock()
            .map_err(|_| MediaAgentError::Codec("decoder poisoned".into()))?;

        // Recorres las NALUs
        for nalu in nal_units(bytes) {
            // Cada decode puede devolver Ok(Some(frame)), Ok(None), o Err(_)
            match guard.decode(nalu) {
                Ok(Some(yuv)) => {
                    let video_frame = decodedyuv_to_rgbframe(&yuv);
                    return Ok(Some(video_frame))
                }, // frame completo listo
                Ok(None) => continue,                  // todavía no hay frame
                Err(e) => return Err(MediaAgentError::Codec(format!("decode error: {e}").into())),
            }
        }

        Ok(None) // No hubo frame todavía
    }
}

fn encode_h264(frame: &VideoFrame) -> Vec<u8> {
    if frame.format != FrameFormat::Yuv420{
        return Vec::new()
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

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

fn decodedyuv_to_rgbframe(yuv: &DecodedYUV) -> VideoFrame {
    let (width, height) = yuv.dimensions();
    let rgb_len = yuv.rgb8_len();
    let mut rgb = vec![0u8; rgb_len];
    yuv.write_rgb8(&mut rgb);

    VideoFrame {
        width: width as u32,
        height: height as u32,
        timestamp_ms: yuv.timestamp().as_millis() as u128,
        format: FrameFormat::Rgb,
        bytes: Arc::new(rgb),
    }
}

struct H264Decoder {
    #[cfg(feature = "openh264")]
    inner: Option<openh264::decoder::Decoder>,
}

impl H264Decoder {
    fn new() -> Self {
        #[cfg(feature = "openh264")]
        {
            let inner = openh264::decoder::Decoder::new().ok();
            Self { inner }
        }
        #[cfg(not(feature = "openh264"))]
        {
            Self {}
        }
    }

    fn decode(&mut self, payload: &[u8]) -> Result<VideoFrame> {
        #[cfg(feature = "openh264")]
        {
            if let Some(decoder) = self.inner.as_mut() {
                match decoder.decode(payload) {
                    Ok(result) => {
                        if let Some(image) = result.image {
                            let plane = image.to_rgb();
                            return Ok(VideoFrame {
                                width: plane.width(),
                                height: plane.height(),
                                format: FrameFormat::Rgb,
                                bytes: Arc::new(plane.as_slice().to_vec()),
                                timestamp_ms: now_millis(),
                            });
                        }
                    }
                    Err(e) => {
                        return Err(MediaAgentError::Codec(format!(
                            "openh264 decode error: {e:?}"
                        )));
                    }
                }
            }
        }

        Ok(VideoFrame {
            width: 0,
            height: 0,
            format: FrameFormat::Rgb,
            bytes: Arc::new(payload.to_vec()),
            timestamp_ms: now_millis(),
        })
    }
}

