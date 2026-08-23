//! Parsing and sanitising of AI replies: pulling a command, a commit message,
//! a JSON plan (junk / structure / rename / semantic-search), or a bounded
//! payload out of free-form model output. Pure functions, validated against the
//! caller's real data so a hallucinated name or path matches nothing.

use std::path::PathBuf;

use crate::{JunkItem, MoveItem, RenameItem};

/// Clean an AI-generated shell command: drop ``` fences and surrounding
/// backticks, and take the first non-empty line (models sometimes add prose).
pub(crate) fn clean_ai_command(raw: &str) -> String {
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("```") {
            continue;
        }
        return t.trim_matches('`').trim().to_string();
    }
    String::new()
}

/// Strip code fences and leading/trailing blank lines from an AI-drafted commit
/// message. Models sometimes wrap the whole thing in ```; the content inside is
/// what we want.
pub(crate) fn clean_ai_commit_message(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.lines().collect();
    // Drop a leading ```… fence and its matching close, if present.
    if lines.first().map(|l| l.trim_start().starts_with("```")).unwrap_or(false) {
        lines.remove(0);
        if let Some(pos) = lines.iter().rposition(|l| l.trim() == "```") {
            lines.truncate(pos);
        }
    }
    let text = lines.join("\n");
    text.trim().to_string()
}

/// The JSON array in a model's reply, parsed — or nothing.
///
/// Isolated by its brackets first, because a model asked for JSON will still
/// wrap it in prose or ``` fences about a third of the time. Four parsers
/// opened with these seven lines.
fn json_array<T: serde::de::DeserializeOwned>(raw: &str) -> Vec<T> {
    let (Some(start), Some(end)) = (raw.find('['), raw.rfind(']')) else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }
    serde_json::from_str(&raw[start..=end]).unwrap_or_default()
}

/// Parse the junk detector's reply into concrete candidates. The model is asked
/// for a JSON array of `{name, reason}`; we strip any fences, parse leniently,
/// and keep only names that match a real entry in `names` (so a hallucinated or
/// mistyped name can never target a file that was not shown). Returns items
/// pre-checked for deletion.
pub(crate) fn parse_junk_reply(raw: &str, names: &[(String, PathBuf)]) -> Vec<JunkItem> {
    #[derive(serde::Deserialize)]
    struct Hit {
        name: String,
        #[serde(default)]
        reason: String,
    }
    let hits: Vec<Hit> = json_array(raw);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for hit in hits {
        // Match by exact name against what was shown; ignore anything else.
        if let Some((_, path)) = names.iter().find(|(n, _)| *n == hit.name) {
            if seen.insert(path.clone()) {
                out.push(JunkItem {
                    path: path.clone(),
                    reason: hit.reason.trim().to_string(),
                    selected: true,
                });
            }
        }
    }
    out
}

/// Sanitise an AI-proposed destination sub-folder: a single relative segment
/// path with no `..`, no absolute root, no drive — so a plan can only ever move
/// files *into* new folders under the current directory, never elsewhere.
pub(crate) fn clean_dest_folder(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    // Reject anything that could escape the current directory — checked before
    // any trimming so a leading separator or a drive letter can't slip through.
    if t.starts_with('/') || t.starts_with('\\') || t.contains(':') {
        return None;
    }
    let t = t.trim_end_matches(['/', '\\']);
    let parts: Vec<&str> = t.split(['/', '\\']).collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return None;
    }
    Some(parts.join("/"))
}

/// Parse the structure suggester's reply into concrete moves. The model returns
/// a JSON array of `{name, folder, reason}`; we keep only names matching a real
/// entry and folders that are safe relative sub-paths, and drop no-ops (a file
/// "moved" into a folder it is already the same as). Pre-checked for action.
pub(crate) fn parse_structure_reply(raw: &str, names: &[(String, PathBuf)]) -> Vec<MoveItem> {
    #[derive(serde::Deserialize)]
    struct Hit {
        name: String,
        #[serde(default)]
        folder: String,
        #[serde(default)]
        reason: String,
    }
    let hits: Vec<Hit> = json_array(raw);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for hit in hits {
        let Some((_, path)) = names.iter().find(|(n, _)| *n == hit.name) else { continue };
        let Some(dest) = clean_dest_folder(&hit.folder) else { continue };
        if !seen.insert(path.clone()) {
            continue;
        }
        out.push(MoveItem {
            path: path.clone(),
            name: hit.name.clone(),
            dest,
            reason: hit.reason.trim().to_string(),
            selected: true,
        });
    }
    out
}

