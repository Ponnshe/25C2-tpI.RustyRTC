use crate::log::log_level::LogLevel;

pub trait LogSink: Send + Sync {
    fn log(&self, level: LogLevel, msg: &str, target: &'static str);
}
