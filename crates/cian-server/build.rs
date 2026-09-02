//! Stamp the build so `:version` can answer "is this the one I just built?".
//!
//! Taketan was told to check the build time in `:version` and there wasn't
//! one — it said `GUI 1.1.0`, which is the same thing yesterday's zip says.
//! Several releases have carried the same version on purpose, and even when
//! the number does move it moves once per release while builds happen all
//! day, so the version can never separate two builds and something else has
//! to.
//!
//! No dependency for this: a `git` that isn't there, or a source tree with no
//! `.git` in it (the offline bundle), falls back to the empty string and the
//! date carries it alone.

use std::process::Command;

fn main() {
    icon();
    // Re-run when HEAD moves, so the stamp cannot go stale in a cached build.
    for p in [".git/HEAD", ".git/refs/heads"] {
        println!("cargo:rerun-if-changed=../../{p}");
    }

    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=CIAN_COMMIT={commit}");

    // Seconds since the epoch, formatted where there is a calendar to hand.
    // Kept as a number here rather than pulling in a date crate for one line.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=CIAN_BUILT_AT={secs}");
}

#[cfg(not(windows))]
fn icon() {}

/// The exe's own icon. The engine is not something anybody double-clicks, but
/// it *is* downloaded on its own as `cian-server-win-x64.exe` and then sits in
/// a folder next to the window it serves — where a generic icon reads as a
/// stray file rather than as part of cian. See `crates/cian-bin/build.rs` for
/// why a `.ico` beside the program does not cover this.
#[cfg(windows)]
fn icon() {
    let icon = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cian.ico");
    assert!(
        std::path::Path::new(icon).exists(),
        "cian.ico is missing from the repository root — run `python3 packaging/icon.py`"
    );
    println!("cargo:rerun-if-changed={icon}");
    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon)
        .set("ProductName", "cian")
        .set("FileDescription", "cian-server — the engine the window talks to");
    res.compile().expect("stamping cian-server.exe with its icon");
}
