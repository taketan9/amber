//! Optional AI features for cian, talking to Azure OpenAI through the same
//! Windows broker (WAM) authentication crmaine uses.
//!
//! The actual auth and API call live in a small Python helper ([`SCRIPT`],
//! embedded at build time and written to a cache dir on first use) because the
//! broker credential is a Python/azure-identity concept with no practical pure
//! Rust equivalent. cian shells out to it, one process per request, and treats
//! any failure — no python, no packages, offline, not signed in — as "AI
//! unavailable": the features simply do not appear. Nothing here ever blocks
//! cian from running.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// The bundled Python helper, materialised to disk when needed.
const SCRIPT: &str = include_str!("../cian_ai.py");

/// How to reach the model. Mirrors crmaine's CLI knobs; `auth_mode` is
/// `broker` (Windows AAD), `apikey`, or `mock` (offline echo, for testing).
#[derive(Debug, Clone)]
pub struct AiConfig {
    pub python: String,
    pub endpoint: String,
    pub model: String,
    pub api_version: String,
    pub auth_mode: String,
    pub api_key: String,
    pub api_base_url: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            python: "python".into(),
            // Same Azure APIM endpoint and model family as crmaine, so an empty
            // `cian.ai{}` works out of the box in that environment.
            endpoint: "https://apim-jri-dev-apim1.azure-api.net/llmaoai".into(),
            // crmaine's own default model; "gpt-5.4" was a guess and is not
            // deployed in that environment (404 on the deployment path).
            model: "gpt-5-mini".into(),
            api_version: "2025-04-01-preview".into(),
            auth_mode: "broker".into(),
            api_key: String::new(),
            api_base_url: String::new(),
        }
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    messages: Vec<Message<'a>>,
    model: &'a str,
    endpoint: &'a str,
    api_version: &'a str,
    auth_mode: &'a str,
    api_key: &'a str,
    api_base_url: &'a str,
    max_tokens: u32,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    #[serde(default)]
    content: String,
    #[serde(default)]
    error: String,
}

/// Write the embedded helper to a stable cache path and return it. Rewritten
/// each call is cheap and keeps it in sync with the binary.
fn script_path() -> Result<PathBuf> {
    let dir = cache_dir().join("cian");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join("cian_ai.py");
    // Only rewrite when the content differs, so a running helper is not clobbered.
    let stale = std::fs::read_to_string(&path).map(|s| s != SCRIPT).unwrap_or(true);
    if stale {
        std::fs::write(&path, SCRIPT).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(path)
}

fn cache_dir() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_CACHE_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x);
        }
    }
    if cfg!(windows) {
        if let Ok(x) = std::env::var("LOCALAPPDATA") {
            if !x.is_empty() {
                return PathBuf::from(x);
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".cache");
    }
    std::env::temp_dir()
}

/// Whether the AI helper is usable: python runs and the packages the auth mode
/// needs import. Does not touch the network or prompt for sign-in. Cheap enough
/// to call once at startup; cache the result.
pub fn available(cfg: &AiConfig) -> bool {
    // `mock` is always available (no packages, no network) — handy for tests.
    let Ok(script) = script_path() else { return false };
    Command::new(&cfg.python)
        .arg(&script)
        .arg("--check")
        .arg(&cfg.auth_mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Send a chat turn and return the assistant's reply. Blocks on the network, so
/// callers run it on a worker thread.
pub fn chat(cfg: &AiConfig, system: &str, user: &str) -> Result<String> {
    let script = script_path()?;
    let mut messages = Vec::new();
    if !system.is_empty() {
        messages.push(Message { role: "system", content: system });
    }
    messages.push(Message { role: "user", content: user });
    let req = ChatRequest {
        messages,
        model: &cfg.model,
        endpoint: &cfg.endpoint,
        api_version: &cfg.api_version,
        auth_mode: &cfg.auth_mode,
        api_key: &cfg.api_key,
        api_base_url: &cfg.api_base_url,
        max_tokens: 1024,
    };
    let body = serde_json::to_vec(&req)?;

    let mut child = Command::new(&cfg.python)
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONUTF8", "1")
        .spawn()
        .with_context(|| format!("launch {} (is Python installed?)", cfg.python))?;
    child.stdin.take().context("stdin")?.write_all(&body).context("send request")?;
    let out = child.wait_with_output().context("run AI helper")?;

    let reply: Reply = serde_json::from_slice(&out.stdout)
        .with_context(|| format!("parse AI reply: {}", String::from_utf8_lossy(&out.stdout)))?;
    if reply.ok {
        Ok(reply.content)
    } else {
        Err(anyhow!("AI: {}", reply.error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have_python() -> bool {
        Command::new("python3").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    #[test]
    fn mock_chat_round_trips_through_python() {
        if !have_python() {
            eprintln!("no python3; skipping");
            return;
        }
        let cfg = AiConfig { python: "python3".into(), auth_mode: "mock".into(), ..Default::default() };
        assert!(available(&cfg), "mock check passes");
        let reply = chat(&cfg, "you are terse", "hello there").unwrap();
        assert_eq!(reply, "[mock] hello there", "the helper echoed the last message");
    }
}
