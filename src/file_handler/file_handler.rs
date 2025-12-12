use std::collections::HashMap;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::config::Config;
use crate::file_handler::events::{FileHandlerEvents, ReaderCommands, WriterCommands};
use crate::file_handler::reader_worker::ReaderWorker;
use crate::file_handler::writer_worker::WriterWorker;
use crate::log::log_sink::LogSink;
use crate::{sink_debug, sink_error, sink_info, sink_warn};

enum WorkerTx {
    Reader(mpsc::Sender<ReaderCommands>),
    Writer(mpsc::Sender<WriterCommands>),
}

pub struct FileHandler {
    _config: Arc<Config>,
    tx_listener: Option<mpsc::Sender<FileHandlerEvents>>,
    workers: Arc<Mutex<HashMap<u32, WorkerTx>>>,
    log_sink: Arc<dyn LogSink>,
}

impl FileHandler {
    pub fn new(config: Arc<Config>, log_sink: Arc<dyn LogSink>) -> Self {
        let (tx, rx) = mpsc::channel();
        let workers = Arc::new(Mutex::new(HashMap::new()));

        let tx_for_listener = tx.clone();
        let workers_clone = workers.clone();
        let config_clone = config.clone();
        let log_sink_clone = log_sink.clone();

        thread::spawn(move || {
            Self::listener_loop(rx, tx_for_listener, workers_clone, config_clone, log_sink_clone);
        });

        Self {
            _config: config,
            tx_listener: Some(tx),
            workers,
            log_sink,
        }
    }

    pub fn send(&self, event: FileHandlerEvents) -> Result<(), String> {
        if let Some(tx) = &self.tx_listener {
            tx.send(event).map_err(|e| e.to_string())
        } else {
            Err("FileHandler is shut down".to_string())
        }
    }

    pub fn shutdown(&mut self) {
        sink_info!(self.log_sink, "FileHandler shutting down");
        
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
            sink_debug!(self.log_sink, "Sent Cancel to worker {}", _id);
        }

        // 2. Drop the listener sender to signal the listener loop to potentially stop (once workers drop theirs)
        self.tx_listener = None;
    }

    fn listener_loop(
        rx: mpsc::Receiver<FileHandlerEvents>,
        tx_listener: mpsc::Sender<FileHandlerEvents>,
        workers: Arc<Mutex<HashMap<u32, WorkerTx>>>,
        config: Arc<Config>,
        log_sink: Arc<dyn LogSink + Send + Sync>,
    ) {
        sink_info!(log_sink, "FileHandler listener started");

        while let Ok(event) = rx.recv() {
            match event {
                FileHandlerEvents::ReadFile { path, id } => {
                    sink_debug!(log_sink, "ReadFile request: {} (id: {})", path, id);
                    let (tx_worker, rx_worker) = mpsc::channel();
                    
                    match ReaderWorker::new(id, path, tx_listener.clone(), rx_worker, log_sink.clone()) {
                        Ok(worker) => {
                            thread::spawn(move || worker.run());
                            workers.lock().unwrap().insert(id, WorkerTx::Reader(tx_worker));
                        }
                        Err(e) => {
                            sink_error!(log_sink, "Failed to create ReaderWorker: {}", e);
                            let _ = tx_listener.send(FileHandlerEvents::Err(e));
                        }
                    }
                }
                FileHandlerEvents::WriteFile { filename, id } => {
                    sink_debug!(log_sink, "WriteFile request: {} (id: {})", filename, id);
                    
                    // "path que se obtiene joineando el filename con la variable obtenida de config"
                    // Default to "./downloads" if not specified.
                    let storage_path = config.get_non_empty_or_default("file_handler", "storage_path", "./downloads");
                    let full_path = Path::new(storage_path).join(&filename);

                    // Ensure directory exists
                    if let Some(parent) = full_path.parent() {
                         if let Err(e) = std::fs::create_dir_all(parent) {
                             sink_error!(log_sink, "Failed to create directory {:?}: {}", parent, e);
                             let _ = tx_listener.send(FileHandlerEvents::Err(e.to_string()));
                             continue;
                         }
                    }

                    let (tx_worker, rx_worker) = mpsc::channel();
                    match WriterWorker::new(id, full_path, tx_listener.clone(), rx_worker, log_sink.clone()) {
                        Ok(worker) => {
                            thread::spawn(move || worker.run());
                            workers.lock().unwrap().insert(id, WorkerTx::Writer(tx_worker));
                        }
                        Err(e) => {
                             sink_error!(log_sink, "Failed to create WriterWorker: {}", e);
                             let _ = tx_listener.send(FileHandlerEvents::Err(e));
                        }
                    }
                }
                FileHandlerEvents::GetChunk(id) => {
                    let map = workers.lock().unwrap();
                    if let Some(WorkerTx::Reader(tx)) = map.get(&id) {
                        if let Err(e) = tx.send(ReaderCommands::GetChunk) {
                             sink_warn!(log_sink, "Failed to send GetChunk to worker {}: {}", id, e);
                        }
                    } else {
                        sink_warn!(log_sink, "GetChunk received for unknown or non-reader worker {}", id);
                    }
                }
                FileHandlerEvents::ReadChunk { id: _id, payload: _ } => {
                    // "Se debe propagar el evento a Engine y dejar el placeholder vacio por ahora"
                    sink_debug!(log_sink, "ReadChunk received from worker {}", _id);
                    // TODO: Propagate to Engine
                }
                FileHandlerEvents::WriteChunk { id, payload } => {
                    let map = workers.lock().unwrap();
                     if let Some(WorkerTx::Writer(tx)) = map.get(&id) {
                        if let Err(e) = tx.send(WriterCommands::WriteChunk(payload)) {
                             sink_warn!(log_sink, "Failed to send WriteChunk to worker {}: {}", id, e);
                        }
                    } else {
                        sink_warn!(log_sink, "WriteChunk received for unknown or non-writer worker {}", id);
                    }
                }
                FileHandlerEvents::ReaderWorkerFinished(id) => {
                    sink_info!(log_sink, "ReaderWorker {} finished successfully", id);
                    workers.lock().unwrap().remove(&id);
                }
                FileHandlerEvents::WriterWorkerFinished(id) => {
                    sink_info!(log_sink, "WriterWorker {} finished successfully", id);
                    workers.lock().unwrap().remove(&id);
                }
                FileHandlerEvents::Cancel(id) => {
                    let mut map = workers.lock().unwrap();
                    if let Some(tx) = map.remove(&id) {
                         match tx {
                             WorkerTx::Reader(tx) => { let _ = tx.send(ReaderCommands::Cancel); }
                             WorkerTx::Writer(tx) => { let _ = tx.send(WriterCommands::Cancel); }
                         }
                         sink_debug!(log_sink, "Cancelled worker {}", id);
                    } else {
                        sink_warn!(log_sink, "Cancel received for unknown worker {}", id);
                    }
                }
                FileHandlerEvents::Err(e) => {
                    sink_error!(log_sink, "FileHandler Error: {}", e);
                }
            }
        }
        sink_info!(log_sink, "FileHandler listener stopped");
    }
}
