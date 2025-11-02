use std::sync::mpsc;

use crate::{
    app::{log_level::LogLevel, log_msg::LogMsg},
    media_agent,
};

#[derive(Clone)]
pub struct LoggerHandle {
    pub(super) tx: mpsc::SyncSender<LogMsg>,
}

impl LoggerHandle {
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
