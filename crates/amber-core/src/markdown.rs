//! Markdown の解釈を 1 か所で行う。描き方の違う 2 つのフロントエンドのために。
//!
//! 端末版ではこれがレンダラと一体になっていた。見出しを認識して、そのまま
//! 装飾済みの ratatui の行を出す。描き方が 1 通りのあいだはそれで動くが、
//! デスクトップ版が 2 通り目になった。そこで認識する側をここへ下ろし、
//! 描く側は上に残した。
//!
//! **もう 1 つの選択肢はパーサーを 2 つ持つことだった**が、パーサーが 2 つあると
//! `*a_b*` の意味について意見が 2 つできる。1 つのプログラムの 2 つのフロントエンドが
//! 自分たちの README の解釈で食い違うのは、小さいが雑に見える。
//!
//! CommonMark に準拠していないのは意図的。ここが読むのは、人が README やノートに
//! 実際に書く Markdown ── 見出し、リスト、フェンス、表、チェックボックス、
//! 4 種類のインライン記号 ── までで、そこで止める。完全な実装は依存が大きく、
//! そのほとんどは一度も使われない。

/// 同じ意味を持つ、ひと続きのテキスト。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    /// `` `code` `` ── 1 つの範囲で、入れ子にはならない。
    Code(String),
    Bold(String),
    Italic(String),
    /// `***both***` — 太字と斜体。二つ持つので入れ子にしたくなるが、この
    /// スキャナは入れ子を解釈しない（`inline` の注記）ので、1 つのバリアントにしてある。
    BoldItalic(String),
    Strike(String),
    Link {
        text: String,
        url: String,
    },
    /// 本文に裸で書かれた `https://…`（依頼 606・本人が決めた）。
    ///
    /// **クリックすれば開くが、ファイルの内容は 1 文字も変えない。** `[テキスト](url)` と
    /// 同じ形にして保存すると、書いていない記号が勝手に増え、ほかのアプリで
    /// 開いた人には別の文字列に見える ── だから `Link` とは別のバリアントにして、
    /// 書き戻す側（`inlineToMd`）が「これは元から裸だった」と分かるように
    /// `data-bare` を付けて出す。
    Bare(String),
    /// `<span style="color:#rrggbb">…</span>` ── ここが解釈する唯一の HTML。
    /// Markdown に色の記法が無く、これがほかのツールにもいちばん通じる書き方
    /// だから。**通すのは検証済みの 16 進の色だけ**なので、`html` での約束 ──
    /// ファイル由来のものはすべて escape する ── は保たれたまま。
    ///
    Colored { text: String, color: String },
}

impl Inline {
    /// 記号を落とした文字列。プレーンテキストが要る用途 ── 幅の計測や検索 ──
    /// で、強調が関係ないとき。
    pub fn text(&self) -> &str {
        match self {
            Inline::Text(t)
            | Inline::Code(t)
            | Inline::Bold(t)
            | Inline::Italic(t)
            | Inline::BoldItalic(t)
            | Inline::Strike(t) => t,
            Inline::Link { text, .. } => text,
            Inline::Bare(t) => t,
            Inline::Colored { text, .. } => text,
        }
    }
}

/// 表の列の寄せ方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// 1 行を、意味のまとまりごとに分割する。
///
/// 文法ではなくスキャナとして書いてある。Markdown のインライン記号は実際には
/// 入れ子にならない ── 斜体の中のリンクの中の太字、を書く人はいない ── し、
/// 閉じていない記号を諦めてテキストのまま残すスキャナの挙動は、はぐれた
/// アスタリスクに対して読み手が期待するものと一致する。
pub fn inline(text: &str) -> Vec<Inline> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Inline> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;

    let flush = |out: &mut Vec<Inline>, buf: &mut String| {
        if !buf.is_empty() {
            out.push(Inline::Text(std::mem::take(buf)));
        }
    };

    while i < chars.len() {
        let c = chars[i];

        // 色の span。`note::spans` が認識するので、デスクトップ版と iPhone で
        // ノートの解釈が 2 通りになることがない。
        if c == '<' {
            let rest: String = chars[i..].iter().collect();
            if let Some((inner, color, took)) = crate::note::first_color(&rest) {
                flush(&mut out, &mut buf);
                out.push(Inline::Colored { text: inner, color });
                i += took;
                continue;
            }
        }

        // インラインコードを先に見る。バッククォートで挟まれた中はすべてリテラルなので、
        // そこにある `*` はアスタリスクであって、強調の開始ではない。
        if c == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|&x| x == '`') {
                flush(&mut out, &mut buf);
                out.push(Inline::Code(chars[i + 1..i + 1 + end].iter().collect()));
                i += end + 2;
                continue;
            }
        }

        // 太字と斜体の同時指定。`***…***` か `___…___`。
        //
        // **`**` より先に見る。** 後ろに置くと `**` が先に食い、閉じの
        // `***` の一つ目までを中身にしてしまう ── 太字の頭にアスタリスクが
        // 1 つ増えて表示される。これはデスクトップ版だけの挙動だった。iPhone は Apple の
        // `AttributedString` に渡していて、そちらは正しく読む。
        //
        // 閉じが無ければ何もせず下の `**` に落ちるので、`***閉じていない**`
        // は今日と同じに読まれる。
        if (c == '*' || c == '_')
            && i + 2 < chars.len()
            && chars[i + 1] == c
            && chars[i + 2] == c
        {
            if let Some(end) = find_run3(&chars, i + 3, c) {
                flush(&mut out, &mut buf);
                out.push(Inline::BoldItalic(chars[i + 3..end].iter().collect()));
                i = end + 3;
                continue;
            }
        }

        // 太字 **…** または __…__
        if (c == '*' || c == '_') && i + 1 < chars.len() && chars[i + 1] == c {
            if let Some(end) = find_run(&chars, i + 2, [c, c]) {
                flush(&mut out, &mut buf);
                out.push(Inline::Bold(chars[i + 2..end].iter().collect()));
                i = end + 2;
                continue;
            }
        }

        // 取り消し線 ~~…~~
        if c == '~' && i + 1 < chars.len() && chars[i + 1] == '~' {
            if let Some(end) = find_run(&chars, i + 2, ['~', '~']) {
                flush(&mut out, &mut buf);
                out.push(Inline::Strike(chars[i + 2..end].iter().collect()));
                i = end + 2;
                continue;
            }
        }

        // 斜体 *…* または _…_。直後が空白なら除外する。これがあるので
        // `a * b * c` の途中が強調にならない。
        if c == '*' || c == '_' {
            if let Some(end) = chars[i + 1..].iter().position(|&x| x == c) {
                let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                if !inner.is_empty() && !inner.starts_with(' ') {
                    flush(&mut out, &mut buf);
                    out.push(Inline::Italic(inner));
                    i += end + 2;
                    continue;
                }
            }
        }

        // **裸の `https://…` も、押せば開く**（依頼 606）。
        //
        // `[テキスト](url)` を先に見てから、ここに来る ── 括弧の中の url を
        // 二度拾わないため。**行の中の区切りで止める** ── 日本語の文では
        // URL のすぐ後ろに句点や閉じ括弧が来るので、そこまで飲み込むと
        // 行き先が壊れる（`https://x/a。` は開けない）。
        if (c == 'h' || c == 'H') && (starts_url(&chars, i)) {
            let mut end = i;
            while end < chars.len() && !url_stop(chars[end]) {
                end += 1;
            }
            // 末尾の句読点は、たいてい文のほう（`…example.com.` の `.`）。
            while end > i && matches!(chars[end - 1], '.' | ',' | '、' | '。' | '!' | '?' | ':' | ';') {
                end -= 1;
            }
            let url: String = chars[i..end].iter().collect();
            // `https://` だけ、のような中身の無いものはただの文字列。
            if url.len() > 8 && safe_url(&url).is_some() {
                flush(&mut out, &mut buf);
                out.push(Inline::Bare(url));
                i = end;
                continue;
            }
        }

        // リンク [テキスト](url)
        if c == '[' {
            if let Some(close) = chars[i + 1..].iter().position(|&x| x == ']') {
                let after = i + 1 + close + 1;
                if chars.get(after) == Some(&'(') {
                    if let Some(paren) = chars[after + 1..].iter().position(|&x| x == ')') {
                        flush(&mut out, &mut buf);
                        out.push(Inline::Link {
                            text: chars[i + 1..i + 1 + close].iter().collect(),
                            url: chars[after + 1..after + 1 + paren].iter().collect(),
                        });
                        i = after + 1 + paren + 1;
                        continue;
                    }
                }
            }
        }

        buf.push(c);
        i += 1;
    }
    flush(&mut out, &mut buf);
    out
}

/// `http://` か `https://` が、ここから始まっているか。
///
/// **直前が文字なら、開始位置ではない** ── `xhttps://…` や、既に `](` の中に
/// ある url を二度拾わないため。
fn starts_url(chars: &[char], at: usize) -> bool {
    if at > 0 {
        let before = chars[at - 1];
        if before.is_alphanumeric() || before == '/' || before == '(' {
            return false;
        }
    }
    let rest: String = chars[at..].iter().take(8).collect();
    let low = rest.to_ascii_lowercase();
    low.starts_with("https://") || low.starts_with("http://")
}

/// URL は、ここで終わる。**空白と、日本語の文で後ろに来るもの。**
fn url_stop(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '<' | '>' | '"' | '`' | '｜' | '|' | '、' | '。' | '）' | ')' | '］' | ']'
                | '」' | '』' | '＞' | '　'
        )
}

