use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};

use crate::{
    log::log_sink::LogSink,
    media_agent::events::MediaAgentEvent,
    media_transport::{event_loops::constants::RECV_TIMEOUT, events::DepacketizerEvent},
    sink_debug, sink_error, sink_info, sink_trace,
};

/// A dedicated background thread that bridges the `DepacketizerWorker` and the `MediaAgent`.
///
/// This event loop acts as a relay in the video **ingress** pipeline. It consumes reassembled
/// video frames produced by the depacketizer and forwards them to the central media agent
/// for decoding and display.
///
/// # Architecture
///
/// * **Input**: Receives `DepacketizerEvent` (e.g., `AnnexBFrameReady`) from the worker thread.
/// * **Processing**: Minimal processing (logging/tracing).
/// * **Output**: Converts and sends the data as `MediaAgentEvent` to the main agent loop.
///
/// Using this intermediate loop prevents the `DepacketizerWorker` (which handles tight network timings)
/// from blocking if the `MediaAgent` is busy processing other tasks.
pub struct DepacketizerEventLoop {
    logger: Arc<dyn LogSink>,
    running_flag: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    event_loop_handler: Option<JoinHandle<()>>,
}

impl DepacketizerEventLoop {
    /// Creates a new, stopped instance of the event loop.
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

    /// Starts the background thread.
    ///
    /// # Arguments
    ///
    /// * `depacketizer_event_rx` - Channel to receive events from the `DepacketizerWorker`.
    /// * `media_agent_event_tx` - Channel to forward events to the `MediaAgent`.
    pub fn start(
        &mut self,
        depacketizer_event_rx: Receiver<DepacketizerEvent>,
        media_agent_event_tx: Sender<MediaAgentEvent>,
    ) {
        let stop_flag = self.stop_flag.clone();
        let running_flag = self.running_flag.clone();

        let logger = self.logger.clone();

        let handle = std::thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                const TIMEOUT: Duration = Duration::from_millis(RECV_TIMEOUT);

                // Use recv_timeout to ensure we can check the `stop_flag` periodically
                // even if no video traffic is incoming.
                match depacketizer_event_rx.recv_timeout(TIMEOUT) {
                    Ok(event) => {
                        let _ = match event {
                            DepacketizerEvent::AnnexBFrameReady { codec_spec, bytes } => {
                                sink_trace!(
                                    logger,
                                    "[DepacketizerEventLoop (MT)] Received AnnexBFrameReady. Sending it to MediaAgent"
                                );
                                // Forward the reassembled frame to the upper layer
                                media_agent_event_tx
                                    .send(MediaAgentEvent::AnnexBFrameReady { codec_spec, bytes })
                            }
                            DepacketizerEvent::EncodedAudioFrameReady {
                                codec_spec,
                                payload,
                            } => {
                                sink_trace!(
                                    logger,
                                    "[DepacketizerEventLoop (MT)] Received EncodedAudioFrameReady. Sending it to MediaAgent"
                                );
                                media_agent_event_tx.send(MediaAgentEvent::EncodedAudioFrame {
                                    codec_spec,
                                    payload,
                                })
                            }
                        };
                    }

                    Err(RecvTimeoutError::Disconnected) => {
                        sink_error!(
                            logger,
                            "[MT Event Loop Depack] The channel has been disconnected"
                        );
                        running_flag.store(false, Ordering::SeqCst);
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        #[cfg(debug_assertions)]
                        sink_trace!(
                            logger,
                            "[MT Event Loop Depack] The channel received nothing in {}ms",
                            RECV_TIMEOUT
                        );
                    }
                }
            }
            sink_debug!(
                logger,
                "[MT Event Loop Depack] Event Loop has received the order to stop"
            );
            running_flag.store(false, Ordering::SeqCst);
        });

        self.running_flag.store(true, Ordering::SeqCst);
        self.event_loop_handler = Some(handle);
    }

    /// Signals the loop to stop and waits for the thread to join.
    ///
    /// # Panics
    ///
    /// Panics if the thread cannot be joined (e.g., if it panicked internally).
    #[allow(clippy::expect_used)]
    pub fn stop(&mut self) {
        sink_debug!(self.logger, "[MT Event Loop MA] Stopping the event loop");
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
