// src/app/log_sink.rs
use crate::app::{log_level::LogLevel, logger_handle::LoggerHandle};

pub trait LogSink: Send + Sync {
    fn log(&self, level: LogLevel, msg: &str, target: &'static str);
}

#[derive(Debug, Clone, Default)]
pub struct NoopLogSink;

impl LogSink for NoopLogSink {
    #[inline]
    fn log(&self, _level: LogLevel, _msg: &str, _target: &'static str) {}
}

impl LogSink for LoggerHandle {
    #[inline]
    fn log(&self, level: LogLevel, msg: &str, target: &'static str) {
        // `try_log` takes Into<String>; &str works (it will allocate).
        let _ = self.try_log(level, msg, target);
    }
}
