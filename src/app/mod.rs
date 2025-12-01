//! The `app` module contains the main GUI application logic, including the `RtcApp` struct,
//! which is the main entry point for the `eframe` application. It also contains helper
//! modules for managing connection state, GPU rendering, and GUI errors.

pub mod conn_state;
pub mod debug_yuv_to_rgb;
pub mod gpu_yuv_renderer;
pub mod gui_error;
pub mod rtc_app;
mod utils;
