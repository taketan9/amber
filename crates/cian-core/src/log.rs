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
///
/// A path that cannot be written to becomes one that can. Diagnostics are
/// asked for at exactly the moment something is wrong, by someone who is
/// already annoyed, and a log that silently goes nowhere costs an evening:
/// `%USERPROFILE%\Desktop` does not exist on a Windows machine whose Desktop
/// is OneDrive's, which is most of them, and the obvious place to ask for the
/// file is the desktop. So the directory is created if it can be, and if it
/// still will not take a file the log lands in the temp directory instead —
/// somewhere, and named, beats nowhere and silent.
fn log_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        let asked = std::env::var_os("CIAN_LOG").filter(|s| !s.is_empty()).map(PathBuf::from)?;
        if let Some(dir) = asked.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        if writable(&asked) {
            return Some(asked);
        }
        // Named after the file that was asked for, so two sessions logging to
        // different places do not land in the same fallback.
        let name = asked.file_name().unwrap_or_else(|| std::ffi::OsStr::new("cian.log"));
        let fallback = std::env::temp_dir().join(name);
        writable(&fallback).then_some(fallback)
    })
    .as_ref()
}

/// Can a line be appended here? Asked once, by creating the file.
fn writable(path: &std::path::Path) -> bool {
    std::fs::OpenOptions::new().create(true).append(true).open(path).is_ok()
}

/// Where the diagnostics are going, for a front end that wants to say so.
///
/// `None` when logging is off. The answer may not be the path that was asked
/// for — see [`log_path`] — which is the whole reason this can be asked.
pub fn destination() -> Option<&'static PathBuf> {
    log_path()
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
