//! Built-in remote file transfer over pure-Rust russh.
//!
//! cian's shell can already open an SSH session; this adds moving files across
//! one without shelling out to `scp`. Two wire protocols are supported and
//! chosen automatically over a single authenticated connection:
//!
//!   * **SFTP** — the modern subsystem (what today's `scp` uses under the hood).
//!     Tried first.
//!   * **SCP** — the classic `rcp`-style protocol, driven by exec'ing
//!     `scp -t`/`scp -f` on the server. Used as a fallback when the SFTP
//!     subsystem is disabled (some appliances and locked-down sshd configs),
//!     which is the whole reason a file manager still needs it.
//!
//! Each transfer runs from cian's ordinary worker threads: it spins a tiny
//! current-thread tokio runtime, reports progress through a callback and
//! watches a cancel flag, exactly like the local file operations.
//!
//! Host-key verification is not done yet — the server's key is accepted
//! unseen. That is a known gap (TeraTerm would prompt); it is called out at the
//! call site so a future change can add a known-hosts check.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, Context, Result};
use russh::client::{self, AuthResult, Handler};
use russh_sftp::client::SftpSession;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Where to connect and who as. The password is resolved by the caller (from
/// the configured value or `password_cmd`) before we get here.
#[derive(Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

/// Cancellation and progress, mirroring `cian_core::progress::Ctl`.
pub struct Ctl<'a> {
    pub cancel: &'a AtomicBool,
    /// Called with `(bytes_done, bytes_total)` as the transfer advances.
    pub on_progress: &'a mut dyn FnMut(u64, u64),
}

/// Which wire protocol carried a transfer, so the UI can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Sftp,
    Scp,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Sftp => "SFTP",
            Transport::Scp => "SCP",
        }
    }
}

/// How much to move per read/write; big enough to keep the link busy, small
/// enough that progress and cancel stay responsive.
const CHUNK: usize = 64 * 1024;

/// Accepts the server's key without checking it — see the module note.
struct BlindClient;

impl Handler for BlindClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Upload a local file to `remote_path` on the server. Tries SFTP, then falls
/// back to the classic SCP protocol if the SFTP subsystem is unavailable.
/// Returns which transport actually carried it.
/// Upload `local` to `remote_path`. `mode` (Unix permission bits, e.g. `0o777`)
/// is applied to the uploaded file when given.
pub fn upload(
    target: &Target,
    local: &Path,
    remote_path: &str,
    mode: Option<u32>,
    ctl: &mut Ctl,
) -> Result<Transport> {
    on_runtime(|| async {
        let handle = connect(target).await?;
        let total = std::fs::metadata(local).map(|m| m.len()).unwrap_or(0);
        match open_sftp(&handle).await {
            Ok(sftp) => {
                sftp_upload(&sftp, local, remote_path, total, mode, ctl).await?;
                Ok(Transport::Sftp)
            }
            Err(_) => {
                scp_upload(&handle, local, remote_path, total, mode, ctl).await?;
                Ok(Transport::Scp)
            }
        }
    })
}

/// Download `remote_path` from the server to a local file. Tries SFTP, then
/// falls back to the classic SCP protocol. Returns which transport carried it.
pub fn download(
    target: &Target,
    remote_path: &str,
    local: &Path,
    ctl: &mut Ctl,
) -> Result<Transport> {
    on_runtime(|| async {
        let handle = connect(target).await?;
        match open_sftp(&handle).await {
            Ok(sftp) => {
                sftp_download(&sftp, remote_path, local, ctl).await?;
                Ok(Transport::Sftp)
            }
            Err(_) => {
                scp_download(&handle, remote_path, local, ctl).await?;
                Ok(Transport::Scp)
            }
        }
    })
}

/// One entry in a remote directory listing (for the download browser).
#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

