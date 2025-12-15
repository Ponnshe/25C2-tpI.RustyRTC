use crate::{
    config::Config,
    log::{log_level::LogLevel, log_msg::LogMsg, logger_handle::LoggerHandle},
};

use std::{
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, TrySendError},
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

// -----------------------------------------------------------------------------
// COMPILE-TIME CONFIGURATION
// -----------------------------------------------------------------------------

/// Flush to disk every 100 lines if debugging/tracing (to see crashes near real-time).
#[cfg(feature = "log-debug")]
const FLUSH_BATCH_SIZE: u32 = 100;

/// Flush to disk every 1000 lines in production/default (to save I/O & CPU).
#[cfg(not(feature = "log-debug"))]
const FLUSH_BATCH_SIZE: u32 = 1_000;

// -----------------------------------------------------------------------------

/// Bounded, non-blocking logger that writes to a per-process log file.
///
/// This struct manages a background worker thread that consumes log messages from a
/// bounded channel and writes them to a file. It also provides a secondary "sampled"
/// channel (`ui_log_rx`) to feed a subset of logs to a UI without overwhelming it.
///
/// # Architecture
///
/// 1. **Producers**: Application threads call `try_log`.
/// 2. **Queue**: A bounded `mpsc` channel buffers messages.
/// 3. **Consumer**: A dedicated background thread writes to disk and flushes periodically.
/// 4. **Sampler**: The background thread forwards a sample of logs to the UI channel.
pub struct Logger {
    handle: LoggerHandle,
    ui_log_rx: std::sync::mpsc::Receiver<String>,
    _thread: Option<std::thread::JoinHandle<()>>,
    file_path: std::path::PathBuf,
    _sample_every: u32,
}

impl Logger {
    /// Initializes the logger for a client application context.
    ///
    /// Reads configuration from `Config` using client-specific keys ("client_log_filename").
    #[must_use]
    pub fn start_client(cap: usize, ui_cap: usize, sample_every: u32, config: Arc<Config>) -> Self {
        Self::start(
            "client_log_filename",
            "client_log_path",
            cap,
            ui_cap,
            sample_every,
            config,
        )
    }

    /// Initializes the logger for a server application context.
    ///
    /// Reads configuration from `Config` using server-specific keys ("server_log_filename").
    #[must_use]
    pub fn start_server(cap: usize, ui_cap: usize, sample_every: u32, config: Arc<Config>) -> Self {
        Self::start(
            "server_log_filename",
            "server_log_path",
            cap,
            ui_cap,
            sample_every,
            config,
        )
    }

    /// Internal helper to resolve configuration and start the logger.
    #[must_use]
    fn start(
        fn_key: &str,
        path_key: &str,
        cap: usize,
        ui_cap: usize,
        sample_every: u32,
        config: Arc<Config>,
    ) -> Self {
        let app_name = config.get_non_empty("Loggin", fn_key);

        if let Some(dir_str) = config.get_non_empty("Logging", path_key) {
            let dir = expand_path(dir_str);
            Self::start_in_dir(dir, app_name, cap, ui_cap, sample_every)
        } else {
            Self::start_default(app_name, cap, ui_cap, sample_every)
        }
    }

    /// Creates a `logs/` directory next to the executable and starts the logger there.
    ///
    /// # Example Filename
    /// `target/debug/logs/roomrtc-20251102_023045-pid1234.log`
    #[must_use]
    pub fn start_default(
        app_name: Option<&str>,
        cap: usize,
        ui_cap: usize,
        sample_every: u32,
    ) -> Self {
        let base = exe_dir_fallback_cwd().join("logs");
        Self::start_in_dir(base, app_name, cap, ui_cap, sample_every)
    }

    /// Starts the logger in a specific directory.
    ///
    /// This function:
    /// 1. Creates the target directory if it is missing.
    /// 2. Generates a unique filename based on the timestamp and process ID (PID).
    /// 3. Spawns the background worker thread.
    ///
    /// # Arguments
    ///
    /// * `dir` - The directory where the log file will be created.
    /// * `app_name` - Optional prefix for the log filename.
    /// * `cap` - Capacity of the main log channel (backpressure buffer).
    /// * `ui_cap` - Capacity of the UI sampling channel.
    /// * `sample_every` - Only 1 out of every N info/debug messages is sent to the UI.
    pub fn start_in_dir<D: AsRef<Path>>(
        dir: D,
        app_name: Option<&str>,
        cap: usize,
        ui_cap: usize,
        sample_every: u32,
    ) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let _ = fs::create_dir_all(&dir);

        // Avoid potential modulo-by-zero later.
        let _sample_every = sample_every.max(1);

        // Calculated once to avoid code repetition.
        let ts = timestamp_for_filename();
        let pid = std::process::id();

        // Determine filename based on whether app_name is provided.
        let fname = if let Some(name) = app_name {
            format!("{}-{}-pid{}.log", name, ts, pid)
        } else {
            format!("{}-pid{}.log", ts, pid)
        };

        let file_path = dir.join(&fname);

        let (tx, rx) = mpsc::sync_channel::<LogMsg>(cap);
        let (ui_tx, ui_rx) = mpsc::sync_channel::<String>(ui_cap);

        // No redundant clone: consume `tx` into the handle (we don't use `tx` afterwards).
        let handle_for_field = LoggerHandle { tx };

        let file_path_clone = file_path.clone();

