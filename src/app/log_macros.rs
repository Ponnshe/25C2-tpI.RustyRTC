//! Simple, leveled logging macros for both `EngineEvent` and direct Logger usage.

/// ----- Common imports these macros expect -----
/// - `crate::app::log_level::`{`LogLevel`, `LogMsg`}
/// - `crate::media_agent::utils::now_millis`
/// - `crate::core::events::EngineEvent::Log`
///
/// Make sure `EngineEvent::Log` carries a `LogMsg`.
/// Generic event macro (leveled) — sends `EngineEvent::Log(LogMsg)` via a `Sender<EngineEvent>`.
#[macro_export]
macro_rules! log_ev {
    ($tx:expr, $lvl:expr, $($arg:tt)*) => {{
        let _ = $tx.send(
            crate::core::events::EngineEvent::Log(
                crate::app::log_msg::LogMsg::new(
                    $lvl,
                    format!($($arg)*),
                    module_path!(),
                    crate::media_agent::utils::now_millis(),
                )
            )
        );
    }};
}
/// Direct logging via anything that implements `LogSink`
/// (e.g., Arc<dyn LogSink>, `LoggerHandle`, `NoopLogSink`, `TestLogSink`).
#[macro_export]
macro_rules! sink_log {
    ($sink:expr, $lvl:expr, $($arg:tt)*) => {{
        // One formatting allocation; pass &str to the trait method.
        let __msg = format!($($arg)*);
        // Method-call syntax works for Arc<dyn LogSink> and for concrete types that implement the trait.
        $sink.log($lvl, &__msg, module_path!());
    }};
}

#[macro_export]
macro_rules! sink_trace { ($sink:expr, $($arg:tt)*) => { $crate::sink_log!($sink, crate::app::log_level::LogLevel::Trace, $($arg)*); } }
#[macro_export]
macro_rules! sink_debug { ($sink:expr, $($arg:tt)*) => { $crate::sink_log!($sink, crate::app::log_level::LogLevel::Debug, $($arg)*); } }
#[macro_export]
macro_rules! sink_info  { ($sink:expr, $($arg:tt)*) => { $crate::sink_log!($sink, crate::app::log_level::LogLevel::Info,  $($arg)*); } }
#[macro_export]
macro_rules! sink_warn  { ($sink:expr, $($arg:tt)*) => { $crate::sink_log!($sink, crate::app::log_level::LogLevel::Warn,  $($arg)*); } }
#[macro_export]
macro_rules! sink_error { ($sink:expr, $($arg:tt)*) => { $crate::sink_log!($sink, crate::app::log_level::LogLevel::Error, $($arg)*); } }

/// Shorthands for common levels via `EngineEvent`
#[macro_export]
macro_rules! log_trace_ev { ($tx:expr, $($arg:tt)*) => { $crate::log_ev!($tx, crate::app::log_level::LogLevel::Trace, $($arg)*); } }
#[macro_export]
macro_rules! log_debug_ev { ($tx:expr, $($arg:tt)*) => { $crate::log_ev!($tx, crate::app::log_level::LogLevel::Debug, $($arg)*); } }
#[macro_export]
macro_rules! log_info_ev  { ($tx:expr, $($arg:tt)*) => { $crate::log_ev!($tx, crate::app::log_level::LogLevel::Info,  $($arg)*); } }
#[macro_export]
macro_rules! log_warn_ev  { ($tx:expr, $($arg:tt)*) => { $crate::log_ev!($tx, crate::app::log_level::LogLevel::Warn,  $($arg)*); } }
#[macro_export]
macro_rules! log_error_ev { ($tx:expr, $($arg:tt)*) => { $crate::log_ev!($tx, crate::app::log_level::LogLevel::Error, $($arg)*); } }

/// UI/background logging macro that calls `self.background_log(level, String)`
/// Requires an &mut self with method `background_log(LogLevel, impl Into<String>)`.
#[macro_export]
macro_rules! bg_log {
    ($self_:expr, $lvl:expr, $($arg:tt)*) => {{
        $self_.background_log($lvl, format!($($arg)*));
    }};
}
#[macro_export]
macro_rules! bg_trace { ($self_:expr, $($arg:tt)*) => { $crate::bg_log!($self_, crate::app::log_level::LogLevel::Trace, $($arg)*); } }
#[macro_export]
macro_rules! bg_debug { ($self_:expr, $($arg:tt)*) => { $crate::bg_log!($self_, crate::app::log_level::LogLevel::Debug, $($arg)*); } }
#[macro_export]
macro_rules! bg_info  { ($self_:expr, $($arg:tt)*) => { $crate::bg_log!($self_, crate::app::log_level::LogLevel::Info,  $($arg)*); } }
#[macro_export]
macro_rules! bg_warn  { ($self_:expr, $($arg:tt)*) => { $crate::bg_log!($self_, crate::app::log_level::LogLevel::Warn,  $($arg)*); } }
#[macro_export]
macro_rules! bg_error { ($self_:expr, $($arg:tt)*) => { $crate::bg_log!($self_, crate::app::log_level::LogLevel::Error, $($arg)*); } }

/// Direct-logger macros when you have a `&Logger` (or handle) at hand.
/// They capture `module_path!()` automatically as `target`.
#[macro_export]
macro_rules! logger_log {
    ($logger:expr, $lvl:expr, $($arg:tt)*) => {{
        let _ = $logger.try_log($lvl, format!($($arg)*), module_path!());
    }};
}
#[macro_export]
macro_rules! logger_trace { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, crate::app::log_level::LogLevel::Trace, $($arg)*); } }
#[macro_export]
macro_rules! logger_debug { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, crate::app::log_level::LogLevel::Debug, $($arg)*); } }
#[macro_export]
macro_rules! logger_info  { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, crate::app::log_level::LogLevel::Info,  $($arg)*); } }
#[macro_export]
macro_rules! logger_warn  { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, crate::app::log_level::LogLevel::Warn,  $($arg)*); } }
#[macro_export]
macro_rules! logger_error { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, crate::app::log_level::LogLevel::Error, $($arg)*); } }
