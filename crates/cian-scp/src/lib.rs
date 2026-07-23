//! Built-in SFTP file transfer, over pure-Rust russh.
//!
//! cian's shell can already open an SSH session; this adds moving files across
//! one without shelling out to `scp`. It is SFTP under the hood (what modern
//! "scp" is anyway), driven from cian's ordinary worker threads: each transfer
//! spins a tiny current-thread tokio runtime, reports progress through a
//! callback and watches a cancel flag, exactly like the local file operations.
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

/// Upload a local file to `remote_path` on the server.
pub fn upload(target: &Target, local: &Path, remote_path: &str, ctl: &mut Ctl) -> Result<()> {
    on_runtime(|| async {
        let (_handle, sftp) = connect_sftp(target).await?;
        let total = std::fs::metadata(local).map(|m| m.len()).unwrap_or(0);
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
        let _ = sftp.close().await;
        Ok(())
    })
}

/// Download `remote_path` from the server to a local file.
pub fn download(target: &Target, remote_path: &str, local: &Path, ctl: &mut Ctl) -> Result<()> {
    on_runtime(|| async {
        let (_handle, sftp) = connect_sftp(target).await?;
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
    })
}

/// Run a future to completion on a private current-thread runtime, so the async
/// client can be driven from a plain (non-async) worker thread.
fn on_runtime<F, Fut>(f: F) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start async runtime")?;
    rt.block_on(f())
}

/// Connect, authenticate with a password, and open an SFTP session. The
/// returned handle must be kept alive for the duration of the transfer — it
/// owns the SSH connection the session's channel rides on.
async fn connect_sftp(target: &Target) -> Result<(client::Handle<BlindClient>, SftpSession)> {
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

    let channel = handle.channel_open_session().await.context("open channel")?;
    channel.request_subsystem(true, "sftp").await.context("request sftp subsystem")?;
    let sftp = SftpSession::new(channel.into_stream()).await.context("start sftp")?;
    Ok((handle, sftp))
}
