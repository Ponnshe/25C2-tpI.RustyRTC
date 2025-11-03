use crate::app::{log_level::LogLevel, log_msg::LogMsg, logger_handle::LoggerHandle};

use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, TrySendError},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

/// Bounded, non-blocking logger that writes to a per-process log file,
/// and provides a sampled "UI tap" channel for lightweight UI display.
pub struct Logger {
    handle: LoggerHandle,
    ui_log_rx: std::sync::mpsc::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
    file_path: std::path::PathBuf,
    sample_every: u32,
}

impl Logger {
    #[must_use]
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
        let handle_for_field = LoggerHandle { tx: tx.clone() };
        let file_path_clone = file_path.clone();

        let thread = thread::Builder::new()
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
                        n.is_multiple_of(sample_every)
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
            handle: handle_for_field,
            ui_log_rx: ui_rx,
            thread,
            file_path,
            sample_every,
        }
    }

    /// Attempts to enqueue a log message without blocking the current thread.
    ///
    /// This method sends the message to the logger’s internal synchronous channel.
    /// If the channel is full, the message is **dropped** and an error is returned.
    ///
    /// This function never blocks — use [`log`](Self::log) for the blocking variant.
    ///
    /// # Parameters
    /// - `level`: The severity level of the message (e.g. `Info`, `Warn`, `Error`).
    /// - `text`: Any type convertible into a `String`, containing the log message.
    ///
    /// # Returns
    /// Returns `Ok(())` if the message was successfully enqueued for logging.
    /// Otherwise, returns a [`TrySendError<LogMsg>`] indicating that the internal
    /// queue was full and the message was **not sent**.
    ///
    /// # Errors
    /// Returns `Err(TrySendError::Full)` if the logger’s internal bounded queue
    /// has reached its capacity.
    /// This error means the message was **dropped** — no retry is performed.
    ///
    /// # Examples
    /// ```ignore
    /// use rustyrtc::app::logger::{Logger, LogLevel};
    ///
    /// let logger = Logger::start_in_dir("logs", "app", 100, 10, 1);
    /// let _ = logger.try_log(LogLevel::Info, "Background task started");
    /// ```
    ///
    /// # See also
    /// - [`std::sync::mpsc::SyncSender::try_send`]
    /// - [`Self::log`] for the blocking variant
    ///
    /// # Panics
    /// This function never panics.
    pub fn try_log<S: Into<String>>(
        &self,
        level: LogLevel,
        text: S,
        target: &'static str,
    ) -> Result<(), TrySendError<LogMsg>> {
        self.handle.try_log(level, text, target)
    }
    /// Give modules a cloneable sink they can keep.
    #[must_use]
    pub fn handle(&self) -> LoggerHandle {
        self.handle.clone()
    }

    #[must_use]
    /// Pull one sampled UI line (if any).
    #[must_use]
    pub fn try_recv_ui(&self) -> Option<String> {
        self.ui_log_rx.try_recv().ok()
    }

    #[must_use]
    /// Optional: expose the chosen file path (nice for debugging).
    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }
}

/// logs next to the executable (target/{debug,release}), or current dir on error.
fn exe_dir_fallback_cwd() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Human-ish timestamp for filenames without extra deps.
/// Example: `20251102_023045`
fn timestamp_for_filename() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match unix_to_utc(secs) {
        Ok(tm) => format!(
            "{:04}{:02}{:02}_{:02}{:02}{:02}",
            tm.year, tm.mon, tm.day, tm.hour, tm.min, tm.sec
        ),
        Err(_) => format!("unix_{secs}"), // graceful fallback, never panics
    }
}

#[derive(Clone, Copy, Debug)]
struct SimpleUtc {
    year: i32,
    mon: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
}

#[derive(Debug)]
enum UtcConvError {
    YearOutOfRange(i128),
    MonthOutOfRange(i128),
    DayOutOfRange(i128),
}

/// Conversión UTC mínima (sin segundos intercalares).
/// Nota: no es `const fn` porque usa Result/try_from; silenciamos solo ese hint.
#[allow(clippy::missing_const_for_fn)]
fn unix_to_utc(mut s: u64) -> Result<SimpleUtc, UtcConvError> {
    let sec = (s % 60) as u32;
    s /= 60;
    let min = (s % 60) as u32;
    s /= 60;
    let hour = (s % 24) as u32;
    s /= 24;

    // Cálculo en i128 para evitar wrap.
    let z: i128 = s as i128 + 719_468;

    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = mp + if mp < 10 { 3 } else { -9 }; // [1, 12]

    let year_i = y + i128::from(m <= 2);

    // Validaciones explícitas + conversiones seguras (sin `expect`).
    if year_i < i32::MIN as i128 || year_i > i32::MAX as i128 {
        return Err(UtcConvError::YearOutOfRange(year_i));
    }
    if !(1..=12).contains(&m) {
        return Err(UtcConvError::MonthOutOfRange(m));
    }
    if !(1..=31).contains(&d) {
        return Err(UtcConvError::DayOutOfRange(d));
    }

    let year = year_i as i32;
    let mon = m as u32;
    let day = d as u32;

    Ok(SimpleUtc {
        year,
        mon,
        day,
        hour,
        min,
        sec,
    })
}
