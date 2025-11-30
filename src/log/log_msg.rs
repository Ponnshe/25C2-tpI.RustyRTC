use crate::log::log_level::LogLevel;

#[derive(Debug, Clone)]
pub struct LogMsg {
    pub level: LogLevel,
    pub ts_ms: u128,
    pub text: String,
    pub target: &'static str, // module path
}

impl LogMsg {
    pub fn new(
        level: LogLevel,
        text: impl Into<String>,
        target: &'static str,
        ts_ms: u128,
    ) -> Self {
        Self {
            level,
            ts_ms,
            text: text.into(),
            target,
        }
    }
}
