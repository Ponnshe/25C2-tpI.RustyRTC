use std::sync::mpsc;

use crate::{
    log::{log_level::LogLevel, log_msg::LogMsg, log_sink::LogSink},
    media_agent,
};

/// Lightweight, cloneable handle to the process logger.
///
/// `LoggerHandle` is a thin, lock-free sink that enqueues `LogMsg` into a
/// bounded `SyncSender`. Calls to [`try_log`](Self::try_log) are non-blocking:
/// if the queue is full, the message is dropped and an error is returned.
///
/// Typical usage is to obtain it from your `Logger` and clone it per module/task.
///
/// # Examples
/// ```ignore
/// // Usually you obtain it via: let handle = logger.handle();
/// handle.try_log(LogLevel::Info, "started task", module_path!())?;
/// ```
#[derive(Clone)]
pub struct LoggerHandle {
    pub(super) tx: mpsc::SyncSender<LogMsg>,
}

impl LogSink for LoggerHandle {
    #[inline]
    fn log(&self, level: LogLevel, msg: &str, target: &'static str) {
        // `try_log` takes Into<String>; &str works (it will allocate).
        let _ = self.try_log(level, msg, target);
    }
}

impl LoggerHandle {
    /// Attempts to enqueue a log message without blocking.
    ///
    /// The message carries a millisecond timestamp from `media_agent::utils::now_millis()`
    /// and the given `target` (e.g., `module_path!()`).
    ///
    /// # Returns
    /// `Ok(())` if the message was queued.
    ///
    /// # Errors
    /// Returns:
    /// - `Err(TrySendError::Full(_))` when the bounded queue is at capacity (message is not sent).
    /// - `Err(TrySendError::Disconnected(_))` when the logger worker has been dropped.
    ///
    /// # Examples
    /// ```ignore
    /// handle.try_log(LogLevel::Warn, "rate-limited", module_path!())?;
    /// ```
    pub fn try_log<S: Into<String>>(
        &self,
        level: LogLevel,
        text: S,
        target: &'static str,
    ) -> Result<(), mpsc::TrySendError<LogMsg>> {
        let msg = LogMsg {
            level,
            ts_ms: media_agent::utils::now_millis(),
            text: text.into(),
            target,
        };
        self.tx.try_send(msg)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::sync::mpsc::{TrySendError, sync_channel};

    #[test]
    fn try_log_ok_when_capacity_available() {
        let (tx, rx) = sync_channel::<LogMsg>(2);
        let h = LoggerHandle { tx };

        let res = h.try_log(LogLevel::Info, "hello", "test::target");
        assert!(res.is_ok(), "expected Ok from try_log");

        let msg = rx.recv().expect("a message should arrive");
        assert_eq!(msg.level, LogLevel::Info);
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.target, "test::target");
        assert!(msg.ts_ms > 0, "timestamp should be non-zero");
    }

    #[test]
    fn try_log_err_full_when_queue_full() {
        // Capacity = 1, send once and do not recv -> next send should be Full.
        let (tx, _rx) = sync_channel::<LogMsg>(1);
        let h = LoggerHandle { tx };

        h.try_log(LogLevel::Info, "first", "test::target")
            .expect("first send should succeed");

        match h.try_log(LogLevel::Info, "second", "test::target") {
            Err(TrySendError::Full(_)) => {} // expected
            other => panic!("expected Full, got: {:?}", other),
        }
    }

    #[test]
    fn try_log_err_disconnected_when_receiver_closed() {
        // Drop the receiver immediately so the channel is disconnected.
        let (tx, rx) = sync_channel::<LogMsg>(1);
        drop(rx);
        let h = LoggerHandle { tx };

        match h.try_log(LogLevel::Error, "won't send", "test::target") {
            Err(TrySendError::Disconnected(_)) => {} // expected
            other => panic!("expected Disconnected, got: {:?}", other),
        }
    }
}
