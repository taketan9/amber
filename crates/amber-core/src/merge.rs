//! 同じノートを、二人（二台）が別々に書いたとき。**別々の場所は混ぜる。同じ
//! 場所は混ぜない ── 両方残して、人に選ばせる。**
//!
//! 使う場所は二つ ── クラウド越しに合わせたときの `sync::Step::Clash` と、
//! 家族と分けている棚で両方が同じノートを書いたとき。どちらも「どちらかを
//! 捨てる」という問いにしない。
//!
//! # Git の混ぜ方を、そのまま
//!
//! 手順は Git の `git merge-file`（libxdiff の `xdl_merge`）と同じにしてある
//! （2026-09-11・本人の希望「Git のマージの仕組みを可能な限りそのまま」）:
//!
//! 1. 前 → こちら、前 → 向こう の**二つの差分**を Myers の算法で取る
//!    （繋ぎ目探しではなく、差分の重なりで「同じ場所か」を決める）
//! 2. **前の行の範囲が重なるか触れ合う**変更をひとつの塊にする。塊の中で、
//!    片方だけが触っていればそちらを採り、両方が同じ字なら一つにし、
//!    両方が別々に触っていれば**ぶつかった場所**
//! 3. ぶつかった場所は、こちらと向こうをもう一度比べて、**同じ行を外に出す**
//!    （Git の zealous）── 段落まるごとではなく、違う行だけがぶつかる
//!
//! 同じ入力を `git merge-file` に食わせたときと、字が一致することを試験で
//! 見張っている（`agrees_with_git_merge_file`）。
//!
//! # Git と違えているところ（amber の側で決めたこと）
//!
//! **ノートには何も書き足さない。** `<<<<<<<` を本文に入れない ── ノートは
//! ただの Markdown で、GitHub でも VS Code でも開く。印を入れると、混ぜた日から
//! **そのノートは amber でしか読めない形**になる。代わりに「どの行が向こうから
//! 来たか」「どこがぶつかったか」を**返り値で**言う（`Merged::came` /
//! `Merged::spots`）── 画面はそれを見て色を差し、三択を出す。
//!
//! **ぶつかった場所は両方残す。** こちらを上、向こうを下。選ぶまで何も失わない。
//!
//! **前書きは鍵ごと。** 本文と同じに扱うと同じ鍵が二行並んで YAML が壊れる。
//! ぶつかった鍵は `Merged::fields` で返し、本文にはこちらの値を置く。
//!
//! # 時刻を見ない
//!
//! どちらが新しいかは決めない。**三つの姿を比べるだけ** ── 分かれる前
//! （`was`）・こちら（`ours`）・向こう（`theirs`）。時計のずれた端末が
//! 毎回勝つ、という負け方をしない。
//!
//! # 一致は、空白を含めた完全一致
//!
//! 行末の空白も改行の有無も、行の一部（本人が決めた・2026-09-11）。丸めると
//! 人の打った字を書き換えることになる。

/// 混ぜた結果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Merged {
    /// 混ざった本文。
    pub text: String,
    /// **向こうから来た行**（`text` の中の行番号・0 起点）。
    /// 画面はここに印を付けられる ── 本文には何も書いていない。
    pub came: Vec<usize>,
    /// ぶつかった場所の、**向こう側の行**（`came` にも入る）。
    pub both: Vec<usize>,
    /// **ぶつかった場所**。人が三択（こちら／向こう／両方）で選ぶ。
    pub spots: Vec<Spot>,
    /// **前書きでぶつかった鍵**。本文にはこちらの値が置いてある。
    pub fields: Vec<Field>,
}

/// ぶつかった場所。`text` の中の行の範囲（0 起点・`(始まり, 行数)`）。
/// こちらが先、向こうがその直後に並んでいる。片方が消していたなら、その側の
/// 行数は 0。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    pub ours: (usize, usize),
    pub theirs: (usize, usize),
}

/// 前書きでぶつかった鍵。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    pub ours: String,
    pub theirs: String,
}

impl Merged {
    /// 人の目が要るか ── ぶつかった場所があるかどうか。
    pub fn needs_eyes(&self) -> bool {
        !self.spots.is_empty() || !self.fields.is_empty()
    }
}

/* ── 入口 ── */

/// 分かれる前・こちら・向こう を混ぜる。
///
/// 行で見る。**字の中までは踏み込まない** ── 一行の中で二人が別の語を
/// 直したとき、字で混ぜると「どちらの文でもない一文」ができる。行ごと
/// 両方残せば、読んだ人が選べる。
pub fn merge(was: &str, ours: &str, theirs: &str) -> Merged {
    // 前書きは鍵ごと ── 三つとも前書きを持っているときだけ。
    if let (Some(a), Some(b), Some(c)) = (head_of(was), head_of(ours), head_of(theirs)) {
        if let Some(out) = merge_with_head(was, ours, theirs, a, b, c) {
            return out;
        }
    }
    merge_lines(was, ours, theirs)
}

/// 本文だけを行で混ぜる。
fn merge_lines(was: &str, ours: &str, theirs: &str) -> Merged {
    let a = records(was);
    let b = records(ours);
    let c = records(theirs);
    let m = merge3(&a, &b, &c);
    Merged {
        text: m.rows.concat(),
        came: m.came,
        both: m.both,
        spots: m.spots,
        fields: Vec::new(),
    }
}

/// 行に割る。**改行は行の一部** ── `"あ"` と `"あ\n"` は別の行（Git と同じ）。
/// 混ぜたあとに繋ぐだけで元の形に戻るので、終わりの改行の有無も守られる。
fn records(s: &str) -> Vec<&str> {
    s.split_inclusive('\n').collect()
}

/* ── 前書き ── */

