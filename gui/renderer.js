'use strict';
// amber の窓、三列。左が行き先、真ん中がノート、右が中身。
//
// **判断はここに書かない。** 題をどう決めるか、絞り込みの AND / OR をどう
// 解くか、チェックをどう切り替えるかは `amber-core` が答える ── iPhone も
// 同じ一枚に訊いている。ここに写した瞬間、**同じ操作なのに Mac と iPhone で
// 結果が違う**が、一度の編集で作れてしまう。

const el = (id) => document.getElementById(id);
const ask = (method, params) => window.amber.call(method, params || {});

const state = {
    root: '',
    notes: [],
    books: [],
    stars: [],
    colors: {},
    /// いま選んでいる行き先。kind は all / book / star / tag。
    dest: { kind: 'all', what: '' },
    filter: '',
    /// 問いの意味。**core に訊いた OR of ANDs** を持っておく。
    groups: [],
    open: null,      // いま開いているノート（一覧の行そのもの）
    stamp: null,     // 開いたときの姿。保存前に「まだ同じファイルか」を訊く
    /// front matter。**エディタには出さないが、保存では必ず戻す。**
    head: '',
    dirty: false,
};

/* ── 印 ── 案 S4「生成りの葉」。**数字は packaging/amber.svg と同じ。**
   `amber.py` の `agree()` が三か所を突き合わせる。片方だけ直すとずれる。 */
let markSeq = 0;
function mark(size) {
    const id = 'am' + (++markSeq);
    return '<svg viewBox="0 0 100 100" width="' + size + '" height="' + size + '" aria-hidden="true">'
        + '<defs>'
        + '<linearGradient id="' + id + 'g" gradientUnits="userSpaceOnUse" x1="15" y1="0" x2="85" y2="100">'
        + '<stop offset="0" stop-color="#ffd97f"/><stop offset="1" stop-color="#f2a62c"/></linearGradient>'
        + '<clipPath id="' + id + 'c"><rect width="100" height="100" rx="26"/></clipPath>'
        + '</defs>'
        + '<rect width="100" height="100" rx="26" fill="url(#' + id + 'g)"/>'
        + '<g clip-path="url(#' + id + 'c)">'
        + '<path d="M12 66 C6 74 0 84 -4 96" fill="none" stroke="#fff4de" stroke-width="7" stroke-linecap="round"/>'
        + '<path d="M10 62 C6 38 26 18 50 15 C74 12 90 24 97 35 C88 50 66 68 44 77 C26 84 12 78 10 62 Z" fill="#fff4de"/>'
        + '<g fill="none" stroke="url(#' + id + 'g)" stroke-width="8" stroke-linecap="round">'
        + '<path d="M24 46 C42 36 60 30 78 27"/>'
        + '<path d="M24 66 C40 57 54 51 66 47"/>'
        + '</g></g></svg>';
}

let sayTimer = null;
function say(text) {
    const box = el('say');
    box.textContent = text;
    box.classList.add('on');
    clearTimeout(sayTimer);
    sayTimer = setTimeout(() => box.classList.remove('on'), 2200);
}

/* ── 行き先（左） ── */

