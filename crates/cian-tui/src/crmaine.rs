//! The crmaine bridge — use crmaine's Ajent / RAG from cian's own UI.
//!
//! crmaine (a VS Code extension) runs its brain as a local Flask server on
//! `127.0.0.1`. cian **attaches** to that already-running server rather than
//! importing crmaine's Python: the port is derived deterministically from the
//! login name (exactly as the extension does), and the endpoint / model /
//! cache-dir are read live from VS Code's `settings.json`. So the flow is:
//! start crmaine in VS Code (which starts the server and signs in), then run
//! cian — `:rag <question>` queries the same index crmaine built.
//!
//! Everything here is plain HTTP to localhost (no dependency, no auth in cian —
//! the running server is already authenticated). The heavy call runs on a
//! worker thread and lands in the AI chat popup, like cian's other AI features.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

use super::*;

/// One streamed event from a crmaine `/query` or `/agent` call. The answer
/// arrives as a run of [`Chunk`](CrmaineEvent::Chunk)s; pipeline steps (query
/// expansion, rerank, a tool call) arrive as [`Stage`](CrmaineEvent::Stage)s
/// shown by the spinner; citations as [`Sources`](CrmaineEvent::Sources).
pub(crate) enum CrmaineEvent {
    Stage(String),
    Chunk(String),
    Sources(Vec<String>),
    Done,
    Error(String),
}

/// The port crmaine's local server listens on. The extension derives it from the
/// login name with djb2 (`h = h*33 + c`, kept to 32 unsigned bits) → 6500..7499,
/// so a multi-user host gives each user their own port. Reproducing it exactly is
/// what lets cian attach with no discovery.
pub(crate) fn crmaine_port(username: &str) -> u16 {
    let mut h: u32 = 5381;
    for b in username.to_lowercase().bytes() {
        // ((h << 5) + h) + c, truncated to 32 bits — matches JS `>>> 0`.
        h = (h << 5).wrapping_add(h).wrapping_add(b as u32);
    }
    6500 + (h % 1000) as u16
}

fn current_username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "default".into())
}

/// VS Code's user `settings.json`, per platform. (Cursor and others live
/// elsewhere; a config `settings_path` overrides this.)
fn default_vscode_settings_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs_home().map(|h| h.join("Library/Application Support/Code/User/settings.json"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Code/User/settings.json"))
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        dirs_home().map(|h| h.join(".config/Code/User/settings.json"))
    }
}

#[cfg(not(target_os = "windows"))]
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Strip JSONC (`//` line and `/* */` block comments, and trailing commas) so
/// `serde_json` can parse VS Code's tolerant settings file. String contents are
/// left untouched — comment markers inside a `"..."` value are kept.
pub(crate) fn strip_jsonc(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_str = false;
    let mut esc = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            '/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            '/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    // Remove trailing commas: `,` followed only by whitespace then `}`/`]`.
    let mut cleaned = String::with_capacity(out.len());
    let ob = out.as_bytes();
    let mut j = 0;
    while j < ob.len() {
        if ob[j] == b',' {
            let mut k = j + 1;
            while k < ob.len() && (ob[k] as char).is_whitespace() {
                k += 1;
            }
            if k < ob.len() && (ob[k] == b'}' || ob[k] == b']') {
                j += 1; // drop the comma
                continue;
            }
        }
        cleaned.push(ob[j] as char);
        j += 1;
    }
    cleaned
}

/// The parts cian needs to talk to crmaine's `/query`, resolved from (in order)
/// the `cian.crmaine{}` overrides, then VS Code's `crmaine.*` settings, then
/// crmaine's own defaults.
pub(crate) struct CrmaineResolved {
    pub port: u16,
    pub cache_dir: String,
    pub endpoint: String,
    pub model: String,
    pub api_version: String,
    pub auth_mode: String,
}

