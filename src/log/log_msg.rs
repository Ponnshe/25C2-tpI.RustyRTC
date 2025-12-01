use crate::log::log_level::LogLevel;

/// Represents a single log message event.
///
/// This struct encapsulates the metadata associated with a log entry,
/// including its severity, timestamp, origin (target), and the message content itself.
#[derive(Debug, Clone)]
pub struct LogMsg {
    /// The severity level of the log (e.g., Info, Warning, Error).
    pub level: LogLevel,
    /// The timestamp of the log event in milliseconds.
    pub ts_ms: u128,
    /// The actual content or payload of the log message.
    pub text: String,
    /// The target source of the log, typically the static module path.
    pub target: &'static str, // module path
}

impl LogMsg {
    /// Creates a new `LogMsg` instance.
    ///
    /// # Arguments
    ///
    /// * `level` - The severity `LogLevel` of the message.
    /// * `text` - The message content. Accepts any type that implements `Into<String>`.
    /// * `target` - A static string representing the log origin (e.g., module path).
    /// * `ts_ms` - The timestamp of the event in milliseconds.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use crate::log::log_level::LogLevel;
    /// use crate::log::LogMsg;
    ///
    /// let msg = LogMsg::new(
    ///     LogLevel::Info,
    ///     "Connection established",
    ///     module_path!(),
    ///     1678900000000
    /// );
    /// ```
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
