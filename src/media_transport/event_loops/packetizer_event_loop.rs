use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};

use crate::{
    app::log_sink::LogSink,
    core::{events::EngineEvent, session::Session},
    media_transport::{
        codec::CodecDescriptor, event_loops::constants::RECV_TIMEOUT, events::PacketizerEvent,
    },
    rtp_session::outbound_track_handle::OutboundTrackHandle,
    sink_debug, sink_error, sink_info,
};

pub struct PacketizerEventLoop {
    logger: Arc<dyn LogSink>,
    running_flag: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    event_loop_handler: Option<JoinHandle<()>>,
}

impl PacketizerEventLoop {
    pub fn new(logger: Arc<dyn LogSink>) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let running_flag = Arc::new(AtomicBool::new(false));
        Self {
            logger,
            running_flag,
            stop_flag,
            event_loop_handler: None,
        }
    }

    pub fn start(
        &mut self,
        packetizer_event_rx: Receiver<PacketizerEvent>,
        outbound_tracks: Arc<Mutex<HashMap<u8, OutboundTrackHandle>>>,
        payload_map: Arc<HashMap<u8, CodecDescriptor>>,
        session: Arc<Mutex<Option<Session>>>,
        event_tx: Sender<EngineEvent>,
    ) {
        let stop_flag = self.stop_flag.clone();
        let running_flag = self.running_flag.clone();

        let logger = self.logger.clone();
        let handle = std::thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                const TIMEOUT: Duration = Duration::from_millis(RECV_TIMEOUT);

                match packetizer_event_rx.recv_timeout(TIMEOUT) {
                    Ok(event) => match event {
                        PacketizerEvent::FramePacketized(frame) => {
                            sink_info!(
                                logger,
                                "[Packetizer Event Loop (MT)] Received FramePacketized from Packetizer"
                            );
                            let guard = outbound_tracks.lock().unwrap();
                            let Some((&pt, _)) = payload_map
                                .iter()
                                .find(|(_pt, desc)| desc.spec == frame.codec_spec)
                            else {
                                sink_error!(
                                    logger,
                                    "[Packetizer Event Loop (MT)] No outbound codec matches codec {:?}",
                                    frame.codec_spec
                                );
                                continue;
                            };
                            sink_debug!(
                                logger,
                                "[Packetizer] outbound_tracks keys: {:?}",
                                guard.keys().collect::<Vec<_>>()
                            );
                            let Some(handle) = guard.get(&pt) else {
                                sink_error!(
                                    logger,
                                    "[Packetizer Event Loop MT] No outbound track for PT {} ({:?})",
                                    pt,
                                    frame.codec_spec
                                );
                                continue;
                            };
                            let mut sess_guard = session.lock().unwrap();
                            sink_info!(
                                logger,
                                "[Packetizer Event Loop (MT)] Using Session to send frame"
                            );
                            if let Some(sess) = sess_guard.as_mut()
                                && let Err(e) = sess.send_rtp_chunks_for_frame(
                                    handle.local_ssrc,
                                    &frame.chunks,
                                    frame.rtp_ts,
                                )
                            {
                                let _ = event_tx.send(EngineEvent::Error(format!(
                                    "[Packetizer Event Loop (MT)] send local frame failed: {e:?}"
                                )));
                            }
                        }
                    },

                    Err(RecvTimeoutError::Disconnected) => {
                        sink_error!(
                            logger,
                            "[MT Event Loop Pack] The channel has been disconnected"
                        );
                        running_flag.store(false, Ordering::SeqCst);
                        break;
                    }

                    Err(RecvTimeoutError::Timeout) => {
                        #[cfg(debug_assertions)]
                        sink_debug!(
                            logger,
                            "[MT Event Loop Pack] The channel received nothing in {}ms",
                            RECV_TIMEOUT
                        );
                    }
                }
            }

            sink_info!(
                logger,
                "[MT Event Loop Depack] Event Loop has received the order to stop"
            );
            running_flag.store(false, Ordering::SeqCst);
        });
        self.running_flag.store(true, Ordering::SeqCst);
        self.event_loop_handler = Some(handle);
    }

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