function tagsOf(notes) {
    const seen = new Map();
    for (const n of notes) for (const t of n.tags || []) seen.set(t, (seen.get(t) || 0) + 1);
    return [...seen.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

const starred = (n) => n.star !== null && n.star !== undefined;

function drawRail() {
    const on = (kind, what) => state.dest.kind === kind && state.dest.what === what;
    const rows = [];
    rows.push('<div id="brand">' + mark(26) + '<div class="name">amber</div></div>');
    rows.push('<button id="new">＋　新しいノート</button>');

    rows.push('<div class="head">ノート</div>');
    rows.push(dest('all', '', 'すべてのノート', state.notes.length, on('all', '')));

    const stars = state.notes.filter(starred);
    if (stars.length || state.stars.length) {
        rows.push('<div class="head">お気に入り</div>');
        rows.push(dest('star', '', '★ すべて', stars.length, on('star', '')));
        for (const sh of state.stars) {
            const n = stars.filter((x) => x.star === sh || (x.star || '').startsWith(sh + '/')).length;
            rows.push(dest('star', sh, sh.split('/').pop(), n, on('star', sh), sh.split('/').length - 1));
        }
    }

    if (state.books.length) {
        rows.push('<div class="head">フォルダ</div>');
        for (const b of state.books) {
            const n = state.notes.filter((x) => x.book === b || x.book.startsWith(b + '/')).length;
            rows.push(dest('book', b, b.split('/').pop(), n, on('book', b),
                           b.split('/').length - 1, state.colors[b]));
        }
    }

    const tags = tagsOf(state.notes);
    if (tags.length) {
        rows.push('<div class="head">タグ</div>');
        // 30 で切る。**タグは増える一方**で、全部並べると行き先の列が
        // 「タグの一覧」になり、フォルダもお気に入りも押し出される。
        for (const [t, n] of tags.slice(0, 30)) rows.push(dest('tag', t, '#' + t, n, on('tag', t)));
    }
    el('rail').innerHTML = rows.join('');
    el('new').onclick = newNote;
    for (const d of el('rail').querySelectorAll('.dest')) {
        d.onclick = () => {
            state.dest = { kind: d.dataset.kind, what: d.dataset.what };
            drawRail();
            drawList();
        };
    }
}

function dest(kind, what, name, n, isOn, depth, color) {
    const sq = color ? '<span class="sq" style="background:' + escapeAttr(color) + '"></span>' : '';
    return '<div class="dest' + (isOn ? ' on' : '') + '" data-kind="' + escapeAttr(kind) + '"'
        + ' data-what="' + escapeAttr(what) + '" data-depth="' + (depth || 0) + '">'
        + sq + '<span class="nm">' + escapeHtml(name) + '</span><span class="n">' + n + '</span></div>';
}

/* ── 一覧（中） ── */

function inDest(n) {
    const kind = state.dest.kind;
    const what = state.dest.what;
    if (kind === 'book') return n.book === what || n.book.startsWith(what + '/');
    if (kind === 'tag') return (n.tags || []).includes(what);
    if (kind === 'star') {
        if (!starred(n)) return false;
        return what === '' || n.star === what || (n.star || '').startsWith(what + '/');
    }
    return true;
}

/// 絞り込み。**問いの意味は core が決める**（`note::terms` = OR of ANDs）。
/// 打鍵ごとではなく、文字が変わったときに一度だけ訊く ── iPhone と同じ形。
/// ここでやるのは当てはめだけ: どれか一組の語が全部あればよい。
function narrowed() {
    const here = state.notes.filter(inDest);
    if (!state.filter.trim() || !state.groups.length) return here;
    return here.filter((n) => {
        const hay = (n.search || (n.title + ' ' + n.excerpt)).toLowerCase();
        return state.groups.some((g) => g.every((w) => hay.includes(w.toLowerCase())));
    });
}

function drawList() {
    const rows = narrowed();
    const what = state.dest.what;
    const name = {
        all: 'すべてのノート',
        book: what.split('/').pop(),
        tag: '#' + what,
        star: what ? '★ ' + what.split('/').pop() : '★ お気に入り',
    }[state.dest.kind] || 'すべてのノート';
    el('where').textContent = name;
    const all = state.notes.filter(inDest).length;
    el('count').textContent = rows.length + ' 件' + (rows.length !== all ? '（' + all + ' 件中）' : '');
    if (!rows.length) {
        el('rows').innerHTML = '<div id="empty">'
            + (state.filter.trim() ? '見つかりません' : 'ここにはまだノートがありません')
            + '</div>';
        return;
    }
    el('rows').innerHTML = rows.map((n) => {
        const open = state.open && state.open.path === n.path;
        const tags = (n.tags || []).slice(0, 3)
            .map((t) => '<span class="tag">' + escapeHtml(t) + '</span>').join('');
        return '<div class="row' + (open ? ' on' : '') + '" data-path="' + escapeAttr(n.path) + '">'
            + '<div class="t">' + (starred(n) ? '<span class="star">★</span> ' : '')
            + escapeHtml(n.title || '(題なし)') + '</div>'
            + '<div class="x">' + escapeHtml(n.excerpt || '') + '</div>'
            + '<div class="m"><span class="d">' + when(n.updated) + '</span>'
            + '<span class="tags">' + tags + '</span></div></div>';
    }).join('');
    for (const r of el('rows').querySelectorAll('.row')) {
        r.onclick = () => openNote(r.dataset.path);
    }
}

/// 「いつ」を人の言葉で。**今日と昨日は日付にしない** ── 見て分かるのは
/// 「さっき書いた」であって「09-05」ではない。
function when(secs) {
    if (!secs) return '';
    const d = new Date(secs * 1000);
    const now = new Date();
    const same = (a, b) => a.toDateString() === b.toDateString();
    if (same(d, now)) return d.toTimeString().slice(0, 5);
    if (same(d, new Date(now.getTime() - 86400000))) return '昨日';
    if (d.getFullYear() === now.getFullYear()) return (d.getMonth() + 1) + '/' + d.getDate();
    return d.getFullYear() + '/' + (d.getMonth() + 1) + '/' + d.getDate();
}

/* ── 中身（右） ── */

let editor = null;
let saveTimer = null;
/// 読み込み中は、変更を変更として数えない。
let loading = false;

function makeEditor() {
    return new Promise((resolve) => {
        require.config({ paths: { vs: 'vendor/monaco/vs' } });
        require(['vs/editor/editor.main'], () => {
            const dark = matchMedia('(prefers-color-scheme: dark)').matches;
            editor = monaco.editor.create(el('ed'), {
                value: '',
                language: 'markdown',
                theme: dark ? 'vs-dark' : 'vs',
                automaticLayout: true,
                wordWrap: 'on',
                lineNumbers: 'off',
                minimap: { enabled: false },
                renderLineHighlight: 'none',
                scrollBeyondLastLine: false,
                fontSize: 15,
                lineHeight: 1.85,
                padding: { top: 18, bottom: 48 },
                folding: false,
                occurrencesHighlight: 'off',
                fontFamily: '"Hiragino Sans", "Yu Gothic UI", ui-monospace, monospace',
            });
            editor.onDidChangeModelContent(() => {
                // **読み込みの `setValue` も変更として届く。** 守らないと、
                // 開いただけで自動保存が走り、触っていないノートの更新時刻が
                // 動く（実際に一本動かした）。同期しているフォルダでは、それが
                // 相手側に「向こうが編集した」と見える ── 何もしていないのに。
                if (loading || !state.open) return;
                state.dirty = true;
                el('state').textContent = '書きかけ';
                clearTimeout(saveTimer);
                saveTimer = setTimeout(save, 900);
            });
            resolve();
        });
    });
}

async function openNote(path) {
    const note = state.notes.find((n) => n.path === path);
    if (!note) return;
    // 開く前に、書きかけを置いていかない。
    if (state.dirty) await save();
    if (!editor) await makeEditor();
    let r;
    try {
        r = await ask('read', { path });
    } catch (e) {
        say('開けません: ' + e.message);
        return;
    }
    // **front matter はエディタに出さない。** 題もタグも作った日も、上の帯が
    // 言っている ── 同じことを二度言ううえ、`---` で始まる四行は書き出しの
    // 邪魔でしかない。**どこで切るかは core が決める**（`note::front`）。
    // 保存では必ず頭を戻す ── 落とすと題もタグも消える。
    let head = '';
    let body = r.text || '';
    try {
        const cut = await ask('split', { text: body });
        head = cut.head || '';
        body = cut.body || '';
    } catch {
        // 切れないなら、そのまま全部見せる。**隠して失うより、出して残す。**
        head = '';
    }
    state.open = note;
    state.stamp = r.stamp || null;
    state.head = head;
    state.dirty = false;
    loading = true;
    editor.setValue(body);
    loading = false;
    el('title').textContent = note.title || '(題なし)';
    el('state').textContent = when(note.updated)
        + ((note.tags || []).length ? '  ' + note.tags.map((t) => '#' + t).join(' ') : '');
    el('top').hidden = false;
    el('blank').hidden = true;
    el('ed').hidden = false;
    editor.layout();
    drawList();
    window.amber.remember({ open: path });
}

/// 保存。**誰かが先に書いていたら上書きしない。**
///
/// 同じフォルダを二つの端末で触るのがこのアプリの前提なので、「開いたときと
/// 同じファイルか」を毎回訊く。違えば人に決めてもらう ── 黙ってどちらかを
/// 捨てるのがいちばん悪い。
async function save() {
    if (!state.open || !editor) return;
    const path = state.open.path;
    // 頭を戻してから書く。**ここを忘れると、保存のたびに front matter が
    // 一枚ずつ消える** ── 題もタグも作った日も。
    const text = state.head + editor.getValue();
    try {
        const r = await ask('write', { path, text, stamp: state.stamp });
        if (r && r.conflict) {
            const keep = confirm(
                'このノートは、開いたあとで別のところから書き換えられています。\n\n'
                + (r.why || '') + '\n\nこちらの内容で上書きしますか？\n'
                + '（「キャンセル」なら、向こうの内容を読み直します）');
            if (!keep) {
                state.dirty = false;
                await openNote(path);
                return;
            }
            await ask('write', { path, text, force: true });
        }
        if (r && r.stamp) state.stamp = r.stamp;
        state.dirty = false;
        el('state').textContent = '保存しました';
        await reload({ quiet: true });
        setTimeout(() => {
            if (!state.dirty && state.open && state.open.path === path) {
                el('state').textContent = when(state.open.updated);
            }
        }, 1400);
    } catch (e) {
        el('state').textContent = '保存できません';
        say('保存できません: ' + e.message);
    }
}

async function newNote() {
    // いまフォルダを見ているなら、そこに作る ── 「どこに出来たか分からない」
    // のがいちばん困る。
    const dir = state.dest.kind === 'book' ? state.root + '/' + state.dest.what : state.root;
    try {
        const r = await ask('new', { dir, title: '' });
        await reload({ quiet: true });
        await openNote(r.path);
        if (editor) editor.focus();
    } catch (e) {
        say('作れません: ' + e.message);
    }
}

/* ── 読み直し ── */

async function reload(opts) {
    try {
        const r = await ask('notes', { path: state.root });
        state.notes = r.notes || [];
        state.books = r.books || [];
        state.stars = r.stars || [];
        state.colors = r.colors || {};
        // 開いていた行を新しいほうに繋ぎ直す（更新時刻が動くので）。
        if (state.open) {
            state.open = state.notes.find((n) => n.path === state.open.path) || state.open;
        }
        drawRail();
        drawList();
    } catch (e) {
        if (!opts || !opts.quiet) say('読めません: ' + e.message);
    }
}

/* ── キー ── */

function moveCursor(delta) {
    const rows = [...el('rows').querySelectorAll('.row')];
    if (!rows.length) return;
    const at = rows.findIndex((r) => r.classList.contains('on'));
    const next = rows[Math.min(rows.length - 1, Math.max(0, (at < 0 ? -1 : at) + delta))];
    if (next) {
        next.click();
        next.scrollIntoView({ block: 'nearest' });
    }
}

// **`e.code` で当てる。`e.key` ではない。** JIS 配列では `key` が
// `Zenkaku` にも `Process` にも `Unidentified` にもなり、IME が拾っている
// 間は `?` すら `Process` になる。cian で「Mac では直ったのに JIS で効かない」
// を二件出している。
document.addEventListener('keydown', (e) => {
    const inField = e.target === el('find');
    const inEditor = el('ed').contains(e.target);

    // **修飾キー付きは、素の一文字より先に。**
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyS') { e.preventDefault(); save(); return; }
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyN') { e.preventDefault(); newNote(); return; }
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyF') {
        e.preventDefault();
        el('find').focus();
        el('find').select();
        return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;

    if (e.code === 'Escape') {
        if (inField) { el('find').blur(); return; }
        if (inEditor) { document.activeElement.blur(); return; }
    }
    // 文字を打っている場所では、素の一文字は文字であって命令ではない。
    if (inField || inEditor) return;

    if (e.code === 'ArrowDown' || e.code === 'KeyJ') { e.preventDefault(); moveCursor(1); }
    else if (e.code === 'ArrowUp' || e.code === 'KeyK') { e.preventDefault(); moveCursor(-1); }
    else if (e.code === 'Enter') { e.preventDefault(); if (editor) editor.focus(); }
    else if (e.code === 'KeyN') { e.preventDefault(); newNote(); }
    else if (e.code === 'Slash') { e.preventDefault(); el('find').focus(); el('find').select(); }
});

/* ── 逃がす ── */

function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) =>
        ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}
const escapeAttr = escapeHtml;

/* ── 起動 ── */

(async function boot() {
    if (navigator.userAgent.includes('Mac')) document.body.classList.add('mac');
    el('blankmark').innerHTML = mark(54);
    const saved = await window.amber.recall();
    state.root = saved.root;

    let t = null;
    el('find').oninput = () => {
        state.filter = el('find').value;
        clearTimeout(t);
        t = setTimeout(async () => {
            const q = state.filter.trim();
            // 問いの意味は core に。**打鍵ごとではなく、止まってから一度。**
            state.groups = q ? ((await ask('terms', { q })).groups || []) : [];
            drawList();
        }, 150);
    };

    await reload();
    if (saved.open && state.notes.some((n) => n.path === saved.open)) await openNote(saved.open);

    // **保存しかけたまま閉じない。**
    window.addEventListener('beforeunload', () => { if (state.dirty) save(); });
})();
