//! A SharePoint address, turned into a path Windows can open.
//!
//! **cian does not authenticate to SharePoint, and does not need to.** Windows
//! mounts a library over WebDAV through its WebClient service, so
//!
//!     https://jri.sharepoint.com/sites/Team/Shared Documents/勤怠.xlsx
//!     \\jri.sharepoint.com@SSL\DavWWWRoot\sites\Team\Shared Documents\勤怠.xlsx
//!
//! are the same place, and the second is *a path* — the panes, the editor,
//! grep and a notes folder all work on it unchanged. Signing in is the
//! browser's job, done once per session (see [`hint`]).
//!
//! **This is crmaine's, ported rather than reinvented** (`todo_scan.py`:
//! `sharepoint_to_unc`, `_sp_split`, `_sp_bad_url`, `_sp_hint`). Every branch
//! below is a shape somebody actually pasted, and the knowledge of which
//! shapes carry a location and which do not was expensive. Writing it again
//! from the idea would have reproduced the idea and none of the shapes.

/// `https://host/rest` → `(host, rest)`. Not a SharePoint URL → `("", "")`.
///
/// Four shapes get pasted, and the location lives somewhere different in each:
///
/// ```text
/// /sites/T/Docs/a.xlsx                        the path itself
/// /:x:/r/sites/T/Docs/a.xlsx?d=w…             prefixed — the `:` must go
/// /sites/T/Docs/Forms/AllItems.aspx?id=…      the location is the query's id
/// /sites/T/Docs/Forms/AllItems.aspx           the library's own front door
/// ```
///
/// The third is **what the address bar holds when you open a folder**, which
/// is what people copy. Miss it and you get `…\Forms\AllItems.aspx`, a place
/// that does not exist.
fn split(url: &str) -> (String, String) {
    let t = url.trim();
    let Some(rest) = t.strip_prefix("https://").or_else(|| t.strip_prefix("HTTPS://")) else {
        return (String::new(), String::new());
    };
    let Some(slash) = rest.find('/') else { return (String::new(), String::new()) };
    let host = rest[..slash].to_string();
    let after = &rest[slash + 1..];
    let (body, query) = match after.find('?') {
        Some(i) => (&after[..i], &after[i + 1..]),
        None => (after, ""),
    };
    let body = body.split('#').next().unwrap_or(body);

    // The query wins when it names a place: a folder opened in the browser
    // puts the real path in `id`, and the visible path is a view page.
    for (k, v) in query.split('&').filter_map(|kv| kv.split_once('=')) {
        let k = k.to_ascii_lowercase();
        if !matches!(k.as_str(), "id" | "rootfolder" | "viewpath") {
            continue;
        }
        let v = decode(v);
        if v.starts_with('/') {
            return (host, v.trim_matches('/').to_string());
        }
    }

    // `/:x:/r/…` — the letter is the kind (x=Excel, w=Word, f=folder) and the
    // `r` is what says a real path follows. **The colons must go**: Windows
    // cannot have one in a path, so leaving them guarantees a failure that
    // looks like a permission problem.
    let body = strip_prefix_marker(body);
    let body = decode(body);
    let parts: Vec<&str> = body.split('/').filter(|x| !x.is_empty()).collect();
    // `…/Forms/AllItems.aspx` is the library's front door; the library itself
    // is what was meant.
    let parts = if parts.len() >= 2
        && parts[parts.len() - 2].eq_ignore_ascii_case("forms")
        && parts[parts.len() - 1].to_ascii_lowercase().ends_with(".aspx")
    {
        &parts[..parts.len() - 2]
    } else {
        &parts[..]
    };
    (host, parts.join("/"))
}

/// Drop a leading `:x:/r/` (or `:x/r/`, which a bad paste produces).
fn strip_prefix_marker(body: &str) -> &str {
    let Some(b) = body.strip_prefix(':') else { return body };
    let mut it = b.char_indices();
    let Some((_, c)) = it.next() else { return body };
    if !c.is_ascii_alphabetic() {
        return body;
    }
    // `:x:/r/` — and `:x/r/`, which a paste that lost a colon produces. The
    // second colon is optional; the slash before the `r` is not.
    let after = &b[c.len_utf8()..];
    let after = after.strip_prefix(':').unwrap_or(after);
    let Some(after) = after.strip_prefix('/') else { return body };
    match after.strip_prefix("r/").or_else(|| after.strip_prefix("R/")) {
        Some(rest) => rest,
        None => body,
    }
}

/// Percent-decoding, bytes then UTF-8 — `%E5%8B%A4` is one character in three
/// escapes, so decoding per-character would produce mojibake.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// A SharePoint URL as a WebDAV UNC path. Anything else comes back unchanged,
/// so a caller can hand this every configured path without asking first.
pub fn to_unc(path: &str) -> String {
    let (host, rest) = split(path);
    if host.is_empty() {
        return path.trim().to_string();
    }
    format!("\\\\{host}@SSL\\DavWWWRoot\\{}", rest.replace('/', "\\"))
}

