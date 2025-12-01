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
    core::{events::EngineEvent, session::Session},
    log::log_sink::LogSink,
    media_agent::events::MediaAgentEvent,
    media_transport::{
        codec::CodecDescriptor,
        error::{MediaTransportError, Result},
        event_loops::constants::RECV_TIMEOUT,
        media_transport_event::{MediaTransportEvent, RtpIn},
        packetizer_worker::PacketizeOrder,
    },
    rtp_session::outbound_track_handle::OutboundTrackHandle,
    sink_debug, sink_error, sink_info, sink_trace,
};

pub struct MediaAgentEventLoop {
    logger: Arc<dyn LogSink>,
    running_flag: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    event_loop_handler: Option<JoinHandle<()>>,
    target_fps: u32,
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
        }
    }
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

        let rtp_ts_step = 90_000 / self.target_fps;

        let logger = self.logger.clone();

        let handle = std::thread::spawn(move || {
            let mut last_received_local_ts_ms = None;
            let mut rtp_ts = rand::random::<u32>();

            while !stop_flag.load(Ordering::SeqCst) {
                match media_transport_event_rx.recv_timeout(Duration::from_millis(RECV_TIMEOUT)) {
                    Ok(event) => match event {
                        MediaTransportEvent::SendEncodedFrame {
                            annexb_frame,
                            timestamp_ms,
                            codec_spec,
                        } => {
                            sink_debug!(
                                logger.clone(),
                                "[MT Event Loop MA] Received SendEncodedFrame."
                            );
                            if last_received_local_ts_ms == Some(timestamp_ms) {
                                continue;
                            }
                            last_received_local_ts_ms = Some(timestamp_ms);

                            let order = PacketizeOrder {
                                annexb_frame,
                                rtp_ts,
                                codec_spec,
                            };
                            sink_trace!(
                                logger.clone(),
                                "[MT Event Loop MA] Sending PacketizeOrder to Packetizer."
                            );
                            if packetizer_order_tx.send(order).is_ok() {
                                rtp_ts = rtp_ts.wrapping_add(rtp_ts_step);
                            }
                        }
                        MediaTransportEvent::RtpIn(pkt) => {
                            sink_trace!(
                                logger,
                                "[MediaAgent Event Loop (MT)] Received Rtp Packet. Sending it to Depacketizer"
                            );
                            sink_trace!(
                                logger,
                                "[MediaAgent Event Loop (MT)] ssrc: {}, seq: {}",
                                pkt.ssrc,
                                pkt.seq,
                            );
                            let _ = rtp_tx.try_send(pkt.clone());
                        }
                        MediaTransportEvent::Established => {
                            sink_info!(logger, "[MediaAgent Event Loop (MT)] Received Established");
                            let mut sess_guard = session.lock().expect("session lock poisoned");
                            if let Some(sess) = sess_guard.as_mut() {
                                if let Err(e) = ensure_outbound_tracks(
                                    sess,
                                    payload_map.clone(),
                                    outbound_tracks.clone(),
                                    logger.clone(),
                                ) {
                                    let _ = event_tx
                                        .send(EngineEvent::Error(format!("media tracks: {e:?}")));
                                }
                                let allowed_pts = allowed_pts.clone();
                                if let Ok(mut w) = allowed_pts.write() {
                                    w.clear();
                                    w.extend(sess.remote_codecs.iter().map(|c| c.payload_type));
                                }
                            }
                        }
                        MediaTransportEvent::Closing | MediaTransportEvent::Closed => {
                            let mut guard = outbound_tracks
                                .lock()
                                .expect("outbound_tracks lock poisoned");
                            guard.clear();
                        }
                        MediaTransportEvent::UpdateBitrate(b) => {
                            sink_info!(
                                logger,
                                "[MediaTransport] Telling MediaAgent to update bitrate {}",
                                b
                            );
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
