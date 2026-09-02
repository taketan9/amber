//! The Windows icon for `cian.exe`, the winit front end.
//!
//! This build is frozen and only compiles when the release workflow is run
//! with `everything`, but it is still the `cian.exe` inside
//! `cian-windows-x64.zip` — the one a person is most likely to double-click.
//! See `crates/cian-bin/build.rs` for why the `.ico` in the zip is not enough
//! on its own.

fn main() {
    icon();
}

#[cfg(not(windows))]
fn icon() {}

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
        .set("FileDescription", "cian — a two-pane file manager");
    res.compile().expect("stamping cian.exe with its icon");
}