/// 前書きの範囲（`---` の行から閉じの `---` の行まで・バイト）。無ければ None。
fn head_of(s: &str) -> Option<(usize, usize)> {
    if !s.starts_with("---\n") {
        return None;
    }
    let rest = &s[4..];
    let close = rest.find("\n---\n").map(|p| p + 4 + 5)
        .or_else(|| if rest.ends_with("\n---") { Some(s.len()) } else { None })?;
    Some((0, close))
}

/// 前書きの行を `鍵: 値` に読む。読めない行が一つでもあれば None（行で混ぜる）。
fn fields_of(head: &str) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    for line in head.lines() {
        if line == "---" || line.trim().is_empty() {
            continue;
        }
        let (k, v) = line.split_once(':')?;
        if k.trim().is_empty() || k.starts_with(' ') {
            return None;
        }
        out.push((k.trim().to_string(), v.trim().to_string()));
    }
    Some(out)
}

fn merge_with_head(
    was: &str, ours: &str, theirs: &str,
    a: (usize, usize), b: (usize, usize), c: (usize, usize),
) -> Option<Merged> {
    let fa = fields_of(&was[a.0..a.1])?;
    let fb = fields_of(&ours[b.0..b.1])?;
    let fc = fields_of(&theirs[c.0..c.1])?;
    let get = |f: &[(String, String)], k: &str| f.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());

    // 鍵の並び ── 前の順、そのあとこちらが足したもの、向こうが足したもの。
    let mut keys: Vec<String> = Vec::new();
    for (k, _) in fa.iter().chain(fb.iter()).chain(fc.iter()) {
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    let mut lines: Vec<String> = Vec::new();
    let mut fields = Vec::new();
    for k in &keys {
        let (va, vb, vc) = (get(&fa, k), get(&fb, k), get(&fc, k));
        let pick = if vb == vc {
            vb
        } else if vb == va {
            vc
        } else if vc == va {
            vb
        } else {
            // 両方が別々に変えた ── こちらの値を置き、ぶつかった鍵として返す。
            fields.push(Field {
                key: k.clone(),
                ours: vb.clone().unwrap_or_default(),
                theirs: vc.clone().unwrap_or_default(),
            });
            // こちらが消していたら、向こうの値を置く（消したものは他に無い）。
            vb.clone().or(vc)
        };
        if let Some(v) = pick {
            lines.push(if v.is_empty() { format!("{k}:\n") } else { format!("{k}: {v}\n") });
        }
    }
    let head_text = format!("---\n{}---\n", lines.concat());
    let head_rows = head_text.matches('\n').count();

    // 本文は行で。
    let body = merge_lines(&was[a.1..], &ours[b.1..], &theirs[c.1..]);
    Some(Merged {
        text: head_text + &body.text,
        came: body.came.into_iter().map(|n| n + head_rows).collect(),
        both: body.both.into_iter().map(|n| n + head_rows).collect(),
        spots: body.spots.into_iter().map(|s| Spot {
            ours: (s.ours.0 + head_rows, s.ours.1),
            theirs: (s.theirs.0 + head_rows, s.theirs.1),
        }).collect(),
        fields,
    })
}

/* ── 三方向（Git の xmerge.c の写し） ──
 *
 * 手順も規則も `xdiff/xmerge.c`（`xdl_do_merge`・`xdl_append_merge`・
 * `xdl_refine_conflicts`・`xdl_simplify_non_conflicts`・`xdl_fill_merge_buffer`）
 * をそのまま Rust に写してある。`git merge-file` の既定（`XDL_MERGE_ZEALOUS_ALNUM`・
 * 印の大きさ以外の旗は無し）に合わせた。読み比べられるように、名前も
 * 近いままにしてある。
 */

struct Woven<'a> {
    rows: Vec<&'a str>,
    came: Vec<usize>,
    both: Vec<usize>,
    spots: Vec<Spot>,
}

/// 前に対する一つの変更（Git の `xdchange_t`）。前の `[i1, i1+chg1)` が、
/// その側の `[i2, i2+chg2)` になった。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Change {
    i1: isize,
    chg1: isize,
    i2: isize,
    chg2: isize,
}

/// 混ぜた一区画（Git の `xdmerge_t`）。`mode` は 0=ぶつかった・1=こちらだけ・
/// 2=向こうだけ・4=同じ直し。`i1`/`chg1` はこちらの行、`i2`/`chg2` は向こうの行。
#[derive(Debug, Clone, Copy)]
struct Region {
    mode: u8,
    i0: isize,
    chg0: isize,
    i1: isize,
    chg1: isize,
    i2: isize,
    chg2: isize,
}

/// `xdl_append_merge` ── **どちらかの側で触れ合っていれば、前の区画に繋ぐ。**
/// 種類が違えば、ぶつかった区画になる。
#[allow(clippy::too_many_arguments)]
fn append(list: &mut Vec<Region>, mode: u8, i0: isize, chg0: isize, i1: isize, chg1: isize, i2: isize, chg2: isize) {
    if let Some(m) = list.last_mut() {
        if i1 <= m.i1 + m.chg1 || i2 <= m.i2 + m.chg2 {
            if mode != m.mode {
                m.mode = 0;
            }
            m.chg0 = i0 + chg0 - m.i0;
            m.chg1 = i1 + chg1 - m.i1;
            m.chg2 = i2 + chg2 - m.i2;
            return;
        }
    }
    list.push(Region { mode, i0, chg0, i1, chg1, i2, chg2 });
}

