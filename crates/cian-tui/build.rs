//! Bake the commit into the binary so `cian --version` can identify a build.
//!
//! Without this there is no way to tell which build is running, which has
//! already cost one debugging session chasing "missing features" that turned
//! out to be an older exe still on PATH.

use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        // Release builds run in a checkout, but a source tarball has no git.
        .unwrap_or_else(|| "unknown".to_string());

    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    println!("cargo:rustc-env=CIAN_COMMIT={}{}", sha, if dirty { "-dirty" } else { "" });
    // Only the commit matters here; rerunning on every source change would be
    // wasteful, but HEAD moving must be picked up.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}