/// Why this URL cannot become a path — empty when it can.
///
/// **Said before reading, not after failing.** "could not open it, perhaps
/// WebDAV" sends somebody down a road with nothing at the end of it; "this
/// link has no location in it" tells them what to copy instead.
pub fn refuse(url: &str) -> Option<String> {
    let (host, rest) = split(url);
    if host.is_empty() {
        return None;
    }
    let body = url.split('?').next().unwrap_or(url);
    let head = body
        .strip_prefix("https://")
        .and_then(|r| r.find('/').map(|i| &r[i + 1..]))
        .unwrap_or("");
    // `/:x:/s/…` — a token, not a place. It *starts* with the prefix marker
    // but `strip_prefix_marker` leaves it alone, because only `/r/` carries a
    // path. Guessing one from a token could open somebody else's document.
    let a_token = head.starts_with(':') && strip_prefix_marker(head) == head;
    let what = if a_token {
        "共有リンク（`/:x:/s/…` の形）"
    } else if rest.is_empty() {
        "サイトの入口だけ"
    } else if rest.to_ascii_lowercase().ends_with(".aspx") {
        "ページ（.aspx）の URL"
    } else {
        return None;
    };
    Some(format!(
        "このリンクにはファイルの場所が入っていません（{what}）。\
         資料を開いて、**アドレス欄の URL** をコピーしてください\
         （`/sites/.../資料.xlsx` のように、途中にフォルダ名が入っている形です）"
    ))
}

/// What to check when the path is right and it still will not open.
///
/// The steps are crmaine's, and the second one is the surprising half: the
/// page says "Edge で表示してください" and that is what success looks like.
pub fn hint() -> &'static str {
    "（SharePoint に「つないでよい」と Windows がまだ覚えていないようです。\
     **この接続はサインアウトや再起動で切れます。** \
     ① Edge でその資料の *フォルダ* を開く \
     ② そのタブを Internet Explorer モードで読み直す\
     （「Edge で表示してください」と出ますが、それで正しいです）\
     ③ エクスプローラーのアドレス欄に同じパスを入れて開く。\
     それでも駄目なら「WebClient」サービスが動いているか確かめてください）"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plain case, and crmaine's own example.
    #[test]
    fn an_address_bar_url_becomes_a_path() {
        assert_eq!(
            to_unc("https://jri.sharepoint.com/sites/Team/Shared Documents/勤怠.xlsx"),
            r"\\jri.sharepoint.com@SSL\DavWWWRoot\sites\Team\Shared Documents\勤怠.xlsx"
        );
    }

    /// Percent escapes and the query that Office Online tacks on.
    #[test]
    fn the_escapes_and_the_query_come_off() {
        assert_eq!(
            to_unc("https://x.sharepoint.com/sites/T/Shared%20Documents/a.xlsx?web=1&e=abc"),
            r"\\x.sharepoint.com@SSL\DavWWWRoot\sites\T\Shared Documents\a.xlsx"
        );
    }

    /// **The shape you get from opening a folder.** The visible path is a view
    /// page and the real one is in `id` — miss it and the answer is a
    /// `…\Forms\AllItems.aspx` that does not exist.
    #[test]
    fn a_folder_url_keeps_its_id_rather_than_the_view_page() {
        assert_eq!(
            to_unc("https://x.sharepoint.com/sites/T/Docs/Forms/AllItems.aspx?id=%2Fsites%2FT%2FDocs%2F2026&viewid=1"),
            r"\\x.sharepoint.com@SSL\DavWWWRoot\sites\T\Docs\2026"
        );
    }

    /// …and with no `id`, the front door means the library itself.
    #[test]
    fn the_library_front_door_means_the_library() {
        assert_eq!(
            to_unc("https://x.sharepoint.com/sites/T/Docs/Forms/AllItems.aspx"),
            r"\\x.sharepoint.com@SSL\DavWWWRoot\sites\T\Docs"
        );
    }

    /// `/:x:/r/…` carries a real path — **and the colons cannot survive**,
    /// because Windows has no place for one in a path.
    #[test]
    fn the_prefix_and_its_colons_are_dropped() {
        assert_eq!(
            to_unc("https://x.sharepoint.com/:x:/r/sites/T/Docs/a.xlsx?d=w1&csf=1"),
            r"\\x.sharepoint.com@SSL\DavWWWRoot\sites\T\Docs\a.xlsx"
        );
        assert!(!to_unc("https://x.sharepoint.com/:x:/r/sites/T/a.xlsx").contains(':'),
                "no colon may reach a Windows path — except the one in @SSL");
    }

    /// Anything that is not one of these comes back untouched, so a caller can
    /// hand it every configured path without asking what kind it is.
    #[test]
    fn a_plain_path_is_left_alone() {
        for p in [r"C:\notes", "/home/me/notes", "~/notes", ""] {
            assert_eq!(to_unc(p), p);
        }
    }

    /// **A share link has no location in it.** Refused before it is tried,
    /// with what to copy instead — guessing a path from a token could open
    /// somebody else's document.
    #[test]
    fn a_share_link_is_refused_with_a_reason() {
        let said = refuse("https://x.sharepoint.com/:x:/s/Team/EQ1a2b3c").unwrap();
        assert!(said.contains("共有リンク"), "{said}");
        assert!(said.contains("アドレス欄"), "and says what to copy: {said}");
        assert!(refuse("https://x.sharepoint.com/sites/T/Docs/a.xlsx").is_none());
        assert!(refuse(r"C:\notes").is_none(), "not a SharePoint URL at all");
    }
}
