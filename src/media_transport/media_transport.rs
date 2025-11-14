use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, RwLock,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    core::{
        events::{EngineEvent, RtpIn},
        session::Session,
    },
    media_agent::{
        media_agent::MediaAgent,
        spec::CodecSpec,
        video_frame::VideoFrame,
    },
    media_transport::{
        codec::CodecDescriptor,
        error::{MediaTransportError, Result},
        payload::{
            h264_depacketizer::H264Depacketizer, h264_packetizer::H264Packetizer,
            rtp_payload_chunk::RtpPayloadChunk,
        },
        constants::{
            RTP_TX_CHANNEL_SIZE,
            DYNAMIC_PAYLOAD_TYPE_START,
        },
    },
    rtp_session::{outbound_track_handle::OutboundTrackHandle, rtp_codec::RtpCodec},
    sink_log,
};


pub struct MediaTransport {
    media_agent: Arc<MediaAgent>,
    event_tx: Sender<EngineEvent>,
    rtp_tx: mpsc::SyncSender<RtpIn>,
    _logger: Arc<dyn LogSink>,
    _rtp_decoder_handle: Option<JoinHandle<()>>,
    payload_map: HashMap<u8, CodecDescriptor>,
    outbound_tracks: HashMap<u8, OutboundTrackHandle>,
    h264_packetizer: H264Packetizer,
    allowed_pts: Arc<RwLock<HashSet<u8>>>,
    rtp_ts: u32,
    rtp_ts_step: u32,
    sent_any_frame: bool,
    last_sent_local_ts_ms: Option<u128>,
}

impl MediaTransport {
    pub fn new(event_tx: Sender<EngineEvent>, logger: Arc<dyn LogSink>) -> Self {
        let media_agent = Arc::new(MediaAgent::new(event_tx.clone(), logger.clone()));
        let mut payload_map = HashMap::new();
        let mut current_pt = DYNAMIC_PAYLOAD_TYPE_START;

        for spec in media_agent.supported_media() {
            let codec_descriptor = match spec.codec_spec {
                CodecSpec::H264 => CodecDescriptor::h264_dynamic(current_pt),
            };
            payload_map.insert(current_pt, codec_descriptor);
            current_pt += 1;
        }

        let h264_packetizer = H264Packetizer::new(1200);
        let (rtp_tx, rtp_rx) = mpsc::sync_channel::<RtpIn>(RTP_TX_CHANNEL_SIZE);
        let allowed_pts = Arc::new(RwLock::new(
            payload_map.keys().copied().collect::<HashSet<u8>>(),
        ));

        let media_agent_for_worker = Arc::clone(&media_agent);
        let payload_map_for_worker = payload_map.clone();

        let rtp_decoder_handle = Some(spawn_rtp_decoder_worker(
            logger.clone(),
            allowed_pts.clone(),
            rtp_rx,
            event_tx.clone(),
            media_agent_for_worker,
            payload_map_for_worker,
        ));

        let target_fps = 30; // TODO: Get from config

        Self {
            media_agent,
            event_tx,
            rtp_tx,
            _logger: logger,
            _rtp_decoder_handle: rtp_decoder_handle,
            payload_map,
            outbound_tracks: HashMap::new(),
            h264_packetizer,
            allowed_pts,
            rtp_ts: rand::random::<u32>(),
            rtp_ts_step: 90_000 / target_fps,
            sent_any_frame: false,
            last_sent_local_ts_ms: None,
        }
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
                    if let Ok(mut w) = self.allowed_pts.write() {
                        w.clear();
                        w.extend(sess.remote_codecs.iter().map(|c| c.payload_type));
                    }
                }
            }
            EngineEvent::Closed | EngineEvent::Closing { .. } => {
                self.outbound_tracks.clear();
            }
            EngineEvent::RtpIn(pkt) => {
                let _ = self.rtp_tx.try_send(RtpIn {
                    payload: pkt.payload.clone(),
                    marker: pkt.marker,
                    timestamp_90khz: pkt.timestamp_90khz,
                    seq: pkt.seq,
                    pt: pkt.pt,
                    ssrc: pkt.ssrc,
                });
            }
            EngineEvent::DecodedVideoFrame(frame) => {
                self.media_agent.set_remote_frame(*frame.clone());
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, session: Option<&Session>) {
        self.media_agent.tick();
        if let Some(session_handle) = session {
            if let Err(e) = self.maybe_send_local_frame(session_handle) {
                let _ = self.event_tx.send(EngineEvent::Error(format!(
                    "[MediaTransport] send local frame failed: {e:?}"
                )));
            }
        }
    }

    pub fn snapshot_frames(&self) -> (Option<VideoFrame>, Option<VideoFrame>) {
        self.media_agent.snapshot_frames()
    }

    pub fn set_bitrate(&mut self, new_bitrate: u32) {
        self.media_agent.set_bitrate(new_bitrate);
        let new_fps = if new_bitrate >= 1_500_000 {
            30
        } else if new_bitrate >= 800_000 {
            25
        } else {
            20
        };
        self.rtp_ts_step = 90_000 / new_fps;
    }

    pub fn codec_descriptors(&self) -> Vec<CodecDescriptor> {
        self.payload_map.values().cloned().collect()
    }

    pub fn local_rtp_codecs(&self) -> Vec<RtpCodec> {
        self.payload_map.values().map(|c| c.rtp.clone()).collect()
    }

    fn ensure_outbound_tracks(&mut self, session: &Session) -> Result<()> {
        for (pt, codec) in &self.payload_map {
            if self.outbound_tracks.contains_key(pt) {
                continue;
            }
            let handle = session
                .register_outbound_track(codec.rtp.clone())
                .map_err(|e| MediaTransportError::Send(e.to_string()))?;
            self.outbound_tracks.insert(*pt, handle);
        }
        Ok(())
    }

    fn maybe_send_local_frame(&mut self, session: &Session) -> Result<()> {
        if self.outbound_tracks.is_empty() {
            return Ok(());
        }

        let (handle, codec_spec) = match self.outbound_tracks.iter().next() {
            Some((pt, handle)) => {
                let spec = self
                    .payload_map
                    .get(pt)
                    .map(|desc| desc.spec)
                    .unwrap_or(CodecSpec::H264); // Fallback, should not happen
                (handle, spec)
            }
            None => return Ok(()),
        };

        let force_keyframe = !self.sent_any_frame;
        let Some((annexb_frame, timestamp_ms)) = self
            .media_agent
            .encode(codec_spec, force_keyframe)
            .map_err(|e| MediaTransportError::Send(e.to_string()))?
        else {
            return Ok(());
        };

        if self.last_sent_local_ts_ms == Some(timestamp_ms) {
            return Ok(());
        }

        let chunks: Vec<RtpPayloadChunk> = self
            .h264_packetizer
            .packetize_annexb_to_payloads(&annexb_frame);
        if chunks.is_empty() {
            return Ok(());
        }

        let ts90k = self.rtp_ts;

        session
            .send_rtp_chunks_for_frame(handle.local_ssrc, &chunks, ts90k)
            .map_err(|e| MediaTransportError::Send(e.to_string()))?;

        self.rtp_ts = self.rtp_ts.wrapping_add(self.rtp_ts_step);
        self.sent_any_frame = true;
        self.last_sent_local_ts_ms = Some(timestamp_ms);
        Ok(())
    }
}