/// List a remote directory over SFTP (browsing needs the SFTP subsystem; the
/// classic SCP protocol cannot enumerate). Directories sort first, then by name.
///
/// Returns the *canonical absolute* path alongside the entries: the caller may
/// pass a relative path like "." (the login home), and resolving it to e.g.
/// `/home/userA` is what lets the browser climb up past the home directory all
/// the way to `/`.
pub fn list_dir(target: &Target, remote_path: &str) -> Result<(String, Vec<RemoteEntry>)> {
    on_runtime(|| async {
        let handle = connect(target).await?;
        let sftp = open_sftp(&handle)
            .await
            .context("this server has no SFTP subsystem, so remote browsing is unavailable")?;
        // Resolve "." / relative paths to an absolute path so parent navigation
        // has something to climb; fall back to the input if the server refuses.
        let canon = sftp
            .canonicalize(remote_path)
            .await
            .unwrap_or_else(|_| remote_path.to_string());
        let read = sftp.read_dir(remote_path).await.context("read remote directory")?;
        let mut out = Vec::new();
        for entry in read {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let meta = entry.metadata();
            out.push(RemoteEntry {
                is_dir: meta.is_dir(),
                size: meta.size.unwrap_or(0),
                name,
            });
        }
        out.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let _ = sftp.close().await;
        Ok((canon, out))
    })
}

// ── SFTP ────────────────────────────────────────────────────────────────────

async fn sftp_upload(
    sftp: &SftpSession,
    local: &Path,
    remote_path: &str,
    total: u64,
    mode: Option<u32>,
    ctl: &mut Ctl<'_>,
) -> Result<()> {
    let mut src = tokio::fs::File::open(local)
        .await
        .with_context(|| format!("open {}", local.display()))?;
    let mut dst = sftp
        .create(remote_path)
        .await
        .with_context(|| format!("create remote {}", remote_path))?;

    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    (ctl.on_progress)(0, total);
    loop {
        if ctl.cancel.load(Ordering::Relaxed) {
            return Err(anyhow!("cancelled"));
        }
        let n = src.read(&mut buf).await.context("read local")?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).await.context("write remote")?;
        done += n as u64;
        (ctl.on_progress)(done, total);
    }
    dst.shutdown().await.context("finish remote file")?;
    // Apply the requested permission bits (e.g. 0o777) to the uploaded file.
    if let Some(m) = mode {
        let attrs = russh_sftp::protocol::FileAttributes { permissions: Some(m), ..Default::default() };
        let _ = sftp.set_metadata(remote_path, attrs).await;
    }
    let _ = sftp.close().await;
    Ok(())
}

async fn sftp_download(
    sftp: &SftpSession,
    remote_path: &str,
    local: &Path,
    ctl: &mut Ctl<'_>,
) -> Result<()> {
    let total = sftp.metadata(remote_path).await.ok().and_then(|m| m.size).unwrap_or(0);
    let mut src = sftp
        .open(remote_path)
        .await
        .with_context(|| format!("open remote {}", remote_path))?;
    let mut dst = tokio::fs::File::create(local)
        .await
        .with_context(|| format!("create {}", local.display()))?;

    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    (ctl.on_progress)(0, total);
    loop {
        if ctl.cancel.load(Ordering::Relaxed) {
            // Don't leave a half file masquerading as the real download.
            let _ = tokio::fs::remove_file(local).await;
            return Err(anyhow!("cancelled"));
        }
        let n = src.read(&mut buf).await.context("read remote")?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).await.context("write local")?;
        done += n as u64;
        (ctl.on_progress)(done, total);
    }
    dst.flush().await.context("finish local file")?;
    let _ = sftp.close().await;
    Ok(())
}

// ── classic SCP ───────────────────────────────────────────────────────────────
//
// The protocol (see OpenSSH's scp.c): one side runs `scp -t DIR` (sink, accepts
// what we push) or `scp -f FILE` (source, streams to us). Control messages and
// file bytes share the channel; each step is acknowledged with a status byte
// (0 = ok, 1 = warning, 2 = fatal), warnings/errors carrying a text line.

