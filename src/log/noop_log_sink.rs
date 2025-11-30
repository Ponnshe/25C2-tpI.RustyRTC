use crate::log::{log_level::LogLevel, log_sink::LogSink};

#[derive(Debug, Clone, Default)]
pub struct NoopLogSink;

impl LogSink for NoopLogSink {
    #[inline]
    fn log(&self, _level: LogLevel, _msg: &str, _target: &'static str) {}
}
