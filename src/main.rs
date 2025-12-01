use rustyrtc::{app::rtc_app::RtcApp, config::Config};
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    let config = Config::load("roomrtc.conf")
        .or_else(|_| Config::load("default.conf"))
        .unwrap_or_else(|e| {
            eprintln!("Error loading config: {e}. Using empty config.");
            Config::empty()
        });
    let config = Arc::new(config);

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "RoomRTC • SDP Messenger",
        native_options,
        Box::new(|cc| {
            let app = RtcApp::new(cc, config);
            Ok(Box::new(app))
        }),
    )
}
