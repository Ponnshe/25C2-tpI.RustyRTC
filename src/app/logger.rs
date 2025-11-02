use crate::app::{log_level::LogLevel, log_msg::LogMsg};

use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

/// Bounded, non-blocking logger that writes to a per-process log file,
/// and provides a sampled "UI tap" channel for lightweight UI display.
pub struct Logger {
    log_tx: SyncSender<LogMsg>,
    ui_log_rx: Receiver<String>,
    handle: Option<JoinHandle<()>>,
    file_path: PathBuf,
    sample_every: u32,
}

impl Logger {
    /// Create logs/ directory next to the executable and start the logger there.
    /// Example: target/debug/logs/roomrtc-20251102_023045-pid1234.log
    pub fn start_default(app_name: &str, cap: usize, ui_cap: usize, sample_every: u32) -> Self {
        let base = exe_dir_fallback_cwd().join("logs");
        Self::start_in_dir(base, app_name, cap, ui_cap, sample_every)
    }

    /// Start the logger in a specific directory.
    /// Creates the directory if missing and chooses a timestamped, per-PID file name.
    pub fn start_in_dir<D: AsRef<Path>>(
        dir: D,
        app_name: &str,
        cap: usize,
        ui_cap: usize,
        sample_every: u32,
    ) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let _ = fs::create_dir_all(&dir);

        let fname = format!(
            "{}-{}-pid{}.log",
            app_name,
            timestamp_for_filename(), // e.g., 20251102_023045
            std::process::id()
        );
        let file_path = dir.join(fname);

        let (tx, rx) = mpsc::sync_channel::<LogMsg>(cap);
        let (ui_tx, ui_rx) = mpsc::sync_channel::<String>(ui_cap);
        let file_path_clone = file_path.clone();

        let handle = thread::Builder::new()
            .name("logger-worker".into())
            .spawn(move || {
                // open file (append, create), buffered writes
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file_path_clone)
                    .unwrap_or_else(|_| {
                        // Last resort: create in current dir
                        OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open("roomrtc-fallback.log")
                            .expect("create fallback log")
                    });
                let mut out = BufWriter::new(file);

                let mut n: u32 = 0;
                let mut dropped_to_ui: usize = 0;

                while let Ok(m) = rx.recv() {
                    let _ = writeln!(&mut out, "[{:?}] {} | {}", m.level, m.ts_ms, m.text);

                    let forward = matches!(m.level, LogLevel::Warn | LogLevel::Error) || {
                        n = n.wrapping_add(1);
                        n % sample_every == 0
                    };

                    if forward
                        && ui_tx
                            .try_send(format!("[{:?}] {}", m.level, m.text))
                            .is_err()
                    {
                        dropped_to_ui += 1;
                    }

                    if dropped_to_ui >= 10 {
                        let _ = ui_tx.try_send(format!(
                            "(logger) UI log queue dropped {dropped_to_ui} lines"
                        ));
                        dropped_to_ui = 0;
                    }
                }

                let _ = out.flush();
            })
            .ok();

        Self {
            log_tx: tx,
            ui_log_rx: ui_rx,
            handle,
            file_path,
            sample_every,
        }
    }

    /// Non-blocking logging. Drops if queue is full.
    pub fn try_log<S: Into<String>>(
        &self,
        level: LogLevel,
        text: S,
        target: &'static str,
    ) -> Result<(), TrySendError<LogMsg>> {
        let msg = LogMsg {
            level,
            ts_ms: crate::media_agent::utils::now_millis(),
            text: text.into(),
            target,
        };
        self.log_tx.try_send(msg)
    }

    /// Pull one sampled UI line (if any).
    pub fn try_recv_ui(&self) -> Option<String> {
        self.ui_log_rx.try_recv().ok()
    }

    /// Optional: expose the chosen file path (nice for debugging).
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Graceful stop (optional — dropping also stops it).
    pub fn stop(mut self) {
        drop(self.log_tx.clone()); // disconnect sender -> worker exits
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        // Disconnect; worker will exit its recv loop and flush.
        let _ = self.log_tx.clone();
    }
}

/// logs next to the executable (target/{debug,release}), or current dir on error.
fn exe_dir_fallback_cwd() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Human-ish timestamp for filenames without extra deps.
/// Example: "20251102_023045"
fn timestamp_for_filename() -> String {
    // If you want pretty local time, consider the `time` crate.
    // This simple version uses UTC seconds since epoch -> formatted into yyyymmdd_hhmmss.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Convert to y/m/d h:m:s with a tiny integer algorithm:
    // Good enough for filenames without pulling in `chrono`.
    // (Epoch: 1970-01-01; this is a simple UTC converter.)
    let tm = unix_to_utc(secs);
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        tm.year, tm.mon, tm.day, tm.hour, tm.min, tm.sec
    )
}

struct SimpleUtc {
    year: i32,
    mon: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
}

// Tiny UTC conversion (no leap seconds). Adapted from civil-time arithmetic.
fn unix_to_utc(mut s: u64) -> SimpleUtc {
    let sec = (s % 60) as u32;
    s /= 60;
    let min = (s % 60) as u32;
    s /= 60;
    let hour = (s % 24) as u32;
    s /= 24;

    // Days since epoch
    let z = s as i64 + 719468; // shift to civil date epoch (0000-03-01 base)
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = mp + if mp < 10 { 3 } else { -9 }; // [1, 12]
    let year = (y + (m <= 2) as i64) as i32;

    SimpleUtc {
        year,
        mon: m as u32,
        day: d as u32,
        hour,
        min,
        sec,
    }
}