/// Cap a diff at roughly `max_bytes` on a line boundary so the AI request stays
/// within budget, appending a marker when truncated.
pub(crate) fn truncate_diff_for_ai(diff: &str, max_bytes: usize) -> String {
    if diff.len() <= max_bytes {
        return diff.to_string();
    }
    let mut out = String::with_capacity(max_bytes + 64);
    for line in diff.lines() {
        if out.len() + line.len() + 1 > max_bytes {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("\n[diff truncated — summarise from what is shown above]\n");
    out
}

/// Sanitise an AI-proposed new filename: a single path segment, no separators,
/// no `..`/`.`, no drive, and not empty — so a rename can only ever change a
/// name within the same directory, never move a file elsewhere.
pub(crate) fn clean_filename(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t == "." || t == ".." {
        return None;
    }
    if t.contains('/') || t.contains('\\') || t.contains(':') || t.contains('\0') {
        return None;
    }
    Some(t.to_string())
}

/// Parse the bulk-rename reply into concrete renames. The model returns a JSON
/// array of `{name, new_name}`; we keep only entries whose `name` matches a real
/// file and whose `new_name` is a safe filename that actually differs, and drop
/// duplicate targets (two files renamed to the same thing). Pre-checked.
pub(crate) fn parse_rename_reply(raw: &str, names: &[(String, PathBuf)]) -> Vec<RenameItem> {
    #[derive(serde::Deserialize)]
    struct Hit {
        name: String,
        #[serde(default, alias = "newName", alias = "new")]
        new_name: String,
    }
    let hits: Vec<Hit> = json_array(raw);
    let mut out = Vec::new();
    let mut used_src = std::collections::HashSet::new();
    let mut used_dst = std::collections::HashSet::new();
    for hit in hits {
        let Some((old, path)) = names.iter().find(|(n, _)| *n == hit.name) else { continue };
        let Some(new) = clean_filename(&hit.new_name) else { continue };
        if new == *old {
            continue; // no-op
        }
        // One proposal per source, and no two files renamed to the same name.
        if !used_src.insert(old.clone()) || !used_dst.insert(new.clone()) {
            continue;
        }
        out.push(RenameItem { path: path.clone(), old: old.clone(), new, selected: true });
    }
    out
}

/// Parse the semantic-search reply into an ordered list of catalog hits. The
/// model returns a JSON array of `{path, reason}`; we keep only paths that match
/// a real catalog entry (compared with separators normalised), in the order the
/// model ranked them, deduped. The reason is folded into the hit's line text so
/// the results list can show *why* each file matched.
pub(crate) fn parse_sem_search_reply(raw: &str, catalog: &[cian_core::search::Hit]) -> Vec<cian_core::search::Hit> {
    #[derive(serde::Deserialize)]
    struct Hit {
        path: String,
        #[serde(default)]
        reason: String,
    }
    let norm = |s: &str| s.replace('\\', "/");
    let picks: Vec<Hit> = json_array(raw);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pick in picks {
        let want = norm(&pick.path);
        if let Some(h) = catalog.iter().find(|h| norm(&h.rel.display().to_string()) == want) {
            if seen.insert(want.clone()) {
                let mut h = h.clone();
                // Fold the reason into the hit's "line" so the results list can
                // show it and Enter previews the file in F3 (line 1), mirroring
                // the grep→viewer flow. The catalog is files-only, so this never
                // tries to open a directory in the viewer.
                let reason = pick.reason.trim();
                h.line = Some((1, if reason.is_empty() {
                    "(relevant)".to_string()
                } else {
                    reason.chars().take(200).collect()
                }));
                out.push(h);
            }
        }
    }
    out
}

/// Cap arbitrary text at roughly `max_bytes` on a line boundary (a char
/// boundary if a single line is longer), appending a marker when truncated.
pub(crate) fn truncate_text_for_ai(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut out = String::with_capacity(max_bytes + 64);
    for line in text.lines() {
        if out.len() + line.len() + 1 > max_bytes {
            // A single over-long line: take a char-boundary prefix of it.
            if out.is_empty() {
                let mut end = max_bytes.min(line.len());
                while end > 0 && !line.is_char_boundary(end) {
                    end -= 1;
                }
                out.push_str(&line[..end]);
            }
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("\n[truncated — summarise from what is shown above]\n");
    out
}
