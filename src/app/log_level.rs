#[derive(Clone, Copy, Debug)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogMsg {
    pub level: LogLevel,
    pub ts_ms: u128,
    pub text: String,
}
