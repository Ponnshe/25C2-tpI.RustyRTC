//! The client binary for the RoomRTC application.
//! It starts the `eframe` application and the `RtcApp`.

use rustyrtc::{app::rtc_app::RtcApp, config::Config};
use std::env;
use std::sync::Arc; // Importamos env para leer argumentos

fn main() -> eframe::Result<()> {
    let args: Vec<String> = env::args().collect();

    let config_result = if args.len() > 1 {
        let path = &args[1];
        println!("Intentando cargar configuración personalizada: {}", path);
        Config::load(path)
    } else {
        Config::load("client_roomrtc.conf").or_else(|_| Config::load("client_default.conf"))
    };

    let config = config_result.unwrap_or_else(|e| {
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
