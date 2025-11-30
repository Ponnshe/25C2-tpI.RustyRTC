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
    sink_debug, sink_error, sink_info,
};

pub struct DepacketizerEventLoop {
    logger: Arc<dyn LogSink>,
    running_flag: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    event_loop_handler: Option<JoinHandle<()>>,
}

impl DepacketizerEventLoop {
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
        depacketizer_event_rx: Receiver<DepacketizerEvent>,
        media_agent_event_tx: Sender<MediaAgentEvent>,
    ) {
        let stop_flag = self.stop_flag.clone();
        let running_flag = self.running_flag.clone();

        let logger = self.logger.clone();

        let handle = std::thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                const TIMEOUT: Duration = Duration::from_millis(RECV_TIMEOUT);
                match depacketizer_event_rx.recv_timeout(TIMEOUT) {
                    Ok(event) => {
                        match event {
                            DepacketizerEvent::AnnexBFrameReady { codec_spec, bytes } => {
                                sink_info!(
                                    logger,
                                    "[DepacketizerEventLoop (MT)] Received AnnexBFrameReady. Sending it to MediaAgent"
                                );
                                media_agent_event_tx
                                    .send(MediaAgentEvent::AnnexBFrameReady { codec_spec, bytes })
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
                        sink_debug!(
                            logger,
                            "[MT Event Loop Depack] The channel received nothing in {}ms",
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
