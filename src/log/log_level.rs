/// Defines the severity levels for log messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    /// Designates very fine-grained informational events.
    Trace,
    /// Designates fine-grained informational events that are most useful to debug an application.
    Debug,
    /// Designates informational messages that highlight the progress of the application at coarse-grained level.
    Info,
    /// Designates potentially harmful situations.
    Warn,
    /// Designates error events that might still allow the application to continue running.
    Error,
}
