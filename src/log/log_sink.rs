use crate::log::log_level::LogLevel;

/// Defines a destination (sink) for log messages.
///
/// This trait acts as an interface for concrete logging backends, such as
/// console output, file storage, or network services.
///
/// Implementations must be `Send` and `Sync` to ensure they can be safely
/// shared and accessed across multiple threads or asynchronous tasks.
pub trait LogSink: Send + Sync {
    /// Records a log message.
    ///
    /// This method is called by the logging system to dispatch a message
    /// to this specific sink.
    ///
    /// # Arguments
    ///
    /// * `level` - The severity level of the log message.
    /// * `msg` - The content of the log message.
    /// * `target` - The static source of the log (e.g., module path).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use crate::log::{LogSink, LogLevel};
    ///
    /// struct ConsoleSink;
    ///
    /// impl LogSink for ConsoleSink {
    ///     fn log(&self, level: LogLevel, msg: &str, target: &'static str) {
    ///         println!("[{:?}] {}: {}", level, target, msg);
    ///     }
    /// }
    /// ```
    fn log(&self, level: LogLevel, msg: &str, target: &'static str);
}
