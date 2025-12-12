#[cfg(test)]
mod tests {
    use super::super::events::{FileHandlerEvents, ReaderCommands, WriterCommands};
    use super::super::reader_worker::ReaderWorker;
    use super::super::writer_worker::WriterWorker;
    use crate::log::NoopLogSink;
    use std::fs::{self, File};
    use std::io::{Write, Read};
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_reader_worker_flow() {
        let tmp_dir = std::env::temp_dir().join("rustyrtc_reader_test");
        fs::create_dir_all(&tmp_dir).unwrap();
        let file_path = tmp_dir.join("test_read.txt");
        let content = b"Hello Reader";
        {
            let mut file = File::create(&file_path).unwrap();
            file.write_all(content).unwrap();
        }

        let (tx_listener, rx_listener) = mpsc::channel();
        let (tx_cmd, rx_cmd) = mpsc::channel();
        let log_sink = Arc::new(NoopLogSink);

        let worker = ReaderWorker::new(
            1,
            file_path.to_str().unwrap().to_string(),
            tx_listener,
            rx_cmd,
            log_sink
        ).unwrap();

        thread::spawn(move || worker.run());

        // Request chunk
        tx_cmd.send(ReaderCommands::GetChunk).unwrap();

        // Expect chunk
        match rx_listener.recv_timeout(Duration::from_secs(1)).unwrap() {
            FileHandlerEvents::ReadChunk { id, payload } => {
                assert_eq!(id, 1);
                assert_eq!(payload, content);
            }
            _ => panic!("Expected ReadChunk"),
        }

        // Request EOF
        tx_cmd.send(ReaderCommands::GetChunk).unwrap();

        // Expect empty chunk (EOF)
        match rx_listener.recv_timeout(Duration::from_secs(1)).unwrap() {
            FileHandlerEvents::ReadChunk { id, payload } => {
                assert_eq!(id, 1);
                assert!(payload.is_empty());
            }
            _ => panic!("Expected empty ReadChunk"),
        }

        // Expect Finished event
        match rx_listener.recv_timeout(Duration::from_secs(1)).unwrap() {
            FileHandlerEvents::ReaderWorkerFinished(id) => {
                assert_eq!(id, 1);
            }
            _ => panic!("Expected ReaderWorkerFinished"),
        }

        fs::remove_dir_all(tmp_dir).unwrap();
    }

    #[test]
    fn test_writer_worker_flow() {
        let tmp_dir = std::env::temp_dir().join("rustyrtc_writer_test");
        fs::create_dir_all(&tmp_dir).unwrap();
        let file_path = tmp_dir.join("test_write.txt");
        
        let (tx_listener, rx_listener) = mpsc::channel();
        let (tx_cmd, rx_cmd) = mpsc::channel();
        let log_sink = Arc::new(NoopLogSink);

        let worker = WriterWorker::new(
            2,
            file_path.clone(),
            tx_listener,
            rx_cmd,
            log_sink
        ).unwrap();

        thread::spawn(move || worker.run());

        let content = b"Hello Writer";
        tx_cmd.send(WriterCommands::WriteChunk(content.to_vec())).unwrap();

        // Send EOF
        tx_cmd.send(WriterCommands::WriteChunk(vec![])).unwrap();

        // Expect Finished event
        match rx_listener.recv_timeout(Duration::from_secs(1)).unwrap() {
            FileHandlerEvents::WriterWorkerFinished(id) => {
                assert_eq!(id, 2);
            }
            _ => panic!("Expected WriterWorkerFinished"),
        }

        // Verify file content
        let mut file = File::open(&file_path).unwrap();
        let mut read_content = Vec::new();
        file.read_to_end(&mut read_content).unwrap();
        assert_eq!(read_content, content);

        fs::remove_dir_all(tmp_dir).unwrap();
    }
}
