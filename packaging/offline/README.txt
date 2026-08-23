cian — building from source on an offline Windows machine
=========================================================

This package is the whole of cian: the source, and every crate it depends
on, already downloaded. Nothing in a build of it reaches the network.

It is the package to bring in-house when the point is to change cian, not
just to run it. To only run it, take cian-windows-x64.zip instead — that
one is already built.


What has to be on the machine first
-----------------------------------

Four things, and none of them can be fetched during the build, so bring
their installers in with this zip.

1. Rust (MSVC toolchain)
     https://forge.rust-lang.org/infra/other-installation-methods.html
     Take the standalone installer for x86_64-pc-windows-msvc — the .msi,
     not rustup-init.exe, which wants the network.
     BUILT-WITH.txt in this folder records the exact version this package
     was vendored and verified with. That version or newer.

2. Visual Studio Build Tools (the C/C++ compiler)
     Rust on Windows compiles through MSVC's linker, and three of cian's
     dependencies build C of their own. In the Visual Studio Installer,
     the "Desktop development with C++" workload is what you want.
     For an offline install, Microsoft's own layout mechanism:
        vs_BuildTools.exe --layout C:\vslayout ^
          --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended ^
          --lang en-US ja-JP
     Run that on a machine with network, carry C:\vslayout across, and
     install from it there.

3. CMake            https://cmake.org/download/  (the .msi)
4. NASM             https://www.nasm.us/          (the .exe installer)

     3 and 4 are for aws-lc-sys, the cryptography behind cian's SFTP.
     It is the one dependency that builds a C library through CMake, and
     on x86_64 Windows its assembly goes through NASM. Both must be on
     PATH — open a new terminal after installing and check:
        cmake --version
        nasm -v

     If a build fails somewhere inside aws-lc-sys, one of these two is
     the reason nine times out of ten.


Building
--------

Open "x64 Native Tools Command Prompt for VS" — it is the one with the
MSVC environment already set — cd into this folder, and:

    cargo build --release --offline

    --offline is not optional politeness. It tells cargo to use the
    crates in vendor\ and to fail loudly rather than quietly trying to
    reach crates.io. If the build succeeds with it, the build needs no
    network at all.

The two programs land in target\release\ :

    cian-tui.exe    the terminal build
    cian.exe        the window build

The window build carries a Japanese Nerd Font inside it, and needs to be
told to:

    cargo build --release --offline --bin cian --features cian-gui/bundled-font

Without that feature it looks for a font on the system instead, which is
fine on a machine that has one.

Running the tests:

    cargo test --workspace --offline


How the offline part works
--------------------------

    vendor\              Every crate, unpacked. About six hundred of them.
    .cargo\config.toml   Tells cargo to read vendor\ instead of crates.io.

Both are already in place. Do not delete .cargo\config.toml — without it
cargo ignores vendor\ entirely and tries the network.


Changing cian
-------------

Editing the source and rebuilding needs nothing further: the crates are
all here.

Adding or upgrading a dependency does need the network, because the new
crate is not in vendor\. Do that on a machine that has one:

    cargo add <crate>            # or edit Cargo.toml
    cargo vendor --versioned-dirs vendor > .cargo/config.toml

and bring the whole folder back across. There is no way around this; a
crate that has never been downloaded has to be downloaded once.


Contents
--------

    Cargo.toml, Cargo.lock, crates\   The source.
    vendor\                            Its dependencies.
    .cargo\config.toml                 The redirect that makes them count.
    crates\cian-gui\fonts\cian.ttf     The bundled font. Normally fetched
                                       during a build; carried here instead.
    examples\init.lua                  A starter configuration.
    packaging\windows\install.ps1      Puts a built exe on PATH.
    BUILT-WITH.txt                     The compiler and commit this was
                                       vendored from.
    README.md / README.ja.md           The manual.
