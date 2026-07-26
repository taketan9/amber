//! Remembering where you were: the two panes' directories and which had focus,
//! saved on quit and restored next launch (when cian is started with no path
//! arguments). Per-user and portable-aware — it lives beside `init.lua`
//! (`session.json`), or next to the executable in portable mode. Nothing
//! sensitive is stored, just directory paths.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{App, FocusedPane};

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Session {
    /// The left pane's directory.
    left: String,
    /// The right pane's directory.
    right: String,
    /// Whether the right pane (rather than the left) had focus.
    #[serde(default)]
    focused_right: bool,
}

impl Session {
    fn dir(s: &str) -> Option<PathBuf> {
        let p = PathBuf::from(s);
        p.is_dir().then_some(p)
    }
    /// The left directory, if it still exists.
    pub(crate) fn left_dir(&self) -> Option<PathBuf> {
        Self::dir(&self.left)
    }
    /// The right directory, if it still exists.
    pub(crate) fn right_dir(&self) -> Option<PathBuf> {
        Self::dir(&self.right)
    }
    pub(crate) fn focused_right(&self) -> bool {
        self.focused_right
    }
}

/// Load the saved session (portable-aware), or `None` if there is none / it is
/// unreadable.
pub(crate) fn restore() -> Option<Session> {
    let path = cian_lua::config_read_path("session.json").filter(|p| p.exists())?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

impl App {
    /// Write the current pane directories and focus to `session.json`. Called on
    /// quit; failures are silent (a missing session just means a default start).
    pub(crate) fn save_session(&self) {
        let Some(path) = cian_lua::config_write_path("session.json") else { return };
        let s = Session {
            left: self.left.active_ref().cwd.display().to_string(),
            right: self.right.active_ref().cwd.display().to_string(),
            focused_right: matches!(self.last_file_pane, FocusedPane::Right),
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&s) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_resolves_existing_dirs_only() {
        let d = tempfile::tempdir().unwrap();
        let s = Session {
            left: d.path().display().to_string(),
            right: "/no/such/dir/xyzzy".into(),
            focused_right: true,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.left_dir(), Some(d.path().to_path_buf()), "existing dir restored");
        assert_eq!(back.right_dir(), None, "a gone directory is dropped");
        assert!(back.focused_right());
    }

    #[test]
    fn a_missing_focus_field_defaults_to_left() {
        // Forward-compatibility: an older/hand-written file without the field.
        let s: Session = serde_json::from_str(r#"{"left":"/a","right":"/b"}"#).unwrap();
        assert!(!s.focused_right());
    }
}
