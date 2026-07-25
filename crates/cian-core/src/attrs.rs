//! File attributes and checksums.
//!
//! Both are things you go to a file manager for and then leave it to run a
//! command: "what are the permissions on this", "does this match the hash they
//! sent me". Hashing streams and takes a cancel flag, because the files worth
//! checksumming are usually the large ones.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};

/// Which digest to compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashKind {
    Md5,
    Sha256,
}

impl HashKind {
    pub fn label(self) -> &'static str {
        match self {
            HashKind::Md5 => "MD5",
            HashKind::Sha256 => "SHA-256",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().replace('-', "").as_str() {
            "md5" => Some(HashKind::Md5),
            "sha256" | "sha2" | "" => Some(HashKind::Sha256),
            _ => None,
        }
    }
}

/// Hash a file, reading it in chunks so a large one neither loads into memory
/// nor delays a cancel until it finishes.
///
/// Returns `Ok(None)` if it was cancelled.
pub fn hash_file(path: &Path, kind: HashKind, cancel: &AtomicBool) -> Result<Option<String>> {
    use md5::Digest as _;

    let mut f = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut buf = vec![0u8; 1024 * 1024];
    let mut md5 = md5::Md5::new();
    let mut sha = sha2::Sha256::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let n = f.read(&mut buf).with_context(|| format!("read {}", path.display()))?;
        if n == 0 {
            break;
        }
        match kind {
            HashKind::Md5 => md5.update(&buf[..n]),
            HashKind::Sha256 => sha.update(&buf[..n]),
        }
    }
    Ok(Some(match kind {
        HashKind::Md5 => format!("{:x}", md5.finalize()),
        HashKind::Sha256 => format!("{:x}", sha.finalize()),
    }))
}

/// A file's permissions in the form the platform actually uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attrs {
    /// Unix mode bits, if this is a Unix filesystem.
    pub mode: Option<u32>,
    /// Whether the file is read-only. Meaningful everywhere.
    pub readonly: bool,
    /// Owner, as a name where one can be resolved.
    pub owner: Option<String>,
    /// Size in bytes. `None` for directories, where a byte count is not what
    /// anyone means by "size".
    pub size: Option<u64>,
    /// True for a directory, so callers can label it rather than show a size.
    pub is_dir: bool,
}

impl Attrs {
    /// `rwxr-xr-x`-style rendering of the mode, or a plain read-only flag
    /// where there are no mode bits to show.
    pub fn describe(&self) -> String {
        match self.mode {
            Some(m) => {
                let bit = |i: u32, c: char| if m & (1 << i) != 0 { c } else { '-' };
                format!(
                    "{}{}{}{}{}{}{}{}{}  ({:04o})",
                    bit(8, 'r'), bit(7, 'w'), bit(6, 'x'),
                    bit(5, 'r'), bit(4, 'w'), bit(3, 'x'),
                    bit(2, 'r'), bit(1, 'w'), bit(0, 'x'),
                    m & 0o7777
                )
            }
            None => {
                if self.readonly {
                    "read-only".to_string()
                } else {
                    "read/write".to_string()
                }
            }
        }
    }
}

pub fn read_attrs(path: &Path) -> Result<Attrs> {
    let meta = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let readonly = meta.permissions().readonly();
    let is_dir = meta.is_dir();
    // A directory's byte length is a filesystem detail, not the "size" a user
    // means; leave it out and let the caller label it as a folder.
    let size = if is_dir { None } else { Some(meta.len()) };

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
        // Resolving a uid to a name needs the passwd database; showing the
        // number is better than pretending to know.
        let owner = Some(meta.uid().to_string());
        Ok(Attrs { mode: Some(mode), readonly, owner, size, is_dir })
    }
    #[cfg(not(unix))]
    {
        Ok(Attrs { mode: None, readonly, owner: None, size, is_dir })
    }
}