/// Read one SCP acknowledgement byte, turning a non-zero status into an error
/// that includes the server's message.
async fn read_ack<S: AsyncReadExt + Unpin>(stream: &mut S) -> Result<()> {
    let mut b = [0u8; 1];
    stream.read_exact(&mut b).await.context("read scp ack")?;
    match b[0] {
        0 => Ok(()),
        code => {
            let msg = read_line(stream).await.unwrap_or_default();
            Err(anyhow!("scp remote {}: {}", if code == 1 { "warning" } else { "error" }, msg.trim()))
        }
    }
}

/// Read bytes up to and including a `\n`, returning the line without it.
async fn read_line<S: AsyncReadExt + Unpin>(stream: &mut S) -> Result<String> {
    let mut out = Vec::new();
    let mut b = [0u8; 1];
    loop {
        stream.read_exact(&mut b).await.context("read scp line")?;
        if b[0] == b'\n' {
            break;
        }
        out.push(b[0]);
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Single-quote a path for the remote shell, since `scp -t/-f`'s argument is
/// expanded by it. Embedded single quotes are closed, escaped and reopened.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

async fn scp_upload(
    handle: &client::Handle<BlindClient>,
    local: &Path,
    remote_path: &str,
    total: u64,
    mode: Option<u32>,
    ctl: &mut Ctl<'_>,
) -> Result<()> {
    // `remote_path` is the full destination file path; scp -t wants a target
    // and the C-line carries the name, so split them.
    let (dir, name) = match remote_path.rsplit_once('/') {
        Some((d, n)) if !n.is_empty() => (if d.is_empty() { "/" } else { d }, n.to_string()),
        _ => (".", remote_path.to_string()),
    };
    let channel = handle.channel_open_session().await.context("open channel")?;
    channel
        .exec(true, format!("scp -t {}", shell_quote(dir)))
        .await
        .context("start remote scp -t")?;
    let mut stream = channel.into_stream();

    let mut src = tokio::fs::File::open(local)
        .await
        .with_context(|| format!("open {}", local.display()))?;
    scp_send(&mut stream, &name, total, mode, &mut src, ctl).await
}

/// Drive the SCP "sink" protocol on an established stream: announce the file,
/// stream `src`, and confirm. Generic over the transport so it can be tested
/// against an in-memory pipe.
async fn scp_send<S, R>(
    stream: &mut S,
    name: &str,
    total: u64,
    mode: Option<u32>,
    src: &mut R,
    ctl: &mut Ctl<'_>,
) -> Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
    R: AsyncReadExt + Unpin,
{
    read_ack(stream).await?; // remote ready
    // The C-line's mode governs the created file's permissions (default 0644).
    let header = format!("C{:04o} {} {}\n", mode.unwrap_or(0o644) & 0o7777, total, name);
    stream.write_all(header.as_bytes()).await.context("send scp header")?;
    stream.flush().await.ok();
    read_ack(stream).await?; // header accepted

    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    (ctl.on_progress)(0, total);
    loop {
        if ctl.cancel.load(Ordering::Relaxed) {
            return Err(anyhow!("cancelled"));
        }
        let n = src.read(&mut buf).await.context("read local")?;
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n]).await.context("send file bytes")?;
        done += n as u64;
        (ctl.on_progress)(done, total);
    }
    stream.write_all(&[0u8]).await.context("finish file")?; // end-of-file ack
    stream.flush().await.ok();
    read_ack(stream).await?; // stored ok
    stream.shutdown().await.ok();
    Ok(())
}

async fn scp_download(
    handle: &client::Handle<BlindClient>,
    remote_path: &str,
    local: &Path,
    ctl: &mut Ctl<'_>,
) -> Result<()> {
    let channel = handle.channel_open_session().await.context("open channel")?;
    channel
        .exec(true, format!("scp -f {}", shell_quote(remote_path)))
        .await
        .context("start remote scp -f")?;
    let mut stream = channel.into_stream();

    let mut dst = tokio::fs::File::create(local)
        .await
        .with_context(|| format!("create {}", local.display()))?;
    match scp_recv(&mut stream, &mut dst, ctl).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // Don't leave a half or empty file masquerading as the download.
            drop(dst);
            let _ = tokio::fs::remove_file(local).await;
            Err(e)
        }
    }
}

