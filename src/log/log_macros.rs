//! Simple, leveled logging macros for `EngineEvent`, `LoggerHandle`, and direct `Logger`.
//!
//! # Feature Flags
//! specific log levels are controlled by cargo features:
//! `log-trace`, `log-debug`, `log-info`, `log-warn`, `log-error`.
//!
//! If a feature is disabled, the corresponding macros expand to `()`, removing
//! all formatting and allocation overhead at compile time.

// ============================================================================
// 1. GENERIC INTERNAL MACROS (The "Workers")
// ============================================================================
// These remain available so the enabled macros below can use them.
// We generally don't call these directly if we want feature-gating.

#[macro_export]
macro_rules! log_ev {
    ($tx:expr, $lvl:expr, $($arg:tt)*) => {{
        let _ = $tx.send(
            $crate::core::events::EngineEvent::Log(
                $crate::log::log_msg::LogMsg::new(
                    $lvl,
                    format!($($arg)*),
                    module_path!(),
                    crate::media_agent::utils::now_millis(),
                )
            )
        );
    }};
}

#[macro_export]
macro_rules! sink_log {
    ($sink:expr, $lvl:expr, $($arg:tt)*) => {{
        let __msg = format!($($arg)*);
        $sink.log($lvl, &__msg, module_path!());
    }};
}

