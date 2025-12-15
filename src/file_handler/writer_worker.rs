use crate::file_handler::events::{FileHandlerEvents, WriterCommands};
use crate::log::log_sink::LogSink;
use crate::{sink_debug, sink_error, sink_info, sink_trace, sink_warn};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, mpsc::Receiver, mpsc::Sender};
use std::time::Duration;

const TIMEOUT_DURATION: Duration = Duration::from_secs(10);

pub struct WriterWorker {
    id: u32,
    writer: BufWriter<File>,
    path: PathBuf,
    tx_listener: Sender<FileHandlerEvents>,
    rx_cmd: Receiver<WriterCommands>,
    log_sink: Arc<dyn LogSink>,
}

impl WriterWorker {
    pub fn new(
        id: u32,
        path: PathBuf,
        tx_listener: Sender<FileHandlerEvents>,
        rx_cmd: Receiver<WriterCommands>,
        log_sink: Arc<dyn LogSink>,
    ) -> Result<Self, String> {
        let file = File::create(&path).map_err(|e| e.to_string())?;
        let writer = BufWriter::new(file);
        Ok(Self {
            id,
            writer,
            path,
            tx_listener,
            rx_cmd,
            log_sink,
        })
    }

    pub fn run(mut self) {
        sink_info!(self.log_sink, "[WRITER_WORKER] Worker {} started", self.id);
        loop {
            match self.rx_cmd.recv_timeout(TIMEOUT_DURATION) {
                Ok(WriterCommands::WriteChunk(payload)) => {
                    sink_trace!(
                        self.log_sink,
                        "[WRITER_WORKER] Worker {} processing WriteChunk of size {}",
                        self.id,
                        payload.len()
                    );
                    if payload.is_empty() {
                        sink_debug!(
                            self.log_sink,
                            "[WRITER_WORKER] Worker {} received EOF",
                            self.id
                        );
                        if let Err(e) = self.writer.flush() {
                            sink_error!(
                                self.log_sink,
                                "[WRITER_WORKER] Worker {} flush error: {}",
                                self.id,
                                e
                            );
                            let _ = self.tx_listener.send(FileHandlerEvents::Err(e.to_string()));
                            self.cleanup();
                        } else {
                            let _ = self
                                .tx_listener
                                .send(FileHandlerEvents::WriterWorkerFinished(self.id));
                        }
                        break;
                    }

                    if let Err(e) = self.writer.write_all(&payload) {
                        sink_error!(
                            self.log_sink,
                            "[WRITER_WORKER] Worker {} write error: {}",
                            self.id,
                            e
                        );
                        let _ = self.tx_listener.send(FileHandlerEvents::Err(e.to_string()));
                        self.cleanup();
                        break;
                    }
                    sink_debug!(
                        self.log_sink,
                        "[WRITER_WORKER] Worker {} wrote {} bytes",
                        self.id,
                        payload.len()
                    );
                    if let Err(e) = self.writer.flush() {
                        sink_error!(
                            self.log_sink,
                            "[WRITER_WORKER] Worker {} flush error: {}",
                            self.id,
                            e
                        );
                        let _ = self.tx_listener.send(FileHandlerEvents::Err(e.to_string()));
                        self.cleanup();
                        break;
                    }
                }
                Ok(WriterCommands::Cancel) => {
                    sink_info!(
                        self.log_sink,
                        "[WRITER_WORKER] Worker {} cancelled",
                        self.id
                    );
                    self.cleanup();
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    sink_warn!(
                        self.log_sink,
                        "[WRITER_WORKER] Worker {} timed out",
                        self.id
                    );
                    self.cleanup();
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    sink_info!(
                        self.log_sink,
                        "[WRITER_WORKER] Worker {} channel disconnected",
                        self.id
                    );
                    self.cleanup();
                    break;
                }
            }
        }
        sink_info!(self.log_sink, "[WRITER_WORKER] Worker {} stopped", self.id);
    }

    fn cleanup(&self) {
        // Try to remove the file
        if let Err(e) = fs::remove_file(&self.path) {
            sink_warn!(
                self.log_sink,
                "[WRITER_WORKER] Worker {} failed to remove file {:?}: {}",
                self.id,
                self.path,
                e
            );
        } else {
            sink_info!(
                self.log_sink,
                "[WRITER_WORKER] Worker {} removed file {:?}",
                self.id,
                self.path
            );
        }
    }
}