/// Drive the SCP "source" protocol on an established stream: request the file,
/// read its C-line and payload into `dst`. Generic for in-memory testing.
async fn scp_recv<S, W>(stream: &mut S, dst: &mut W, ctl: &mut Ctl<'_>) -> Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    stream.write_all(&[0u8]).await.context("scp start")?; // tell remote to proceed
    stream.flush().await.ok();

    // Skip any leading directory/time messages until the file's C-line arrives.
    let line = loop {
        let l = read_line(stream).await?;
        match l.as_bytes().first() {
            Some(b'C') => break l,
            Some(b'T') => {
                stream.write_all(&[0u8]).await.ok(); // ack mtime line, keep going
                stream.flush().await.ok();
            }
            Some(1) | Some(2) => return Err(anyhow!("scp remote: {}", l[1..].trim())),
            _ => return Err(anyhow!("unexpected scp reply: {:?}", l)),
        }
    };
    // C<mode> <size> <name>
    let mut parts = line[1..].splitn(3, ' ');
    let _mode = parts.next().unwrap_or("");
    let total: u64 = parts.next().unwrap_or("0").trim().parse().unwrap_or(0);
    stream.write_all(&[0u8]).await.context("ack C-line")?; // start sending
    stream.flush().await.ok();

    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    (ctl.on_progress)(0, total);
    while done < total {
        if ctl.cancel.load(Ordering::Relaxed) {
            return Err(anyhow!("cancelled"));
        }
        let want = ((total - done) as usize).min(buf.len());
        let n = stream.read(&mut buf[..want]).await.context("read remote bytes")?;
        if n == 0 {
            return Err(anyhow!("scp: connection closed mid-file"));
        }
        dst.write_all(&buf[..n]).await.context("write local")?;
        done += n as u64;
        (ctl.on_progress)(done, total);
    }
    read_ack(stream).await?; // trailing status after the payload
    stream.write_all(&[0u8]).await.ok(); // final ack
    stream.flush().await.ok();
    dst.flush().await.context("finish local file")?;
    stream.shutdown().await.ok();
    Ok(())
}

// ── connection ────────────────────────────────────────────────────────────────

/// Run a future to completion on a private current-thread runtime, so the async
/// client can be driven from a plain (non-async) worker thread.
fn on_runtime<F, Fut, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start async runtime")?;
    rt.block_on(f())
}

/// Connect and authenticate with a password. The returned handle owns the SSH
/// connection every channel rides on, so it must outlive the transfer.
async fn connect(target: &Target) -> Result<client::Handle<BlindClient>> {
    let config = std::sync::Arc::new(client::Config::default());
    let mut handle = client::connect(config, (target.host.as_str(), target.port), BlindClient)
        .await
        .with_context(|| format!("connect {}:{}", target.host, target.port))?;

    match handle
        .authenticate_password(target.user.clone(), target.password.clone())
        .await
        .context("authenticate")?
    {
        AuthResult::Success => {}
        AuthResult::Failure { .. } => {
            return Err(anyhow!("authentication failed (wrong password?)"))
        }
    }
    Ok(handle)
}

/// Open an SFTP session on an authenticated connection. Fails (so the caller can
/// fall back to SCP) when the server has no SFTP subsystem.
async fn open_sftp(handle: &client::Handle<BlindClient>) -> Result<SftpSession> {
    let channel = handle.channel_open_session().await.context("open channel")?;
    channel.request_subsystem(true, "sftp").await.context("request sftp subsystem")?;
    let sftp = SftpSession::new(channel.into_stream()).await.context("start sftp")?;
    Ok(sftp)
}

