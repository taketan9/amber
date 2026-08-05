//! The fuzzy pickers: a **command palette** (`C` / `:palette`) over the
//! commands, and a **fuzzy jump** (`Z` / `:jump`) over recently-visited
//! directories and bookmarks. Both share [`cian_core::fuzzy`] for ranking.

use std::path::PathBuf;

use super::*;

/// The commands the palette offers, as `(verb, description, takes_arg)`. Kept a
/// curated list (not every alias) so the picker stays scannable.
#[rustfmt::skip]
fn command_list() -> &'static [(&'static str, (&'static str, &'static str), bool)] {
    &[
        ("rag",        ("crmaine RAG: ask the running crmaine server", "crmaine RAG に質問"), true),
        ("agent",      ("crmaine Ajent: an agent answer", "crmaine エージェント回答"), true),
        ("coding",     ("crmaine Coding: ask about the current file's code", "crmaine Coding: 現在のファイルのコードを相談"), true),
        ("raginfo",    ("crmaine diagnostics: port, server, config", "crmaine 診断: ポート・接続・設定"), false),
        ("index",      ("index a folder into cian's own RAG index", "フォルダを cian の RAG にインデックス"), true),
        ("ragshared",  ("go back to crmaine's own index for :rag", ":rag を crmaine 本体のインデックスに戻す"), false),
        ("impact",     ("crmaine: which docs a change would touch", "crmaine: 変更の影響を受ける文書"), true),
        ("contradiction", ("crmaine: find conflicting statements on a topic", "crmaine: トピックの矛盾を検出"), true),
        ("glossary",   ("crmaine: generate a glossary of the corpus", "crmaine: コーパスの用語集を生成"), false),
        ("ai",         ("AI chat (Carmine)", "AI チャット（カーマイン）"), false),
        ("aicmd",      ("AI: shell command from a description", "AI: 説明からコマンド"), true),
        ("aicommit",   ("AI: draft a commit message", "AI: コミットメッセージ下書き"), false),
        ("aijunk",     ("AI: detect junk files", "AI: 不要ファイル検出"), false),
        ("aiorganize", ("AI: suggest a folder structure", "AI: フォルダ構成を提案"), false),
        ("airename",   ("AI: rename by instruction", "AI: 指示でリネーム"), false),
        ("aisearch",   ("AI: semantic file search", "AI: 意味検索"), true),
        ("aierror",    ("AI: explain the last shell error", "AI: 直前のエラーを説明"), false),
        ("aidiff",     ("AI: explain the diff on screen", "AI: 差分を説明"), false),
        ("ailog",      ("AI: triage the selected log", "AI: ログを診断"), false),
        ("du",         ("disk usage: what's biggest here", "容量分析"), false),
        ("count",      ("count files, lines and steps", "ファイル/行/ステップ数"), false),
        ("diff",       ("compare the two panes", "左右を比較"), false),
        ("dupes",      ("find duplicate files", "重複ファイル検出"), false),
        ("brename",    ("bulk rename by a pattern", "パターン一括リネーム"), false),
        ("each",       ("run a command per marked file ({} = path)", "マーク各ファイルにコマンド実行（{} = パス）"), true),
        ("sftp",       ("open a server in this pane (remote pane)", "サーバをペインで開く"), false),
        ("ssh",        ("ssh connect picker", "SSH 接続ピッカー"), false),
        ("zip",        ("zip the selection (-e for a password)", "選択を zip（-e で暗号化）"), false),
        ("tar",        ("tar the selection", "選択を tar"), false),
        ("targz",      ("tar.gz the selection", "選択を tar.gz"), false),
        ("unzip",      ("extract the archive here", "書庫をここに展開"), false),
        ("hash",       ("checksum the selection (md5 / sha256)", "チェックサム"), true),
        ("attr",       ("permissions & owner", "権限・所有者"), false),
        ("chmod",      ("change the mode (octal)", "モード変更（8進）"), true),
        ("hidden",     ("show / hide dotfiles", "隠しファイル表示切替"), false),
        ("theme",      ("theme gallery / set a theme", "テーマ選択"), true),
        ("sync",       ("synchronize input across shell panes", "シェル入力同期"), false),
        ("snip",       ("snippet launcher", "スニペット"), false),
        ("macros",     ("run a macro", "マクロ実行"), false),
        ("stage",      ("git add the selection", "git add"), false),
        ("unstage",    ("git reset the selection", "git reset"), false),
        ("discard",    ("discard worktree changes", "変更を破棄"), false),
        ("gitlog",     ("commit log", "コミットログ"), false),
        ("gitdiff",    ("working-tree diff vs HEAD", "作業ツリーの差分"), false),
        ("jump",       ("fuzzy-jump to a recent directory", "最近のディレクトリへ移動"), false),
        ("cd",         ("go to a typed path", "パス入力で移動"), true),
        ("edit",       ("open in the external editor", "外部エディタで開く"), false),
        ("reload",     ("reload init.lua", "init.lua を再読込"), false),
        ("menu",       ("the right-click menu", "右クリックメニュー"), false),
        ("where",      ("show config / state file paths", "設定ファイルの場所"), false),
    ]
}