/// `from` 以降で、`mark` が 3 文字続く箇所の開始位置。
fn find_run3(chars: &[char], from: usize, mark: char) -> Option<usize> {
    let mut i = from;
    while i + 2 < chars.len() {
        if chars[i] == mark && chars[i + 1] == mark && chars[i + 2] == mark {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `from` 以降で、`marker` が 2 文字続く箇所の開始位置。
fn find_run(chars: &[char], from: usize, marker: [char; 2]) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == marker[0] && chars[i + 1] == marker[1] {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ---- ブロックの判定 ----
//
// どれも 1 行について 1 つのことだけを答える。端末版の時点で既に分かれて
// いたので、そのままここへ下ろせた。
// unchanged.

/// `## Heading` → `(2, "Heading")`.
/// 見出しのアンカー。GitHub と同じ作り方 ── 小文字にし、空白をハイフンにし、
/// 記号を落とす。
///
/// **連続するハイフンをまとめないのは意図的。** GitHub の slugger は
/// `v1.2 — notes` を `v12--notes` にする ── ダッシュが落ち、その前後の空白が
/// それぞれハイフンになる ── そして README の中のリンクは*その結果*に対して
/// 書かれている。きれいに整えたアンカーは、その文書自身のどのリンクも
/// 指していない、ただの見栄えのいい文字列になる。
///
/// 日本語は落とさず*残す*。GitHub は href では percent encode し、id には
/// 文字をそのまま残す ── 落とすと、日本語の文書では見出しが全部同じ空の
/// アンカーに潰れる。アンカーが無いよりも悪い。デスクトップ版は id を引く前に
/// href を decode する。
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().chars() {
        if c.is_whitespace() {
            out.push('-');
        } else if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        }
        // それ以外 ── `.` `(` `:` 絵文字など ── は、GitHub と同じように落とす。
        //
    }
    out
}

pub fn heading(line: &str) -> Option<(usize, String)> {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = t[hashes..].trim_start();
    // `#hashtag` は見出しではない。見出しは `#` の後ろに空白がある。
    if rest.len() == t.len() - hashes {
        return None;
    }
    Some((hashes, rest.to_string()))
}

/// `---` `***` `___` が単独で置かれた行。
pub fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

/// ` ```rust ` → `Some("rust")`、` ``` ` → `Some("")`。
/// **`~~~` も枠。** GFM は三つ以上の `` ` `` と `~` のどちらも枠にする。
/// `~~~` を知らないと、中のテキストが段落として整形される ── `~~` が取り消し線に
/// 読まれ、中に `- ` があれば点になる。**コードが Markdown として解釈
/// される**ので、見え方だけの話では済まない（2026-09-08 の往復の試験で出た）。
pub fn fence_lang(line: &str) -> Option<String> {
    let t = line.trim_start();
    for mark in ["```", "~~~"] {
        if let Some(rest) = t.strip_prefix(mark) {
            // 記号そのものが続くのは、4 つ以上のフェンス（``````）── 記号を全部
            // 落としてから言語を読む。
            let rest = rest.trim_start_matches(mark.chars().next().unwrap());
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// `- item` / `1. item` → `(marker, text, indent)`.
pub fn list_item(raw: &str) -> Option<(String, String, usize)> {
    let indent = raw.len() - raw.trim_start().len();
    let t = raw.trim_start();
    for m in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(m) {
            return Some(("•".to_string(), rest.to_string(), indent));
        }
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let after = &t[digits..];
        for m in [". ", ") "] {
            if let Some(rest) = after.strip_prefix(m) {
                return Some((format!("{}{}", &t[..digits], m.trim_end()), rest.to_string(), indent));
            }
        }
    }
    None
}

/// `[ ] thing` / `[x] thing` → `(done, text)`.
pub fn task_item(text: &str) -> Option<(bool, String)> {
    let t = text.trim_start();
    for (mark, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
        if let Some(rest) = t.strip_prefix(mark) {
            return Some((done, rest.to_string()));
        }
    }
    None
}

/// `| --- | :-: |` ── 表のヘッダー行の下に来る区切り行。
pub fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    if !t.contains('-') || !t.starts_with('|') {
        return false;
    }
    t.trim_matches('|')
        .split('|')
        .all(|c| {
            let c = c.trim();
            !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// `| a | b |` のセル。
pub fn split_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// `:-:` → 中央、`--:` → 右、それ以外 → 左。
pub fn cell_align(sep: &str) -> Align {
    let s = sep.trim();
    match (s.starts_with(':'), s.ends_with(':')) {
        (true, true) => Align::Center,
        (false, true) => Align::Right,
        _ => Align::Left,
    }
}

// ---- HTML への描画 ----
//
// デスクトップ版側の処理。端末版は同じ解析結果を装飾付きの行として描くが、
// こちらは文書にする。それが、端末にできなくてウィンドウにできる唯一のこと ──
// 本物のプロポーショナルフォント、本物の表、本物のコードブロック。
//
// **テキストはすべて escape する。** README はどこかから来たファイルであり、
// 中身をそのまま実行するプレビューは、誰かが clone したリポジトリに入っていた
// ものを何でも実行するプレビューになる。

/// そのままだとマークアップになる 5 文字を escape する。
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// `href` に入れても安全な URL。
///
/// リンクの `javascript:` は最も古い手口で、README はどこかから来たファイル。
/// http・https・mailto・相対パスのいずれでもないものは、リンクにしない ──
/// テキストとして表示するので何も隠さず、ただクリックできないだけにする。
///
fn safe_url(url: &str) -> Option<String> {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
        return Some(esc(u));
    }
    // 相対パスはスキームを持たない。`foo:bar` はスキームかもしれないので、
    // 最初のスラッシュより前にコロンがあるものは拒否する。
    let scheme_ish = u.split('/').next().unwrap_or("").contains(':');
    if !scheme_ish && !u.is_empty() {
        return Some(esc(u));
    }
    None
}

/// 段落の 1 行を、前後を削って、**行末の改行マークを保持したまま**返す。
///
/// **行頭の空白は削らない。** 全角空白（`　`）はインデントで、人が打ったもの
/// ── `trim()` は全角空白も削るので、半角の空白と tab だけを落とす。
///
/// **行末のマークは 2 種類ある。** 空白 2 つと `\` は、どちらも「ここで改行」の
/// マーク（CommonMark の hard break）。amber は改行をそのまま改行として描くので
/// **見た目は同じ**だが、人が打った文字なので**そのまま復元できるように**どちらかを
/// 覚えておく（制御文字を一つ挟む ── ふつうの Markdown には出てこない）。
fn mark_break(line: &str) -> String {
    let head = line.trim_start_matches([' ', '\t']);
    let body = head.trim_end_matches([' ', '\t']);
    // `\` が二つ以上並んでいるなら、最後の一つは逃がされた `\` であって
    // 改行マークではない ── 数えて奇数のときだけマークとして扱う。
    let slashes = body.len() - body.trim_end_matches('\\').len();
    if slashes % 2 == 1 {
        return format!("{}\u{2}", &body[..body.len() - 1]);
    }
    if head.len() >= body.len() + 2 {
        return format!("{body}\u{1}");
    }
    body.to_string()
}

/// 段落の中の改行を `<br>` にする。**マークがあったところは、マークごと保持させる。**
///
/// GitHub（CommonMark）は段落の中の一つの改行を空白にして繋ぐが、amber は
/// そうしない ── 2026-09-08 に決めた（`PAPER.ja.md` 六章）。理由は見え方では
/// なく、**文字が消えるから**（`to_html` の段落の注記）。
fn breaks(html: &str) -> String {
    html.replace("\u{1}\n", "<br data-hard=\"  \">")
        .replace("\u{2}\n", "<br data-hard=\"\\\">")
        .replace('\n', "<br>")
}

fn inline_html(text: &str) -> String {
    let mut out = String::new();
    for piece in inline(text) {
        match piece {
            Inline::Text(t) => out.push_str(&esc(&t)),
            Inline::Code(t) => {
                out.push_str("<code>");
                out.push_str(&esc(&t));
                out.push_str("</code>");
            }
            Inline::Bold(t) => {
                out.push_str("<strong>");
                out.push_str(&esc(&t));
                out.push_str("</strong>");
            }
            Inline::Italic(t) => {
                out.push_str("<em>");
                out.push_str(&esc(&t));
                out.push_str("</em>");
            }
            Inline::BoldItalic(t) => {
                out.push_str("<strong><em>");
                out.push_str(&esc(&t));
                out.push_str("</em></strong>");
            }
            Inline::Strike(t) => {
                out.push_str("<del>");
                out.push_str(&esc(&t));
                out.push_str("</del>");
            }
            // 色はここに来る前に 16 進 6 桁として検証済み。つまり style 属性を
            // 書き出すのはここ 1 か所だけで、ほかの内容が混ざることはない。
            //
            //
            // **色の内側の記号は解釈する**（依頼 633）── 以前は中身を文字として escape して
            // いたので、`<span …>**hoge**</span>` が `**hoge**` と出た。OneNote
            // から来た表の見出し（濃い地に白い太字）がそれで、本人の画面には
            // アスタリスクがそのまま並んだ。スキャナは入れ子を解釈しないが、**ここで 1 段
            // だけ潜る**ぶんには同じ取り決めを保てる ── 中のテキストもこの関数が escape する。
            Inline::Colored { text, color } => {
                out.push_str(&format!(
                    "<span style=\"color:{color}\">{}</span>",
                    inline_html(&text)
                ));
            }
            Inline::Link { text, url } => match safe_url(&url) {
                Some(href) => {
                    out.push_str(&format!("<a href=\"{href}\">{}</a>", esc(&text)));
                }
                // 隠さずに表示する ── ただしクリックはできない。
                None => out.push_str(&esc(&text)),
            },
            // **`data-bare` は約束。** 書き戻す側はこれを見て、`[…](…)` に
            // せず URL の文字列だけを戻す ── ファイルの内容が変わらない。
            Inline::Bare(url) => match safe_url(&url) {
                Some(href) => out.push_str(&format!(
                    "<a href=\"{href}\" data-bare=\"1\">{}</a>",
                    esc(&url)
                )),
                None => out.push_str(&esc(&url)),
            },
        }
    }
    out
}

/// 書く道具。**選んだところだけを渡してもらい、置き換えたものを返す。**
///
/// 位置（何文字目か）は受け取らない ── JS は UTF-16 の桁で数え、Rust は
/// 文字数で数えるので、絵文字が 1 つ混ざるだけで境界がずれる。選択された文字列そのものを
/// もらえば、その食い違いは起きようがない。
///
/// **ロジックはここにある。** iPhone の `Marks.deepen` とデスクトップ版が別々に「見出しを
/// 深くする」を持つと、2 つのフロントエンドで操作感が分かれる。いまはデスクトップ版だけが
/// ここを通っており、iPhone は Swift の写しを持ったままなので、**揃えるのは
/// これから**（`REQUESTS.ja.md` に置いた）。
pub mod marks {
    /// 挟む。**もう挟まっているなら外す。** 間違えて押した瞬間に欲しくなる。
    pub fn wrap(text: &str, mark: &str) -> String {
        if mark.is_empty() {
            return text.to_string();
        }
        // 選択が無いときは記号だけ置く ── 中にカーソルを置いて打てるように。
        if text.is_empty() {
            return format!("{mark}{mark}");
        }
        if let Some(inner) = text
            .strip_prefix(mark)
            .and_then(|t| t.strip_suffix(mark))
            .filter(|_| text.len() >= mark.len() * 2)
        {
            return inner.to_string();
        }
        format!("{mark}{text}{mark}")
    }

    /// 行頭の記号。**すべての行に付いていれば外し、1 つでも無ければ付ける。**
    ///
    /// 付けるときは、先に**別の行頭記号を外す** ── 箇条書きを引用にすると
    /// `> - もの` になるのは、たいてい望んだことではない。
    pub fn prefix(text: &str, mark: &str) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let numbered = mark.starts_with(|c: char| c.is_ascii_digit());
        let has = |l: &str| -> bool {
            let t = l.trim_start();
            if numbered {
                let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
                return d > 0 && t[d..].starts_with(". ");
            }
            t.starts_with(mark)
                // `- [ ] ` は `- [x] ` でも付いている。
                || (mark == "- [ ] " && (t.starts_with("- [x] ") || t.starts_with("- [X] ")))
        };
        // 空行は数に入れない ── 一行だけ空いているせいで外れない、を防ぐ。
        let live: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
        let all = !live.is_empty() && live.iter().all(|l| has(l));

        let mut n = 0usize;
        lines
            .iter()
            .map(|l| {
                if l.trim().is_empty() {
                    return l.to_string();
                }
                let indent = &l[..l.len() - l.trim_start().len()];
                let body = strip_any(l.trim_start());
                if all {
                    return format!("{indent}{body}");
                }
                n += 1;
                if numbered {
                    format!("{indent}{n}. {body}")
                } else {
                    format!("{indent}{mark}{body}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 行頭に付いている記号を、種類を問わず外す。
    fn strip_any(t: &str) -> &str {
        for m in ["- [ ] ", "- [x] ", "- [X] ", "> ", "- ", "* ", "+ "] {
            if let Some(r) = t.strip_prefix(m) {
                return r;
            }
        }
        let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if d > 0 {
            if let Some(r) = t[d..].strip_prefix(". ") {
                return r;
            }
        }
        t
    }

    /// 見出しを、この深さにする。`0` なら見出しをやめる。
    ///
    /// **押すたびに深くなる（[`deepen`]）とは別の処理。** ツールバーのボタンは 1 つで
    /// 済ませたいが、鍵盤からは `###` に一打で行きたい ── Inkdrop も
    /// `toggle-heading-1` … `-4` を別々に持っている。
    pub fn level(text: &str, n: usize) -> String {
        text.split('\n')
            .map(|l| {
                let t = l.trim_start();
                let indent = &l[..l.len() - t.len()];
                let had = t.chars().take_while(|c| *c == '#').count();
                let body = t[had..].trim_start();
                if n == 0 {
                    return format!("{indent}{body}");
                }
                format!("{indent}{} {body}", "#".repeat(n.min(6)))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 見出し。**押すたびに深くなる** ── `#` → `##` → `###` → 無し。
    ///
    /// ボタンを三つ置くと、一つの考えに三つの名前が付く。
    pub fn deepen(text: &str) -> String {
        text.split('\n')
            .map(|l| {
                let t = l.trim_start();
                let indent = &l[..l.len() - t.len()];
                let n = t.chars().take_while(|c| *c == '#').count();
                let body = t[n..].trim_start();
                if body.is_empty() && n == 0 {
                    return l.to_string();
                }
                match n {
                    0 => format!("{indent}# {body}"),
                    1 => format!("{indent}## {body}"),
                    2 => format!("{indent}### {body}"),
                    _ => format!("{indent}{body}"),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 埋め込んだマーカーを `data-line` / `data-span` に変える。
///
/// マーカーは `\u{1}` 行番号 `\u{2}`。**次のマーカーまでが、そのブロックが消費した行**。
/// 何も出力しなかった回（空行など）はマーカーが続くだけなので、そのまま削除する。
///
/// 挿入先は**マーカーのすぐ後ろの開始タグ**。表や引用のように中にタグを持つものでも、
/// 外側の 1 つだけに挿入される ── 中まで挿入すると、クリックした場所によって違う行が
/// 返ることになる。
fn stamps(raw: &str, total: usize) -> String {
    // まず (マーカーの位置, 行番号, マーカーの長さ) を集める。
    let mut marks: Vec<(usize, usize, usize)> = Vec::new();
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 1 {
            if let Some(end) = raw[i..].find('\u{2}') {
                let n: usize = raw[i + 1..i + end].parse().unwrap_or(0);
                marks.push((i, n, end + 1));
                i += end + 1;
                continue;
            }
        }
        i += 1;
    }

    let mut out = String::with_capacity(raw.len());
    let mut cut = 0;
    for (k, &(at, line, len)) in marks.iter().enumerate() {
        out.push_str(&raw[cut..at]);
        cut = at + len;
        // 次のマーカーの行番号までが、このブロックの行数。
        let next = marks.get(k + 1).map(|m| m.1).unwrap_or(total);
        let span = next.saturating_sub(line).max(1);
        // **終了タグは飛ばして、次の開始タグに挿入する。**
        //
        // 箇条書きの二つ目以降は、前の項目を閉じてから始まる ── そこで
        // 最初に見つかる `>` は `</li>` のもので、そこに差すと
        // `</li data-line="10">` というタグとして成立しないものができる（実際に作ったことがある）。
        let rest = &raw[cut..];
        let mut j = 0;
        loop {
            let r = &rest[j..];
            j += r.len() - r.trim_start().len();
            let r = &rest[j..];
            if r.starts_with("</") {
                match r.find('>') {
                    Some(p) => {
                        j += p + 1;
                        continue;
                    }
                    None => break,
                }
            }
            break;
        }
        if rest[j..].starts_with('<') && !rest[j..].starts_with("</") {
            if let Some(p) = rest[j..].find('>') {
                out.push_str(&rest[..j + p]);
                out.push_str(&format!(" data-line=\"{line}\" data-span=\"{span}\""));
                cut += j + p;
            }
        }
    }
    out.push_str(&raw[cut..]);
    out
}

/// `> [!NOTE]` の一行を、注記の種類に。**GitHub が読める五つだけ。**
///
/// 増やすと、ここでだけ見える記法になり、同じノートが GitHub で壊れる。
/// その行は `<details>` の開始か。**行全体がそのタグのときだけ。**
///
/// `open` を書いてあれば、初めから開いた姿で出す（`<details open>`）。
pub fn is_details_open(t: &str) -> bool {
    let t = t.trim();
    t == "<details>" || (t.starts_with("<details ") && t.ends_with('>'))
}

pub fn alert_kind(t: &str) -> Option<String> {
    let rest = t.strip_prefix('>')?.trim_start();
    let inner = rest.strip_prefix("[!")?.strip_suffix(']')?;
    let k = inner.to_ascii_lowercase();
    matches!(k.as_str(), "note" | "tip" | "important" | "warning" | "caution").then_some(k)
}

/// Markdown を HTML として描画する。
///
/// パーサーを共有している端末版のレンダラと同じく、行単位で処理する。ここが
/// 読むのは人が実際に書く Markdown であって、仕様書に書かれた Markdown ではない。
pub fn to_html(lines: &[String]) -> String {
    render(lines, true)
}

/// 各かたまりに、**元の何行目から何行ぶんか**を差す。
///
/// 表示画面で直接編集するのに要る ── クリックしたブロックの元のテキストが取れなければ、
/// 表示画面は閲覧専用のままになる。チェックボックスが `data-line` を持って
/// いるのと同じ理由で、**何番目のブロックかを数えると front matter のあるノートで
/// ずれる**。
///
/// マーカーはいったん制御文字で置き、最後に属性へ変える ── ブロックが何行
/// 消費したかは、**次のブロックが始まる場所**を見るまで分からない（フェンスも
/// 表も引用も、閉じるまで進む）。開始は毎回の先頭で分かるので、マーカーだけ先に
/// 置いておけば `continue` で抜ける枝も漏れない。
///
/// 引用や注記の中で呼び直すときは差さない（`stamp = false`）── 中の行番号は
/// 切り出したあとの数え方で、ファイルの行番号ではない。
fn render(lines: &[String], stamp: bool) -> String {
    let mut out = String::new();
    // front matter があれば取り除く。あれはノートが自分自身を説明するもので、
    // 本文として述べていることではないし、タイトルとタグはこれが描かれる場所の
    // 画面に既に出ている。先頭の `---` が front matter か水平線かは `note::front`
    // が判断する ── 判断が 1 か所なので、iPhone とデスクトップ版でノートの
    // 開始位置の解釈が一致する。
    let mut i = crate::note::front(lines).lines;
    // どのリスト階層が開いているかを、インデントごとに持つ。番号付きかどうかも。
    // Markdown の入れ子はインデントだけで決まるので、インデントが入れ子構造の
    // すべて。フラグはどちらのタグで閉じるかだけを持つ。
    //
    // **番号は `<ol>` で出す。** ここが `<ul>` のままだと、書いた `1. 2. 3.`
    // が黒丸で出る ── 書いたものと読めるものが違うので、Markdown が壊れて
    // いるように見える。何番から始めるかは `list_item` が返す記号から取る。
    let mut open_lists: Vec<(usize, bool)> = Vec::new();

    // リスト項目は何かが終わらせるまで開いたままにする。より深いリストは、
    // 上の項目の*内側*に属するから。書いたそばから `<li>` を閉じると、入れ子の
    // `<ul>` が親の中ではなく隣に置かれる ── ブラウザはそれを受け入れたうえで、
    // 誤ったインデントで表示する。
    //
    // `li_open` の意味は「いちばん内側の開いているリストに、閉じていない項目が
    // ある」。深く入るときは意図的に開いたままにし、戻るときに閉じる。入れ子の
    // リストを閉じると、その親についても同じ問いが再び立つ ── 親の項目もまた
    // 閉じられていないので。
    let mut li_open = false;
    /// 開いているリストをすべて閉じる。リスト項目でないものは、すべてのリストを
    /// 終わらせる ── リストの後の段落は、そのリストの中ではない。
    fn close_all_lists(out: &mut String, open: &mut Vec<(usize, bool)>, li: &mut bool) {
        while let Some((_, ord)) = open.pop() {
            if *li {
                out.push_str("</li>\n");
            }
            out.push_str(if ord { "</ol>\n" } else { "</ul>\n" });
            *li = !open.is_empty();
        }
        *li = false;
    }

    fn close_lists_to(out: &mut String, open: &mut Vec<(usize, bool)>, li: &mut bool, indent: usize) {
        while open.last().is_some_and(|(d, _)| *d > indent) {
            if *li {
                out.push_str("</li>\n");
            }
            let (_, ord) = open.pop().unwrap();
            out.push_str(if ord { "</ol>\n" } else { "</ul>\n" });
            // このリストが入れ子になっていた側の項目は、まだ開いている。
            *li = !open.is_empty();
        }
        if *li && !open.is_empty() {
            out.push_str("</li>\n");
            *li = false;
        }
    }

    while i < lines.len() {
        if stamp {
            out.push('\u{1}');
            out.push_str(&i.to_string());
            out.push('\u{2}');
        }
        let raw = &lines[i];
        let t = raw.trim();

        // フェンスは閉じるまでの内容をそのまま取り込む。
        if let Some(lang) = fence_lang(raw) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            let mut body = String::new();
            i += 1;
            while i < lines.len() && fence_lang(&lines[i]).is_none() {
                body.push_str(&esc(&lines[i]));
                body.push('\n');
                i += 1;
            }
            i += 1; // the closing fence
            let class = if lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", esc(&lang))
            };
            out.push_str(&format!("<pre><code{class}>{body}</code></pre>\n"));
            continue;
        }

        if t.is_empty() {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            i += 1;
            continue;
        }

        if is_rule(raw) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            // **書いた形をそのまま憶える。** `---` `***` `___` はどれも
            // 水平線で、見た目も同じ ── けれどテキストに戻すときに丸めると、
            // 人の書いた行が書き換わる（同期先では差分になる）。
            out.push_str(&format!("<hr data-mark=\"{}\">\n", esc(t)));
            i += 1;
            continue;
        }

        if let Some((level, text)) = heading(raw) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            // アンカーを付ける。同じファイルの `[…](#usage)` に着地先ができる。
            // README のリンクはほとんどが自分自身か隣のファイル宛てで、どちらも
            // 辿れないプレビューは、その文書が指しているもののほとんどを開けない。
            //
            out.push_str(&format!(
                "<h{level} id=\"{}\">{}</h{level}>\n",
                slug(&text),
                inline_html(&text)
            ));
            i += 1;
            continue;
        }

        // 表 ── ヘッダー行、区切り行、そして行が続くかぎりの本体。
        if t.starts_with('|') && i + 1 < lines.len() && is_table_separator(&lines[i + 1]) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            let head = split_cells(raw);
            let aligns: Vec<Align> = split_cells(&lines[i + 1]).iter().map(|c| cell_align(c)).collect();
            let at = |n: usize| match aligns.get(n).copied().unwrap_or_default() {
                Align::Left => "",
                Align::Center => " style=\"text-align:center\"",
                Align::Right => " style=\"text-align:right\"",
            };
            // **区切り行は、元のテキストのまま保持させる。**
            //
            // `Align` は三つしか無く、`:---` と `---` はどちらも `Left` に
            // なる ── 見た目は同じでよいが、**テキストに戻すときに `:---` が
            // `---` になる**（人の書いた行が書き換わる）。長さ（`-----`）も
            // 同じ話。見え方は `style` が、戻し方はこちらが受け持つ。
            let seps = split_cells(&lines[i + 1]);
            let sep = |n: usize| {
                seps.get(n)
                    .map(|c| format!(" data-sep=\"{}\"", esc(c.trim())))
                    .unwrap_or_default()
            };
            out.push_str("<table>\n<thead><tr>");
            for (n, c) in head.iter().enumerate() {
                out.push_str(&format!("<th{}{}>{}</th>", at(n), sep(n), inline_html(c)));
            }
            out.push_str("</tr></thead>\n<tbody>\n");
            i += 2;
            while i < lines.len() && lines[i].trim().starts_with('|') {
                out.push_str("<tr>");
                for (n, c) in split_cells(&lines[i]).iter().enumerate() {
                    out.push_str(&format!("<td{}>{}</td>", at(n), inline_html(c)));
                }
                out.push_str("</tr>\n");
                i += 1;
            }
            out.push_str("</tbody>\n</table>\n");
            continue;
        }

        // 行そのものが画像なら、画像として出す。**どこからが画像かは
        // `note::lone_image` の 1 か所** ── iPhone が画像として表示する行をデスクトップ版が
        // テキストで出すと、同じノートが 2 つの見た目を持つ。
        if let Some(crate::note::Block::Image { alt, link }) = crate::note::lone_image(t) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            match safe_url(&link) {
                Some(src) => {
                    // **大きさは題の中に書く**（Marp と同じ・依頼 413）。
                    let (alt, size) = picture_size(&alt);
                    out.push_str(&format!(
                        "<img src=\"{src}\" alt=\"{}\"{size}>\n",
                        esc(&alt)
                    ));
                }
                // 表示できない参照先なら、書いてあったものをそのままテキストで。
                // **隠して失うより、出して残す。**
                None => out.push_str(&format!("<p>{}</p>\n", esc(t))),
            }
            i += 1;
            continue;
        }

        // **折りたたみ**（依頼 619・本人「`<summary>` をつかって文書を
        // 折りたたむことができるよね？ コードがめちゃくちゃ長くて見にくい」）。
        //
        // **記法は増やさない。** `<details>` は GitHub がそのまま畳む形で、
        // メモ帳で開いた人にも「畳んであるもの」と読める ── ambər だけの
        // 記号を作ると、同じノートがよそで壊れる。
        //
        // **行全体がタグのときだけ。** 本文の途中に `<details>` と書いた
        // だけで畳みはじめると、山括弧を書いた行が消える（注記と同じ筋）。
        //
        // 中身はふつうに組み直す（`render`）ので、畳んだ中に枠も表も図も
        // 入る ── 畳みたいのは、たいてい長い枠。
        if is_details_open(t) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            let open = t.contains(" open");
            i += 1;
            // 見出しは次の一行（`<summary>…</summary>`）。無くてもよい。
            let mut title = String::new();
            if i < lines.len() {
                let head = lines[i].trim();
                if let Some(rest) = head.strip_prefix("<summary>") {
                    if let Some(inner) = rest.strip_suffix("</summary>") {
                        title = inner.to_string();
                        i += 1;
                    }
                }
            }
            // **入れ子も数える。** 中の `</details>` で外が閉じると、
            // そこから下がぜんぶ畳みの外へ出る。
            let mut body = Vec::new();
            let mut depth = 1usize;
            while i < lines.len() {
                let q = lines[i].trim();
                if q == "</details>" {
                    depth -= 1;
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                    body.push(lines[i - 1].clone());
                    continue;
                }
                if is_details_open(q) {
                    depth += 1;
                }
                body.push(lines[i].clone());
                i += 1;
            }
            // **押すところが無い畳みは作らない。** 見出しを書かなかった人にも
            // 三角だけでなくテキストも出す ── 何が畳んであるのか、閉じた状態で分かる。
            let head = if title.trim().is_empty() {
                "詳しく".to_string()
            } else {
                inline_html(&title)
            };
            out.push_str(&format!(
                "<details{}><summary>{head}</summary>\n",
                if open { " open" } else { "" }
            ));
            out.push_str(&render(&body, false));
            out.push_str("</details>\n");
            continue;
        }

        // GitHub 風の注記。`> [!NOTE]` に続く引用を、色の付いた枠にする。
        //
        // **引用の中の一行目でしか始まらない。** 本文に `[!NOTE]` と書いた
        // だけで枠になると、角括弧を書いただけの行が消える。GitHub が
        // 読めるものと同じ五つだけを受ける ── 増やすと、ここでだけ見える
        // 記法になり、ノートが GitHub で壊れる。
        if let Some(kind) = alert_kind(t) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() {
                let q = lines[i].trim();
                let Some(rest) = q.strip_prefix('>') else { break };
                body.push(rest.trim_start().to_string());
                i += 1;
            }
            let name = match kind.as_str() {
                "note" => "備忘",
                "tip" => "ヒント",
                "important" => "重要",
                "warning" => "注意",
                _ => "警告",
            };
            out.push_str(&format!(
                "<div class=\"alert {kind}\"><p class=\"alert-h\">{name}</p>\n"
            ));
            out.push_str(&render(&body, false));
            out.push_str("</div>\n");
            continue;
        }

        if t.starts_with("> ") || t == ">" {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            let mut body = Vec::new();
            while i < lines.len() {
                let q = lines[i].trim();
                let Some(rest) = q.strip_prefix('>') else { break };
                body.push(rest.trim_start().to_string());
                i += 1;
            }
            out.push_str("<blockquote>\n");
            out.push_str(&render(&body, false));
            out.push_str("</blockquote>\n");
            continue;
        }

        if let Some((mark, text, indent)) = list_item(raw) {
            // `list_item` は記号をそのまま返す ── 黒丸なら "•"、番号なら "1."。
            let ord = mark != "•";
            // 1 から始まらない番号は `start` で渡す。書いた番号で出ないと、
            // 途中から続ける箇条書き（手順の続き）が毎回 1 に戻る。
            let open_tag = |o: bool| -> String {
                if !o {
                    return "<ul>\n".to_string();
                }
                let n: usize = mark.trim_end_matches(['.', ')']).parse().unwrap_or(1);
                if n == 1 {
                    "<ol>\n".to_string()
                } else {
                    format!("<ol start=\"{n}\">\n")
                }
            };
            if open_lists.last().is_some_and(|(d, _)| indent > *d) {
                // より深い階層 ── 親の項目は開いたままにし、このリストをその中に入れる。
                open_lists.push((indent, ord));
                out.push_str(&open_tag(ord));
            } else {
                close_lists_to(&mut out, &mut open_lists, &mut li_open, indent);
                // 同じ深さで記号が変わったら、別のリスト ── 黒丸の続きに
                // 番号を混ぜると、片方の記法がもう片方の見た目で出る。
                if open_lists.last().is_some_and(|(_, o)| *o != ord) {
                    close_all_lists(&mut out, &mut open_lists, &mut li_open);
                }
                if open_lists.is_empty() {
                    open_lists.push((indent, ord));
                    out.push_str(&open_tag(ord));
                }
            }
            // **行の記号を、そのまま保持する。** `- ` `* ` `+ ` はどれも同じ黒丸で、
            // `1. ` `1) ` はどれも番号 ── 見た目は同じだが、テキストに戻すときに
            // 丸めると人の書いた行が書き換わる。番号そのものも憶える
            // （`1. 1. 1.` と書いた一覧を `1. 2. 3.` に振り直さない）。
            let head = raw.trim_start();
            let mark_raw = &head[..head.len() - text.len()];
            let mark_at = format!(" data-mark=\"{}\"", esc(mark_raw));
            match task_item(&text) {
                // 元の行番号を一緒に持たせる。見えるのに押せないチェックボックスは、
                // 結局その行を自分で探させることになる ── そして `note::set_check` は
                // 行番号を取るので、デスクトップ版が動かすのに必要なのはこれだけ。
                //
                //
                // **`<button>` で出す。`<span>` ではない。** 押せるものは
                // 操作できるものとして名乗るべきで、そうでないと読み上げは
                // ただのテキストとして読み上げ、Tab では辿り着けず、キーボードだけの
                // 人には「無い」のと同じになる。操作できるチェックボックスが 1 つ出せない
                // だけで、ノートの半分が触れなくなる。
                Some((done, rest)) => {
                    // チェックの文字（`[x]` か `[X]`）も、そのまま保持する。
                    let box_at = text.trim_start();
                    let up = box_at.starts_with("[X]");
                    out.push_str(&format!(
                        "<li class=\"task\"{}{}><button type=\"button\" class=\"box\" data-line=\"{}\" aria-pressed=\"{}\">{}</button>{}",
                        mark_at,
                        if up { " data-box=\"X\"" } else { "" },
                        i,
                        done,
                        if done { "☑" } else { "☐" },
                        inline_html(&rest),
                    ));
                }
                None => out.push_str(&format!("<li{}>{}", mark_at, inline_html(&text))),
            }
            li_open = true;
            i += 1;
            continue;
        }

        // 段落 ── この行と、それに続く「ほかの何かではない」行。
        // else.
        //
        // **改行は、改行として描く。** GitHub（CommonMark）は段落の中の
        // 一つの改行を空白にして繋ぐが、amber はそうしない ── 2026-09-08 に
        // 決めた（`PAPER.ja.md` 六章）。
        //
        // 理由は見た目ではなく、**文字が消えるから**。「表示」画面は描画した
        // 結果からテキストに戻す（`paperToMd`）ので、空白でつないだ段落は**空白で
        // つながれたまま保存される** ── 3 行で書いた段落が、画面で 1 文字
        // 打った瞬間に一行になる。同期しているフォルダなら、それが全部
        // むこうへ差分として飛ぶ（実物で踏んだ）。
        //
        // **ファイルは一文字も変わらない**（`\n` のまま）── 描き方だけの話。
        // GitHub へ持っていくと繋がって見えるが、日本語の間に半角の空白が
        // 入る今の描き方は、記号を知らない人には「打った改行が消えた」に
        // しか見えない。行末の空白二つと `\` は、これまで通り改行
        // （`inline_html` が見る）。
        //
        // **行頭の空白は削らない。** 全角空白（`　`）はインデントで、人が
        // 打ったもの ── `trim()` は全角空白も削るので、`trim_matches` で
        // 半角の空白と制御文字だけを落とす。行末は落としてよい（行末の
        // 空白二つは `inline_html` が改行に直したあとなので、ここでは
        // もう意味を持たない）。
        close_all_lists(&mut out, &mut open_lists, &mut li_open);
        let mut para = Vec::new();
        while i < lines.len() {
            let p = &lines[i];
            let pt = p.trim();
            if pt.is_empty()
                || heading(p).is_some()
                || is_rule(p)
                || fence_lang(p).is_some()
                || list_item(p).is_some()
                || pt.starts_with('|')
                || pt.starts_with('>')
                // **折りたたみのタグで段落を切る**（依頼 619）。切らないと
                // `もとの一行。` の次の行に `<details>` と書いただけで
                // 段落に取り込まれ、山括弧が文字として表示される（実機で出た）。
                // 囲み（``` ）が `fence_lang` で切れているのと同じ筋。
                || is_details_open(pt)
                || pt == "</details>"
            {
                break;
            }
            para.push(mark_break(p));
            i += 1;
        }
        out.push_str(&format!("<p>{}</p>\n", breaks(&inline_html(&para.join("\n")))));
    }
    close_all_lists(&mut out, &mut open_lists, &mut li_open);
    if stamp {
        return stamps(&out, lines.len());
    }
    out
}


/// **画像の大きさを、題の中の指示から読む**（依頼 413）。
///
/// `![width:200px](猫.png)` ── Marp と同じ書き方にした。新しい記法を
/// 作らないのは、**ここで作った書き方は他のどこでも通じない**から:
/// GitHub でも VS Code でも、この行はただの画像に見えるだけで壊れない。
///
/// 読むのは `width:` `w:` `height:` `h:` の四つ。**残りは題のまま**返す
/// ので、`![猫 w:200px](…)` は「猫」という説明の付いた 200px の画像になる。
///
/// 長さは**数値と単位だけ**しか通さない ── `style` に人の書いた文字列を
/// そのまま入れる経路になるので、`}` や `;` の混ざったものは指定と見なさず、
/// 題の一部として置いておく（見えなくなるより、見えるほうがよい）。
fn picture_size(alt: &str) -> (String, String) {
    let mut words = Vec::new();
    let (mut w, mut h) = (None, None);
    for word in alt.split(' ') {
        let got = match word.split_once(':') {
            Some(("width", v) | ("w", v)) => length(v).map(|v| (&mut w, v)),
            Some(("height", v) | ("h", v)) => length(v).map(|v| (&mut h, v)),
            _ => None,
        };
        match got {
            Some((slot, v)) => *slot = Some(v),
            None => words.push(word),
        }
    }
    // **片方だけ言われたら、もう片方は釣り合わせる。** `width` だけ指して
    // 高さを CSS のままにすると、`height:auto` を持たない土台で画像が歪む。
    let css = match (w, h) {
        (None, None) => String::new(),
        (Some(w), None) => format!("width:{w};height:auto"),
        (None, Some(h)) => format!("height:{h};width:auto"),
        (Some(w), Some(h)) => format!("width:{w};height:{h}"),
    };
    let size = if css.is_empty() { String::new() } else { format!(" style=\"{css}\"") };
    (words.join(" ").trim().to_string(), size)
}

/// `200` `200px` `50%` `10em` ── 数と、知っている単位だけ。
fn length(v: &str) -> Option<String> {
    let (num, unit) = match v.find(|c: char| !c.is_ascii_digit() && c != '.') {
        Some(at) => (&v[..at], &v[at..]),
        None => (v, ""),
    };
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    match unit {
        // 単位を書かなければ px（Marp と同じ）。
        "" => Some(format!("{num}px")),
        "px" | "%" | "em" | "rem" | "vw" | "vh" => Some(format!("{num}{unit}")),
        _ => None,
    }
}

/// **画像の大きさを、押して選べるようにするための書き換え**（依頼 420）。
///
/// 記法を覚えていない人が、画像を押して「小さめ」を選ぶと、amber が
/// `![猫 w:200px](…)` と**書いておく** ── 覚えている人が打つのと同じ書き方で。
/// 2 つの経路が同じところへ行き着くので、片方で付けたサイズをもう片方で直せる。
///
/// `width` が `None` なら指示を外す（はばいっぱいに戻す）。
/// **代替テキストは動かさない** ── 書いた人の言葉なので、順番も含めてそのまま。
pub fn set_picture_size(line: &str, width: Option<&str>) -> String {
    let Some(crate::note::Block::Image { alt, link }) = crate::note::lone_image(line.trim()) else {
        // 画像の行でないなら、触らない ── 読めないものを書き換えない。
        return line.to_string();
    };
    let mut words: Vec<&str> = alt
        .split(' ')
        .filter(|w| !matches!(w.split_once(':'),
            Some(("width" | "w" | "height" | "h", v)) if length(v).is_some()))
        .filter(|w| !w.is_empty())
        .collect();
    let hold;
    if let Some(w) = width {
        hold = format!("w:{w}");
        // **サイズ指定は後ろに置く。** 代替テキストが先に読めるほうが、コード画面を
        // 開いた人に「何の画像か」が先に届く。
        words.push(&hold);
    }
    format!("![{}]({link})", words.join(" "))
}


#[cfg(test)]
mod tests {
    #[test]
    fn 画像サイズの設定は_人が打つのと同じ形で書く() {
        let one = |line: &str, w: Option<&str>| super::set_picture_size(line, w);

        // 付ける。
        assert_eq!(one("![猫](a.png)", Some("200px")), "![猫 w:200px](a.png)");
        // 付け直す ── 二つ並べない。
        assert_eq!(one("![猫 w:200px](a.png)", Some("400px")), "![猫 w:400px](a.png)");
        // 外す ── 説明は残る。
        assert_eq!(one("![猫 w:200px](a.png)", None), "![猫](a.png)");
        // 説明が無くても壊れない。
        assert_eq!(one("![](a.png)", Some("200px")), "![w:200px](a.png)");
        assert_eq!(one("![w:200px](a.png)", None), "![](a.png)");
        // 縦の指示も一緒に落ちる（大きさは一か所で決める）。
        assert_eq!(one("![猫 h:80px w:1](a.png)", Some("200px")), "![猫 w:200px](a.png)");
        // **画像の行でないものは、触らない。**
        assert_eq!(one("ただの本文", Some("200px")), "ただの本文");
        assert_eq!(one("# 見出し", None), "# 見出し");
        // 書いた文字列を、もう一度読み取れる（往復する）。
        let out = super::to_html(&[one("![猫](a.png)", Some("200px"))]);
        assert!(out.contains(r#"style="width:200px;height:auto""#), "{out}");
        assert!(out.contains(r#"alt="猫""#), "{out}");
    }

    #[test]
    fn 画像サイズは_marp_の書き方も読み_言葉を残す() {
        let one = |md: &str| super::to_html(&[md.to_string()]);

        let out = one("![width:200px](猫.png)");
        assert!(out.contains(r#"style="width:200px;height:auto""#), "{out}");
        assert!(out.contains(r#"alt="""#), "題まで残っている: {out}");

        // 短い書き方と、説明の同居。
        let out = one("![猫 w:200](猫.png)");
        assert!(out.contains(r#"style="width:200px;height:auto""#), "{out}");
        assert!(out.contains(r#"alt="猫""#), "説明が落ちた: {out}");

        // 縦横そろえて言われたら、そのまま。
        let out = one("![w:200px h:80%](猫.png)");
        assert!(out.contains(r#"style="width:200px;height:80%""#), "{out}");

        // 大きさを言われていない画像は、いままでどおり。
        let out = one("![猫](猫.png)");
        assert!(!out.contains("style="), "{out}");
        assert!(out.contains(r#"alt="猫""#), "{out}");

        // **サイズ指定に見えないものは、指定として扱わない。** `style` に人の書いた文字列を
        // そのまま入れる経路を作らない。
        for bad in ["w:200}", "w:red", "w:", "w:1;color:red", "width:2em;x"] {
            let out = one(&format!("![{bad}](猫.png)"));
            assert!(!out.contains("style="), "{bad} が通った: {out}");
            assert!(out.contains("alt="), "{bad}: {out}");
        }
    }


    /// 描画結果を確かめるときは、**行マーカーを外して見る。**
    ///
    /// `data-line` / `data-span` は「元の何行目か」という覚書で、組み方の
    /// 一部ではない。文字列でそのまま比べると、マーカーを足したときに描画の
    /// テストが全部落ちる ── 落ちたのは組み方ではないのに。
    ///
    /// `super::to_html` を覆っているので、この段のテストは自動でこちらを
    /// 通る。マーカー自体を確かめるテストだけ `super::to_html` を名指しする。
    fn to_html(lines: &[String]) -> String {
        let mut out = super::to_html(lines);
        for key in [" data-line=\"", " data-span=\""] {
            while let Some(at) = out.find(key) {
                let Some(end) = out[at + key.len()..].find('"') else { break };
                out.replace_range(at..at + key.len() + end + 1, "");
            }
        }
        out
    }

    #[test]
    fn 一行に色付きの語が二つあっても両方残る() {
        // 一度これで壊れた: `find` はバイトを数え、走査は文字を数えていたので、
        // 日本語を挟むと span の**先まで**飛び越えて、次の span の途中から
        // 文字が表示されていた。
        let line = "ふつうの字と<span style=\"color:#D9822B\">だいだいの字</span>と、\
<span style=\"color:#0E93A8\">シアン</span>。";
        let out = to_html(&lines(line));
        assert!(out.contains("<span style=\"color:#d9822b\">だいだいの字</span>"), "{out}");
        assert!(out.contains("<span style=\"color:#0e93a8\">シアン</span>"), "{out}");
        assert!(!out.contains("e=&quot;color"), "span の途中から字が出ている: {out}");
        assert!(out.contains("と、"), "間の字が食われた: {out}");
    }

    #[test]
    fn 太字と斜体の同時指定は_両方として読む() {
        // 同じノートが、デスクトップ版と iPhone で違って見えていた（原因は `inline`）。
        for line in ["***両方***", "___両方___"] {
            let out = to_html(&lines(line));
            assert!(out.contains("<strong><em>両方</em></strong>"), "{line}: {out}");
            assert!(!out.contains("*両方"), "印が字として出ている {line}: {out}");
            assert!(!out.contains("_両方"), "印が字として出ている {line}: {out}");
        }
        assert_eq!(inline("***両方***"), vec![Inline::BoldItalic("両方".into())]);
    }

    #[test]
    fn 閉じない三連記号は_元の形に戻す() {
        // 「3 つ並んでいたら必ず重ねがけ」にすると、閉じていない記号のある行が
        // 今日と違う形に読まれる ── 直したつもりの隣で、触っていない行が変わる。
        let out = to_html(&lines("***閉じていない**"));
        assert!(out.contains("<strong>*閉じていない</strong>"), "{out}");
        // 二つ並んだ太字は、重ねがけではない。
        assert_eq!(
            inline("**太****い**"),
            vec![Inline::Bold("太".into()), Inline::Bold("い".into())],
        );
        assert_eq!(inline("2 * 3 = 6"), vec![Inline::Text("2 * 3 = 6".into())]);
        assert_eq!(inline("`a***b***c`"), vec![Inline::Code("a***b***c".into())]);
        // **閉じは三つ数える。** 中の `**` で閉じたことにすると、そこから
        // 3 文字飛ばすので `b` が消えたまま出る ── 見た目ではなく、文字が落ちる。
        assert_eq!(inline("***a**b***"), vec![Inline::BoldItalic("a**b".into())]);
    }

    #[test]
    fn front_matter_は自己説明であって_本文ではない() {
        let out = to_html(&lines("---\ntitle: 週報\ntags: [仕事]\n---\n\n# 見出し\n"));
        assert!(!out.contains("title:"), "{out}");
        assert!(out.contains("見出し"), "{out}");
        // 先頭の `---` が前書きでないなら、これまで通り区切り線。
        let rule = to_html(&lines("---\n\n本文。\n"));
        assert!(rule.contains("<hr"), "{rule}");
    }

    #[test]
    fn escape_を通っても色だけは残り_ほかは残らない() {
        let out = to_html(&lines("ふつうと<span style=\"color:#0E93A8\">シアン</span>。"));
        assert!(out.contains("<span style=\"color:#0e93a8\">シアン</span>"), "{out}");
        // 他の HTML は、これまでどおり文字として出す。
        let out = to_html(&lines("<span onclick=\"x\">あ</span>"));
        assert!(out.contains("&lt;span"), "{out}");
        assert!(!out.contains("onclick=\"x\""), "{out}");
        // 色以外の span も文字のまま。
        let out = to_html(&lines("<span class=\"x\">あ</span>"));
        assert!(out.contains("&lt;span"), "{out}");
    }
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn インライン記号の解釈は一度だけ() {
        assert_eq!(
            inline("a **b** c `d` [e](http://x) ~~f~~"),
            vec![
                Inline::Text("a ".into()),
                Inline::Bold("b".into()),
                Inline::Text(" c ".into()),
                Inline::Code("d".into()),
                Inline::Text(" ".into()),
                Inline::Link { text: "e".into(), url: "http://x".into() },
                Inline::Text(" ".into()),
                Inline::Strike("f".into()),
            ]
        );
    }

    /// **裸の URL は、クリックできるがファイルの内容は変わらない**（依頼 606・本人が決めた）。
    ///
    /// GitHub と同じで、書いた `https://…` はそのまま押して開ける。ただし
    /// **ファイルの内容は 1 文字も変えない** ── `[url](url)` に書き換えると、
    /// 打っていない記号が増え、ほかのアプリで開いた人には別の文字列に見える。
    /// 書き戻す側（`inlineToMd`）はその目印（`data-bare`）を見る。
    #[test]
    fn 裸の_url_は押せるが_字は変わらない() {
        let got = super::to_html(&["見て https://example.com/a 。".to_string()]);
        assert!(got.contains("data-bare=\"1\""), "裸の印が要る: {got}");
        assert!(got.contains("href=\"https://example.com/a\""), "{got}");
        // **句点まで飲み込まない。** `…/a。` は開けない行き先になる。
        assert!(!got.contains("/a。"), "句点は URL の外: {got}");
        // **英文の終止符も、文のほう。** 空白で切れないので、末尾で落とす。
        let dot = super::to_html(&["see https://example.com/a. next".to_string()]);
        assert!(dot.contains("href=\"https://example.com/a\""), "終止符は URL の外: {dot}");
        let comma = super::to_html(&["a https://example.com/b, b".to_string()]);
        assert!(comma.contains("href=\"https://example.com/b\""), "読点も外: {comma}");

        // `[テキスト](url)` は今までどおり ── 裸 URL の目印は付かない。
        let named = super::to_html(&["[例](https://example.com/a)".to_string()]);
        assert!(!named.contains("data-bare"), "名前付きは裸ではない: {named}");

        // 括弧の中の url を二度拾わない。
        let once = inline("[例](https://example.com/a)");
        assert_eq!(once.len(), 1, "{once:?}");

        // **直前が文字なら、開始位置ではない。**
        let glued = inline("xhttps://example.com/a");
        assert!(
            !glued.iter().any(|i| matches!(i, Inline::Bare(_))),
            "字にくっついた綴りは URL ではない: {glued:?}"
        );

        // 閉じ括弧・鍵括弧で止まる（日本語の文の形）。
        for line in ["（https://example.com/a）", "「https://example.com/a」"] {
            let got = super::to_html(&[line.to_string()]);
            assert!(got.contains("href=\"https://example.com/a\""), "{line}: {got}");
        }

        // `javascript:` は通さない（`safe_url` の約束はそのまま）。
        let bad = inline("javascript:alert(1)");
        assert!(!bad.iter().any(|i| matches!(i, Inline::Bare(_))), "{bad:?}");
    }

    /// **折りたたみ**（依頼 619・本人「`<summary>` をつかって文書を
    /// 折りたたむことができるよね？ コードがめちゃくちゃ長くて見にくい」）。
    ///
    /// 記法は `<details>` ── GitHub がそのまま畳む形で、メモ帳で開いた人にも
    /// 「畳んであるもの」と読める。**ambər だけの記号は作らない。**
    #[test]
    fn 折りたたみは_details_で出す() {
        let md = "<details>\n<summary>ながい **コード**</summary>\n\n                  ```js\nconst a = 1;\n```\n\n</details>\n";
        let got = to_html(&lines(md));
        assert!(got.contains("<details"), "畳みにならない: {got}");
        assert!(got.contains("<summary>ながい <strong>コード</strong></summary>"),
                "見出しの飾りが出ない: {got}");
        // **中はふつうに組み直す** ── 畳みたいのは、たいてい長い枠。
        assert!(got.contains("<pre") && got.contains("const a = 1;"), "中の枠が出ない: {got}");
        // 山括弧が文字として出ていないこと（以前は `&lt;details&gt;` と出ていた）。
        assert!(!got.contains("&lt;details"), "字のまま出ている: {got}");

        // 見出しを書かなかった人にも、押すところがあること。
        let bare = to_html(&lines("<details>\n\n中身\n\n</details>\n"));
        assert!(bare.contains("<summary>詳しく</summary>"), "{bare}");

        // `open` と書いてあれば、初めから開いた姿で。
        let open = to_html(&lines("<details open>\n\n中身\n\n</details>\n"));
        assert!(open.contains("<details open"), "{open}");

        // **入れ子も数える。** 中の `</details>` で外が閉じると、
        // そこから下がぜんぶ畳みの外へ出る。
        let nest = to_html(&lines(
            "<details>\n<summary>そと</summary>\n\n<details>\n<summary>なか</summary>\n\n             おく\n\n</details>\n\n</details>\n\nそと側の段\n",
        ));
        assert_eq!(nest.matches("<details").count(), 2, "{nest}");
        assert!(nest.contains("<p data-line") || nest.contains("そと側の段"), "{nest}");
        // 外の畳みの中に、内の畳みが入っていること。
        let outer = &nest[nest.find("<details").unwrap()..];
        assert!(outer.find("なか").unwrap() < outer.find("</details>").unwrap(), "{nest}");

        // **行全体がタグのときだけ。** 本文の途中に書いた山括弧は文字のまま。
        let mid = to_html(&lines("これは <details> という札です\n"));
        assert!(mid.contains("&lt;details&gt;"), "字のまま出ない: {mid}");

        // **段落の次の行でも畳みになる**（段落に飲み込まれない）。
        let after = to_html(&lines("まえの段。\n<details>\n\n中身\n\n</details>\n"));
        assert!(after.contains("<p") && after.contains("<details"), "飲み込まれた: {after}");
    }

    #[test]
    fn 閉じていない記号はテキストのまま() {
        // はぐれたアスタリスクはアスタリスクのまま。閉じない強調の開始として
        // 扱うと、その行の残り全部を飲み込む。
        assert_eq!(inline("2 * 3 = 6"), vec![Inline::Text("2 * 3 = 6".into())]);
    }

    #[test]
    fn コードが強調より優先される() {
        // バッククォートの中の `*` はアスタリスク。`*` をバッククォートで囲む
        // 理由がまさにそれ。
        assert_eq!(inline("`a*b*c`"), vec![Inline::Code("a*b*c".into())]);
    }

    #[test]
    fn 本文中の_html_は表示され_実行されない() {
        let html = to_html(&lines("<script>alert(1)</script>"));
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(!html.contains("<script>"), "{html}");
    }

    #[test]
    fn 押せる升は_押せるものとして名乗る() {
        // `<span>` で出していた頃、読み上げはただのテキストとして読み、Tab では
        // 辿り着けなかった ── 操作できるチェックボックスが 1 つ出せないだけで、ノートの
        // 半分が触れなくなる。
        let out = super::to_html(&lines("- [ ] やること\n- [x] 済んだ\n"));
        assert!(out.contains("<button type=\"button\" class=\"box\""), "升が押せない: {out}");
        assert!(out.contains("aria-pressed=\"false\""), "入り切りが伝わらない: {out}");
        assert!(out.contains("aria-pressed=\"true\""), "入り切りが伝わらない: {out}");
        // 行番号は残す ── `note::set_check` が取るのはこれ。
        assert!(out.contains("data-line=\"0\"") && out.contains("data-line=\"1\""), "{out}");
    }

    #[test]
    fn github_の注記は五つだけ受ける() {
        let out = to_html(&lines("> [!NOTE]\n> 覚えておくこと。\n"));
        assert!(out.contains("<div class=\"alert note\">"), "枠にならない: {out}");
        assert!(out.contains("覚えておくこと。"), "中身が消えた: {out}");
        let out = to_html(&lines("> [!WARNING]\n> 気をつける。\n"));
        assert!(out.contains("alert warning"), "{out}");

        // ふつうの引用は、ふつうの引用のまま。
        let out = to_html(&lines("> ふつう\n"));
        assert!(out.contains("<blockquote>") && !out.contains("alert"), "{out}");
        // **引用の中の一行目でしか始まらない。** 本文に書いた角括弧が
        // 消えると、書いたものが読めるものと違う。
        let out = to_html(&lines("本文に [!NOTE] と書いた\n"));
        assert!(out.contains("[!NOTE]"), "本文の角括弧が消えた: {out}");
        // 知らない種類は、ふつうの引用。ここでだけ見える記法を増やさない。
        let out = to_html(&lines("> [!SPICY]\n> から\n"));
        assert!(!out.contains("alert"), "知らない種類が枠になった: {out}");
    }

    #[test]
    fn かたまりは元の行を持って出る() {
        // 表示画面で直接編集するのに要る ── クリックしたブロックの元のテキストが
        // 取れなければ、表示画面は閲覧専用のままになる。
        let out = super::to_html(&lines("---\ntitle: t\n---\n\n# 題\n\n本文。\n続き。\n"));
        assert!(out.contains("<h1 id=\"題\" data-line=\"4\" data-span=\"1\">"), "{out}");
        // 折り返した段落は**一つのかたまりで二行ぶん**。
        assert!(out.contains("<p data-line=\"6\" data-span=\"2\">"), "{out}");

        // 枠は閉じるまでが一つ。
        let out = super::to_html(&lines("```\na\nb\n```\nあと\n"));
        assert!(out.contains("<pre data-line=\"0\" data-span=\"4\">"), "{out}");

        // **終了タグには挿入しない。** 箇条書きの 2 つ目は前の項目を閉じてから
        // 始まるので、素朴に「次の `>`」を探すと `</li data-line=…>` に
        // なる（一度そうなった）。
        let out = super::to_html(&lines("- あ\n- い\n"));
        assert!(!out.contains("</li data-line"), "閉じ札に差さっている: {out}");
        assert!(out.contains("data-line=\"1\""), "二つ目に差さっていない: {out}");

        // 引用の中で数え直さない ── 中の行番号はファイルの行番号ではない。
        let out = super::to_html(&lines("> 引用\n> の中\n"));
        assert_eq!(out.matches("data-line").count(), 1, "中まで差さっている: {out}");
    }

    #[test]
    fn 番号つきは番号で出る() {
        // 書いた `1. 2.` が黒丸で出ると、書いたものと読めるものが違う。
        let out = to_html(&lines("1. 一つ\n2. 二つ\n"));
        assert!(out.contains("<ol>"), "番号つきが <ol> で出ていない: {out}");
        assert!(!out.contains("<ul>"), "黒丸が混ざっている: {out}");
        assert!(out.contains("</ol>"), "閉じていない: {out}");

        // 途中から続ける手順は、書いた番号から始まる。
        let out = to_html(&lines("3. 三つめから\n"));
        assert!(out.contains("<ol start=\"3\">"), "始まりが渡っていない: {out}");

        // 黒丸は黒丸のまま。
        let out = to_html(&lines("- 黒丸\n"));
        assert!(out.contains("<ul>") && !out.contains("<ol"), "{out}");

        // 同じ深さで記号が変われば、別のリスト。
        let out = to_html(&lines("- 黒丸\n1. 番号\n"));
        assert!(out.contains("</ul>") && out.contains("<ol>"), "混ざっている: {out}");
    }

    #[test]
    fn 行そのものが画像なら画像で出る() {
        let out = to_html(&lines("![猫](cat.jpg)\n"));
        assert!(out.contains("<img src=\"cat.jpg\" alt=\"猫\">"), "画像になっていない: {out}");
        assert!(!out.contains("!<a"), "`!` が字のまま残っている: {out}");

        // 表示できない参照先は、隠さずに文字で残す。
        let out = to_html(&lines("![だめ](javascript:alert(1))\n"));
        assert!(!out.contains("<img"), "危ない画像が出ている: {out}");
        assert!(!out.contains("javascript:alert(1)</"), "そのまま href になっている: {out}");
        assert!(out.contains("だめ"), "書いてあったものが消えている: {out}");
    }

    #[test]
    fn javascript_のリンクはリンクにしない() {
        // 最も古い手口で、README はどこかから来たファイル。テキストは表示される
        // が、どこへも飛ばないだけ。
        let html = to_html(&lines("[click](javascript:alert(1))"));
        assert!(html.contains("click"), "{html}");
        assert!(!html.contains("<a "), "{html}");
    }

    #[test]
    fn 相対リンクは動く() {
        let html = to_html(&lines("[readme](docs/README.md)"));
        assert!(html.contains(r#"<a href="docs/README.md">readme</a>"#), "{html}");
    }

    /// 見出しの深さを、一打で。
    #[test]
    fn 見出しの深さを_直に決められる() {
        use super::marks::level;
        assert_eq!(level("ためし", 1), "# ためし");
        assert_eq!(level("# ためし", 3), "### ためし");
        assert_eq!(level("### ためし", 1), "# ためし");
        // 0 は見出しをやめる ── 何段目からでも。
        assert_eq!(level("###### ためし", 0), "ためし");
        // インデントは残す（箇条書きの中の見出しを潰さない）。
        assert_eq!(level("  ## 中", 1), "  # 中");
        // 六より深い見出しは無い。
        assert_eq!(level("あ", 9), "###### あ");
    }

    #[test]
    fn 表は寄せ方を保つ() {
        let html = to_html(&lines("| a | b |\n| :- | --: |\n| 1 | 2 |"));
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains(r#"text-align:right"#), "{html}");
        // **区切り行は、元のテキストのまま保持させる。** `Align` は 3 種類しか無く、
        // `:-` と `-` はどちらも `Left` になる ── 見え方は同じでよいが、
        // テキストに戻すときに `:-` が `---` になると、人の書いた行が書き換わる。
        assert!(html.contains(r#"data-sep=":-""#), "{html}");
        assert!(html.contains(r#"data-sep="--:""#), "{html}");
    }

    #[test]
    fn 入れ子のリストは順に閉じる() {
        let html = to_html(&lines("- one\n  - deep\n- two"));
        assert_eq!(html.matches("<ul>").count(), 2, "{html}");
        assert_eq!(html.matches("</ul>").count(), 2, "{html}");
        // **`<li` で数える。** 記号を保持するようになって `<li data-mark=…>`
        // になったので、`<li>` で数えると 0 になる（振る舞いは変わって
        // いない）。`</li>` は `<` の次が `/` なので、これには当たらない。
        assert_eq!(html.matches("<li").count(), 3, "{html}");
        assert_eq!(html.matches("</li>").count(), 3, "{html}");
    }

    #[test]
    fn 入れ子のリストは親の項目の中に入る() {
        // 最初の版が出していたのは `<li>one</li><ul>…</ul>` ── ブラウザは受け入れる
        // が、入れ子が違うかのようにインデントされる。
        // not there.
        let html = to_html(&lines("- one\n  - deep"));
        let li = html.find(">one").unwrap();
        let ul = html[li..].find("<ul>").unwrap();
        let close = html[li..].find("</li>").unwrap();
        assert!(ul < close, "nested <ul> must come before its parent's </li>\n{html}");
    }

    #[test]
    fn コードブロックはそのまま出す() {
        let html = to_html(&lines("```rust\nlet x = *p;\n```"));
        assert!(html.contains(r#"<code class="language-rust">"#), "{html}");
        assert!(html.contains("let x = *p;"), "{html}");
        // 通りすがりに強調へ変えたりはしない。
        assert!(!html.contains("<em>"), "{html}");
    }

    #[test]
    fn リストは次のものの前で閉じる() {
        // 最初の版はいちばん外側のリストの記録を、`</ul>` を出さずに捨てていた。
        // そのためリストの後の段落がリストの中に入り、リストのある文書すべてで
        // ずっとインデントされたままになっていた。
        let html = to_html(&lines("- one\n  - deep\n- two\n\npara"));
        assert_eq!(html.matches("<ul>").count(), html.matches("</ul>").count(), "{html}");
        assert!(html.find("</ul>").unwrap() < html.find("<p>para").unwrap(), "{html}");
    }

    #[test]
    fn 改行で折り返した段落は一つの段落() {
        let html = to_html(&lines("one\ntwo\n\nthree"));
        assert_eq!(html.matches("<p>").count(), 2, "{html}");
        // **段落は一つ。ただし、改行は改行として描く**（2026-09-08 に決めた）。
        // 以前は `one two` と空白でつないでいた ── 「表示」画面はこの結果から
        // テキストに戻すので、**つないだまま保存され、3 行の段落が 1 行になった**。
        assert!(html.contains("<p>one<br>two</p>"), "{html}");
    }

    #[test]
    fn 行末の改行の印は_そのまま憶える() {
        // 空白 2 つと `\` は、どちらも「ここで改行」のマーク。amber は改行を
        // そのまま描くので**見た目は同じ**だが、人が打った文字なので、
        // テキストに戻すときにそのまま復元できるように保持しておく。
        let two = to_html(&lines("one  \ntwo"));
        assert!(two.contains(r#"<br data-hard="  ">"#), "{two}");
        let slash = to_html(&lines("one\\\ntwo"));
        assert!(slash.contains(r#"<br data-hard="\">"#), "{slash}");
        // マークの無い改行は、ただの `<br>`。
        let bare = to_html(&lines("one\ntwo"));
        assert!(bare.contains("<br>") && !bare.contains("data-hard"), "{bare}");
    }

    #[test]
    fn 行頭の全角空白は_落とさない() {
        // インデントは人が打った文字。`trim()` は全角空白も削るので、削るものを
        // 半角の空白と tab に絞ってある。
        let html = to_html(&lines("　字下げた段落。"));
        assert!(html.contains("<p>　字下げた段落。</p>"), "{html}");
    }

    #[test]
    fn 波線の枠も_枠として組む() {
        // `~~~` を知らないと、中のテキストが段落として整形される ── `~~` が
        // 取り消し線に読まれ、**コードが Markdown として解釈される**。
        let html = to_html(&lines("~~~\n- これは点ではない\n~~~"));
        assert!(html.contains("<pre><code>"), "{html}");
        assert!(!html.contains("<del>"), "{html}");
        assert!(!html.contains("<li>"), "{html}");
    }

    #[test]
    fn チェックボックスは印が付き_元の行番号を持つ() {
        let html = super::to_html(&lines("- [x] done\n- [ ] not"));
        assert!(html.contains("☑"), "{html}");
        assert!(html.contains("☐"), "{html}");
        // 押せるようにするのに要るのはこれだけ ── `note::set_check` は
        // 行番号を取る。前書きの分もちゃんと数える。
        assert!(html.contains("data-line=\"0\""), "{html}");
        assert!(html.contains("data-line=\"1\""), "{html}");
        let with_front = super::to_html(&lines("---\ntitle: x\n---\n\n- [ ] a\n"));
        assert!(with_front.contains("data-line=\"4\""), "{with_front}");
    }

    #[test]
    fn 見出しにはリンク先のアンカーが付く() {
        assert_eq!(slug("Usage"), "usage");
        assert_eq!(slug("Getting started!"), "getting-started");
        // 空白 2 つはハイフン 2 つになり、その間で落ちたダッシュも同様。
        // GitHub の slugger がそうしていて、README のリンクはその結果に対して
        // 書かれている。
        assert_eq!(slug("v1.2 — notes (draft)"), "v12--notes-draft");
        // 日本語は残す。落とすと、日本語の文書では見出しが全部同じ空のアンカーに
        // 潰れてしまう。
        assert_eq!(slug("使い方"), "使い方");
        assert_eq!(slug("  trailing  "), "trailing", "the ends are trimmed first");
        assert_eq!(slug("###"), "");

        let html = to_html(&["# 使い方".to_string(), "## Getting started".to_string()]);
        assert!(html.contains("<h1 id=\"使い方\">"), "{html}");
        assert!(html.contains("<h2 id=\"getting-started\">"), "{html}");
    }
}