#[cfg(test)]
mod tests {
    //! The SCP wire protocol runs against a fake "remote" on the other end of an
    //! in-memory duplex, so the framing (acks, C-line, payload) is exercised
    //! without a real SSH server — which cian can't stand up in CI anyway.
    use super::*;

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn shell_quote_wraps_and_escapes() {
        assert_eq!(shell_quote("/tmp/x"), "'/tmp/x'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn scp_send_speaks_the_sink_protocol() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let payload = b"hello scp world".to_vec();
            let (mut ours, mut remote) = tokio::io::duplex(4096);

            // The fake `scp -t` running on the "server".
            let expect = payload.clone();
            let server = tokio::spawn(async move {
                remote.write_all(&[0u8]).await.unwrap(); // ready
                let header = read_line(&mut remote).await.unwrap();
                remote.write_all(&[0u8]).await.unwrap(); // header ok
                let size: usize = header[1..].split(' ').nth(1).unwrap().parse().unwrap();
                let mut body = vec![0u8; size];
                remote.read_exact(&mut body).await.unwrap();
                let mut z = [0u8; 1];
                remote.read_exact(&mut z).await.unwrap(); // end-of-file zero
                assert_eq!(z[0], 0);
                remote.write_all(&[0u8]).await.unwrap(); // stored ok
                (header, body, expect)
            });

            let cancel = no_cancel();
            let mut prog = |_a: u64, _b: u64| {};
            let mut ctl = Ctl { cancel: &cancel, on_progress: &mut prog };
            let mut src = std::io::Cursor::new(payload.clone());
            scp_send(&mut ours, "file.txt", payload.len() as u64, Some(0o777), &mut src, &mut ctl)
                .await
                .unwrap();

            let (header, body, expect) = server.await.unwrap();
            assert_eq!(header, format!("C0777 {} file.txt", expect.len()));
            assert_eq!(body, expect);
        });
    }

    #[test]
    fn scp_recv_reads_the_source_protocol() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let payload = b"downloaded bytes!!".to_vec();
            let (mut ours, mut remote) = tokio::io::duplex(4096);

            // The fake `scp -f` running on the "server".
            let sent = payload.clone();
            let server = tokio::spawn(async move {
                let mut z = [0u8; 1];
                remote.read_exact(&mut z).await.unwrap(); // client says go
                // A leading T (mtime) line must be tolerated and acked.
                remote.write_all(b"T1700000000 0 1700000000 0\n").await.unwrap();
                remote.read_exact(&mut z).await.unwrap(); // ack of T
                let header = format!("C0644 {} dl.bin\n", sent.len());
                remote.write_all(header.as_bytes()).await.unwrap();
                remote.read_exact(&mut z).await.unwrap(); // ack of C
                remote.write_all(&sent).await.unwrap();
                remote.write_all(&[0u8]).await.unwrap(); // trailing status
                remote.read_exact(&mut z).await.unwrap(); // final ack
            });

            let cancel = no_cancel();
            let mut prog = |_a: u64, _b: u64| {};
            let mut ctl = Ctl { cancel: &cancel, on_progress: &mut prog };
            let mut dst: Vec<u8> = Vec::new();
            scp_recv(&mut ours, &mut dst, &mut ctl).await.unwrap();

            server.await.unwrap();
            assert_eq!(dst, payload);
        });
    }

    #[test]
    fn scp_recv_surfaces_a_remote_error() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let (mut ours, mut remote) = tokio::io::duplex(1024);
            let server = tokio::spawn(async move {
                let mut z = [0u8; 1];
                remote.read_exact(&mut z).await.unwrap();
                // status 1 + message line = warning/error the client must report.
                remote.write_all(&[1u8]).await.unwrap();
                remote.write_all(b"scp: /nope: No such file\n").await.unwrap();
            });
            let cancel = no_cancel();
            let mut prog = |_a: u64, _b: u64| {};
            let mut ctl = Ctl { cancel: &cancel, on_progress: &mut prog };
            let mut dst: Vec<u8> = Vec::new();
            let err = scp_recv(&mut ours, &mut dst, &mut ctl).await.unwrap_err();
            assert!(err.to_string().contains("No such file"), "got: {err}");
            server.await.unwrap();
        });
    }
}
