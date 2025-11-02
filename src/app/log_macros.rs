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
