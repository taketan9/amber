//! A small fuzzy matcher, shared by the command palette and the jump list.
//!
//! `score(query, text)` returns `None` when `query` is not a (case-insensitive)
//! subsequence of `text`, and otherwise a number where higher is a better match.
//! It rewards matches at word/path boundaries, runs of consecutive matches, and
//! an early first match, and lightly penalises very long candidates — enough to
//! rank a picker sensibly without the cost of a full fzf-style DP.

/// Characters that begin a "word" — a match right after one (or at the very
/// start) is worth much more, so "aio" ranks `ai/organize` above `mainaio`.
fn is_boundary(c: char) -> bool {
    matches!(c, '/' | '\\' | '_' | '-' | ' ' | '.' | ':' | '@')
}

/// Fuzzy score of `query` against `text`. `None` = no match.
pub fn score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = text.chars().collect();
    let tl: Vec<char> = text.to_lowercase().chars().collect();

    let mut qi = 0usize;
    let mut score = 0i32;
    let mut prev: Option<usize> = None;
    let mut first: Option<usize> = None;

    for (ti, &c) in tl.iter().enumerate() {
        if qi >= q.len() || c != q[qi] {
            continue;
        }
        score += 1;
        // Boundary bonus: start of string, or just after a separator.
        if ti == 0 || is_boundary(t[ti - 1]) {
            score += 10;
        }
        // Consecutive-match bonus.
        if let Some(p) = prev {
            if ti == p + 1 {
                score += 6;
            }
        }
        prev = Some(ti);
        if first.is_none() {
            first = Some(ti);
        }
        qi += 1;
    }

    if qi != q.len() {
        return None;
    }
    // Prefer an early first match and a shorter candidate.
    score -= first.unwrap_or(0) as i32 / 4;
    score -= t.len() as i32 / 16;
    Some(score)
}

/// Rank `items` by their fuzzy score against `query`, best first, dropping
/// non-matches; returns their original indices. A stable sort keeps ties in
/// input order, and an empty query keeps every item in its original order.
pub fn rank<'a, I>(query: &str, items: I) -> Vec<usize>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut scored: Vec<(usize, i32)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, s)| score(query, s).map(|sc| (i, sc)))
        .collect();
    if !query.is_empty() {
        scored.sort_by_key(|&(_, sc)| std::cmp::Reverse(sc));
    }
    scored.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_subsequence_does_not_match() {
        assert!(score("xyz", "readme.md").is_none());
        assert!(score("mdd", "readme.md").is_none()); // only two d's? m-d-d needs two d after m
    }

    #[test]
    fn subsequence_matches_case_insensitively() {
        assert!(score("RME", "readme").is_some());
        assert!(score("orga", "ai/organize").is_some());
        assert!(score("", "anything") == Some(0));
    }

    #[test]
    fn boundary_and_consecutive_matches_score_higher() {
        // "org" at a path boundary beats the same run buried mid-word.
        let boundary = score("org", "ai/organize").unwrap();
        let buried = score("org", "storage").unwrap();
        assert!(boundary > buried, "boundary {boundary} should beat buried {buried}");

        // A consecutive run beats a gappy match with no boundaries to reward.
        let run = score("read", "readme").unwrap();
        let gappy = score("read", "rxexaxd").unwrap();
        assert!(run > gappy, "run {run} should beat gappy {gappy}");
    }

    #[test]
    fn rank_orders_best_first_and_drops_non_matches() {
        let items = ["storage", "ai/organize", "notes.txt"];
        let order = rank("org", items.iter().copied());
        assert_eq!(order.len(), 2, "notes.txt has no 'org' subsequence");
        assert_eq!(order[0], 1, "ai/organize (boundary) ranks first");
    }

    #[test]
    fn empty_query_keeps_input_order() {
        let items = ["b", "a", "c"];
        assert_eq!(rank("", items.iter().copied()), vec![0, 1, 2]);
    }
}
