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

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

use super::*;

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
        let cache_dir = cfg
            .cache_dir
            .clone()
            .or_else(|| from_settings(&settings, "crmaine.cacheDir"))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "crmaine cacheDir unknown — set crmaine.cacheDir in VS Code, or cache_dir in \
                 cian.crmaine{}"
                    .to_string()
            })?;

        let port = cfg.port.unwrap_or_else(|| crmaine_port(&current_username()));

        Ok(CrmaineResolved { port, cache_dir, endpoint, model, api_version, auth_mode })
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

    /// Shared driver for the crmaine chat endpoints (`/query`, `/agent`): resolve
    /// config, fire the blocking HTTP call on a worker, and open the chat popup.
    fn start_crmaine(&mut self, path: &'static str, label: &str, question: &str) {
        let question = question.trim().to_string();
        if question.is_empty() {
            self.message =
                Some(tr(self.lang, "type a question after the command", "コマンドの後に質問を入力").into());
            return;
        }
        let cfg = match self.crmaine_resolved() {
            Ok(c) => c,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        if self.crmaine_rx.is_some() {
            self.message = Some(tr(self.lang, "crmaine is busy", "crmaine 実行中").into());
            return;
        }

        let body = build_body(&question, &cfg);
        let port = cfg.port;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crmaine_call(port, path, &body));
        });
        self.crmaine_rx = Some(rx);
        self.popup = Popup::AiChat {
            input: String::new(),
            log: vec![ChatMsg { user: true, text: format!("{}: {}", label, question) }],
            scroll: usize::MAX,
            pending: true,
            sel: None,
        };
    }

    /// Collect a finished crmaine reply into the chat. Returns true to repaint.
    pub(crate) fn poll_crmaine(&mut self) -> bool {
        let Some(rx) = &self.crmaine_rx else { return false };
        let msg = match rx.try_recv() {
            Ok(m) => m,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("crmaine worker stopped".to_string())
            }
        };
        self.crmaine_rx = None;
        let text = match msg {
            Ok(answer) => answer,
            Err(e) => format!("crmaine error: {}", e),
        };
        if let Popup::AiChat { log, pending, scroll, .. } = &mut self.popup {
            log.push(ChatMsg { user: false, text });
            *pending = false;
            *scroll = usize::MAX;
        } else {
            self.popup = Popup::Notice { lines: text.lines().map(|l| l.to_string()).collect() };
        }
        true
    }
}

/// The JSON request body for `/query` and `/agent`. Both read the same core
/// fields; `/agent` simply ignores `cache_dir` / `query_expansion` / `rerank`.
pub(crate) fn build_body(question: &str, cfg: &CrmaineResolved) -> String {
    serde_json::json!({
        "question": question,
        "cache_dir": cfg.cache_dir,
        "model": cfg.model,
        "endpoint": cfg.endpoint,
        "api_version": cfg.api_version,
        "auth_mode": cfg.auth_mode,
        "query_expansion": true,
        "rerank": true,
    })
    .to_string()
}

/// Do the whole blocking call: health-check, POST `path`, parse the SSE answer.
/// Returns the rendered answer (with sources appended) or an error.
fn crmaine_call(port: u16, path: &str, body: &str) -> Result<String, String> {
    // A quick health check gives a clear "start it in VS Code" message rather
    // than a raw connection error.
    if http_get(port, "/health").is_err() {
        return Err(format!(
            "crmaine server not reachable on 127.0.0.1:{} — start crmaine in VS Code first",
            port
        ));
    }
    let raw = http_post_sse(port, path, body)?;
    let (answer, sources, error) = parse_sse_answer(&raw);
    if let Some(e) = error {
        return Err(e);
    }
    let mut out = answer.trim().to_string();
    if out.is_empty() {
        out.push_str("(no answer)");
    }
    if !sources.is_empty() {
        out.push_str("\n\n— sources —\n");
        for s in sources {
            out.push_str("• ");
            out.push_str(&s);
            out.push('\n');
        }
    }
    Ok(out)
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

fn http_post_sse(port: u16, path: &str, body: &str) -> Result<String, String> {
    let mut s = connect(port)?;
    let req = format!(
        "POST {} HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        body.len(),
        body
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let _ = s.shutdown(Shutdown::Write);
    read_response_body(s)
}

/// Parse crmaine's `/query` SSE stream. Events are `data: {json}` lines; the
/// answer arrives as `{"type":"chunk","text":…}`, retrieved files as
/// `{"type":"sources",…}`, and `{"type":"error","message":…}` on failure.
pub(crate) fn parse_sse_answer(body: &str) -> (String, Vec<String>, Option<String>) {
    let mut answer = String::new();
    let mut sources = Vec::new();
    let mut error = None;
    for line in body.lines() {
        let Some(rest) = line.trim_start().strip_prefix("data:") else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(rest.trim()) else { continue };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("chunk") => {
                if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                    answer.push_str(t);
                }
            }
            Some("sources") | Some("agent_sources") => {
                let key = if v.get("sources").is_some() { "sources" } else { "agent_sources" };
                if let Some(arr) = v.get(key).and_then(|s| s.as_array()) {
                    for item in arr {
                        // A source may be a bare filename string or an object.
                        if let Some(s) = item.as_str() {
                            sources.push(s.to_string());
                        } else if let Some(f) =
                            item.get("file").or_else(|| item.get("path")).and_then(|f| f.as_str())
                        {
                            sources.push(f.to_string());
                        }
                    }
                }
            }
            Some("error") => {
                error = Some(
                    v.get("message").and_then(|m| m.as_str()).unwrap_or("crmaine error").to_string(),
                );
            }
            _ => {}
        }
    }
    (answer, sources, error)
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
    fn parse_sse_accumulates_chunks_and_sources() {
        let body = "\
data: {\"type\":\"progress\",\"n\":1}
data: {\"type\":\"chunk\",\"text\":\"Hello \"}
data: {\"type\":\"chunk\",\"text\":\"world\"}
data: {\"type\":\"sources\",\"sources\":[\"a.md\",{\"file\":\"b.sql\"}]}
data: {\"type\":\"done\"}
";
        let (answer, sources, error) = parse_sse_answer(body);
        assert_eq!(answer, "Hello world");
        assert_eq!(sources, vec!["a.md".to_string(), "b.sql".to_string()]);
        assert!(error.is_none());
    }

    #[test]
    fn parse_sse_surfaces_an_error_event() {
        let body = "data: {\"type\":\"error\",\"message\":\"index empty\"}\n";
        let (_a, _s, error) = parse_sse_answer(body);
        assert_eq!(error.as_deref(), Some("index empty"));
    }
}