impl App {
    /// Open the command palette (`C` / `:palette`).
    pub(crate) fn start_command_palette(&mut self) {
        let ja = self.lang == Lang::Ja;
        let items: Vec<PaletteItem> = command_list()
            .iter()
            .map(|(verb, (en, jp), takes_arg)| PaletteItem {
                label: format!(":{}", verb),
                detail: (if ja { *jp } else { *en }).to_string(),
                value: verb.to_string(),
                takes_arg: *takes_arg,
            })
            .collect();
        self.open_palette(PaletteKind::Commands, items);
    }

    /// Open the fuzzy jump picker (`Z` / `:jump`): recent directories from
    /// both panes' history, plus bookmarked folders, deduped.
    pub(crate) fn start_fuzzy_jump(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut items = Vec::new();
        let push = |items: &mut Vec<PaletteItem>, seen: &mut std::collections::HashSet<String>, p: &std::path::Path, tag: &str| {
            let s = p.display().to_string();
            if !p.is_dir() || !seen.insert(s.clone()) {
                return;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| s.clone());
            items.push(PaletteItem { label: name, detail: format!("{}  {}", tag, s), value: s, takes_arg: false });
        };
        // Recent directories from both panes (most recent first).
        for tabs in [&self.left, &self.right] {
            let pane = tabs.active_ref();
            push(&mut items, &mut seen, &pane.cwd, "•");
            for p in &pane.history {
                push(&mut items, &mut seen, p, "recent");
            }
        }
        // Bookmarked folders (shortcuts whose target is a directory).
        for sc in bookmark_dirs(&self.shortcuts.entries) {
            push(&mut items, &mut seen, &sc, "bookmark");
        }
        if items.is_empty() {
            self.message = Some(tr(self.lang, "no recent directories to jump to", "移動できる履歴がありません").into());
            return;
        }
        self.open_palette(PaletteKind::Jump, items);
    }

    fn open_palette(&mut self, kind: PaletteKind, items: Vec<PaletteItem>) {
        let shown = (0..items.len()).collect();
        self.popup = Popup::Palette { kind, query: String::new(), items, shown, cursor: 0, scroll: 0 };
    }

    /// Re-rank the picker's rows against the current query (called on every edit).
    pub(crate) fn palette_refilter(&mut self) {
        if let Popup::Palette { query, items, shown, cursor, scroll, .. } = &mut self.popup {
            let labels = items.iter().map(|it| it.label.as_str());
            *shown = cian_core::fuzzy::rank(query, labels);
            *cursor = 0;
            *scroll = 0;
        }
    }

    /// Move the picker's cursor by `delta`, clamped.
    pub(crate) fn palette_move(&mut self, delta: isize) {
        if let Popup::Palette { shown, cursor, .. } = &mut self.popup {
            if shown.is_empty() {
                return;
            }
            let n = shown.len() as isize;
            *cursor = (*cursor as isize + delta).clamp(0, n - 1) as usize;
        }
    }

    /// Act on the highlighted row: run the command / prefill it, or jump.
    pub(crate) fn palette_accept(&mut self) {
        let (kind, value, takes_arg) = {
            let Popup::Palette { kind, items, shown, cursor, .. } = &self.popup else { return };
            let Some(&idx) = shown.get(*cursor) else { return };
            let it = &items[idx];
            (*kind, it.value.clone(), it.takes_arg)
        };
        self.popup = Popup::None;
        match kind {
            PaletteKind::Jump => {
                if let Some(p) = self.active_pane_mut() {
                    let _ = p.jump_to(PathBuf::from(value));
                }
            }
            PaletteKind::Commands if takes_arg => {
                // Prefill command mode so the argument can be typed.
                self.command_buffer = format!("{} ", value);
                self.mode = Mode::Command;
            }
            PaletteKind::Commands => {
                self.command_buffer = value;
                self.run_command();
            }
        }
    }
}

/// The directory targets among the flat list of bookmarks (nested groups walked).
fn bookmark_dirs(entries: &[Shortcut]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(list: &[Shortcut], out: &mut Vec<PathBuf>) {
        for s in list {
            if let Some(children) = &s.children {
                walk(children, out);
            } else if let Some(t) = &s.target {
                let expanded = crate::expand_path(t);
                if expanded.is_dir() {
                    out.push(expanded);
                }
            }
        }
    }
    walk(entries, &mut out);
    out
}
