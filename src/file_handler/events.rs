#[derive(Debug, Clone)]
pub enum ReaderCommands {
    GetChunk,
    Cancel,
}

#[derive(Debug, Clone)]
pub enum WriterCommands {
    WriteChunk(Vec<u8>),
    Cancel,
}

#[derive(Debug, Clone)]
pub enum FileHandlerEvents {
    ReadFile { path: String, id: u32 },
    WriteFile { filename: String, id: u32 },
    GetChunk(u32),
    ReadChunk { id: u32, payload: Vec<u8> },
    WriteChunk { id: u32, payload: Vec<u8> },
    ReaderWorkerFinished(u32),
    WriterWorkerFinished(u32),
    Cancel(u32),
    Err(String),
}