        let _thread = thread::Builder::new()
            .name("logger-worker".into())
            .spawn(move || {
                // Try target file -> temp file -> sink (never panic).
                let writer: Box<dyn Write + Send> = if let Ok(f) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file_path_clone)
                {
                    Box::new(f)
                } else {
                    let fallback = std::env::temp_dir().join("roomrtc-fallback.log");
                    match OpenOptions::new().create(true).append(true).open(&fallback) {
                        Ok(f) => Box::new(f),
                        Err(_) => Box::new(io::sink()),
                    }
                };

                let mut out: BufWriter<Box<dyn Write + Send>> = BufWriter::new(writer);

                let mut n: u32 = 0;
                let mut lines_written: u32 = 0;
                let mut dropped_to_ui: usize = 0;

                while let Ok(m) = rx.recv() {
                    let _ = writeln!(&mut out, "[{:?}] {} | {}", m.level, m.ts_ms, m.text);
                    lines_written = lines_written.wrapping_add(1);

                    // Flush periodically to ensure data persists on crash.
                    if lines_written.is_multiple_of(FLUSH_BATCH_SIZE) {
                        let _ = out.flush();
                    }

                    // Determine if this message should be forwarded to the UI.
                    // Warn/Error are always forwarded; others are sampled.
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

                    // Report dropped UI messages if the queue is backing up.
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
            _thread,
            file_path,
            _sample_every,
        }
    }

    /// Attempts to enqueue a log message without blocking the current thread.
    ///
    /// This method sends the message to the logger’s internal synchronous channel.
    /// If the channel is full, the message is **dropped** and an error is returned.
    ///
    /// This function never blocks.
    ///
    /// # Parameters
    /// - `level`: The severity level of the message (e.g. `Info`, `Warn`, `Error`).
    /// - `text`: Any type convertible into a `String`, containing the log message.
    /// - `target`: The static module path where the log originated.
    ///
    /// # Returns
    /// Returns `Ok(())` if the message was successfully enqueued for logging.
    /// Otherwise, returns a [`TrySendError<LogMsg>`] indicating that the internal
    /// queue was full and the message was **not sent**.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Assuming logger is initialized
    /// let _ = logger.try_log(LogLevel::Info, "Processing started", module_path!());
    /// ```
    pub fn try_log<S: Into<String>>(
        &self,
        level: LogLevel,
        text: S,
        target: &'static str,
    ) -> Result<(), TrySendError<LogMsg>> {
        self.handle.try_log(level, text, target)
    }

    /// Returns a cloneable handle to the logger sink.
    ///
    /// Useful for passing the logging capability to other modules or threads
    /// without transferring ownership of the main `Logger` struct.
    #[must_use]
    pub fn handle(&self) -> LoggerHandle {
        self.handle.clone()
    }

    /// Attempts to retrieve one sampled log line for UI display.
    ///
    /// Returns `None` if the UI channel is empty.
    #[must_use]
    pub fn try_recv_ui(&self) -> Option<String> {
        self.ui_log_rx.try_recv().ok()
    }

    /// Returns the path of the active log file.
    ///
    /// Useful for debugging or displaying the log location to the user.
    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }
}

/// Locates the `logs` directory next to the executable (target/{debug,release}),
/// or falls back to the current working directory on error.
fn exe_dir_fallback_cwd() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Generates a human-readable timestamp for filenames without external dependencies.
///
/// Output Format: `YYYYMMDD_HHMMSS` (e.g., `20251102_023045`)
fn timestamp_for_filename() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    unix_to_utc(secs).map_or_else(
        |_| format!("unix_{secs}"), // graceful fallback, never panics
        |tm| {
            format!(
                "{:04}{:02}{:02}_{:02}{:02}{:02}",
                tm.year, tm.mon, tm.day, tm.hour, tm.min, tm.sec
            )
        },
    )
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
    Year,
    Month,
    Day,
}

/// Minimal UTC conversion (Civl Time) to avoid importing `chrono`.
///
/// Implements the algorithm to convert UNIX timestamp to a Gregorian date.
/// Note: not a `const fn` because it uses `Result/try_from`.
///
/// # Errors
///
/// Returns a [`UtcConvError`] if the calculated components generally overflow or
/// cannot be represented in standard integer types:
///
/// * [`UtcConvError::Year`] - If the calculated year does not fit in an `i32`.
/// * [`UtcConvError::Month`] - If the month cannot be converted to `u32` (unlikely by algorithm design).
/// * [`UtcConvError::Day`] - If the day cannot be converted to `u32` (unlikely by algorithm design).
#[allow(clippy::missing_const_for_fn, clippy::many_single_char_names)]
fn unix_to_utc(mut s: u64) -> Result<SimpleUtc, UtcConvError> {
    use std::convert::TryFrom;

    let sec = (s % 60) as u32;
    s /= 60;
    let min = (s % 60) as u32;
    s /= 60;
    let hour = (s % 24) as u32;
    s /= 24;

    // Use i128 to prevent overflow during intermediate calculations.
    let z: i128 = i128::from(s) + 719_468;

    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = mp + if mp < 10 { 3 } else { -9 }; // [1, 12]

    let year_i = y + i128::from(m <= 2);

    let year = i32::try_from(year_i).map_err(|_| UtcConvError::Year)?;
    let mon = u32::try_from(m).map_err(|_| UtcConvError::Month)?;
    let day = u32::try_from(d).map_err(|_| UtcConvError::Day)?;

    Ok(SimpleUtc {
        year,
        mon,
        day,
        hour,
        min,
        sec,
    })
}

/// Expands tilde (`~`) in file paths to the user's home directory.
fn expand_path(path_str: &str) -> PathBuf {
    if path_str.starts_with("~") {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from);

        if let Some(mut home_path) = home {
            if path_str == "~" {
                return home_path;
            }
            if path_str.starts_with("~/") || path_str.starts_with("~\\") {
                home_path.push(&path_str[2..]);
                return home_path;
            }
        }
    }
    PathBuf::from(path_str)
}