fn merge3<'a>(base: &[&'a str], ours: &[&'a str], theirs: &[&'a str]) -> Woven<'a> {
    let s1 = script(base, ours);
    let s2 = script(base, theirs);
    let mut w = Woven { rows: Vec::new(), came: Vec::new(), both: Vec::new(), spots: Vec::new() };

    // `xdl_merge` ── 片方が手つかずなら、もう片方をそのまま。
    if s1.is_empty() {
        // こちらは手つかず ── 向こうをそのまま。**来た行にだけ印**（変更の台本から）。
        w.rows.extend_from_slice(theirs);
        for c in &s2 {
            for k in c.i2..c.i2 + c.chg2 {
                w.came.push(k as usize);
            }
        }
        return w;
    }
    if s2.is_empty() {
        w.rows.extend_from_slice(ours);
        return w;
    }

    // `xdl_do_merge`。
    let mut list: Vec<Region> = Vec::new();
    let (mut x1, mut x2) = (0usize, 0usize);
    while x1 < s1.len() && x2 < s2.len() {
        let (a, b) = (s1[x1], s2[x2]);
        if a.i1 + a.chg1 < b.i1 {
            append(&mut list, 1, a.i1, a.chg1, a.i2, a.chg2, b.i2 - b.i1 + a.i1, a.chg1);
            x1 += 1;
            continue;
        }
        if b.i1 + b.chg1 < a.i1 {
            append(&mut list, 2, b.i1, b.chg1, a.i2 - a.i1 + b.i1, b.chg1, b.i2, b.chg2);
            x2 += 1;
            continue;
        }
        let same = a.i1 == b.i1 && a.chg1 == b.chg1 && a.chg2 == b.chg2
            && (0..a.chg2).all(|k| ours[(a.i2 + k) as usize] == theirs[(b.i2 + k) as usize]);
        if !same {
            let off = a.i1 - b.i1;
            let ffo = off + a.chg1 - b.chg1;
            let (mut i0, mut i1, mut i2) = (a.i1, a.i2, b.i2);
            if off > 0 {
                i0 -= off;
                i1 -= off;
            } else {
                i2 += off;
            }
            let mut chg0 = a.i1 + a.chg1 - i0;
            let mut chg1 = a.i2 + a.chg2 - i1;
            let mut chg2 = b.i2 + b.chg2 - i2;
            if ffo < 0 {
                chg0 -= ffo;
                chg1 -= ffo;
            } else {
                chg2 += ffo;
            }
            append(&mut list, 0, i0, chg0, i1, chg1, i2, chg2);
        }
        let e1 = a.i1 + a.chg1;
        let e2 = b.i1 + b.chg1;
        if e1 >= e2 {
            x2 += 1;
        }
        if e2 >= e1 {
            x1 += 1;
        }
    }
    let shift2 = theirs.len() as isize - base.len() as isize;
    while x1 < s1.len() {
        let a = s1[x1];
        append(&mut list, 1, a.i1, a.chg1, a.i2, a.chg2, a.i1 + shift2, a.chg1);
        x1 += 1;
    }
    let shift1 = ours.len() as isize - base.len() as isize;
    while x2 < s2.len() {
        let b = s2[x2];
        append(&mut list, 2, b.i1, b.chg1, b.i1 + shift1, b.chg1, b.i2, b.chg2);
        x2 += 1;
    }

    refine(&mut list, ours, theirs);
    simplify(&mut list, ours);

    // `xdl_fill_merge_buffer` ── こちらを土台に、向こうだけの区画とぶつかった
    // 区画を差し込む。印は付けない。
    let mut i: isize = 0;
    for m in &list {
        if m.mode == 0 {
            for k in i..m.i1 {
                w.rows.push(ours[k as usize]);
            }
            let os = w.rows.len();
            for k in m.i1..m.i1 + m.chg1 {
                w.rows.push(ours[k as usize]);
            }
            let ts = w.rows.len();
            for k in m.i2..m.i2 + m.chg2 {
                w.came.push(w.rows.len());
                w.both.push(w.rows.len());
                w.rows.push(theirs[k as usize]);
            }
            if m.chg1 > 0 || m.chg2 > 0 {
                w.spots.push(Spot { ours: (os, m.chg1 as usize), theirs: (ts, m.chg2 as usize) });
            }
        } else if m.mode & 3 != 0 {
            for k in i..m.i1 {
                w.rows.push(ours[k as usize]);
            }
            if m.mode & 1 != 0 {
                for k in m.i1..m.i1 + m.chg1 {
                    w.rows.push(ours[k as usize]);
                }
            }
            if m.mode & 2 != 0 {
                for k in m.i2..m.i2 + m.chg2 {
                    w.came.push(w.rows.len());
                    w.rows.push(theirs[k as usize]);
                }
            }
        } else {
            continue;
        }
        i = m.i1 + m.chg1;
    }
    for k in i..ours.len() as isize {
        w.rows.push(ours[k as usize]);
    }
    w
}

/// `xdl_refine_conflicts` ── ぶつかった区画の中で、こちらと向こうをもう一度
/// 比べ、**同じ行は外に出す**。違いが一つも無ければ「同じ直し」。
fn refine(list: &mut Vec<Region>, ours: &[&str], theirs: &[&str]) {
    let mut out: Vec<Region> = Vec::with_capacity(list.len());
    for m in list.iter() {
        if m.mode != 0 || m.chg1 == 0 || m.chg2 == 0 {
            out.push(*m);
            continue;
        }
        let o = &ours[m.i1 as usize..(m.i1 + m.chg1) as usize];
        let t = &theirs[m.i2 as usize..(m.i2 + m.chg2) as usize];
        let xs = script(o, t);
        if xs.is_empty() {
            out.push(Region { mode: 4, ..*m });
            continue;
        }
        for x in xs {
            out.push(Region {
                mode: 0, i0: m.i0, chg0: m.chg0,
                i1: x.i1 + m.i1, chg1: x.chg1, i2: x.i2 + m.i2, chg2: x.chg2,
            });
        }
    }
    *list = out;
}

