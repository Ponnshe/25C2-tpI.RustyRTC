use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::config::Config;
use crate::core::events::EngineEvent;
use crate::file_handler::events::{FileHandlerEvents, ReaderCommands, WriterCommands};
use crate::file_handler::reader_worker::ReaderWorker;
use crate::file_handler::writer_worker::WriterWorker;
use crate::log::log_sink::LogSink;
use crate::sctp::events::SctpFileProperties;
use crate::{sink_debug, sink_error, sink_info, sink_trace, sink_warn};

enum WorkerTx {
    Reader(mpsc::Sender<ReaderCommands>),
    Writer(mpsc::Sender<WriterCommands>),
}

pub struct FileHandler {
    _config: Arc<Config>,
    tx_listener: Mutex<Option<mpsc::Sender<FileHandlerEvents>>>,
    workers: Arc<Mutex<HashMap<u32, WorkerTx>>>,
    log_sink: Arc<dyn LogSink>,
}

impl FileHandler {
    pub fn new(
        config: Arc<Config>,
        log_sink: Arc<dyn LogSink>,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let workers = Arc::new(Mutex::new(HashMap::new()));

        let tx_for_listener = tx.clone();
        let workers_clone = workers.clone();
        let config_clone = config.clone();
        let log_sink_clone = log_sink.clone();

        thread::spawn(move || {
            Self::listener_loop(
                rx,
                tx_for_listener,
                workers_clone,
                config_clone,
                log_sink_clone,
                event_tx,
            );
        });

        Self {
            _config: config,
            tx_listener: Mutex::new(Some(tx)),
            workers,
            log_sink,
        }
    }

    pub fn send(&self, event: FileHandlerEvents) -> Result<(), String> {
        let guard = self.tx_listener.lock().map_err(|e| e.to_string())?;
        if let Some(tx) = &*guard {
            tx.send(event).map_err(|e| e.to_string())
        } else {
            Err("FileHandler is shut down".to_string())
        }
    }

    pub fn shutdown(&self) {
        sink_info!(self.log_sink, "[FILE_HANDLER] Shutting down");

        // 1. Cancel all workers
        let mut workers = self.workers.lock().unwrap();
        for (_id, worker_tx) in workers.drain() {
            match worker_tx {
                WorkerTx::Reader(tx) => {
                    let _ = tx.send(ReaderCommands::Cancel);
                }
                WorkerTx::Writer(tx) => {
                    let _ = tx.send(WriterCommands::Cancel);
                }
            }
            sink_debug!(self.log_sink, "[FILE_HANDLER] Sent Cancel to worker {}", _id);
        }

        // 2. Drop the listener sender to signal the listener loop to potentially stop (once workers drop theirs)
        if let Ok(mut guard) = self.tx_listener.lock() {
            *guard = None;
        }
    }