/// Pull the `crmaine.*` values out of a parsed settings object.
fn from_settings(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

impl App {
    /// Resolve everything needed for a crmaine call, or an error string
    /// explaining what's missing.
    pub(crate) fn crmaine_resolved(&self) -> Result<CrmaineResolved, String> {
        let cfg = self
            .config
            .crmaine
            .as_ref()
            .ok_or_else(|| "crmaine not configured — add cian.crmaine{} to init.lua".to_string())?;

        // Read VS Code settings (best-effort — overrides can stand in for it).
        let settings_path = cfg
            .settings_path
            .as_ref()
            .map(PathBuf::from)
            .or_else(default_vscode_settings_path);
        let settings: serde_json::Value = settings_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&strip_jsonc(&s)).ok())
            .unwrap_or(serde_json::Value::Null);

        // models is an array; take the first.
        let vs_model = settings
            .get("crmaine.models")
            .and_then(|m| m.as_array())
            .and_then(|a| a.first())
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());

        let endpoint = cfg
            .endpoint
            .clone()
            .or_else(|| from_settings(&settings, "crmaine.azureEndpoint"))
            .unwrap_or_else(|| "https://apim-jri-dev-apim1.azure-api.net/llmoai".into());
        let model = cfg
            .model
            .clone()
            .or(vs_model)
            .unwrap_or_else(|| "gpt-5-mini".into());
        let api_version = cfg
            .api_version
            .clone()
            .or_else(|| from_settings(&settings, "crmaine.apiVersion"))
            .unwrap_or_else(|| "2025-04-01-preview".into());
        let auth_mode = cfg
            .auth_mode
            .clone()
            .or_else(|| from_settings(&settings, "crmaine.authMode"))
            .unwrap_or_else(|| "broker".into());
        // An `:index`-built isolated index wins over the crmaine/VS Code one.
        let cache_dir = if let Some(o) = self.crmaine_cache_override.clone() {
            o
        } else {
            cfg.cache_dir
                .clone()
                .or_else(|| from_settings(&settings, "crmaine.cacheDir"))
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    "crmaine cacheDir unknown — set crmaine.cacheDir in VS Code, or cache_dir in \
                     cian.crmaine{}"
                        .to_string()
                })?
        };

        let port = cfg.port.unwrap_or_else(|| crmaine_port(&current_username()));

        Ok(CrmaineResolved { port, cache_dir, endpoint, model, api_version, auth_mode })
    }

    /// `:raginfo` / `:crmaine?` — a diagnostic that shows exactly what cian
    /// resolved (port, cache dir, endpoint, model) and whether the local server
    /// answers. This is the first thing to run when `:rag` "does nothing": it
    /// tells you the port it's dialling and whether crmaine is actually up.
    pub(crate) fn crmaine_doctor(&mut self) {
        let cfg = match self.crmaine_resolved() {
            Ok(c) => c,
            Err(e) => {
                self.popup = Popup::Notice {
                    lines: vec!["crmaine — not ready".to_string(), String::new(), e],
                };
                return;
            }
        };
        // A short probe: does anything answer on the derived port? http_get
        // succeeds as long as the server is listening (even a 404 means "up").
        let reachable = http_get(cfg.port, "/health").is_ok();
        let status = if reachable {
            tr(self.lang, "reachable — server is up", "接続OK — サーバ稼働中")
        } else {
            tr(self.lang, "NOT reachable — start crmaine in VS Code first", "未接続 — 先に VS Code で crmaine を起動")
        };
        let lines = vec![
            tr(self.lang, "crmaine — diagnostics", "crmaine — 診断").to_string(),
            String::new(),
            format!("{:<12}: {} ({})", "user", current_username(), tr(self.lang, "port derives from this", "この名前からポート算出")),
            format!("{:<12}: 127.0.0.1:{}", "port", cfg.port),
            format!("{:<12}: {}", "server", status),
            String::new(),
            format!(
                "{:<12}: {}{}",
                "cache_dir",
                cfg.cache_dir,
                if self.crmaine_cache_override.is_some() {
                    tr(self.lang, "  (:index — isolated)", "  (:index — 分離)")
                } else {
                    tr(self.lang, "  (crmaine's own)", "  (crmaine 本体)")
                }
            ),
            format!("{:<12}: {}", "endpoint", cfg.endpoint),
            format!("{:<12}: {}", "model", cfg.model),
            format!("{:<12}: {}", "api_version", cfg.api_version),
            format!("{:<12}: {}", "auth_mode", cfg.auth_mode),
            String::new(),
            tr(
                self.lang,
                "If reachable but :rag hangs, the index is likely still loading in VS Code — wait for it to finish.",
                "接続OKでも :rag が返らない場合は、VS Code 側でインデックス読込中の可能性大 — 完了を待ってください。",
            ).to_string(),
        ];
        self.popup = Popup::Notice { lines };
    }

    /// `:rag <question>` — ask crmaine's RAG (`/query`) over the running server.
    pub(crate) fn start_rag(&mut self, question: &str) {
        self.start_crmaine("/query", "RAG", question);
    }

    /// `:agent <task>` — crmaine's Ajent (`/agent`): an LLM answer with the
    /// configured persona (lite-RAG / tools), over the running server.
    pub(crate) fn start_agent(&mut self, question: &str) {
        self.start_crmaine("/agent", "Agent", question);
    }

    /// `:index [path]` — build a fresh BM25 index of the given folder (or the
    /// active pane's directory) into cian's own index dir, then point `:rag` /
    /// `:agent` at it. This is the "ignore crmaine's index, answer only from what
    /// I indexed here" flow; `:ragshared` switches back to crmaine's index.
    pub(crate) fn start_index(&mut self, path_arg: &str) {
        if self.config.crmaine.is_none() {
            self.message = Some("crmaine not configured — add cian.crmaine{} to init.lua".into());
            return;
        }
        if self.crmaine_rx.is_some() {
            self.message = Some(tr(self.lang, "crmaine is busy", "crmaine 実行中").into());
            return;
        }
        // The folder to index: an explicit argument, else the active pane's cwd.
        let folder = {
            let a = path_arg.trim();
            if a.is_empty() {
                match self.active_pane().map(|p| p.cwd.clone()) {
                    Some(d) => d,
                    None => {
                        self.message = Some("no directory to index".into());
                        return;
                    }
                }
            } else {
                crate::expand_path(a)
            }
        };
        if !folder.is_dir() {
            self.message = Some(format!("not a directory: {}", folder.display()));
            return;
        }
        let cache = crmaine_index_dir();
        if let Err(e) = std::fs::create_dir_all(&cache) {
            self.message = Some(format!("cannot create index dir: {e}"));
            return;
        }
        let cache_s = cache.display().to_string();
        // Resolve the port (config resolve needs a cache dir; the override we are
        // about to set supplies one, so set it first).
        self.crmaine_cache_override = Some(cache_s.clone());
        let port = match self.crmaine_resolved() {
            Ok(c) => c.port,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        let body = serde_json::json!({
            "folders": [folder.display().to_string()],
            "cache_dir": cache_s,
        })
        .to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if http_get(port, "/health").is_err() {
                let _ = tx.send(CrmaineEvent::Error(format!(
                    "server not reachable on 127.0.0.1:{port} — start crmaine in VS Code first"
                )));
                return;
            }
            if let Err(e) = stream_sse(port, "/index", &body, parse_index_line, |ev| {
                let _ = tx.send(ev);
            }) {
                let _ = tx.send(CrmaineEvent::Error(e));
            }
        });
        self.crmaine_rx = Some(rx);
        self.crmaine_stage = Some(tr(self.lang, "indexing…", "インデックス構築中…").into());
        self.crmaine_sources.clear();
        // After indexing, a typed follow-up should query the freshly-built
        // index (RAG), so the console chat is a RAG conversation.
        self.start_ai_chat(
            ChatMode::Rag,
            vec![ChatMsg {
                user: true,
                text: format!("{}: {}", tr(self.lang, "index", "インデックス"), folder.display()),
            }],
            true,
        );
    }

    /// `:ragshared` — stop using the `:index`-built index and go back to
    /// crmaine's own (VS Code) index for queries.
    pub(crate) fn crmaine_use_shared_index(&mut self) {
        if self.crmaine_cache_override.take().is_some() {
            self.message =
                Some(tr(self.lang, "using crmaine's own index", "crmaine のインデックスを使用").into());
        } else {
            self.message = Some(
                tr(self.lang, "already using crmaine's index", "すでに crmaine のインデックス").into(),
            );
        }
    }

    /// Open a fresh crmaine chat (`/query` = RAG, `/agent` = Ajent) with the
    /// first question. Follow-ups typed into the chat reuse the same endpoint via
    /// [`Self::send_ai_message`] → [`Self::fire_crmaine`].
    fn start_crmaine(&mut self, path: &'static str, label: &str, question: &str) {
        let question = question.trim().to_string();
        if question.is_empty() {
            self.message =
                Some(tr(self.lang, "type a question after the command", "コマンドの後に質問を入力").into());
            return;
        }
        if self.crmaine_rx.is_some() {
            self.message = Some(tr(self.lang, "crmaine is busy", "crmaine 実行中").into());
            return;
        }
        // Fail before opening a popup if crmaine isn't configured/reachable-config.
        if let Err(e) = self.crmaine_resolved() {
            self.message = Some(e);
            return;
        }
        let mode = if path == "/agent" { ChatMode::Agent } else { ChatMode::Rag };
        self.start_ai_chat(mode, vec![ChatMsg { user: true, text: format!("{label}: {question}") }], true);
        // First turn — no prior history.
        self.fire_crmaine(path, &question, serde_json::Value::Null);
    }

    /// Send `question` to a crmaine endpoint (`/query` or `/agent`), streaming
    /// the answer into the already-open chat. `history` (prior turns as
    /// `[{role,content}]`) is used by `/agent`; `/query` ignores it. Assumes the
    /// user turn is already in the log and `pending` is set.
    pub(crate) fn fire_crmaine(&mut self, path: &'static str, question: &str, history: serde_json::Value) {
        let cfg = match self.crmaine_resolved() {
            Ok(c) => c,
            Err(e) => {
                self.fail_pending_chat(e);
                return;
            }
        };
        if self.crmaine_rx.is_some() {
            self.message = Some(tr(self.lang, "crmaine is busy", "crmaine 実行中").into());
            return;
        }
        let mut body = serde_json::json!({
            "question": question,
            "cache_dir": cfg.cache_dir,
            "model": cfg.model,
            "endpoint": cfg.endpoint,
            "api_version": cfg.api_version,
            "auth_mode": cfg.auth_mode,
            "query_expansion": true,
            "rerank": true,
        });
        if path == "/agent" {
            body["history"] = history;
        }
        let body = body.to_string();
        let port = cfg.port;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if http_get(port, "/health").is_err() {
                let _ = tx.send(CrmaineEvent::Error(format!(
                    "server not reachable on 127.0.0.1:{port} — start crmaine in VS Code first"
                )));
                return;
            }
            let parse = |l: &str| parse_sse_line(l).into_iter().collect::<Vec<_>>();
            if let Err(e) = stream_sse(port, path, &body, parse, |ev| {
                let _ = tx.send(ev);
            }) {
                let _ = tx.send(CrmaineEvent::Error(e));
            }
        });
        self.crmaine_rx = Some(rx);
        self.crmaine_stage = Some(tr(self.lang, "connecting…", "接続中…").into());
        self.crmaine_sources.clear();
    }

    /// Prior chat turns as `[{role,content}]` for `/agent`, excluding the final
    /// user turn (the question being sent now).
    pub(crate) fn crmaine_history_json(&self) -> serde_json::Value {
        let mut arr = Vec::new();
        if let Popup::AiChat { log, .. } = &self.popup {
            let upto = log.len().saturating_sub(1);
            for m in &log[..upto] {
                arr.push(serde_json::json!({
                    "role": if m.user { "user" } else { "assistant" },
                    "content": m.text,
                }));
            }
        }
        serde_json::Value::Array(arr)
    }

    /// Report an error and stop the chat spinner (a request that never launched).
    pub(crate) fn fail_pending_chat(&mut self, msg: String) {
        self.message = Some(msg);
        if let Popup::AiChat { pending, .. } = &mut self.popup {
            *pending = false;
        }
    }

    /// Drain streamed crmaine events into the chat: append answer chunks live,
    /// track the pipeline stage for the spinner, and stash citations to append
    /// when the answer ends. Returns true to repaint.
    pub(crate) fn poll_crmaine(&mut self) -> bool {
        // Take the receiver out so the event handlers below can borrow `self`
        // mutably; it goes back at the end unless the stream has finished.
        let Some(rx) = self.crmaine_rx.take() else { return false };
        let mut events = Vec::new();
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(ev) => events.push(ev),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        let mut changed = false;
        let mut finished: Option<Option<String>> = None; // Some(err?) = end
        for ev in events {
            match ev {
                CrmaineEvent::Stage(s) => {
                    self.crmaine_stage = Some(s);
                    changed = true;
                }
                CrmaineEvent::Chunk(t) => {
                    self.crmaine_stage = None; // the answer has started
                    self.append_crmaine_answer(&t);
                    changed = true;
                }
                CrmaineEvent::Sources(v) => {
                    self.crmaine_sources = v;
                    changed = true;
                }
                CrmaineEvent::Done => {
                    finished = Some(None);
                    break;
                }
                CrmaineEvent::Error(e) => {
                    finished = Some(Some(e));
                    break;
                }
            }
        }
        if finished.is_none() && disconnected {
            finished = Some(Some("crmaine stream ended unexpectedly".into()));
        }
        // Not done yet — keep polling next tick.
        if finished.is_none() {
            self.crmaine_rx = Some(rx);
            return changed;
        }
        if let Some(err) = finished {
            self.crmaine_rx = None;
            self.crmaine_stage = None;
            // Append citations, then close out the turn.
            if err.is_none() && !self.crmaine_sources.is_empty() {
                let mut block = String::from("\n\n— sources —\n");
                for s in std::mem::take(&mut self.crmaine_sources) {
                    block.push_str("• ");
                    block.push_str(&s);
                    block.push('\n');
                }
                self.append_crmaine_answer(&block);
            }
            if let Some(e) = err {
                self.append_crmaine_answer(&format!("\n⚠ {e}"));
            }
            if let Popup::AiChat { pending, scroll, log, .. } = &mut self.popup {
                // If nothing streamed at all, say so rather than leave it blank.
                if log.last().map(|m| m.user).unwrap_or(true) {
                    log.push(ChatMsg { user: false, text: "(no answer)".into() });
                }
                *pending = false;
                *scroll = usize::MAX;
            }
            changed = true;
        }
        changed
    }

    /// Append streamed text to the assistant's turn, starting a new assistant
    /// message if the last entry is the user's question.
    fn append_crmaine_answer(&mut self, text: &str) {
        if let Popup::AiChat { log, scroll, .. } = &mut self.popup {
            match log.last_mut() {
                Some(m) if !m.user => m.text.push_str(text),
                _ => log.push(ChatMsg { user: false, text: text.to_string() }),
            }
            *scroll = usize::MAX;
        }
    }
}

