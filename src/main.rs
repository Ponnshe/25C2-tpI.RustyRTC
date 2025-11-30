use rustyrtc::app::rtc_app::RtcApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "RoomRTC • SDP Messenger",
        native_options,
        Box::new(|cc| Ok(Box::new(RtcApp::new(cc)))),
    )
}