fn spawn_rtp_decoder_worker(
    logger: Arc<dyn LogSink>,
    allowed_pts: Arc<RwLock<HashSet<u8>>>,
    rtp_rx: Receiver<RtpIn>,
    event_tx: Sender<EngineEvent>,
    media_agent: Arc<MediaAgent>,
    payload_map: HashMap<u8, CodecDescriptor>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("media-transport-rtp".into())
        .spawn(move || {
            let mut depack = H264Depacketizer::new();

            while let Ok(pkt) = rtp_rx.recv() {
                let ok_pt = allowed_pts
                    .read()
                    .map(|set| set.contains(&pkt.pt))
                    .unwrap_or(false);
                if !ok_pt {
                    sink_log!(
                        logger.as_ref(),
                        LogLevel::Debug,
                        "[MediaTransport] dropping RTP PT={}",
                        pkt.pt
                    );
                    continue;
                }

                let Some(codec_desc) = payload_map.get(&pkt.pt) else {
                    sink_log!(
                        logger.as_ref(),
                        LogLevel::Warn,
                        "[MediaTransport] unknown payload type {}",
                        pkt.pt
                    );
                    continue;
                };

                if let Some(au) =
                    depack.push_rtp(&pkt.payload, pkt.marker, pkt.timestamp_90khz, pkt.seq)
                {
                    match media_agent.decode(codec_desc.spec, &au) {
                        Ok(Some(frame)) => {
                            let _ = event_tx.send(EngineEvent::DecodedVideoFrame(Box::new(frame)));
                        }
                        Ok(None) => {
                            sink_log!(
                                logger.as_ref(),
                                LogLevel::Debug,
                                "[MediaTransport] decoder needs more NALs for this AU"
                            );
                        }
                        Err(e) => {
                            sink_log!(
                                logger.as_ref(),
                                LogLevel::Error,
                                "[MediaTransport] decode error: {e:?}"
                            );
                        }
                    }
                }
            }
        })
        .expect("spawn media-transport-rtp")
}