    fn listener_loop(
        rx: mpsc::Receiver<FileHandlerEvents>,
        tx_listener: mpsc::Sender<FileHandlerEvents>,
        workers: Arc<Mutex<HashMap<u32, WorkerTx>>>,
        config: Arc<Config>,
        log_sink: Arc<dyn LogSink + Send + Sync>,
        event_tx: mpsc::Sender<EngineEvent>,
    ) {
        sink_info!(log_sink, "[FILE_HANDLER] Listener started");
        let mut active_readers = HashSet::new();

        while let Ok(event) = rx.recv() {
            match event {
                FileHandlerEvents::ReadFile { path, id } => {
                    sink_debug!(log_sink, "[FILE_HANDLER] ReadFile request: {} (id: {})", path, id);
                    let (tx_worker, rx_worker) = mpsc::channel();

                    let path_obj = Path::new(&path);
                    let file_name = path_obj
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

                    sink_trace!(log_sink, "[FILE_HANDLER] Spawning ReaderWorker for id: {}", id);

                    match ReaderWorker::new(
                        id,
                        path,
                        tx_listener.clone(),
                        rx_worker,
                        log_sink.clone(),
                    ) {
                        Ok(worker) => {
                            thread::spawn(move || worker.run());
                            workers
                                .lock()
                                .unwrap()
                                .insert(id, WorkerTx::Reader(tx_worker));

                            // Notify Engine to send offer
                            let props = SctpFileProperties {
                                file_name,
                                file_size,
                                transaction_id: id,
                            };
                            sink_trace!(log_sink, "[FILE_HANDLER] Sending SendFileOffer to Engine for id: {}", id);
                            let _ = event_tx.send(EngineEvent::SendFileOffer(props));
                        }
                        Err(e) => {
                            sink_error!(log_sink, "[FILE_HANDLER] Failed to create ReaderWorker: {}", e);
                            let _ = tx_listener.send(FileHandlerEvents::Err(e));
                        }
                    }
                }
                FileHandlerEvents::RemoteAccepted(id) => {
                    sink_info!(log_sink, "[FILE_HANDLER] Remote accepted file {}, activating reader", id);
                    active_readers.insert(id);
                }
                FileHandlerEvents::WriteFile { filename, id } => {
                    sink_debug!(log_sink, "[FILE_HANDLER] WriteFile request: {} (id: {})", filename, id);

                    // "path que se obtiene joineando el filename con la variable obtenida de config"
                    // Default to "./downloads" if not specified.
                    let storage_path = config.get_non_empty_or_default(
                        "file_handler",
                        "storage_path",
                        "./downloads",
                    );
                    let full_path = Path::new(storage_path).join(&filename);

                    // Ensure directory exists
                    if let Some(parent) = full_path.parent() {
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            sink_error!(log_sink, "[FILE_HANDLER] Failed to create directory {:?}: {}", parent, e);
                            let _ = tx_listener.send(FileHandlerEvents::Err(e.to_string()));
                            continue;
                        }
                    }

                    sink_trace!(log_sink, "[FILE_HANDLER] Spawning WriterWorker for id: {}", id);

                    let (tx_worker, rx_worker) = mpsc::channel();
                    match WriterWorker::new(
                        id,
                        full_path,
                        tx_listener.clone(),
                        rx_worker,
                        log_sink.clone(),
                    ) {
                        Ok(worker) => {
                            thread::spawn(move || worker.run());
                            workers
                                .lock()
                                .unwrap()
                                .insert(id, WorkerTx::Writer(tx_worker));
                        }
                        Err(e) => {
                            sink_error!(log_sink, "[FILE_HANDLER] Failed to create WriterWorker: {}", e);
                            let _ = tx_listener.send(FileHandlerEvents::Err(e));
                        }
                    }
                }
                FileHandlerEvents::GetChunk(id) => {
                    sink_trace!(log_sink, "[FILE_HANDLER] Processing GetChunk for id: {}", id);
                    let map = workers.lock().unwrap();
                    if let Some(WorkerTx::Reader(tx)) = map.get(&id) {
                        if let Err(e) = tx.send(ReaderCommands::GetChunk) {
                            sink_warn!(log_sink, "[FILE_HANDLER] Failed to send GetChunk to worker {}: {}", id, e);
                        }
                    } else {
                        sink_warn!(
                            log_sink,
                            "[FILE_HANDLER] GetChunk received for unknown or non-reader worker {}",
                            id
                        );
                    }
                }
                FileHandlerEvents::ReadChunk { id, payload } => {
                    sink_debug!(log_sink, "[FILE_HANDLER] ReadChunk received from worker {}", id);
                    sink_trace!(log_sink, "[FILE_HANDLER] Forwarding ReadChunk to Engine. Payload size: {}", payload.len());
                    let _ = event_tx.send(EngineEvent::SendFileChunk(id, payload));
                }
                FileHandlerEvents::WriteChunk { id, payload } => {
                    sink_trace!(log_sink, "[FILE_HANDLER] Processing WriteChunk for id: {}. Payload size: {}", id, payload.len());
                    let map = workers.lock().unwrap();
                    if let Some(WorkerTx::Writer(tx)) = map.get(&id) {
                        if let Err(e) = tx.send(WriterCommands::WriteChunk(payload)) {
                            sink_warn!(
                                log_sink,
                                "[FILE_HANDLER] Failed to send WriteChunk to worker {}: {}",
                                id,
                                e
                            );
                        }
                    } else {
                        sink_warn!(
                            log_sink,
                            "[FILE_HANDLER] WriteChunk received for unknown or non-writer worker {}",
                            id
                        );
                    }
                }
                FileHandlerEvents::ReaderWorkerFinished(id) => {
                    sink_info!(log_sink, "[FILE_HANDLER] ReaderWorker {} finished successfully", id);
                    workers.lock().unwrap().remove(&id);
                    active_readers.remove(&id);
                    let _ = event_tx.send(EngineEvent::SendFileEnd(id));
                }
                FileHandlerEvents::WriterWorkerFinished(id) => {
                    sink_info!(log_sink, "[FILE_HANDLER] WriterWorker {} finished successfully", id);
                    workers.lock().unwrap().remove(&id);
                    let _ = event_tx.send(EngineEvent::Status(format!("File download complete: {}", id)));
                }
                FileHandlerEvents::Cancel(id) => {
                    sink_info!(log_sink, "[FILE_HANDLER] Processing Cancel for id: {}", id);
                    active_readers.remove(&id);
                    let mut map = workers.lock().unwrap();
                    if let Some(tx) = map.remove(&id) {
                        match tx {
                            WorkerTx::Reader(tx) => {
                                let _ = tx.send(ReaderCommands::Cancel);
                            }
                            WorkerTx::Writer(tx) => {
                                let _ = tx.send(WriterCommands::Cancel);
                            }
                        }
                        sink_debug!(log_sink, "[FILE_HANDLER] Cancelled worker {}", id);
                    } else {
                        sink_warn!(log_sink, "[FILE_HANDLER] Cancel received for unknown worker {}", id);
                    }
                }
                FileHandlerEvents::Err(e) => {
                    sink_error!(log_sink, "[FILE_HANDLER] Error: {}", e);
                    let _ = event_tx.send(EngineEvent::Error(format!("FileHandler: {}", e)));
                }
                FileHandlerEvents::DrainChunks => {
                     // sink_trace!(log_sink, "[FILE_HANDLER] Processing DrainChunks");
                     // GetChunk to all *active* readers
                     let map = workers.lock().unwrap();
                     for (id, worker) in map.iter() {
                         if let WorkerTx::Reader(tx) = worker {
                             if active_readers.contains(id) {
                                 let _ = tx.send(ReaderCommands::GetChunk);
                             }
                         }
                     }
                }
            }
        }
        sink_info!(log_sink, "[FILE_HANDLER] Listener stopped");
    }
}
