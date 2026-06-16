//! The launcher's own diagnostic log — a real `lucerna.log`, distinct from the
//! game's console output and the game's `latest.log`.
//!
//! Before this module Lucerna's diagnostics used `eprintln!`, which on the
//! Windows GUI build go to a discarded stderr — so there was no launcher log at
//! all (and the Logs viewer's "Launcher logs" group actually showed the game's
//! captured console). `diag!` is a drop-in superset of `eprintln!`: it still
//! prints to stderr (dev/terminal parity) AND appends a timestamped line to the
//! app-wide `<app_data>/logs/lucerna.log`.
//!
//! Best-effort throughout: a failed file write never panics and never blocks;
//! before `init` runs (or if it failed) `diag!` simply behaves like `eprintln!`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use tauri::AppHandle;

/// Rotate `lucerna.log` once it reaches this size, keeping one previous file.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// The append target for the launcher log. Small wrapper so the write path is
/// unit-testable without the global sink.
struct Appender {
    file: File,
}

impl Appender {
    /// Append one `"{ts} {msg}\n"` line. Best-effort — write/flush errors are
    /// swallowed (a broken log must never disrupt the launcher).
    fn write_line(&mut self, ts: &str, msg: &str) {
        let _ = writeln!(self.file, "{ts} {msg}");
        let _ = self.file.flush();
    }
}

static SINK: OnceLock<Mutex<Appender>> = OnceLock::new();

/// `true` once the log has grown to its cap and should be rotated.
fn should_rotate(size: u64, cap: u64) -> bool {
    size >= cap
}

/// Local wall-clock timestamp for a log line (`2026-06-16 14:05:09.123`).
fn now_stamp() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

/// Open (creating if needed) the launcher log under `<app_data>/logs/`,
/// rotating an over-cap previous file, and install the global sink. Idempotent
/// in practice (the `OnceLock` ignores a second `set`). Best-effort: any
/// failure logs to stderr and leaves the sink unset (so `diag!` degrades to
/// plain `eprintln!`).
pub fn init(app: &AppHandle) {
    if let Err(e) = try_init(app) {
        eprintln!("[diag] launcher log init failed: {e}");
    }
}

fn try_init(app: &AppHandle) -> std::io::Result<()> {
    let dir = crate::paths::app_logs_dir(app)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("lucerna.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if should_rotate(meta.len(), MAX_BYTES) {
            // Keep exactly one previous file; ignore failure (next launch retries).
            let _ = std::fs::rename(&path, dir.join("lucerna.prev.log"));
        }
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    let _ = writeln!(
        file,
        "{} === Lucerna {} — session start ===",
        now_stamp(),
        env!("CARGO_PKG_VERSION"),
    );
    let _ = file.flush();
    let _ = SINK.set(Mutex::new(Appender { file }));
    Ok(())
}

/// Backing function for the `diag!` macro. Prints to stderr (matching the old
/// `eprintln!` behavior) and, when the sink is initialized, appends a
/// timestamped line to `lucerna.log`. Never panics.
#[doc(hidden)]
pub fn _write(msg: &str) {
    eprintln!("{msg}");
    if let Some(sink) = SINK.get() {
        if let Ok(mut appender) = sink.lock() {
            appender.write_line(&now_stamp(), msg);
        }
    }
}

/// Launcher diagnostic log line. Drop-in superset of `eprintln!`: same format
/// syntax, still prints to stderr, and also lands (timestamped) in
/// `lucerna.log` once [`init`] has run.
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => {{
        $crate::diag::_write(&format!($($arg)*));
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn should_rotate_at_or_over_cap() {
        assert!(!should_rotate(0, 100));
        assert!(!should_rotate(99, 100));
        assert!(should_rotate(100, 100));
        assert!(should_rotate(101, 100));
    }

    #[test]
    fn now_stamp_is_nonempty() {
        assert!(!now_stamp().is_empty());
    }

    #[test]
    fn appender_writes_timestamped_line() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lucerna.log");
        {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            let mut a = Appender { file };
            a.write_line("2026-06-16 00:00:00.000", "[enrich] hello world");
            a.write_line("2026-06-16 00:00:01.000", "second line");
        }
        let mut s = String::new();
        File::open(&path).unwrap().read_to_string(&mut s).unwrap();
        assert!(s.contains("2026-06-16 00:00:00.000 [enrich] hello world"));
        assert!(s.contains("2026-06-16 00:00:01.000 second line"));
        assert_eq!(s.lines().count(), 2);
    }
}
