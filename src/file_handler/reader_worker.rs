use crate::file_handler::events::{FileHandlerEvents, ReaderCommands};
use crate::log::log_sink::LogSink;
use crate::{sink_debug, sink_error, sink_info, sink_trace, sink_warn};
use std::fs::File;
use std::io::{BufReader, Read};
use std::sync::{Arc, mpsc::Receiver, mpsc::Sender};

const CHUNK_SIZE: usize = 1024;

pub struct ReaderWorker {
    id: u32,
    reader: BufReader<File>,
    tx_listener: Sender<FileHandlerEvents>,
    rx_cmd: Receiver<ReaderCommands>,
    log_sink: Arc<dyn LogSink>,
}

impl ReaderWorker {
    pub fn new(
        id: u32,
        path: String,
        tx_listener: Sender<FileHandlerEvents>,
        rx_cmd: Receiver<ReaderCommands>,
        log_sink: Arc<dyn LogSink>,
    ) -> Result<Self, String> {
        let file = File::open(&path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        Ok(Self {
            id,
            reader,
            tx_listener,
            rx_cmd,
            log_sink,
        })
    }

    pub fn run(mut self) {
        sink_info!(self.log_sink, "[READER_WORKER] Worker {} started", self.id);
        
        let file_size = self.reader.get_ref().metadata().map(|m| m.len()).unwrap_or(0);
        let mut total_read = 0;

        while let Ok(cmd) = self.rx_cmd.recv() {
            match cmd {
                ReaderCommands::GetChunk => {
                    sink_trace!(
                        self.log_sink,
                        "[READER_WORKER] Worker {} processing GetChunk",
                        self.id
                    );
                    let mut buffer = vec![0u8; CHUNK_SIZE];
                    match self.reader.read(&mut buffer) {
                        Ok(0) => {
                            // EOF
                            sink_debug!(self.log_sink, "[READER_WORKER] Worker {} EOF", self.id);
                            let _ = self.tx_listener.send(FileHandlerEvents::ReadChunk {
                                id: self.id,
                                payload: Vec::new(),
                            });
                            let _ = self
                                .tx_listener
                                .send(FileHandlerEvents::ReaderWorkerFinished(self.id));
                            break;
                        }
                        Ok(n) => {
                            buffer.truncate(n);
                            total_read += n as u64;
                            sink_debug!(
                                self.log_sink,
                                "[READER_WORKER] Worker {} read {} bytes (Total: {}/{})",
                                self.id,
                                n,
                                total_read,
                                file_size
                            );
                            
                            let _ = self.tx_listener.send(FileHandlerEvents::UploadProgress {
                                id: self.id,
                                current: total_read as usize,
                                total: file_size as usize,
                            });

                            if let Err(e) = self.tx_listener.send(FileHandlerEvents::ReadChunk {
                                id: self.id,
                                payload: buffer,
                            }) {
                                sink_warn!(
                                    self.log_sink,
                                    "[READER_WORKER] Worker {} failed to send ReadChunk: {}",
                                    self.id,
                                    e
                                );
                                break;
                            }
                            // Pacing to avoid UDP buffer overflow
                            std::thread::sleep(std::time::Duration::from_micros(500));
                        }
                        Err(e) => {
                            sink_error!(
                                self.log_sink,
                                "[READER_WORKER] Worker {} read error: {}",
                                self.id,
                                e
                            );
                            let _ = self.tx_listener.send(FileHandlerEvents::Err(e.to_string()));
                        }
                    }
                }
                ReaderCommands::Cancel => {
                    sink_info!(
                        self.log_sink,
                        "[READER_WORKER] Worker {} cancelled",
                        self.id
                    );
                    break;
                }
            }
        }
        sink_info!(self.log_sink, "[READER_WORKER] Worker {} stopped", self.id);
    }
}