/// `xdl_simplify_non_conflicts` ── ぶつかった区画どうしのあいだが**三行以下**、
/// または英数字を一つも含まなければ、あいだの行ごとひとつにまとめる
/// （`XDL_MERGE_ZEALOUS_ALNUM`・`git merge-file` の既定）。
fn simplify(list: &mut Vec<Region>, ours: &[&str]) {
    let mut n = 0usize;
    while n + 1 < list.len() {
        let (m, next) = (list[n], list[n + 1]);
        let begin = m.i1 + m.chg1;
        let end = next.i1;
        let alnum = (begin..end).any(|k| ours[k as usize].bytes().any(|b| b.is_ascii_alphanumeric()));
        if m.mode != 0 || next.mode != 0 || (end - begin > 3 && alnum) {
            n += 1;
            continue;
        }
        list[n].chg1 = next.i1 + next.chg1 - m.i1;
        list[n].chg2 = next.i2 + next.chg2 - m.i2;
        list.remove(n + 1);
    }
}

/* ── 差分の台本（`xdl_do_diff` + `xdl_change_compact` + `xdl_build_script`） ── */

/// `a` → `b` の変更の並び。Myers で揃えたあと、Git と同じに**塊をずらして
/// 整える**（`xdl_change_compact`）── 同じ字の行（空行など）が並ぶところで、
/// どの行を消したことにするかが Git と揃う。
fn script(a: &[&str], b: &[&str]) -> Vec<Change> {
    let pairs = myers(a, b);
    let mut ca = vec![true; a.len()];
    let mut cb = vec![true; b.len()];
    for &(x, y) in &pairs {
        ca[x] = false;
        cb[y] = false;
    }
    compact(a, &mut ca, &mut cb);
    compact(b, &mut cb, &mut ca);
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < ca.len() || j < cb.len() {
        if (i < ca.len() && ca[i]) || (j < cb.len() && cb[j]) {
            let (i1, i2) = (i, j);
            while i < ca.len() && ca[i] {
                i += 1;
            }
            while j < cb.len() && cb[j] {
                j += 1;
            }
            out.push(Change { i1: i1 as isize, chg1: (i - i1) as isize, i2: i2 as isize, chg2: (j - i2) as isize });
        } else {
            i += 1;
            j += 1;
        }
    }
    out
}

/// 変わった行のひと続き（Git の `xdlgroup`）。
#[derive(Clone, Copy)]
struct Group {
    start: usize,
    end: usize,
}

fn group_init(ch: &[bool]) -> Group {
    let mut end = 0;
    while end < ch.len() && ch[end] {
        end += 1;
    }
    Group { start: 0, end }
}

fn group_next(ch: &[bool], g: &mut Group) -> bool {
    if g.end == ch.len() {
        return false;
    }
    g.start = g.end + 1;
    g.end = g.start;
    while g.end < ch.len() && ch[g.end] {
        g.end += 1;
    }
    true
}

fn group_previous(ch: &[bool], g: &mut Group) -> bool {
    if g.start == 0 {
        return false;
    }
    g.end = g.start - 1;
    g.start = g.end;
    while g.start > 0 && ch[g.start - 1] {
        g.start -= 1;
    }
    true
}

fn slide_down(recs: &[&str], ch: &mut [bool], g: &mut Group) -> bool {
    if g.end < recs.len() && recs[g.start] == recs[g.end] {
        ch[g.start] = false;
        g.start += 1;
        ch[g.end] = true;
        g.end += 1;
        while g.end < ch.len() && ch[g.end] {
            g.end += 1;
        }
        true
    } else {
        false
    }
}

fn slide_up(recs: &[&str], ch: &mut [bool], g: &mut Group) -> bool {
    if g.start > 0 && recs[g.start - 1] == recs[g.end - 1] {
        g.start -= 1;
        ch[g.start] = true;
        g.end -= 1;
        ch[g.end] = false;
        while g.start > 0 && ch[g.start - 1] {
            g.start -= 1;
        }
        true
    } else {
        false
    }
}

/// `xdl_change_compact`（字下げの目安は使わない ── `git merge-file` の既定と同じ）。
///
/// 塊を上へ寄せ、下へ寄せ、ぶつかった塊は繋ぐ。動かせる幅があるなら、
/// **もう片方の側にも変更がある位置**に揃え、無ければいちばん下に置く。
fn compact(recs: &[&str], ch: &mut [bool], other: &mut [bool]) {
    let mut g = group_init(ch);
    let mut go = group_init(other);
    loop {
        if g.end != g.start {
            let mut earliest_end;
            let mut end_matching_other: Option<usize>;
            loop {
                let groupsize = g.end - g.start;
                end_matching_other = None;
                while slide_up(recs, ch, &mut g) {
                    if !group_previous(other, &mut go) {
                        return;    // 揃いが壊れた ── Git は BUG にする。ここは整えずに返す
                    }
                }
                earliest_end = g.end;
                if go.end > go.start {
                    end_matching_other = Some(g.end);
                }
                loop {
                    if !slide_down(recs, ch, &mut g) {
                        break;
                    }
                    if !group_next(other, &mut go) {
                        return;
                    }
                    if go.end > go.start {
                        end_matching_other = Some(g.end);
                    }
                }
                if groupsize == g.end - g.start {
                    break;
                }
            }
            if g.end != earliest_end && end_matching_other.is_some() {
                while go.end == go.start {
                    if !slide_up(recs, ch, &mut g) || !group_previous(other, &mut go) {
                        return;
                    }
                }
            }
        }
        if !group_next(ch, &mut g) {
            break;
        }
        if !group_next(other, &mut go) {
            break;
        }
    }
}

