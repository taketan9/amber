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

/// Candidate user `settings.json` paths, in preference order: VS Code first,
/// then Cursor (a VS Code fork some sites use for crmaine). A config
/// `settings_path` overrides this list entirely. The first one that exists and
/// parses wins.
fn editor_settings_candidates() -> Vec<PathBuf> {
    // Per editor, the "<app>/User/settings.json" leaf under the OS config root.
    let editors = ["Code", "Cursor"];
    #[cfg(target_os = "macos")]
    let root = dirs_home().map(|h| h.join("Library/Application Support"));
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let root = dirs_home().map(|h| h.join(".config"));

    match root {
        Some(r) => editors.iter().map(|e| r.join(e).join("User/settings.json")).collect(),
        None => Vec::new(),
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

        // Read the editor's settings (best-effort — overrides can stand in for
        // it). An explicit `settings_path` wins; otherwise try VS Code, then
        // Cursor, taking the first that exists and parses.
        let candidates: Vec<PathBuf> = match &cfg.settings_path {
            Some(p) => vec![PathBuf::from(p)],
            None => editor_settings_candidates(),
        };
        let settings: serde_json::Value = candidates
            .iter()
            .find_map(|p| {
                let s = std::fs::read_to_string(p).ok()?;
                serde_json::from_str(&strip_jsonc(&s)).ok()
            })
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
            .unwrap_or_default();
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

    /// Open an empty crmaine chat window in `mode` (RAG / Agent), ready for the
    /// question to be typed in the window. Used by the right-click menu and by a
    /// bare `:rag` / `:agent` with no argument.
    pub(crate) fn open_crmaine_chat(&mut self, mode: ChatMode) {
        if self.config.crmaine.is_none() {
            self.message = Some(tr(self.lang, "crmaine not configured — add cian.crmaine{} to init.lua", "crmaine が未設定です — init.lua に cian.crmaine{} を追加してください").into());
            return;
        }
        self.start_ai_chat(mode, Vec::new(), false);
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
            self.message = Some(tr(self.lang, "crmaine not configured — add cian.crmaine{} to init.lua", "crmaine が未設定です — init.lua に cian.crmaine{} を追加してください").into());
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
                        self.message = Some(tr(self.lang, "no directory to index", "インデックス対象のディレクトリがありません").into());
                        return;
                    }
                }
            } else {
                crate::expand_path(a)
            }
        };
        if !folder.is_dir() {
            self.message = Some(if self.lang == crate::theme::Lang::Ja {
            format!("ディレクトリではありません: {}", folder.display())
        } else {
            format!("not a directory: {}", folder.display())
        });
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
        // index (RAG), so the console chat is a RAG conversation — but the
        // window is named for the action that opened it, not for the follow-up.
        self.start_ai_chat_as(
            ChatMode::Rag,
            ChatSkin {
                title: format!(
                    "crmaine - {}",
                    tr(self.lang, "Index this folder", "このフォルダをインデックス")
                ),
                simple: false,
            },
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

    /// `:impact <change>` — crmaine impact analysis: which docs a change touches.
    pub(crate) fn start_impact(&mut self, change: &str) {
        let change = change.trim();
        if change.is_empty() {
            self.message = Some(tr(self.lang, "usage: :impact <what changes>", "使い方: :impact <変更内容>").into());
            return;
        }
        self.start_crmaine_tool(
            "/impact",
            tr(self.lang, "Impact analysis", "影響分析"),
            change,
            serde_json::json!({ "change_description": change }),
        );
    }

    /// `:contradiction <topic>` — find statements across the corpus that conflict.
    pub(crate) fn start_contradiction(&mut self, topic: &str) {
        let topic = topic.trim();
        if topic.is_empty() {
            self.message = Some(tr(self.lang, "usage: :contradiction <topic>", "使い方: :contradiction <トピック>").into());
            return;
        }
        self.start_crmaine_tool(
            "/contradiction",
            tr(self.lang, "Find contradictions", "矛盾検出"),
            topic,
            serde_json::json!({ "topic": topic }),
        );
    }

    /// `:coding [question]` / `:code` — crmaine Coding: an Ajent chat seeded with
    /// the current file's code. Uses the file open in the F3 viewer if there is
    /// one, else the file under the cursor. With no question, asks for a review.
    pub(crate) fn start_coding(&mut self, arg: &str) {
        if self.crmaine_rx.is_some() {
            self.message = Some(tr(self.lang, "crmaine is busy", "crmaine 実行中").into());
            return;
        }
        if let Err(e) = self.crmaine_resolved() {
            self.message = Some(e);
            return;
        }
        let Some((name, code)) = self.coding_context() else {
            self.message = Some(
                tr(
                    self.lang,
                    "no file — put the cursor on a file, or open one with F3",
                    "ファイルがありません — カーソルをファイルに合わせるか F3 で開いてください",
                )
                .into(),
            );
            return;
        };
        let ask = arg.trim();
        let ask: String = if ask.is_empty() {
            tr(
                self.lang,
                "Review this code and point out issues and improvements.",
                "このコードをレビューして、問題点と改善案を挙げてください。",
            )
            .into()
        } else {
            ask.to_string()
        };
        // The visible turn stays short; the full code goes in the request only.
        let shown = format!("{}: {} — {}", tr(self.lang, "Coding", "コーディング"), name, ask);
        let question = format!("File: {name}\n\n```\n{code}\n```\n\n{ask}");
        self.start_ai_chat(ChatMode::Coding, vec![ChatMsg { user: true, text: shown }], true);
        self.fire_crmaine("/agent", &question, serde_json::Value::Null);
    }

    /// The code to hand crmaine Coding: the F3 viewer's file if open, else the
    /// file under the cursor. `None` when there is no readable file. Bounded so a
    /// huge file can't blow the token budget.
    fn coding_context(&self) -> Option<(String, String)> {
        if let Popup::Viewer { title, view, .. } = &self.popup {
            let text = view.lines.join("\n");
            if !text.trim().is_empty() {
                return Some((title.clone(), truncate_text_for_ai(&text, 24_000)));
            }
        }
        let e = self.active_pane()?.selected()?;
        if e.is_dir {
            return None;
        }
        let text = std::fs::read_to_string(&e.path).ok()?;
        Some((e.name.clone(), truncate_text_for_ai(&text, 24_000)))
    }

    /// `:searchfiles <words>` — keyword-search crmaine's currently-loaded corpus
    /// and panelize the matching files into the active pane (like a grep result).
    /// Searches whatever index crmaine has loaded (follows the last `:rag`/`:index`).
    pub(crate) fn start_searchfiles(&mut self, query: &str) {
        let keywords: Vec<String> = query.split_whitespace().map(str::to_string).collect();
        if keywords.is_empty() {
            self.message = Some(tr(self.lang, "usage: :searchfiles <words>", "使い方: :searchfiles <語>").into());
            return;
        }
        let port = match self.crmaine_resolved() {
            Ok(c) => c.port,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        let body = serde_json::json!({ "keywords": keywords, "mode": "and" }).to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res: Result<Vec<(String, String)>, String> = (|| {
                if http_get(port, "/health").is_err() {
                    return Err(format!("server not reachable on 127.0.0.1:{port}"));
                }
                let raw = http_post(port, "/search_files", &body)?;
                let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                if let Some(e) = v.get("error").and_then(|x| x.as_str()) {
                    if !e.is_empty() {
                        return Err(e.to_string());
                    }
                }
                let mut out = Vec::new();
                if let Some(arr) = v.get("files").and_then(|f| f.as_array()) {
                    for it in arr {
                        let name = it.get("file").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let path = it.get("path").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        if !path.is_empty() {
                            out.push((name, path));
                        }
                    }
                }
                Ok(out)
            })();
            let _ = tx.send(res);
        });
        self.searchfiles_rx = Some(rx);
        self.message = Some(tr(self.lang, "searching crmaine's corpus…", "crmaine のコーパスを検索中…").into());
    }

    /// Panelize a finished `:searchfiles` result into the active pane. Returns
    /// true to repaint.
    pub(crate) fn poll_searchfiles(&mut self) -> bool {
        let Some(rx) = &self.searchfiles_rx else { return false };
        match rx.try_recv() {
            Ok(res) => {
                self.searchfiles_rx = None;
                match res {
                    Ok(files) if !files.is_empty() => {
                        let entries: Vec<cian_core::Entry> = files
                            .iter()
                            .map(|(name, path)| {
                                cian_core::Entry::flat(
                                    std::path::Path::new(name),
                                    std::path::PathBuf::from(path),
                                    false,
                                )
                            })
                            .collect();
                        let n = entries.len();
                        let label = tr(self.lang, "crmaine search", "crmaine 検索").to_string();
                        if let Some(t) = self.active_file_tabs_mut() {
                            t.active_mut().enter_flat(label, entries);
                        }
                        self.message = Some(format!("{} {}", n, tr(self.lang, "files", "件")));
                    }
                    Ok(_) => {
                        self.message = Some(tr(self.lang, "no matching files", "一致するファイルなし").into())
                    }
                    Err(e) => self.message = Some(format!("crmaine search: {e}")),
                }
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.searchfiles_rx = None;
                true
            }
        }
    }

    /// `:ragdebug [question]` — show what the retriever actually picked for a
    /// question: the tokens it searched with, and the top chunks with their raw
    /// BM25 scores. With no argument it re-runs the last `:rag` question, which
    /// is the case that matters — the answer just came out wrong and the
    /// question is "did it read the wrong thing, or read the right thing badly?"
    ///
    /// `/debug_search` bypasses the cache and the score cutoff, so it shows the
    /// ranking as the retriever saw it, not as the answer pipeline trimmed it.
    pub(crate) fn start_debug_search(&mut self, query: &str) {
        let query = match query.trim() {
            "" => match self.crmaine_last_question.clone() {
                Some(q) => q,
                None => {
                    self.message = Some(
                        tr(
                            self.lang,
                            "usage: :ragdebug <question>  (or ask :rag first)",
                            "使い方: :ragdebug <質問>（先に :rag でも可）",
                        )
                        .into(),
                    );
                    return;
                }
            },
            q => q.to_string(),
        };
        if self.debug_search_rx.is_some() {
            self.message = Some(tr(self.lang, "crmaine is busy", "crmaine 実行中").into());
            return;
        }
        let cfg = match self.crmaine_resolved() {
            Ok(c) => c,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        let body =
            serde_json::json!({ "query": query, "cache_dir": cfg.cache_dir }).to_string();
        let port = cfg.port;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = (|| {
                if http_get(port, "/health").is_err() {
                    return Err(format!(
                        "server not reachable on 127.0.0.1:{port} — start crmaine in VS Code first"
                    ));
                }
                let raw = http_post(port, "/debug_search", &body)?;
                parse_debug_search(&raw)
            })();
            let _ = tx.send(res);
        });
        self.debug_search_rx = Some(rx);
        self.debug_query = query;
        self.message =
            Some(tr(self.lang, "asking the retriever…", "検索過程を取得中…").into());
    }

    /// Open the finished `:ragdebug` trace as a report. Returns true to repaint.
    pub(crate) fn poll_debug_search(&mut self) -> bool {
        let Some(rx) = &self.debug_search_rx else { return false };
        match rx.try_recv() {
            Ok(res) => {
                self.debug_search_rx = None;
                match res {
                    Ok(d) => {
                        let index = match self.crmaine_resolved() {
                            Ok(c) => c.cache_dir,
                            Err(_) => String::new(),
                        };
                        let lines = debug_report_lines(
                            self.lang,
                            &self.debug_query,
                            &index,
                            self.crmaine_cache_override.is_some(),
                            &d,
                            REPORT_W,
                        );
                        // Esc should land back in the conversation this explains,
                        // so a chat underneath is kept rather than dropped.
                        let back = match std::mem::replace(&mut self.popup, Popup::None) {
                            p @ Popup::AiChat { .. } => p,
                            _ => Popup::None,
                        };
                        self.message = None;
                        self.popup = Popup::Report {
                            title: tr(self.lang, " what RAG retrieved ", " RAG が拾った断片 ")
                                .to_string(),
                            lines,
                            scroll: 0,
                            back: Box::new(back),
                        };
                    }
                    Err(e) => self.message = Some(format!("crmaine debug: {e}")),
                }
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.debug_search_rx = None;
                true
            }
        }
    }

    /// `:glossary` — generate a glossary of the indexed corpus.
    pub(crate) fn start_glossary(&mut self) {
        self.start_crmaine_tool(
            "/glossary",
            tr(self.lang, "Generate glossary", "用語集を生成"),
            tr(self.lang, "generating…", "生成中…"),
            serde_json::json!({ "synonyms_file": "" }),
        );
    }

    /// Shared runner for crmaine's SSE analysis tools: build the request from the
    /// resolved config plus `extra`, stream the result into a chat popup.
    pub(crate) fn start_crmaine_tool(
        &mut self,
        path: &'static str,
        label: &str,
        shown: &str,
        extra: serde_json::Value,
    ) {
        if self.crmaine_rx.is_some() {
            self.message = Some(tr(self.lang, "crmaine is busy", "crmaine 実行中").into());
            return;
        }
        let cfg = match self.crmaine_resolved() {
            Ok(c) => c,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        let mut body = serde_json::json!({
            "cache_dir": cfg.cache_dir,
            "model": cfg.model,
            "endpoint": cfg.endpoint,
            "api_version": cfg.api_version,
            "auth_mode": cfg.auth_mode,
        });
        if let Some(obj) = extra.as_object() {
            for (k, v) in obj {
                body[k.as_str()] = v.clone();
            }
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
            if let Err(e) = stream_sse(port, path, &body, parse_tool_line, |ev| {
                let _ = tx.send(ev);
            }) {
                let _ = tx.send(CrmaineEvent::Error(e));
            }
        });
        self.crmaine_rx = Some(rx);
        self.crmaine_stage = Some(tr(self.lang, "working…", "実行中…").into());
        self.crmaine_sources.clear();
        // A tool result is a one-shot; a typed follow-up is just a plain chat.
        // The answer streaming in is crmaine's, so the window keeps crmaine's
        // name and colour even though `mode` routes follow-ups to the local
        // model.
        self.start_ai_chat_as(
            ChatMode::Ai,
            ChatSkin { title: format!("crmaine - {label}"), simple: false },
            vec![ChatMsg { user: true, text: format!("{label}: {shown}") }],
            true,
        );
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
        // Remember the RAG question so `:ragdebug` / Ctrl+D can ask "what did
        // you retrieve for *that*?" without it being typed again. Only `/query`
        // — an `/agent` question may be a whole file of code (`:coding`).
        if path == "/query" {
            self.crmaine_last_question = Some(question.to_string());
        }
        let port = cfg.port;
        let rid = self.arm_crmaine_request(port);
        let mut body = serde_json::json!({
            "question": question,
            "cache_dir": cfg.cache_dir,
            "model": cfg.model,
            "endpoint": cfg.endpoint,
            "api_version": cfg.api_version,
            "auth_mode": cfg.auth_mode,
            "query_expansion": true,
            "rerank": true,
            "request_id": rid,
        });
        if path == "/agent" {
            body["history"] = history;
        }
        // Images pasted into the chat (Alt+V). crmaine runs on this machine, so
        // it reads the files itself and does the base64/Vision part server-side
        // — cian only hands over the paths. Cleared once they're on their way.
        if !self.chat_attachments.is_empty() {
            let files: Vec<String> = std::mem::take(&mut self.chat_attachments)
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            body["attached_files"] = serde_json::json!(files);
        }
        let body = body.to_string();
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

    /// `Ctrl+↑` / `Ctrl+↓` in the chat: tell crmaine the last answer was good or
    /// bad, so it boosts / demotes the files it cited. Only meaningful for a RAG
    /// answer (one with sources); a plain chat has nothing to rate.
    pub(crate) fn send_crmaine_feedback(&mut self, up: bool) {
        if self.crmaine_last_sources.is_empty() {
            self.message = Some(
                tr(self.lang, "no RAG answer to rate", "評価できる RAG 回答がありません").into(),
            );
            return;
        }
        let cfg = match self.crmaine_resolved() {
            Ok(c) => c,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        // The last question and a short preview of the answer, from the log.
        let (mut question, mut response) = (String::new(), String::new());
        if let Popup::AiChat { log, .. } = &self.popup {
            for m in log.iter().rev() {
                if response.is_empty() && !m.user {
                    response = m.text.chars().take(200).collect();
                } else if question.is_empty() && m.user {
                    question = m.text.chars().take(200).collect();
                }
                if !question.is_empty() && !response.is_empty() {
                    break;
                }
            }
        }
        let body = serde_json::json!({
            "cache_dir": cfg.cache_dir,
            "rating": if up { "up" } else { "down" },
            "sources": self.crmaine_last_sources.clone(),
            "question": question,
            "response_preview": response,
        })
        .to_string();
        let port = cfg.port;
        std::thread::spawn(move || {
            let _ = http_post(port, "/feedback", &body);
        });
        self.message = Some(if up {
            tr(self.lang, "↑ feedback sent to crmaine", "↑ フィードバックを送信").into()
        } else {
            tr(self.lang, "↓ feedback sent to crmaine", "↓ フィードバックを送信").into()
        });
    }

    /// Take the next `request_id` and remember it (with the port) as in-flight,
    /// so [`Self::cancel_crmaine`] can stop exactly this call.
    fn arm_crmaine_request(&mut self, port: u16) -> String {
        self.crmaine_req_seq = self.crmaine_req_seq.wrapping_add(1);
        let id = format!("cian-{}", self.crmaine_req_seq);
        self.crmaine_inflight = Some((id.clone(), port));
        id
    }

    /// Stop the crmaine answer that is streaming: tell the server to cancel this
    /// `request_id` (fire-and-forget) and drop the local stream. Used by Esc.
    pub(crate) fn cancel_crmaine(&mut self) {
        if self.crmaine_rx.is_none() {
            return;
        }
        if let Some((rid, port)) = self.crmaine_inflight.take() {
            std::thread::spawn(move || {
                let body = serde_json::json!({ "request_id": rid }).to_string();
                let _ = http_post(port, "/cancel", &body);
            });
        }
        self.crmaine_rx = None;
        self.crmaine_stage = None;
        self.crmaine_sources.clear();
        self.append_crmaine_answer(&format!("\n⚠ {}", tr(self.lang, "cancelled", "中断しました")));
        if let Popup::AiChat { pending, scroll, .. } = &mut self.popup {
            *pending = false;
            *scroll = usize::MAX;
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
            self.crmaine_inflight = None;
            // Remember this answer's citations so feedback can name them.
            self.crmaine_last_sources = self.crmaine_sources.clone();
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
    pub(crate) fn append_crmaine_answer(&mut self, text: &str) {
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

/// Parse an SSE line for the analysis tools (`/impact`, `/contradiction`,
/// `/glossary`). They stream both `chunk` (answer tokens) and `progress`
/// (status lines) as body text, closing on `done` / `error`.
fn parse_tool_line(line: &str) -> Vec<CrmaineEvent> {
    let Some(rest) = line.trim_start().strip_prefix("data:") else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(rest.trim()) else { return Vec::new() };
    match v.get("type").and_then(|t| t.as_str()) {
        Some("chunk") => v
            .get("text")
            .and_then(|t| t.as_str())
            .map(|t| vec![CrmaineEvent::Chunk(t.to_string())])
            .unwrap_or_default(),
        Some("progress") => v
            .get("message")
            .and_then(|m| m.as_str())
            .map(|m| vec![CrmaineEvent::Chunk(format!("{m}\n"))])
            .unwrap_or_default(),
        Some("done") => vec![CrmaineEvent::Done],
        Some("error") => vec![CrmaineEvent::Error(
            v.get("message").and_then(|m| m.as_str()).unwrap_or("crmaine error").to_string(),
        )],
        _ => Vec::new(),
    }
}

/// The width the `:ragdebug` report is laid out at. Fixed (like the manual's)
/// because the lines are built before the popup is drawn; 72 keeps it whole on
/// an 80-column terminal.
const REPORT_W: usize = 72;

/// One chunk the retriever scored for a `/debug_search` query.
pub(crate) struct DebugHit {
    pub file: String,
    pub score: f64,
    pub chunk_id: Option<i64>,
    pub preview: String,
}

/// A `/debug_search` reply: the tokens the query became, and the chunks that
/// scored above zero, best first.
pub(crate) struct DebugSearch {
    pub hits: Vec<DebugHit>,
    /// The query's tokens, as the server tokenized it (it sends the first 30).
    pub tokens: Vec<String>,
    /// How many tokens there were in total — more than `tokens.len()` when the
    /// server truncated the list.
    pub token_count: usize,
}

/// Parse a `/debug_search` reply. The server answers with a plain JSON object;
/// an `error` (typically "index not loaded") comes back as `Err` so the caller
/// can say so instead of showing an empty ranking.
pub(crate) fn parse_debug_search(raw: &str) -> Result<DebugSearch, String> {
    let v: serde_json::Value = serde_json::from_str(raw.trim()).map_err(|e| e.to_string())?;
    if let Some(e) = v.get("error").and_then(|x| x.as_str()) {
        if !e.is_empty() {
            return Err(e.to_string());
        }
    }
    let tokens: Vec<String> = v
        .get("tokens")
        .and_then(|t| t.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let token_count =
        v.get("token_count").and_then(|c| c.as_u64()).unwrap_or(tokens.len() as u64) as usize;
    let mut hits = Vec::new();
    if let Some(arr) = v.get("results").and_then(|r| r.as_array()) {
        for it in arr {
            hits.push(DebugHit {
                file: it.get("file").and_then(|x| x.as_str()).unwrap_or("?").to_string(),
                score: it.get("score").and_then(|x| x.as_f64()).unwrap_or(0.0),
                chunk_id: it.get("chunk_id").and_then(|x| x.as_i64()),
                preview: it.get("preview").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            });
        }
    }
    Ok(DebugSearch { hits, tokens, token_count })
}

/// Lay the retrieval trace out as report lines: what the question was searched
/// as, which index answered, then each chunk with a bar scaled to the best
/// score and the opening of its text. The bar is what makes a bad retrieval
/// obvious at a glance — a flat run of near-equal scores means nothing in the
/// corpus really matched, while one tall bar on the wrong file means the
/// tokens went somewhere unexpected.
pub(crate) fn debug_report_lines(
    lang: Lang,
    query: &str,
    index: &str,
    isolated: bool,
    d: &DebugSearch,
    width: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    const LABEL_W: usize = 10;
    let label_w = LABEL_W;
    // A wrapped `label: value` block, with the continuation lines indented
    // under the value so the labels stay readable down the left edge.
    fn field(label: &str, value: &str, width: usize) -> Vec<String> {
        crate::util::wrap_str(value, width.saturating_sub(LABEL_W + 2))
            .iter()
            .enumerate()
            .map(|(i, part)| {
                let head = if i == 0 {
                    format!("{:<LABEL_W$}: ", label)
                } else {
                    " ".repeat(LABEL_W + 2)
                };
                format!("{head}{part}")
            })
            .collect()
    }
    let field = |label: &str, value: &str| field(label, value, width);
    out.extend(field(tr(lang, "question", "質問"), query));
    // A cache path is long and identifies itself at both ends, so it is cut in
    // the middle rather than wrapped, and which index it is says so underneath.
    out.extend(field(
        tr(lang, "index", "インデックス"),
        &crate::util::truncate_middle(index, width.saturating_sub(label_w + 2)),
    ));
    out.push(format!(
        "{}{}",
        " ".repeat(label_w + 2),
        if isolated {
            tr(lang, "(:index — isolated)", "(:index — 分離)")
        } else {
            tr(lang, "(crmaine's own)", "(crmaine 本体)")
        }
    ));
    let tokens = if d.tokens.is_empty() {
        tr(lang, "(none — nothing to search with)", "（なし — 検索語になっていません）").to_string()
    } else {
        let shown = d.tokens.join(" ");
        if d.token_count > d.tokens.len() {
            format!("{shown} …(+{})", d.token_count - d.tokens.len())
        } else {
            shown
        }
    };
    out.extend(field(&format!("{} ({})", tr(lang, "tokens", "検索語"), d.token_count), &tokens));
    out.push(String::new());

    if d.hits.is_empty() {
        for l in [
            tr(
                lang,
                "Nothing scored above zero — no chunk contains these words.",
                "スコアが 0 を超える断片がありません — どの断片にもこの語がありません。",
            ),
            "",
            tr(
                lang,
                ":rag may still have found something through query expansion,",
                ":rag はクエリ拡張で何か拾っている可能性はありますが、",
            ),
            tr(
                lang,
                "but the question as typed matches nothing in this index.",
                "質問そのままではこのインデックスに一致するものがありません。",
            ),
            tr(
                lang,
                "Check the folder is indexed (:raginfo, :index), or use the",
                "対象フォルダがインデックス済みか確認（:raginfo / :index）、または",
            ),
            tr(lang, "words the documents actually use.", "文書が実際に使っている語で試してください。"),
        ] {
            out.push(l.to_string());
        }
        return out;
    }

    // Said plainly, because these numbers are *not* the answer's final ordering:
    // `/query` expands the question and reranks, `/debug_search` does neither.
    // What this shows is the lexical ground truth the rest is built on.
    out.push(
        tr(
            lang,
            "raw BM25 for the question as typed — no cutoff, expansion or rerank",
            "質問そのままの生 BM25 — 足切り・クエリ拡張・再ランクなし",
        )
        .to_string(),
    );
    out.push("─".repeat(width));
    let top = d.hits.first().map(|h| h.score).unwrap_or(0.0).max(f64::MIN_POSITIVE);
    const BAR_W: usize = 16;
    for (i, h) in d.hits.iter().enumerate() {
        let filled = ((h.score / top) * BAR_W as f64).round().clamp(0.0, BAR_W as f64) as usize;
        let bar = format!("{}{}", "█".repeat(filled), "·".repeat(BAR_W - filled));
        let chunk = match h.chunk_id {
            Some(c) => format!(" #{c}"),
            None => String::new(),
        };
        // rank(3) + space + bar(16) + 2 + score(9) + 2 = 33 columns before the name.
        let name_w = width.saturating_sub(BAR_W + 17 + crate::util::width(&chunk));
        out.push(format!(
            "{:>2}. {}  {}  {}{}",
            i + 1,
            bar,
            crate::util::pad_left(&format!("{:.4}", h.score), 9),
            crate::util::truncate_middle(&h.file, name_w.max(8)),
            chunk,
        ));
        // The opening of the chunk itself — the point of the whole report is
        // seeing that this text is (or isn't) what the question was about.
        let preview = h.preview.replace(['\n', '\r', '\t'], " ");
        for part in crate::util::wrap_str(preview.trim(), width.saturating_sub(6)) {
            if !part.is_empty() {
                out.push(format!("      {part}"));
            }
        }
        out.push(String::new());
    }
    out
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

/// POST a JSON body and return the (non-streamed) response body. For the small
/// request/reply endpoints: `/cancel`, `/feedback`, `/search_files`.
pub(crate) fn http_post(port: u16, path: &str, body: &str) -> Result<String, String> {
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
    fn parse_tool_line_streams_progress_and_chunks_as_body() {
        // The analysis tools stream both status and answer as body text.
        match parse_tool_line("data: {\"type\":\"progress\",\"message\":\"scanning\"}").as_slice() {
            [CrmaineEvent::Chunk(t)] => assert_eq!(t, "scanning\n"),
            _ => panic!("progress should become a body line"),
        }
        match parse_tool_line("data: {\"type\":\"chunk\",\"text\":\"found\"}").as_slice() {
            [CrmaineEvent::Chunk(t)] => assert_eq!(t, "found"),
            _ => panic!("chunk should stream verbatim"),
        }
        assert!(matches!(
            parse_tool_line("data: {\"type\":\"done\"}").as_slice(),
            [CrmaineEvent::Done]
        ));
        assert!(matches!(
            parse_tool_line("data: {\"type\":\"error\",\"message\":\"x\"}").as_slice(),
            [CrmaineEvent::Error(_)]
        ));
    }

    #[test]
    fn parse_debug_search_reads_the_ranking_and_the_tokens() {
        let raw = r#"{"results":[
            {"file":"keihi.md","score":18.4213,"chunk_id":12,"preview":"出張費は帰着日の翌月10日までに"},
            {"file":"faq.md","score":2.5,"preview":"よくある質問"}
        ],"token_count":7,"tokens":["出張","費","精算"]}"#;
        let d = parse_debug_search(raw).expect("parses");
        assert_eq!(d.token_count, 7, "the total, not the shown list");
        assert_eq!(d.tokens, vec!["出張", "費", "精算"]);
        assert_eq!(d.hits.len(), 2);
        assert_eq!(d.hits[0].file, "keihi.md");
        assert_eq!(d.hits[0].chunk_id, Some(12));
        assert!((d.hits[0].score - 18.4213).abs() < 1e-9);
        // chunk_id is optional in the reply.
        assert_eq!(d.hits[1].chunk_id, None);
    }

    #[test]
    fn parse_debug_search_surfaces_an_index_not_loaded_error() {
        let raw = r#"{"error":"インデックス未ロード","results":[],"token_count":0,"tokens":[]}"#;
        assert_eq!(parse_debug_search(raw).err().as_deref(), Some("インデックス未ロード"));
        // An empty `error` is not an error — the server sends the key regardless.
        assert!(parse_debug_search(r#"{"error":"","results":[]}"#).is_ok());
    }

    #[test]
    fn debug_report_shows_the_question_tokens_and_a_bar_per_hit() {
        let d = DebugSearch {
            hits: vec![
                DebugHit {
                    file: "keihi.md".into(),
                    score: 20.0,
                    chunk_id: Some(12),
                    preview: "出張費は帰着日の\n翌月10日までに".into(),
                },
                DebugHit { file: "faq.md".into(), score: 5.0, chunk_id: None, preview: "".into() },
            ],
            tokens: vec!["出張".into(), "費".into()],
            token_count: 5,
        };
        let out = debug_report_lines(Lang::En, "expenses deadline", "C:\\idx", false, &d, 72);
        let text = out.join("\n");
        assert!(text.contains("expenses deadline"), "the question heads the report");
        assert!(text.contains("出張 費"), "the tokens it searched with");
        assert!(text.contains("…(+3)"), "says how many tokens were not shown");
        assert!(text.contains("keihi.md #12"), "file and chunk");
        assert!(text.contains("20.0000") && text.contains("5.0000"), "raw scores");
        // The bar is scaled to the best score: full for the top hit, a quarter
        // for a hit scoring a quarter as much.
        assert!(out.iter().any(|l| l.contains(&"█".repeat(16))), "top hit fills the bar");
        assert!(out.iter().any(|l| l.contains("████············")), "5/20 is a quarter bar");
        // The preview is flattened onto one line rather than breaking the layout.
        assert!(text.contains("出張費は帰着日の 翌月10日までに"));
        assert!(out.iter().all(|l| crate::util::width(l) <= 72), "fits the report width");
    }

    #[test]
    fn debug_report_says_so_when_nothing_matched() {
        let d = DebugSearch { hits: Vec::new(), tokens: vec!["x".into()], token_count: 1 };
        let text = debug_report_lines(Lang::En, "q", "/idx", true, &d, 72).join("\n");
        assert!(text.contains("Nothing scored above zero"));
        assert!(text.contains("query expansion"), "doesn't overclaim what :rag saw");
        assert!(text.contains(":index — isolated"), "names the index that was searched");
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
