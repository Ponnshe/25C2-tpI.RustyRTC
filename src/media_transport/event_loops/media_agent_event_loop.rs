use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender},
    },
    thread::JoinHandle,
    time::Duration,
};

use crate::{
    audio::types::AudioFrame, core::{events::EngineEvent, session::Session}, log::log_sink::LogSink, media_agent::events::MediaAgentEvent, media_transport::{
        codec::CodecDescriptor,
        error::{MediaTransportError, Result},
        event_loops::constants::{RECV_TIMEOUT, RTP_PT_AUDIO_PCM},
        media_transport_event::{MediaTransportEvent, RtpIn},
        packetizer_worker::PacketizeOrder,
    }, rtp::rtp_sender::RtpSender, rtp_session::outbound_track_handle::OutboundTrackHandle, sink_debug, sink_error, sink_info, sink_trace, sink_warn
};

/// The central control loop for the Media Transport's Egress (Outgoing) pipeline.
///
/// This event loop is responsible for:
/// 1. **Frame Scheduling**: Receiving encoded frames, assigning RTP timestamps, and ordering the Packetizer.
/// 2. **Session Management**: Reacting to connection events (`Established`) to register RTP tracks.
/// 3. **Flow Control**: Handling bitrate updates and forwarding them to the Media Agent.
pub struct MediaAgentEventLoop {
    logger: Arc<dyn LogSink>,
    running_flag: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    event_loop_handler: Option<JoinHandle<()>>,
    target_fps: u32,
    audio_rtp_sender: Option<RtpSender>,
}