/// POST `path` and stream its SSE body, handing each parsed event to
/// `on_event` as it arrives (so the answer paints token by token). HTTP/1.0 so
/// the server closes the connection at the end and there is no chunked framing
/// to decode; the SSE body is everything after the header block.
fn stream_sse(
    port: u16,
    path: &str,
    body: &str,
    parse: impl Fn(&str) -> Vec<CrmaineEvent>,
    mut on_event: impl FnMut(CrmaineEvent),
) -> Result<(), String> {
    let stream = connect(port)?;
    let mut w = stream.try_clone().map_err(|e| e.to_string())?;
    let req = format!(
        "POST {} HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        body.len(),
        body
    );
    w.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let _ = w.shutdown(Shutdown::Write);

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    // Skip the HTTP header block, up to the blank line that ends it.
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(()); // closed before any body
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    // Then the SSE body, one line at a time.
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        for ev in parse(&line) {
            let done = matches!(ev, CrmaineEvent::Done | CrmaineEvent::Error(_));
            on_event(ev);
            if done {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Parse an `/index` SSE line. Progress lines stream in as body text; `done`
/// carries the chunk count (shown, then closes); `error` closes with a message.
fn parse_index_line(line: &str) -> Vec<CrmaineEvent> {
    let Some(rest) = line.trim_start().strip_prefix("data:") else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(rest.trim()) else { return Vec::new() };
    match v.get("type").and_then(|t| t.as_str()) {
        Some("progress") => v
            .get("message")
            .and_then(|m| m.as_str())
            .map(|m| vec![CrmaineEvent::Chunk(format!("{m}\n"))])
            .unwrap_or_default(),
        Some("done") => {
            let n = v.get("chunks").and_then(|c| c.as_u64()).unwrap_or(0);
            vec![
                CrmaineEvent::Chunk(format!(
                    "\n✓ indexed {n} chunks — :rag / :agent now answer from this index \
                     (:ragshared to switch back)\n"
                )),
                CrmaineEvent::Done,
            ]
        }
        Some("error") => vec![CrmaineEvent::Error(
            v.get("message").and_then(|m| m.as_str()).unwrap_or("index failed").to_string(),
        )],
        _ => Vec::new(),
    }
}

/// Cian's own RAG index directory — where `:index` writes, kept apart from
/// crmaine's so building one never disturbs the other.
fn crmaine_index_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = dirs_home().map(|h| h.join(".cache"));
    base.unwrap_or_else(std::env::temp_dir).join("cian").join("rag-index")
}

fn connect(port: u16) -> Result<TcpStream, String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    // RAG answers stream token by token and can take a while.
    let _ = s.set_read_timeout(Some(Duration::from_secs(180)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    Ok(s)
}

/// Read the full response body of a request. HTTP/1.0 so the server streams and
/// closes (no chunked encoding to decode); the SSE body is everything after the
/// header block.
fn read_response_body(mut s: TcpStream) -> Result<String, String> {
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    Ok(text.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("").to_string())
}

fn http_get(port: u16, path: &str) -> Result<String, String> {
    let mut s = connect(port)?;
    let req = format!("GET {} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n", path);
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    read_response_body(s)
}

/// Parse one SSE `data: {json}` line into an event, or `None` for a line that
/// isn't a recognised event (comments, keep-alives, pipeline steps we ignore).
/// The answer is `chunk`, citations are `sources` / `agent_sources`, pipeline
/// steps become a `Stage`, and `error` / `done` / `cancelled` close the stream.
pub(crate) fn parse_sse_line(line: &str) -> Option<CrmaineEvent> {
    let rest = line.trim_start().strip_prefix("data:")?;
    let v: serde_json::Value = serde_json::from_str(rest.trim()).ok()?;
    match v.get("type").and_then(|t| t.as_str())? {
        "chunk" => v
            .get("text")
            .and_then(|t| t.as_str())
            .map(|t| CrmaineEvent::Chunk(t.to_string())),
        "sources" | "agent_sources" => {
            let key = if v.get("sources").is_some() { "sources" } else { "agent_sources" };
            let mut out = Vec::new();
            if let Some(arr) = v.get(key).and_then(|s| s.as_array()) {
                for item in arr {
                    // A source may be a bare filename string or an object.
                    if let Some(s) = item.as_str() {
                        out.push(s.to_string());
                    } else if let Some(f) =
                        item.get("file").or_else(|| item.get("path")).and_then(|f| f.as_str())
                    {
                        out.push(f.to_string());
                    }
                }
            }
            Some(CrmaineEvent::Sources(out))
        }
        "query_expanded" => {
            let n = v.get("queries").and_then(|q| q.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(CrmaineEvent::Stage(format!("expanded to {n} queries…")))
        }
        "hyde_generated" => Some(CrmaineEvent::Stage("drafting a hypothesis…".into())),
        "rerank_start" => Some(CrmaineEvent::Stage("reranking results…".into())),
        "tool_call_start" => {
            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
            Some(CrmaineEvent::Stage(format!("running {name}…")))
        }
        "error" => Some(CrmaineEvent::Error(
            v.get("message").and_then(|m| m.as_str()).unwrap_or("crmaine error").to_string(),
        )),
        "done" | "cancelled" => Some(CrmaineEvent::Done),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_matches_the_extensions_djb2() {
        // Cross-checked against the extension's JS formula via node.
        assert_eq!(crmaine_port("default"), 7158);
        assert_eq!(crmaine_port("t-yamada"), 6815);
        assert_eq!(crmaine_port("alice"), 6975);
        // Case-insensitive, and always in range.
        assert_eq!(crmaine_port("ALICE"), crmaine_port("alice"));
        for name in ["a", "verylongusername", "x1y2z3"] {
            let p = crmaine_port(name);
            assert!((6500..=7499).contains(&p), "{name} → {p} out of range");
        }
    }

    #[test]
    fn strip_jsonc_removes_comments_and_trailing_commas_but_not_strings() {
        let src = r#"{
            // a line comment
            "crmaine.cacheDir": "C:\\idx", /* block */
            "crmaine.note": "http://x // not a comment",
            "crmaine.models": ["gpt-5-mini",],
        }"#;
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc(src)).expect("parses");
        assert_eq!(v["crmaine.cacheDir"], "C:\\idx");
        assert_eq!(v["crmaine.note"], "http://x // not a comment");
        assert_eq!(v["crmaine.models"][0], "gpt-5-mini");
    }

    #[test]
    fn parse_sse_line_classifies_events() {
        // Unknown / keep-alive lines are ignored.
        assert!(parse_sse_line("data: {\"type\":\"progress\",\"n\":1}").is_none());
        assert!(parse_sse_line(": keep-alive").is_none());

        match parse_sse_line("data: {\"type\":\"chunk\",\"text\":\"Hello \"}") {
            Some(CrmaineEvent::Chunk(t)) => assert_eq!(t, "Hello "),
            _ => panic!("expected a chunk"),
        }
        match parse_sse_line("data: {\"type\":\"sources\",\"sources\":[\"a.md\",{\"file\":\"b.sql\"}]}") {
            Some(CrmaineEvent::Sources(v)) => assert_eq!(v, vec!["a.md".to_string(), "b.sql".to_string()]),
            _ => panic!("expected sources"),
        }
        assert!(matches!(parse_sse_line("data: {\"type\":\"done\"}"), Some(CrmaineEvent::Done)));
        assert!(matches!(parse_sse_line("data: {\"type\":\"cancelled\"}"), Some(CrmaineEvent::Done)));
    }

    #[test]
    fn parse_sse_line_surfaces_an_error_and_stages() {
        match parse_sse_line("data: {\"type\":\"error\",\"message\":\"index empty\"}") {
            Some(CrmaineEvent::Error(e)) => assert_eq!(e, "index empty"),
            _ => panic!("expected an error"),
        }
        match parse_sse_line("data: {\"type\":\"query_expanded\",\"queries\":[\"a\",\"b\",\"c\"]}") {
            Some(CrmaineEvent::Stage(s)) => assert!(s.contains('3')),
            _ => panic!("expected a stage"),
        }
        assert!(matches!(parse_sse_line("data: {\"type\":\"rerank_start\"}"), Some(CrmaineEvent::Stage(_))));
    }
}
