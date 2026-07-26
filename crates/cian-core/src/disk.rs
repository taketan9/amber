//! Disk-space for the filesystem a path lives on.
//!
//! A file manager that browses huge trees and multi-gigabyte DB dumps wants the
//! free space of the current mount in view at all times — so a copy or an
//! extract is a glance away from "will this even fit". We ask the OS directly
//! (via `fs4`, which wraps `statvfs` on Unix and `GetDiskFreeSpaceExW` on
//! Windows); a path that cannot be queried simply yields `None`.

use std::path::Path;

/// Free and total bytes of the filesystem containing `path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Bytes available to the current user (already accounts for root-reserve).
    pub free: u64,
    /// Total size of the filesystem.
    pub total: u64,
}

impl Usage {
    /// Bytes in use — total minus free, saturating (some filesystems report a
    /// free count larger than total for unprivileged users).
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }

    /// Fraction used in `0.0..=1.0` (0 when the total is unknown/zero).
    pub fn used_fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.used() as f64 / self.total as f64).clamp(0.0, 1.0)
        }
    }
}

/// Query the filesystem `path` sits on. `None` if the OS call fails (a path that
/// does not exist, a permission error, or an unsupported target).
pub fn usage(path: &Path) -> Option<Usage> {
    let free = fs4::available_space(path).ok()?;
    let total = fs4::total_space(path).ok()?;
    Some(Usage { free, total })
}

/// A compact human-readable byte size: `12.3G`, `948M`, `4.0K`, `512B`. Binary
/// units (1024), one decimal from kilobytes up, trimmed of a trailing `.0`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{}{}", bytes, UNITS[0])
    } else {
        let s = format!("{:.1}", v);
        let s = s.strip_suffix(".0").unwrap_or(&s).to_string();
        format!("{}{}", s, UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_scales_and_trims() {
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(1024), "1K");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(4 * 1024), "4K");
        assert_eq!(human_size(12_800_000_000), "11.9G");
    }

    #[test]
    fn usage_of_the_current_dir_is_sane() {
        // The filesystem the tests run on always answers, and free ≤ total.
        let u = usage(Path::new(".")).expect("current dir is queryable");
        assert!(u.total > 0);
        assert!(u.free <= u.total);
        assert!((0.0..=1.0).contains(&u.used_fraction()));
    }
}