impl MediaAgentEventLoop {
    pub fn new(target_fps: u32, logger: Arc<dyn LogSink>) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let running_flag = Arc::new(AtomicBool::new(false));
        Self {
            logger,
            running_flag,
            stop_flag,
            event_loop_handler: None,
            target_fps,
            audio_rtp_sender: None,
        }
    }
    

    /// Starts the event loop thread.
    ///
    /// # Arguments
    ///
    /// * `media_transport_event_rx`: Input channel for transport events (Encoded frames, session status).
    /// * `packetizer_order_tx`: Output channel to instruct the Packetizer worker.
    /// * `rtp_tx`: Direct channel to the UDP socket for raw packet sending.
    /// * `session`: Reference to the core RTP Session.
    /// * `payload_map`: Configured codecs.
    /// * `outbound_tracks`: State of active outbound RTP streams.
    /// * `event_tx`: Channel to report errors/status to the main Engine.
    /// * `allowed_pts`: Set of allowed Payload Types (updated upon negotiation).
    /// * `media_agent_tx`: Back-channel to the Media Agent (e.g., for bitrate commands).
    #[allow(clippy::too_many_arguments, clippy::similar_names)]
    #[allow(clippy::expect_used)]
    pub fn start(
        &mut self,
        media_transport_event_rx: Receiver<MediaTransportEvent>,
        packetizer_order_tx: Sender<PacketizeOrder>,
        rtp_tx: SyncSender<RtpIn>,
        session: Arc<Mutex<Option<Session>>>,
        payload_map: Arc<HashMap<u8, CodecDescriptor>>,
        outbound_tracks: Arc<Mutex<HashMap<u8, OutboundTrackHandle>>>,
        event_tx: Sender<EngineEvent>,
        allowed_pts: Arc<RwLock<HashSet<u8>>>,
        media_agent_tx: Sender<MediaAgentEvent>,
    ) {
        let stop_flag = self.stop_flag.clone();
        let running_flag = self.running_flag.clone();
        let mut audio_sender = self.audio_rtp_sender.take();

        // Calculate the RTP timestamp increment per frame (90kHz clock).
        // E.g., for 30fps: 90000 / 30 = 3000 ticks per frame.
        let rtp_ts_step = 90_000 / self.target_fps;

        let logger = self.logger.clone();

        let handle = std::thread::spawn(move || {
            let mut audio_sender = audio_sender.take().unwrap_or_else(|| {
                RtpSender::audio_opus(rand::random())
            });
            let mut last_received_local_ts_ms = None;
            // Initialize random start timestamp for security/standard compliance.
            let mut rtp_ts = rand::random::<u32>();

            while !stop_flag.load(Ordering::SeqCst) {
                match media_transport_event_rx.recv_timeout(Duration::from_millis(RECV_TIMEOUT)) {
                    Ok(event) => match event {
                        // --- Egress Video Path ---
                        MediaTransportEvent::SendEncodedFrame {
                            annexb_frame,
                            timestamp_ms,
                            codec_spec, 
                        } => {
                            sink_debug!(
                                logger.clone(),
                                "[MT Event Loop MA] Received SendEncodedFrame."
                            );
                            // Simple deduplication logic
                            if last_received_local_ts_ms == Some(timestamp_ms) {
                                continue;
                            }
                            last_received_local_ts_ms = Some(timestamp_ms);

                            // Construct the order for the packetizer worker
                            let order = PacketizeOrder {
                                annexb_frame,
                                rtp_ts, // Assign the monotonic RTP timestamp
                                codec_spec,
                            };
                            
                            sink_trace!(
                                logger.clone(),
                                "[MT Event Loop MA] Sending PacketizeOrder to Packetizer."
                            );
                            
                            // Send to Packetizer and increment timestamp for the next frame
                            if packetizer_order_tx.send(order).is_ok() {
                                rtp_ts = rtp_ts.wrapping_add(rtp_ts_step);
                            }
                        }
                        
                        // --- Raw Packet Forwarding ---
                        MediaTransportEvent::RtpIn(pkt) => {
                            // PT típico de audio (el que estés usando)
                            if pkt.pt == RTP_PT_AUDIO_PCM {
                                if let Some(frame) = decode_rtp_audio(pkt) {
                                    let _ = media_agent_tx.send(
                                        MediaAgentEvent::RemoteAudioFrame(frame)
                                    );
                                }
                            } else {
                                sink_trace!(
                                    logger,
                                    "[MediaAgent Event Loop (MT)] Forwarding raw RTP/RTCP packet to socket"
                                );
                                let _ = rtp_tx.try_send(pkt.clone());
                            }
                        }

                        // --- Control Plane: Connection Established ---
                        MediaTransportEvent::Established => {
                            sink_info!(logger, "[MediaAgent Event Loop (MT)] Received Established");
                            let mut sess_guard = session.lock().expect("session lock poisoned");
                            
                            if let Some(sess) = sess_guard.as_mut() {
                                // 1. Register outbound tracks (SSRCs) in the RTP session
                                if let Err(e) = ensure_outbound_tracks(
                                    sess,
                                    payload_map.clone(),
                                    outbound_tracks.clone(),
                                    logger.clone(),
                                ) {
                                    let _ = event_tx
                                        .send(EngineEvent::Error(format!("media tracks: {e:?}")));
                                }

                                // 2. Update allowed Payload Types based on remote SDP negotiation
                                let allowed_pts = allowed_pts.clone();
                                if let Ok(mut w) = allowed_pts.write() {
                                    w.clear();
                                    w.extend(sess.remote_codecs.iter().map(|c| c.payload_type));
                                }
                            }
                        }

                        //Send Audio Frame: Recognized audio
                        MediaTransportEvent::SendAudioFrame { samples, timestamp, channels } => {
                            if let Err(e) = send_rtp_audio_thread(&mut audio_sender, &rtp_tx, samples, timestamp, channels, &logger) {
                                sink_warn!(logger, "[audio] send_rtp_audio failed: {e}");
                            }
                        }                        

                        // --- Control Plane: Cleanup ---
                        MediaTransportEvent::Closing | MediaTransportEvent::Closed => {
                            let mut guard = outbound_tracks
                                .lock()
                                .expect("outbound_tracks lock poisoned");
                            guard.clear();
                        }

                        // --- Flow Control ---
                        MediaTransportEvent::UpdateBitrate(b) => {
                            sink_info!(
                                logger,
                                "[MediaTransport] Telling MediaAgent to update bitrate {}",
                                b
                            );
                            // Relay command back to the Application Layer (Encoder)
                            let _ = media_agent_tx.send(MediaAgentEvent::UpdateBitrate(b));
                        }
                    },

                    Err(RecvTimeoutError::Disconnected) => {
                        sink_error!(
                            logger,
                            "[MT Event Loop MA] The channel has been disconnected"
                        );
                        running_flag.store(false, Ordering::SeqCst);
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        #[cfg(debug_assertions)]
                        sink_debug!(
                            logger,
                            "[MT Event Loop MA] The channel received nothing in {}ms",
                            RECV_TIMEOUT
                        );
                    }
                }
            }
            sink_info!(
                logger,
                "[MT Event Loop MA] Event Loop has received the order to stop"
            );
            running_flag.store(false, Ordering::SeqCst);
        });
        self.running_flag.store(true, Ordering::SeqCst);
        self.event_loop_handler = Some(handle);
    }

    #[allow(clippy::expect_used)]
    pub fn stop(&mut self) {
        sink_info!(self.logger, "[MT Event Loop MA] Stopping the event loop");
        self.stop_flag.store(true, Ordering::SeqCst);

        if let Some(handle) = self.event_loop_handler.take() {
            handle.join().expect("Failed to join event loop thread");
        }

        sink_info!(
            self.logger,
            "[MT Event Loop MA] The event loop has been stopped"
        );
    }
    
}

