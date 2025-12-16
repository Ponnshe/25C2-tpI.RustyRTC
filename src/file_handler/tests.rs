#[cfg(test)]
#[allow(clippy::module_inception)]
#[allow(clippy::expect_used)]
mod tests {
    use super::super::events::{FileHandlerEvents, ReaderCommands, WriterCommands};
    use super::super::reader_worker::ReaderWorker;
    use super::super::writer_worker::WriterWorker;
    use crate::log::NoopLogSink;
    use std::fs::{self, File};
    use std::io::{Read, Write};
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_reader_worker_flow() {
        let tmp_dir = std::env::temp_dir().join("rustyrtc_reader_test");
        fs::create_dir_all(&tmp_dir).expect("failed to create temp dir");
        let file_path = tmp_dir.join("test_read.txt");
        let content = b"Hello Reader";
        {
            let mut file = File::create(&file_path).expect("failed to create file");
            file.write_all(content).expect("failed to write content");
        }

        let (tx_listener, rx_listener) = mpsc::channel();
        let (tx_cmd, rx_cmd) = mpsc::channel();
        let log_sink = Arc::new(NoopLogSink);

        let worker = ReaderWorker::new(
            1,
            file_path.to_str().expect("invalid path").to_string(),
            tx_listener,
            rx_cmd,
            log_sink,
        )
        .expect("failed to create worker");

        thread::spawn(move || worker.run());

        // Request chunk
        tx_cmd
            .send(ReaderCommands::GetChunk)
            .expect("failed to send command");

        // Expect chunk
        match rx_listener
            .recv_timeout(Duration::from_secs(1))
            .expect("recv timeout")
        {
            FileHandlerEvents::ReadChunk { id, payload } => {
                assert_eq!(id, 1);
                assert_eq!(payload, content);
            }
            _ => panic!("Expected ReadChunk"),
        }

        // Request EOF
        tx_cmd
            .send(ReaderCommands::GetChunk)
            .expect("failed to send command");

        // Expect empty chunk (EOF)
        match rx_listener
            .recv_timeout(Duration::from_secs(1))
            .expect("recv timeout")
        {
            FileHandlerEvents::ReadChunk { id, payload } => {
                assert_eq!(id, 1);
                assert!(payload.is_empty());
            }
            _ => panic!("Expected empty ReadChunk"),
        }

        // Expect Finished event
        match rx_listener
            .recv_timeout(Duration::from_secs(1))
            .expect("recv timeout")
        {
            FileHandlerEvents::ReaderWorkerFinished(id) => {
                assert_eq!(id, 1);
            }
            _ => panic!("Expected ReaderWorkerFinished"),
        }

        fs::remove_dir_all(tmp_dir).expect("failed to remove tmp dir");
    }

    #[test]
    fn test_reader_worker_chunking() {
        let tmp_dir = std::env::temp_dir().join("rustyrtc_reader_chunking_test");
        fs::create_dir_all(&tmp_dir).expect("failed to create temp dir");
        let file_path = tmp_dir.join("test_read_large.bin");
        
        // Create content: 16KB + 1KB
        let chunk_size = 16 * 1024;
        let extra_size = 1024;
        let total_size = chunk_size + extra_size;
        let content: Vec<u8> = (0..total_size).map(|i| (i % 255) as u8).collect();
        
        {
            let mut file = File::create(&file_path).expect("failed to create file");
            file.write_all(&content).expect("failed to write content");
        }

        let (tx_listener, rx_listener) = mpsc::channel();
        let (tx_cmd, rx_cmd) = mpsc::channel();
        let log_sink = Arc::new(NoopLogSink);

        let worker = ReaderWorker::new(
            1,
            file_path.to_str().expect("invalid path").to_string(),
            tx_listener,
            rx_cmd,
            log_sink,
        )
        .expect("failed to create worker");

        thread::spawn(move || worker.run());

        // 1. Request first chunk
        tx_cmd
            .send(ReaderCommands::GetChunk)
            .expect("failed to send command");

        // Expect full 16KB chunk
        match rx_listener
            .recv_timeout(Duration::from_secs(1))
            .expect("recv timeout")
        {
            FileHandlerEvents::ReadChunk { id, payload } => {
                assert_eq!(id, 1);
                assert_eq!(payload.len(), chunk_size, "First chunk should be full chunk size");
                assert_eq!(payload, content[..chunk_size]);
            }
            _ => panic!("Expected ReadChunk"),
        }

        // 2. Request second chunk
        tx_cmd
            .send(ReaderCommands::GetChunk)
            .expect("failed to send command");

        // Expect remaining 1KB chunk
        match rx_listener
            .recv_timeout(Duration::from_secs(1))
            .expect("recv timeout")
        {
            FileHandlerEvents::ReadChunk { id, payload } => {
                assert_eq!(id, 1);
                assert_eq!(payload.len(), extra_size, "Second chunk should be remaining size");
                assert_eq!(payload, content[chunk_size..]);
            }
            _ => panic!("Expected ReadChunk"),
        }

        // 3. Request EOF
        tx_cmd
            .send(ReaderCommands::GetChunk)
            .expect("failed to send command");

        // Expect empty chunk (EOF)
        match rx_listener
            .recv_timeout(Duration::from_secs(1))
            .expect("recv timeout")
        {
            FileHandlerEvents::ReadChunk { id, payload } => {
                assert_eq!(id, 1);
                assert!(payload.is_empty());
            }
            _ => panic!("Expected empty ReadChunk"),
        }

        // Expect Finished event
        match rx_listener
            .recv_timeout(Duration::from_secs(1))
            .expect("recv timeout")
        {
            FileHandlerEvents::ReaderWorkerFinished(id) => {
                assert_eq!(id, 1);
            }
            _ => panic!("Expected ReaderWorkerFinished"),
        }

        fs::remove_dir_all(tmp_dir).expect("failed to remove tmp dir");
    }

    #[test]
    fn test_writer_worker_flow() {
        let tmp_dir = std::env::temp_dir().join("rustyrtc_writer_test");
        fs::create_dir_all(&tmp_dir).expect("failed to create tmp dir");
        let file_path = tmp_dir.join("test_write.txt");

        let (tx_listener, rx_listener) = mpsc::channel();
        let (tx_cmd, rx_cmd) = mpsc::channel();
        let log_sink = Arc::new(NoopLogSink);

        let worker = WriterWorker::new(2, file_path.clone(), tx_listener, rx_cmd, log_sink)
            .expect("failed to create worker");

        thread::spawn(move || worker.run());

        let content = b"Hello Writer";
        tx_cmd
            .send(WriterCommands::WriteChunk(content.to_vec()))
            .expect("failed to send command");

        // Send EOF
        tx_cmd
            .send(WriterCommands::WriteChunk(vec![]))
            .expect("failed to send command");

        // Expect Finished event
        match rx_listener
            .recv_timeout(Duration::from_secs(1))
            .expect("recv timeout")
        {
            FileHandlerEvents::WriterWorkerFinished(id) => {
                assert_eq!(id, 2);
            }
            _ => panic!("Expected WriterWorkerFinished"),
        }

        // Verify file content
        let mut file = File::open(&file_path).expect("failed to open file");
        let mut read_content = Vec::new();
        file.read_to_end(&mut read_content)
            .expect("failed to read file");
        assert_eq!(read_content, content);

        fs::remove_dir_all(tmp_dir).expect("failed to remove tmp dir");
    }
}