#[macro_export]
macro_rules! bg_log {
    ($self_:expr, $lvl:expr, $($arg:tt)*) => {{
        $self_.background_log($lvl, format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! logger_log {
    ($logger:expr, $lvl:expr, $($arg:tt)*) => {{
        let __msg = format!($($arg)*);
        $logger.log($lvl, &__msg, module_path!());
    }};
}

// ============================================================================
// 2. LEVEL-SPECIFIC MACROS (Feature Gated)
// ============================================================================

// ---------------------- TRACE ----------------------
#[cfg(feature = "log-trace")]
#[macro_export]
macro_rules! sink_trace   { ($sink:expr, $($arg:tt)*)   => { $crate::sink_log!($sink, $crate::log::log_level::LogLevel::Trace, $($arg)*) } }
#[cfg(feature = "log-trace")]
#[macro_export]
macro_rules! log_trace_ev { ($tx:expr, $($arg:tt)*)     => { $crate::log_ev!($tx, $crate::log::log_level::LogLevel::Trace, $($arg)*) } }
#[cfg(feature = "log-trace")]
#[macro_export]
macro_rules! bg_trace     { ($self_:expr, $($arg:tt)*)  => { $crate::bg_log!($self_, $crate::log::log_level::LogLevel::Trace, $($arg)*) } }
#[cfg(feature = "log-trace")]
#[macro_export]
macro_rules! logger_trace { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, $crate::log::log_level::LogLevel::Trace, $($arg)*) } }

#[cfg(not(feature = "log-trace"))]
#[macro_export]
macro_rules! sink_trace {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-trace"))]
#[macro_export]
macro_rules! log_trace_ev {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-trace"))]
#[macro_export]
macro_rules! bg_trace {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-trace"))]
#[macro_export]
macro_rules! logger_trace {
    ($($arg:tt)*) => {
        ()
    };
}

// ---------------------- DEBUG ----------------------
#[cfg(feature = "log-debug")]
#[macro_export]
macro_rules! sink_debug   { ($sink:expr, $($arg:tt)*)   => { $crate::sink_log!($sink, $crate::log::log_level::LogLevel::Debug, $($arg)*); } }
#[cfg(feature = "log-debug")]
#[macro_export]
macro_rules! log_debug_ev { ($tx:expr, $($arg:tt)*)     => { $crate::log_ev!($tx, $crate::log::log_level::LogLevel::Debug, $($arg)*); } }
#[cfg(feature = "log-debug")]
#[macro_export]
macro_rules! bg_debug     { ($self_:expr, $($arg:tt)*)  => { $crate::bg_log!($self_, $crate::log::log_level::LogLevel::Debug, $($arg)*); } }
#[cfg(feature = "log-debug")]
#[macro_export]
macro_rules! logger_debug { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, $crate::log::log_level::LogLevel::Debug, $($arg)*); } }

#[cfg(not(feature = "log-debug"))]
#[macro_export]
macro_rules! sink_debug {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-debug"))]
#[macro_export]
macro_rules! log_debug_ev {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-debug"))]
#[macro_export]
macro_rules! bg_debug {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-debug"))]
#[macro_export]
macro_rules! logger_debug {
    ($($arg:tt)*) => {
        ()
    };
}

// ---------------------- INFO ----------------------
#[cfg(feature = "log-info")]
#[macro_export]
macro_rules! sink_info   { ($sink:expr, $($arg:tt)*)   => { $crate::sink_log!($sink, $crate::log::log_level::LogLevel::Info, $($arg)*); } }
#[cfg(feature = "log-info")]
#[macro_export]
macro_rules! log_info_ev { ($tx:expr, $($arg:tt)*)     => { $crate::log_ev!($tx, $crate::log::log_level::LogLevel::Info, $($arg)*); } }
#[cfg(feature = "log-info")]
#[macro_export]
macro_rules! bg_info     { ($self_:expr, $($arg:tt)*)  => { $crate::bg_log!($self_, $crate::log::log_level::LogLevel::Info, $($arg)*); } }
#[cfg(feature = "log-info")]
#[macro_export]
macro_rules! logger_info { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, $crate::log::log_level::LogLevel::Info, $($arg)*); } }

#[cfg(not(feature = "log-info"))]
#[macro_export]
macro_rules! sink_info {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-info"))]
#[macro_export]
macro_rules! log_info_ev {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-info"))]
#[macro_export]
macro_rules! bg_info {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-info"))]
#[macro_export]
macro_rules! logger_info {
    ($($arg:tt)*) => {
        ()
    };
}

// ---------------------- WARN ----------------------
#[cfg(feature = "log-warn")]
#[macro_export]
macro_rules! sink_warn   { ($sink:expr, $($arg:tt)*)   => { $crate::sink_log!($sink, $crate::log::log_level::LogLevel::Warn, $($arg)*) } }
#[cfg(feature = "log-warn")]
#[macro_export]
macro_rules! log_warn_ev { ($tx:expr, $($arg:tt)*)     => { $crate::log_ev!($tx, $crate::log::log_level::LogLevel::Warn, $($arg)*); } }
#[cfg(feature = "log-warn")]
#[macro_export]
macro_rules! bg_warn     { ($self_:expr, $($arg:tt)*)  => { $crate::bg_log!($self_, $crate::log::log_level::LogLevel::Warn, $($arg)*); } }
#[cfg(feature = "log-warn")]
#[macro_export]
macro_rules! logger_warn { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, $crate::log::log_level::LogLevel::Warn, $($arg)*); } }

#[cfg(not(feature = "log-warn"))]
#[macro_export]
macro_rules! sink_warn {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-warn"))]
#[macro_export]
macro_rules! log_warn_ev {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-warn"))]
#[macro_export]
macro_rules! bg_warn {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-warn"))]
#[macro_export]
macro_rules! logger_warn {
    ($($arg:tt)*) => {
        ()
    };
}

// ---------------------- ERROR ----------------------
// Generally always enabled, but consistent structure allows user to disable if really needed.
#[cfg(feature = "log-error")]
#[macro_export]
macro_rules! sink_error   { ($sink:expr, $($arg:tt)*)   => { $crate::sink_log!($sink, $crate::log::log_level::LogLevel::Error, $($arg)*); } }
#[cfg(feature = "log-error")]
#[macro_export]
macro_rules! log_error_ev { ($tx:expr, $($arg:tt)*)     => { $crate::log_ev!($tx, $crate::log::log_level::LogLevel::Error, $($arg)*); } }
#[cfg(feature = "log-error")]
#[macro_export]
macro_rules! bg_error     { ($self_:expr, $($arg:tt)*)  => { $crate::bg_log!($self_, $crate::log::log_level::LogLevel::Error, $($arg)*); } }
#[cfg(feature = "log-error")]
#[macro_export]
macro_rules! logger_error { ($logger:expr, $($arg:tt)*) => { $crate::logger_log!($logger, $crate::log::log_level::LogLevel::Error, $($arg)*); } }

#[cfg(not(feature = "log-error"))]
#[macro_export]
macro_rules! sink_error {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-error"))]
#[macro_export]
macro_rules! log_error_ev {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-error"))]
#[macro_export]
macro_rules! bg_error {
    ($($arg:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "log-error"))]
#[macro_export]
macro_rules! logger_error {
    ($($arg:tt)*) => {
        ()
    };
}