/* ── Myers の差分（線形の記憶で） ── */

/// `a` の行が `b` のどこに残っているか（揃った行の組・両方とも昇順）。
///
/// Myers の O(ND) の算法を、まん中の蛇で二つに割る形で（記憶は線形）。
/// Git（libxdiff）と同じ算法なので、揃え方の癖もおおむね同じになる。
fn myers<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let cap = a.len() + b.len() + 3;
    let mut vf = V::new(cap);
    let mut vb = V::new(cap);
    conquer(a, 0, a.len(), b, 0, b.len(), &mut out, &mut vf, &mut vb);
    out
}

/// 負の添え字を持つ配列（対角線の番号 `k` で引く）。
struct V {
    off: isize,
    v: Vec<isize>,
}

impl V {
    fn new(n: usize) -> V {
        V { off: n as isize, v: vec![0; 2 * n + 1] }
    }
}

impl std::ops::Index<isize> for V {
    type Output = isize;
    fn index(&self, k: isize) -> &isize { &self.v[(k + self.off) as usize] }
}
impl std::ops::IndexMut<isize> for V {
    fn index_mut(&mut self, k: isize) -> &mut isize { &mut self.v[(k + self.off) as usize] }
}

#[allow(clippy::too_many_arguments)]
fn conquer<'a>(
    a: &[&'a str], mut a0: usize, mut a1: usize,
    b: &[&'a str], mut b0: usize, mut b1: usize,
    out: &mut Vec<(usize, usize)>, vf: &mut V, vb: &mut V,
) {
    // 前後の揃っているところを先に取る。
    while a0 < a1 && b0 < b1 && a[a0] == b[b0] {
        out.push((a0, b0));
        a0 += 1;
        b0 += 1;
    }
    let mut tail = Vec::new();
    while a0 < a1 && b0 < b1 && a[a1 - 1] == b[b1 - 1] {
        a1 -= 1;
        b1 -= 1;
        tail.push((a1, b1));
    }
    if a0 < a1 && b0 < b1 {
        if let Some((x, y)) = middle(a, a0, a1, b, b0, b1, vf, vb) {
            conquer(a, a0, x, b, b0, y, out, vf, vb);
            conquer(a, x, a1, b, y, b1, out, vf, vb);
        }
        // 蛇が見つからない ＝ 揃う行が一つも無い（消して足しただけ）。
    }
    tail.reverse();
    out.extend(tail);
}

/// まん中の蛇の頭。前から伸ばす道と後ろから伸ばす道が出会う点。
#[allow(clippy::too_many_arguments)]
fn middle(
    a: &[&str], a0: usize, a1: usize,
    b: &[&str], b0: usize, b1: usize,
    vf: &mut V, vb: &mut V,
) -> Option<(usize, usize)> {
    let n = (a1 - a0) as isize;
    let m = (b1 - b0) as isize;
    let delta = n - m;
    let odd = delta & 1 == 1;
    vf[1] = 0;
    vb[1] = 0;
    let d_max = (n + m + 1) / 2 + 1;
    for d in 0..d_max {
        // 前から。
        let mut k = d;
        while k >= -d {
            let mut x = if k == -d || (k != d && vf[k - 1] < vf[k + 1]) { vf[k + 1] } else { vf[k - 1] + 1 };
            let mut y = x - k;
            let (x0, y0) = (x, y);
            while x < n && y < m && a[a0 + x as usize] == b[b0 + y as usize] {
                x += 1;
                y += 1;
            }
            vf[k] = x;
            if odd && (k - delta).abs() < d && vf[k] + vb[-(k - delta)] >= n {
                return Some((a0 + x0 as usize, b0 + y0 as usize));
            }
            k -= 2;
        }
        // 後ろから。
        let mut k = d;
        while k >= -d {
            let mut x = if k == -d || (k != d && vb[k - 1] < vb[k + 1]) { vb[k + 1] } else { vb[k - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[a1 - 1 - x as usize] == b[b1 - 1 - y as usize] {
                x += 1;
                y += 1;
            }
            vb[k] = x;
            if !odd && (k - delta).abs() <= d && vb[k] + vf[-(k - delta)] >= n {
                return Some((a1 - x as usize, b1 - y as usize));
            }
            k -= 2;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &[&str]) -> String {
        s.join("\n") + "\n"
    }

    /* ── 以前からある決めごと ── */

    #[test]
    fn different_places_both_land() {
        // 二人が別の場所を書いた ── どちらも残る。これが**ふだんの姿**で、
        // 家族と買い物リストを分けていれば、たいていこうなる。
        let was = t(&["# 買い物", "", "- 牛乳", "- パン"]);
        let ours = t(&["# 買い物", "", "- 牛乳", "- パン", "- 卵"]);
        let theirs = t(&["# 買い物", "", "- 牛乳 2本", "- パン"]);

        let m = merge(&was, &ours, &theirs);
        assert_eq!(m.text, t(&["# 買い物", "", "- 牛乳 2本", "- パン", "- 卵"]));
        assert_eq!(m.came, vec![2]);
        assert!(!m.needs_eyes());
    }

    #[test]
    fn the_same_line_from_both_keeps_both() {
        // **同じ行を、二人が別々に書き換えた。** どちらかを捨てない ──
        // 両方置いて、人が選ぶ（`spots`）。
        let was = t(&["集合は 10 時"]);
        let ours = t(&["集合は 11 時"]);
        let theirs = t(&["集合は 12 時"]);

        let m = merge(&was, &ours, &theirs);
        assert_eq!(m.text, t(&["集合は 11 時", "集合は 12 時"]));
        assert_eq!(m.came, vec![1]);
        assert_eq!(m.both, vec![1]);
        assert_eq!(m.spots, vec![Spot { ours: (0, 1), theirs: (1, 1) }]);
        assert!(m.needs_eyes(), "人が読んで、要らないほうを消す");
    }

    #[test]
    fn a_line_deleted_here_and_written_there_is_kept() {
        // **迷ったら残す。** こちらで消し、向こうで書き足した行は残る ──
        // そして、それは**ぶつかった場所**（消した側は行数 0）。
        let was = t(&["- 牛乳", "- パン"]);
        let ours = t(&["- パン"]);
        let theirs = t(&["- 牛乳 2本", "- パン"]);

        let m = merge(&was, &ours, &theirs);
        assert_eq!(m.text, t(&["- 牛乳 2本", "- パン"]));
        assert_eq!(m.spots, vec![Spot { ours: (0, 0), theirs: (0, 1) }]);
        assert!(m.needs_eyes());
    }

    #[test]
    fn one_side_untouched_takes_the_other() {
        let was = t(&["ひとつ", "ふたつ"]);
        let ours = was.clone();
        let theirs = t(&["ひとつ", "ふたつ", "みっつ"]);

        let m = merge(&was, &ours, &theirs);
        assert_eq!(m.text, theirs);
        // **来た行にだけ印が付く。** 触っていない二行には付かない。
        assert_eq!(m.came, vec![2]);

        let m = merge(&was, &theirs, &was);
        assert_eq!(m.text, theirs);
        assert!(m.came.is_empty());
    }

    #[test]
    fn the_same_edit_on_both_sides_is_not_a_clash() {
        let was = t(&["集合は 10 時"]);
        let same = t(&["集合は 11 時"]);
        let m = merge(&was, &same, &same);
        assert_eq!(m.text, same);
        assert!(m.came.is_empty(), "二重に置かない");
        assert!(!m.needs_eyes());
    }

    #[test]
    fn the_last_newline_survives() {
        let was = "あ\nい\n";
        let m = merge(was, "あ\nい\nう\n", "あ\nい\n");
        assert!(m.text.ends_with('\n'));

        // 元から無いなら、足さない。
        let m = merge("あ", "あ\nい", "あ");
        assert!(!m.text.ends_with('\n'), "{:?}", m.text);

        // **改行は行の一部**（Git と同じ）── こちらが末尾の改行を消したら、消える。
        let m = merge("a\nb\n", "a\nb", "a\nb\n");
        assert_eq!(m.text, "a\nb");
        assert!(!m.needs_eyes());
    }

    #[test]
    fn empty_sides_do_not_panic() {
        assert_eq!(merge("", "", "").text, "");
        assert_eq!(merge("", "あ\n", "").text, "あ\n");
        assert_eq!(merge("あ\n", "", "").text, "");
        let m = merge("あ\n", "", "い\n");
        assert!(m.text.contains('い'), "消しと書き足しなら、書いたほうを残す");
        assert!(m.needs_eyes());
        // 空の前から、両方が別の字を書いた ── ぶつかる（Git と同じ）。
        let m = merge("", "a\n", "b\n");
        assert_eq!(m.text, "a\nb\n");
        assert_eq!(m.spots, vec![Spot { ours: (0, 1), theirs: (1, 1) }]);
    }

    #[test]
    fn a_note_with_a_diagram_survives_a_merge() {
        let was = t(&["# 図", "", "```mermaid", "flowchart LR", "  A --> B", "```", "", "あと"]);
        let ours = t(&["# 図", "", "```mermaid", "flowchart LR", "  A --> B", "```", "", "あと", "追記"]);
        let theirs = t(&["# 図と字", "", "```mermaid", "flowchart LR", "  A --> B", "```", "", "あと"]);

        let m = merge(&was, &ours, &theirs);
        assert!(m.text.contains("```mermaid\nflowchart LR\n  A --> B\n```"), "{:?}", m.text);
        assert!(m.text.contains("# 図と字"));
        assert!(m.text.contains("追記"));
        assert!(!m.needs_eyes());
    }

    #[test]
    fn the_marked_rows_point_at_the_merged_text() {
        let was = t(&["a", "b", "c"]);
        let ours = t(&["a", "b", "c", "ours"]);
        let theirs = t(&["a", "THEIRS", "c"]);

        let m = merge(&was, &ours, &theirs);
        let rows: Vec<&str> = m.text.lines().collect();
        for &n in &m.came {
            assert!(n < rows.len(), "行番号が本文の外: {n}");
        }
        assert_eq!(m.came.iter().map(|&n| rows[n]).collect::<Vec<_>>(), vec!["THEIRS"]);
    }

    #[test]
    fn a_very_long_note_still_answers() {
        let was: String = (0..3000).map(|i| format!("行 {i}\n")).collect();
        let ours = was.replace("行 0\n", "こちら\n");
        let theirs = was.replace("行 2999\n", "向こう\n");
        let m = merge(&was, &ours, &theirs);
        assert!(m.text.contains("こちら"));
        assert!(m.text.contains("向こう"));
        assert!(!m.needs_eyes());
    }

    /* ── Git の `merge-file` と同じ答え（2026-09-11 に実物で確かめた形） ── */

    #[test]
    fn touching_changes_clash_like_git() {
        // 隣り合う行を別々に直した ── あいだに変わっていない行が無いので、
        // Git はぶつかったと見る。同じにする。
        let m = merge("a\nb\nc\n", "a\nB\nc\n", "a\nb\nC\n");
        assert_eq!(m.text, "a\nB\nc\nb\nC\n");
        assert_eq!(m.spots, vec![Spot { ours: (1, 2), theirs: (3, 2) }]);

        // 一行あいだが空いていれば、両方入る。
        let m = merge("a\nb\nc\nd\n", "a\nB\nc\nd\n", "a\nb\nc\nD\n");
        assert_eq!(m.text, "a\nB\nc\nD\n");
        assert!(!m.needs_eyes());

        // 離れた二か所でも、あいだの一行を片方が触っていれば触れ合う。
        let m = merge("a\nb\nc\nd\ne\n", "a\nb\nX\nd\ne\n", "a\nY\nc\nd\ne\n");
        assert_eq!(m.text, "a\nb\nX\nY\nc\nd\ne\n");
        assert_eq!(m.spots, vec![Spot { ours: (1, 2), theirs: (3, 2) }]);
    }

    #[test]
    fn zealous_pulls_equal_rows_out_of_a_clash() {
        // 同じ置き換えの中の一行だけ違う ── 揃っている行は外に出て、
        // 違う一行だけがぶつかる（Git の zealous）。
        let m = merge("x\n", "a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(m.text, "a\nb\nB\nc\n");
        assert_eq!(m.spots, vec![Spot { ours: (1, 1), theirs: (2, 1) }]);
        assert_eq!(m.both, vec![2]);
    }

    #[test]
    fn additions_at_the_end() {
        // 両方が同じ行を足した → 一つ。別の行を足した → ぶつかる。片方だけ → 入る。
        assert!(!merge("a\nb\nc\n", "a\nb\nc\nd\n", "a\nb\nc\nd\n").needs_eyes());
        let m = merge("a\nb\nc\n", "a\nb\nc\nd\n", "a\nb\nc\ne\n");
        assert_eq!(m.text, "a\nb\nc\nd\ne\n");
        assert_eq!(m.spots, vec![Spot { ours: (3, 1), theirs: (4, 1) }]);
        let m = merge("a\nb\nc\n", "a\nb\nc\nd\n", "a\nb\nc\n");
        assert_eq!(m.text, "a\nb\nc\nd\n");
        assert!(!m.needs_eyes());
    }

    #[test]
    fn deleted_here_edited_there_is_a_spot_with_an_empty_side() {
        let m = merge("a\nb\nc\n", "a\nc\n", "a\nBB\nc\n");
        assert_eq!(m.text, "a\nBB\nc\n");
        assert_eq!(m.spots, vec![Spot { ours: (1, 0), theirs: (1, 1) }]);
    }

    /* ── 前書き ── */

    #[test]
    fn front_matter_merges_by_key() {
        let was = "---\ntitle: 買い物\ntags: [家]\n---\n\n- 牛乳\n";
        let ours = "---\ntitle: 買い物\ntags: [家, 急ぎ]\n---\n\n- 牛乳\n";
        let theirs = "---\ntitle: 週末の買い物\ntags: [家]\n---\n\n- 牛乳\n";
        let m = merge(was, ours, theirs);
        // 別々の鍵を触った ── どちらも入り、同じ鍵は二行にならない。
        assert_eq!(m.text, "---\ntitle: 週末の買い物\ntags: [家, 急ぎ]\n---\n\n- 牛乳\n");
        assert!(!m.needs_eyes());
    }

    #[test]
    fn front_matter_clash_keeps_ours_and_reports_the_key() {
        let was = "---\ntags: [家]\n---\n\n本文\n";
        let ours = "---\ntags: [家, 仕事]\n---\n\n本文\n";
        let theirs = "---\ntags: [家, 急ぎ]\n---\n\n本文\n";
        let m = merge(was, ours, theirs);
        assert_eq!(m.text, ours, "本文にはこちらの値。同じ鍵を二行並べない");
        assert_eq!(m.fields, vec![Field {
            key: "tags".into(), ours: "[家, 仕事]".into(), theirs: "[家, 急ぎ]".into(),
        }]);
        assert!(m.needs_eyes());
    }

    #[test]
    fn body_marks_are_offset_past_the_front_matter() {
        let was = "---\ntitle: t\n---\n\na\nb\n";
        let ours = "---\ntitle: t\n---\n\na\nb\n";
        let theirs = "---\ntitle: t\n---\n\na\nB\n";
        let m = merge(was, ours, theirs);
        let rows: Vec<&str> = m.text.lines().collect();
        assert_eq!(m.came.iter().map(|&n| rows[n]).collect::<Vec<_>>(), vec!["B"]);
    }

    /* ── Git を正解として、機械で突き合わせる ── */

    /// この機械に Git があれば、`git merge-file -p` と同じ字になるかを見る。
    /// **印の行（`<<<<<<<` `=======` `>>>>>>>`）を落とした Git の出力が、
    /// こちらの本文と一字一句同じ**であること ── ぶつかったところも、
    /// こちら→向こうの順で両方置くので、印を落とせば同じになるはず。
    fn git_says(was: &str, ours: &str, theirs: &str) -> Option<(String, bool)> {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("amber-merge-{}-{}", std::process::id(), rand_seed()));
        std::fs::create_dir_all(&dir).ok()?;
        for (name, text) in [("base", was), ("ours", ours), ("theirs", theirs)] {
            let mut f = std::fs::File::create(dir.join(name)).ok()?;
            f.write_all(text.as_bytes()).ok()?;
        }
        let out = std::process::Command::new("git")
            .args(["merge-file", "-p", "ours", "base", "theirs"])
            .current_dir(&dir)
            .output()
            .ok()?;
        let _ = std::fs::remove_dir_all(&dir);
        let code = out.status.code().unwrap_or(-1);
        if code < 0 {
            return None;
        }
        let text = String::from_utf8(out.stdout).ok()?;
        let clean: String = text
            .split_inclusive('\n')
            .filter(|l| !(l.starts_with("<<<<<<< ") || l.starts_with("=======") || l.starts_with(">>>>>>> ")))
            .collect();
        Some((clean, code > 0))
    }

    fn rand_seed() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(1);
        N.fetch_add(1, Ordering::Relaxed)
    }

    /// 依存を増やさない乱数（xorshift）。**同じ種なら同じ列** ── 落ちたときに
    /// 同じ形をもう一度作れる。
    struct Dice(u64);
    impl Dice {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: usize) -> usize { (self.next() % n.max(1) as u64) as usize }
    }

    /// ノートらしい前の字（空行の多い Markdown）。
    fn some_note(d: &mut Dice) -> Vec<String> {
        let words = ["牛乳", "パン", "卵", "掃除", "洗濯", "散歩", "会議", "電話", "買い出し", "休み"];
        let n = 3 + d.below(12);
        let mut out = Vec::new();
        out.push("# 題\n".to_string());
        for i in 0..n {
            match d.below(4) {
                0 => out.push("\n".to_string()),
                1 => out.push(format!("- {}\n", words[d.below(words.len())])),
                2 => out.push(format!("{}は{}。\n", words[d.below(words.len())], words[d.below(words.len())])),
                _ => out.push(format!("{} {}\n", i, words[d.below(words.len())])),
            }
        }
        out
    }

    /// でたらめに一〜三か所直す（書き換え・消す・足す）。
    fn edited(d: &mut Dice, rows: &[String]) -> Vec<String> {
        let mut out = rows.to_vec();
        for _ in 0..(1 + d.below(3)) {
            if out.is_empty() {
                out.push("足した\n".to_string());
                continue;
            }
            let at = d.below(out.len());
            match d.below(3) {
                0 => out[at] = format!("直した{}\n", d.below(100)),
                1 => { out.remove(at); }
                _ => out.insert(at, format!("足した{}\n", d.below(100))),
            }
        }
        out
    }

    #[test]
    fn agrees_with_git_merge_file() {
        // Git が無い機械では飛ばす（CI の Windows など）。
        if git_says("a\n", "a\n", "a\n").is_none() {
            eprintln!("git が無いので飛ばします");
            return;
        }
        let mut d = Dice(0x9e37_79b9_7f4a_7c15);
        let mut differ = Vec::new();
        let total = 400;
        for n in 0..total {
            let was = some_note(&mut d);
            let ours = edited(&mut d, &was);
            let theirs = edited(&mut d, &was);
            let (was, ours, theirs) = (was.concat(), ours.concat(), theirs.concat());
            let m = merge(&was, &ours, &theirs);
            let (git, clashed) = git_says(&was, &ours, &theirs).unwrap();
            if m.text != git || m.needs_eyes() != clashed {
                differ.push(format!(
                    "#{n}\n前:\n{was}\nこちら:\n{ours}\n向こう:\n{theirs}\nGit（印を落として）:\n{git}\namber:\n{}\nぶつかった: git={clashed} amber={}\n",
                    m.text, m.needs_eyes()
                ));
            }
        }
        // **ぜんぶ同じであること。** 揃え方の癖は Myers を同じにしてあるので
        // 揃うはず ── 違いが出たら、それは癖ではなく作りの違い。
        assert!(differ.is_empty(), "{} / {} 件が Git と違います。最初の三つ:\n{}",
            differ.len(), total, differ.iter().take(3).cloned().collect::<Vec<_>>().join("\n----\n"));
    }

    /* ── 性質 ── でたらめな直しを量産して、失わないことを見る ── */

    #[test]
    fn nothing_written_by_one_side_alone_is_lost() {
        let mut d = Dice(42);
        for _ in 0..2000 {
            let was = some_note(&mut d);
            let ours = edited(&mut d, &was);
            let theirs = edited(&mut d, &was);
            let m = merge(&was.concat(), &ours.concat(), &theirs.concat());
            let out: Vec<&str> = m.text.split_inclusive('\n').collect();
            // こちらが足した行（前に無い行）は、必ず結果にある。向こうも同じ。
            for r in ours.iter().chain(theirs.iter()) {
                if !was.contains(r) {
                    assert!(out.contains(&r.as_str()), "足した行が消えました: {r:?}\n前:{:?}\nこちら:{:?}\n向こう:{:?}\n結果:{:?}",
                        was, ours, theirs, m.text);
                }
            }
            // 同じ入力なら同じ出力。
            assert_eq!(merge(&was.concat(), &ours.concat(), &theirs.concat()), m);
            // 印は本文の中を指す。
            let n = out.len();
            for &k in m.came.iter().chain(m.both.iter()) {
                assert!(k < n, "印が本文の外: {k} / {n}");
            }
            for s in &m.spots {
                assert!(s.ours.0 + s.ours.1 <= n && s.theirs.0 + s.theirs.1 <= n);
                assert_eq!(s.ours.0 + s.ours.1, s.theirs.0, "こちらの直後に向こう");
            }
        }
    }

    #[test]
    fn swapping_sides_keeps_the_same_rows() {
        // こちらと向こうを入れ替えても、**残る行の顔ぶれは同じ**（順が違うだけ）。
        let mut d = Dice(7);
        for _ in 0..1000 {
            let was = some_note(&mut d);
            let ours = edited(&mut d, &was);
            let theirs = edited(&mut d, &was);
            let a = merge(&was.concat(), &ours.concat(), &theirs.concat());
            let b = merge(&was.concat(), &theirs.concat(), &ours.concat());
            let mut x: Vec<&str> = a.text.split_inclusive('\n').collect();
            let mut y: Vec<&str> = b.text.split_inclusive('\n').collect();
            x.sort();
            y.sort();
            assert_eq!(x, y, "入れ替えると顔ぶれが変わりました\n前:{:?}\nこちら:{:?}\n向こう:{:?}", was, ours, theirs);
            assert_eq!(a.needs_eyes(), b.needs_eyes());
        }
    }
}
