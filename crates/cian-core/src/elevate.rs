//! Retrying a file copy with administrator rights on Windows.
//!
//! Writing into a protected directory (`C:\Program Files`, `C:\Windows`, …)
//! fails for an ordinary process with "Access is denied". There is no way to
//! push past the ACL without elevation, so — exactly like Explorer's "you'll
//! need to provide administrator permission" dialog — cian offers to redo the
//! copy through a UAC prompt.
//!
//! It works by writing a small PowerShell script describing the copies and
//! launching it elevated with `Start-Process -Verb RunAs`. The elevated process
//! (robocopy for trees, Copy-Item for single files) runs outside cian's own
//! progress bar; cian just waits for it and reports whether it succeeded. On
//! non-Windows platforms there is no equivalent, so the call reports that.

use std::path::PathBuf;

use anyhow::{anyhow, Result};

/// Does any link in this error chain report an OS "permission denied"? That is
/// the cue, on Windows, that an administrator retry might get through.
pub fn is_permission_denied(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
            .unwrap_or(false)
    })
}

/// One copy to perform: `src` (a file or a directory tree) into the directory
/// `dest_dir`.
#[derive(Debug, Clone)]
pub struct CopyItem {
    pub src: PathBuf,
    pub dest_dir: PathBuf,
}

/// Escape a string for a PowerShell single-quoted literal: only `'` is special,
/// and it is doubled.
fn ps_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Build the PowerShell script that performs `items` (and deletes the sources
/// afterwards when `move_after`). Kept separate from the launching so its
/// quoting can be tested off-Windows.
fn build_ps_script(items: &[CopyItem], move_after: bool) -> String {
    let mut s = String::new();
    // Fail on the first real error; robocopy is checked by its own exit code.
    s.push_str("$ErrorActionPreference = 'Stop'\n");
    s.push_str("try {\n");
    for it in items {
        let src = ps_single_quote(&it.src.display().to_string());
        let dst = ps_single_quote(&it.dest_dir.display().to_string());
        s.push_str(&format!("  if (Test-Path -LiteralPath {src} -PathType Container) {{\n"));
        s.push_str(&format!(
            "    $target = Join-Path {dst} (Split-Path -Leaf {src})\n"
        ));
        // robocopy handles large trees and long paths; /E copies subdirs
        // including empty ones, /R:1 /W:1 keeps a bad file from hanging.
        s.push_str(&format!(
            "    robocopy {src} $target /E /COPY:DAT /R:1 /W:1 | Out-Null\n"
        ));
        // robocopy uses exit codes 0-7 for success, 8+ for failure.
        s.push_str("    if ($LASTEXITCODE -ge 8) { throw 'robocopy failed' }\n");
        s.push_str("  } else {\n");
        s.push_str(&format!(
            "    Copy-Item -LiteralPath {src} -Destination {dst} -Force\n"
        ));
        s.push_str("  }\n");
        if move_after {
            s.push_str(&format!("  Remove-Item -LiteralPath {src} -Recurse -Force\n"));
        }
    }
    s.push_str("  exit 0\n");
    s.push_str("} catch {\n");
    s.push_str("  exit 1\n");
    s.push_str("}\n");
    s
}

/// Perform `items` with administrator rights, prompting once via UAC. Blocks
/// until the elevated copy finishes (or the user declines the prompt).
pub fn elevated_copy(items: &[CopyItem], move_after: bool) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    run_elevated(&build_ps_script(items, move_after))
}

#[cfg(windows)]
fn run_elevated(script: &str) -> Result<()> {
    use anyhow::Context;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    // A unique temp path; PowerShell 5.1 reads a .ps1 as ANSI unless it starts
    // with a UTF-8 BOM, so prepend one to keep non-ASCII (e.g. Japanese) paths
    // intact.
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let path = std::env::temp_dir().join(format!("cian-elevate-{}-{}.ps1", std::process::id(), stamp));
    {
        let mut f = std::fs::File::create(&path)
            .with_context(|| format!("write elevation script {}", path.display()))?;
        f.write_all("\u{feff}".as_bytes()).ok(); // UTF-8 BOM
        f.write_all(script.as_bytes()).context("write elevation script")?;
    }

    // Outer (non-elevated) PowerShell launches the elevated one and waits,
    // propagating its exit code. A declined UAC prompt makes Start-Process
    // throw, so the outer process exits non-zero and we report a failure.
    let launcher = format!(
        "$p = Start-Process powershell -Verb RunAs -Wait -PassThru \
         -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','{}'; exit $p.ExitCode",
        path.display()
    );
    let status = std::process::crate::proc::quiet("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &launcher])
        .status()
        .context("launch elevated PowerShell")?;

    let _ = std::fs::remove_file(&path);

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "the elevated copy did not complete (the administrator prompt may have been declined)"
        ))
    }
}

#[cfg(not(windows))]
fn run_elevated(_script: &str) -> Result<()> {
    // Elevation here is a Windows/UAC concept. Elsewhere the answer is to run
    // the copy from a shell with the right permissions (sudo, etc.), which cian
    // deliberately does not do for the user.
    Err(anyhow!("an administrator retry is only available on Windows"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_denied_is_detected_through_the_chain() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let wrapped = anyhow::Error::new(io).context("create C:/Program Files/x");
        assert!(is_permission_denied(&wrapped));

        let other = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "missing",
        ));
        assert!(!is_permission_denied(&other));
    }

    #[test]
    fn the_script_quotes_paths_and_picks_the_right_command() {
        let items = vec![
            CopyItem { src: PathBuf::from(r"C:\tmp\a file.txt"), dest_dir: PathBuf::from(r"C:\Program Files\App") },
        ];
        let s = build_ps_script(&items, false);
        // Single-quoted literals, with spaces preserved and no move step.
        assert!(s.contains("'C:\\tmp\\a file.txt'"), "{s}");
        assert!(s.contains("'C:\\Program Files\\App'"), "{s}");
        assert!(s.contains("Copy-Item"), "{s}");
        assert!(s.contains("robocopy"), "{s}");
        assert!(!s.contains("Remove-Item"), "no delete unless moving");
    }

    #[test]
    fn a_move_deletes_the_source_and_an_apostrophe_is_escaped() {
        let items = vec![CopyItem {
            src: PathBuf::from(r"C:\it's mine"),
            dest_dir: PathBuf::from(r"C:\Windows\dst"),
        }];
        let s = build_ps_script(&items, true);
        assert!(s.contains("Remove-Item"), "move removes the source");
        // The apostrophe is doubled inside the single-quoted literal.
        assert!(s.contains("'C:\\it''s mine'"), "{s}");
    }
}
