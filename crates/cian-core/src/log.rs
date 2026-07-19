//! Opt-in diagnostic logging.
//!
//! cian is a full-screen TUI, so it cannot print diagnostics to the terminal —
//! anything written to stdout would corrupt the display. Instead, setting
//! `CIAN_LOG=/path/to/file` makes [`log`] append timestamped lines there.
//! When the variable is unset (the normal case) every call is a cheap no-op.
//!
//! This exists to make rare, hard-to-reproduce faults reportable: panics, PTY
//! spawn failures, and lock poisoning that would otherwise surface only as the
//! UI mysteriously freezing.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Resolved once from `$CIAN_LOG`; `None` means logging is off.
fn log_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::var_os("CIAN_LOG").filter(|s| !s.is_empty()).map(PathBuf::from)
    })
    .as_ref()
}

/// Whether logging is enabled, so callers can skip building expensive messages.
pub fn enabled() -> bool {
    log_path().is_some()
}

/// Append one timestamped line to the log file. Never panics and never fails
/// loudly: a broken log path must not take down the file manager.
pub fn log(msg: &str) {
    let Some(path) = log_path() else { return };
    // Serialise writes so lines from the UI thread and PTY reader threads do
    // not interleave mid-line.
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock();

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}.{:03}", d.as_secs(), d.subsec_millis()))
        .unwrap_or_else(|_| "?".to_string());

    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {}", stamp, msg);
    }
}