/// Apply a `chmod`-style mode. Octal only: symbolic forms like `u+x` are a
/// small language of their own and guessing at them would be worse than
/// refusing.
pub fn set_mode(path: &Path, spec: &str) -> Result<()> {
    let spec = spec.trim();
    let mode = u32::from_str_radix(spec, 8)
        .with_context(|| format!("not an octal mode: {:?} (try 644 or 755)", spec))?;
    if mode > 0o7777 {
        anyhow::bail!("mode out of range: {:o}", mode);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("chmod {} {}", spec, path.display()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        // Windows has no mode bits; the closest thing is the read-only flag,
        // and silently reinterpreting 644 as "read-only" would be a lie.
        let _ = (mode, path);
        anyhow::bail!("Windows has no mode bits — use `:readonly on|off` instead")
    }
}

/// Set or clear the read-only flag. The one attribute that means something on
/// every platform cian runs on.
pub fn set_readonly(path: &Path, readonly: bool) -> Result<()> {
    let meta = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let mut perms = meta.permissions();
    perms.set_readonly(readonly);
    fs::set_permissions(path, perms)
        .with_context(|| format!("set read-only on {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_known_values() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x");
        fs::write(&f, b"abc").unwrap();
        let cancel = AtomicBool::new(false);

        assert_eq!(
            hash_file(&f, HashKind::Md5, &cancel).unwrap().unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hash_file(&f, HashKind::Sha256, &cancel).unwrap().unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// Files worth checksumming are the big ones, so the read has to be
    /// interruptible rather than run to the end regardless.
    #[test]
    fn hashing_can_be_cancelled() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("big");
        fs::write(&f, vec![0u8; 4 * 1024 * 1024]).unwrap();
        let cancel = AtomicBool::new(true);
        assert!(hash_file(&f, HashKind::Sha256, &cancel).unwrap().is_none());
    }

    #[test]
    fn hash_kind_parses_the_spellings_people_type() {
        assert_eq!(HashKind::parse("md5"), Some(HashKind::Md5));
        assert_eq!(HashKind::parse("MD-5"), Some(HashKind::Md5));
        assert_eq!(HashKind::parse("sha256"), Some(HashKind::Sha256));
        assert_eq!(HashKind::parse("SHA-256"), Some(HashKind::Sha256));
        // Bare `:hash` means the sensible default.
        assert_eq!(HashKind::parse(""), Some(HashKind::Sha256));
        assert_eq!(HashKind::parse("crc32"), None);
    }

    #[test]
    fn reading_attributes_reports_something_useful_on_every_platform() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x");
        fs::write(&f, b"hi").unwrap();
        let a = read_attrs(&f).unwrap();
        assert!(!a.readonly);
        let shown = a.describe();
        assert!(!shown.is_empty());
        #[cfg(unix)]
        assert!(shown.contains('r'), "expected an rwx rendering, got {:?}", shown);
    }

    #[test]
    fn read_only_can_be_set_and_cleared() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x");
        fs::write(&f, b"hi").unwrap();

        set_readonly(&f, true).unwrap();
        assert!(read_attrs(&f).unwrap().readonly);
        set_readonly(&f, false).unwrap();
        assert!(!read_attrs(&f).unwrap().readonly);
    }

    #[cfg(unix)]
    #[test]
    fn a_mode_is_applied_and_a_bad_one_refused() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x");
        fs::write(&f, b"hi").unwrap();

        set_mode(&f, "600").unwrap();
        assert_eq!(read_attrs(&f).unwrap().mode.unwrap() & 0o777, 0o600);
        assert_eq!(read_attrs(&f).unwrap().describe().split_whitespace().next().unwrap(), "rw-------");

        // Symbolic forms are refused rather than guessed at.
        assert!(set_mode(&f, "u+x").is_err());
        assert!(set_mode(&f, "999").is_err(), "9 is not an octal digit");
        // Unchanged after the failures.
        assert_eq!(read_attrs(&f).unwrap().mode.unwrap() & 0o777, 0o600);
    }
}