fn send_rtp_audio_thread(
    sender: &mut RtpSender,
    rtp_tx: &SyncSender<RtpIn>,
    samples: Vec<i16>,
    _timestamp: Duration,
    channels: u16,
    logger: &Arc<dyn LogSink>,
) -> Result<()> {
    let mut payload = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        payload.extend_from_slice(&s.to_le_bytes());
    }

    let samples_per_frame = (payload.len() / 2 / channels as usize) as u32;
    let packet = sender.build_packet(payload, false, samples_per_frame);

    let raw = packet
    .encode()
    .map_err(|e| MediaTransportError::Send(format!("rtp encode: {e:?}")))?;

    rtp_tx.try_send(RtpIn {
        pt: packet.payload_type(),
        marker: packet.marker(),
        timestamp_90khz: packet.timestamp(),
        seq: packet.seq(),
        ssrc: packet.ssrc(),
        payload: raw,
    })
    .map_err(|e| MediaTransportError::Send(format!("rtp_tx try_send: {e}")))?;


    sink_trace!(logger, "[audio] RTP sent seq={} ts={} ssrc={}", packet.seq(), packet.timestamp(), packet.ssrc());
    Ok(())
}

fn decode_rtp_audio(pkt: RtpIn) -> Option<AudioFrame> {
    // PCM i16 little-endian
    if pkt.payload.len() % 2 != 0 {
        return None;
    }

    let mut samples = Vec::with_capacity(pkt.payload.len() / 2);
    for chunk in pkt.payload.chunks_exact(2) {
        samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }

    let ms: u64 = (u64::from(pkt.timestamp_90khz) * 1000) / 48_000;

    Some(AudioFrame {
        timestamp: Duration::from_millis(ms),
        samples,
        channels: 1,
    })
}


/// Helper to register outbound tracks in the RTP session if they don't exist yet.
///
/// Ensures that for every supported codec in `payload_map`, there is a corresponding
/// `OutboundTrackHandle` in the session to manage SSRCs and sequence numbers.
#[allow(clippy::expect_used)]
fn ensure_outbound_tracks(
    session: &Session,
    payload_map: Arc<HashMap<u8, CodecDescriptor>>,
    outbound_tracks: Arc<Mutex<HashMap<u8, OutboundTrackHandle>>>,
    logger: Arc<dyn LogSink>,
) -> Result<()> {
    for (pt, codec) in payload_map.iter() {
        let mut guard = outbound_tracks
            .lock()
            .expect("outbound_tracks lock poisoned");
            
        if guard.contains_key(pt) {
            continue;
        }
        
        // Register new track with the underlying RTP session
        let handle = session
            .register_outbound_track(codec.rtp_representation.clone())
            .map_err(|e| MediaTransportError::Send(e.to_string()))?;
            
        sink_debug!(
            logger,
            "[ensure_outbound_tracks] Adding outbound track PT {} ({:?})",
            pt,
            codec.rtp_representation
        );
        guard.insert(*pt, handle);
    }
    Ok(())
}
