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
    /// 期間の絞り込み（`{ which: 'updated'|'created', days }`）。無ければ null。
    when: null,
    /// amber の外にある一本を、単発で開いているか。
    guest: false,
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

/* ── 印 ── */

/// アプリの中の印。**アプリのアイコンそのもの**を小さくして出す。
///
/// 前はここに葉（案 S4）を SVG で写して描いていた。写しは `packaging/amber.svg`・
/// `packaging/amber.py`・`ios/Cian/Writing.swift` にもあり、四か所が同じ形かを
/// `agree()` が見張っていた ── それでも**アイコンを替えた日に、中の印だけが
/// 前の絵のまま残った**。見張れるのは「四つの写しが揃っているか」であって、
/// 「アイコンと同じか」ではなかった。同じ一枚を渡せば、ずれようがない。
///
/// 128px の一枚で足りる ── いちばん大きい使い方（54）の2倍と、iPhone の
/// 38pt の3倍（114）を両方覆う。
function mark(size) {
    return '<img src="../packaging/amber-mark.png" width="' + size + '"'
        + ' height="' + size + '" alt="" aria-hidden="true">';
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
    rows.push('<div id="railtop"></div>');
    // 「＋」は全角の空白で離していた ── 字と記号のあいだが不揃いになる。
    // 印は札の中に描く（同じ太さ・同じ大きさで、字と揃う）。
    rows.push('<button id="new">'
        + '<svg viewBox="0 0 16 16" aria-hidden="true">'
        + '<path d="M8 3.2v9.6M3.2 8h9.6" stroke="currentColor" stroke-width="1.9"'
        + ' stroke-linecap="round"/></svg>'
        + '<span>新しいノート</span></button>');

    rows.push('<div class="head">ノート</div>');
    rows.push(dest('all', '', 'すべてのノート', state.notes.length, on('all', '')));

    const stars = state.notes.filter(starred);
    {
        // **一つも無くても段は出す。** 無いときに段ごと消えると、
        // 最初の一つを作る道がどこにも無くなる ── 「実装されていないのか、
        // 見えないだけなのか」が使う人には見分けられない。
        rows.push(head('ブックマーク', 'star'));
        rows.push(dest('star', '', '★ すべて', stars.length, on('star', '')));
        for (const sh of state.stars) {
            const n = stars.filter((x) => x.star === sh || (x.star || '').startsWith(sh + '/')).length;
            rows.push(dest('star', sh, sh.split('/').pop(), n, on('star', sh), sh.split('/').length - 1));
        }
    }

    {
        rows.push(head('フォルダ', 'book'));
        for (const b of state.books) {
            const n = state.notes.filter((x) => x.book === b || x.book.startsWith(b + '/')).length;
            rows.push(dest('book', b, b.split('/').pop(), n, on('book', b),
                           b.split('/').length - 1, state.colors[b]));
        }
    }

    const tags = tagsOf(state.notes);
    {
        rows.push(head('タグ', 'tag'));
        // 30 で切る。**タグは増える一方**で、全部並べると行き先の列が
        // 「タグの一覧」になり、フォルダもブックマークも押し出される。
        for (const [t, n] of tags.slice(0, 30)) rows.push(dest('tag', t, '#' + t, n, on('tag', t)));
    }
    el('rail').innerHTML = rows.join('');
    el('new').onclick = newNote;
    for (const b of el('rail').querySelectorAll('.plus')) {
        b.onclick = (e) => { e.stopPropagation(); railPlus(b.dataset.plus); };
    }
    for (const d of el('rail').querySelectorAll('.dest')) {
        d.onclick = () => {
            state.dest = { kind: d.dataset.kind, what: d.dataset.what };
            drawRail();
            drawList();
        };
        d.oncontextmenu = (e) => {
            e.preventDefault();
            railMenu(d.dataset.kind, d.dataset.what, { x: e.clientX, y: e.clientY });
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
    let here = state.notes.filter(inDest);
    // **期間は言葉ではなく、日付そのもので絞る。** `updated:` のような
    // 書き方を増やすと、覚える記法がまた一つ増える ── これは札で出して
    // 札で外すもの。
    if (state.when) {
        const key = state.when.which;
        const days = state.when.days;
        const edge = Date.now() / 1000 - Math.abs(days) * 86400;
        here = here.filter((n) => {
            const at = n[key];
            if (!at) return false;
            return days < 0 ? at < Date.now() / 1000 - 365 * 86400 : at >= edge;
        });
    }
    if (!state.filter.trim() || !state.groups.length) return here;
    return here.filter((n) => state.groups.some((g) => g.every((t) => hitTerm(n, t))));
}

/// 一語が当たるか。**見出しごとに探し先が違う。**
///
/// `tag:定型` `book:仕事` `title:週報`（`タグ:` `フォルダ:` `題:` も同じ）と
/// `-` の打ち消し。どれが見出しでどれが字かを決めるのは `note::terms` で、
/// ここは決めない ── 窓が自分で `:` を数えはじめると、iPhone と別のものが
/// 見つかる検索窓が二つできる。
///
/// **`body:` は無い。** 一覧が持っているのは本文の頭 100 字だけなので、
/// 受けると「本文を探したのに見つからない」を作る。奥の一文は `find` の仕事。
function hitTerm(n, t) {
    let hay;
    switch (t.field) {
        case 'title': hay = n.title || ''; break;
        case 'tag': hay = (n.tags || []).join(' '); break;
        case 'book': hay = n.book || ''; break;
        default: hay = n.search || ((n.title || '') + ' ' + (n.excerpt || ''));
    }
    return hay.toLowerCase().includes(t.word) !== t.not;
}

/// 一覧の並び。**iPhone と同じ三つ**（`NotesStore.sorted`）。
///
/// 名前順は `localeCompare(..., {numeric: true})` ── 素の `<` は「あ」と「い」も
/// `note-2` と `note-10` も両方まちがえる。iPhone 側は Foundation の
/// `localizedStandardCompare` で、**同じ規則をそれぞれの土地の言葉で言って
/// いる**。core に上げなかったのはそのため ── Rust には土地を知った自然順が
/// 標準に無く、上げると iPhone の並びのほうが悪くなる。
const ORDERS = [
    ['updated', '更新が新しい順'],
    ['created', '作成が新しい順'],
    ['title', '名前順'],
];
let order = 'updated';

function sortNotes(list) {
    const out = [...list];
    if (order === 'title') {
        out.sort((a, b) => (a.title || '').localeCompare(b.title || '', 'ja', { numeric: true }));
    } else if (order === 'created') {
        out.sort((a, b) => (b.created || 0) - (a.created || 0));
    } else {
        out.sort((a, b) => (b.updated || 0) - (a.updated || 0));
    }
    return out;
}

function drawOrder() {
    const at = ORDERS.findIndex(([k]) => k === order);
    el('order').textContent = ORDERS[at < 0 ? 0 : at][1];
}

el('findhow').onclick = () => cmdFind();
el('whenchip').onclick = () => { state.when = null; drawFind(); drawList(); };
el('guestclose').onclick = closeGuest;
for (const b of el('tablebar').querySelectorAll('button')) {
    // 押した瞬間に caret を失わないように、`mousedown` を止める。
    b.onmousedown = (e) => e.preventDefault();
    b.onclick = () => tableDo(b.dataset.do);
}

el('order').onclick = () => {
    const at = ORDERS.findIndex(([k]) => k === order);
    order = ORDERS[(at + 1) % ORDERS.length][0];
    window.amber.remember({ order });
    drawOrder();
    drawList();
};

function drawList() {
    const rows = sortNotes(narrowed());
    const what = state.dest.what;
    const name = {
        all: 'すべてのノート',
        book: what.split('/').pop(),
        tag: '#' + what,
        star: what ? '★ ' + what.split('/').pop() : '★ ブックマーク',
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
    // ブックマークは**上に別枠で**（iPhone と同じ形）。並び順に混ぜて上へ
    // 寄せるのではなく、別の段にする ── 二つの場所に同じノートが出ると、
    // 人はそれを二度消そうとする。「上に留める」を別に作らないのはこれが
    // あるからで、「これは大事」と言う道を二つ持たない。
    const stuck = state.dest.kind === 'star' ? [] : rows.filter(starred);
    const rest = state.dest.kind === 'star' ? rows : rows.filter((n) => !starred(n));
    let html = '';
    if (stuck.length) {
        html += '<div class="sect">ブックマーク</div>' + stuck.map(row).join('');
        if (rest.length) html += '<div class="sect">ノート</div>';
    }
    html += rest.map(row).join('');
    el('rows').innerHTML = html;
    for (const r of el('rows').querySelectorAll('.row')) {
        r.onclick = () => openNote(r.dataset.path);
        // 右押しでも、⋯ と同じ献立。**開いてから出す** ── 開いていない
        // ノートに「削除」を出すと、どれが消えるのか画面が言っていない。
        r.oncontextmenu = async (e) => {
            e.preventDefault();
            if (!state.open || state.open.path !== r.dataset.path) await openNote(r.dataset.path);
            openMenu({ right: e.clientX + 190, bottom: e.clientY });
        };
    }
}

function row(n) {
    const open = state.open && state.open.path === n.path;
    const tags = (n.tags || []).slice(0, 3)
        .map((t) => '<span class="tag">' + escapeHtml(t) + '</span>').join('');
    // チェックのあるノートは、いくつ済んだかを出す。**数えるだけで、
    // 新しい欄は作らない** ── 進み具合は既にノートの中に書いてある。
    const done = (n.excerpt || '').match(/\[x\]/gi)?.length || 0;
    const todo = (n.excerpt || '').match(/\[ \]/g)?.length || 0;
    const bar = done + todo ? '<span class="done">' + done + '/' + (done + todo) + '</span>' : '';
    return '<div class="row' + (open ? ' on' : '') + '" data-path="' + escapeAttr(n.path) + '">'
        + '<div class="t">' + (starred(n) ? '<span class="star">★</span> ' : '')
        + escapeHtml(n.title || '(題なし)') + '</div>'
        + '<div class="x">' + escapeHtml(n.excerpt || '') + '</div>'
        + '<div class="m"><span class="d">' + when(n.updated) + '</span>' + bar
        + '<span class="tags">' + tags + '</span></div></div>';
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
        // **絶対の道で渡す。**
        //
        // 相対のままだと、Monaco が worker のために組み立てる道が
        // `file:///vendor/monaco/…`（ファイルシステムの根から）になり、
        // 取り込みに失敗する ── 失敗しても本体スレッドに落ちて動くので
        // 気づかないが、長いノートで窓が固まる。`console-message` を
        // 端末へ流して初めて見えた。
        const here = new URL('vendor/', location.href).href;
        require.config({
            paths: {
                vs: here + 'monaco/vs',
                'monaco-vim': here + 'monaco-vim/monaco-vim.umd',
            },
        });
        window.MonacoEnvironment = {
            getWorkerUrl: () => here + 'monaco/vs/base/worker/workerMain.js',
        };
        require(['vs/editor/editor.main'], () => {
            const dark = isDark();
            editor = monaco.editor.create(el('ed'), {
                value: '',
                language: 'markdown',
                theme: dark ? 'vs-dark' : 'vs',
                automaticLayout: true,
                wordWrap: 'on',
                lineNumbers: lineNo ? 'on' : 'off',
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
            // **Monaco の中でも同じキーを鳴らす。** エディタが先に拾って
            // `document` まで来ない組み合わせがあり、そのときだけ効かない
            // という形で必ず一度は踏む。
            const KM = monaco.KeyMod;
            const KC = monaco.KeyCode;
            editor.addCommand(KM.CtrlCmd | KC.KeyE, () => toggleRead());
            editor.addCommand(KM.CtrlCmd | KC.KeyP, () => toggleSplit());
            editor.addCommand(KC.F12, () => setZen(!zen));

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
                readSoon();
                drawCount();
                zonesSoon();
                if (tocOn) clearTimeout(tocTimer), tocTimer = setTimeout(drawToc, 400);
            });
            // 憶えていたなら vim で始める。**作った直後に。** 後から
            // 入れると、最初のノートだけ素のまま、という形になる。
            if (fontStep) setFont(fontStep, true);
            if (vimOn) setVim(true);
            resolve();
        });
    });
}

async function openNote(path, opts) {
    // 一覧に無い一本（外から来たもの）は、`opts.guest` が持ってくる。
    const note = (opts && opts.guest) || state.notes.find((n) => n.path === path);
    if (!note) return;
    // たどっている最中は積まない ── 積むと前へ戻れなくなる。
    if (!opts || !opts.walking) trailPush(path);
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
    drawCount();
    applyView();
    drawZones();
    if (!state.guest) drawList();
    window.amber.remember({ open: path });
}

/* ── vim ── */

/// **既定は素のメモ帳。** 入れたい人だけが入れる ── 知らずに入っていると、
/// `i` を押すまで一文字も打てない画面になり、それは壊れているのと同じに見える。
///
/// 中身は `monaco-vim`（CodeMirror の vim をそのまま移したもの）── 自前で
/// 書くと、`ci"` や `.` のような「本物なら動くのに動かない」に必ず当たる。
let vimOn = false;
let vimMode = null;
let VimLib = null;

/// `monaco-vim` を読む。UMD の AMD の枝が `monaco-editor/esm/…/editor.api`
/// を要求してくるので、**既に読んである `monaco` を返す偽物**を先に置く。
function loadVim() {
    if (VimLib) return Promise.resolve(VimLib);
    return new Promise((resolve, reject) => {
        try {
            define('monaco-editor/esm/vs/editor/editor.api', [], () => monaco);
        } catch {
            // 二度目は既に定義済み。**それは失敗ではない。**
        }
        require(['monaco-vim'], (lib) => { VimLib = lib; resolve(lib); }, reject);
    });
}

async function setVim(on) {
    vimOn = on;
    document.body.classList.toggle('vim', on);
    window.amber.remember({ vim: on });
    drawMarks();
    if (!editor) return;
    if (!on) {
        if (vimMode) { vimMode.dispose(); vimMode = null; }
        el('vim').textContent = '';
        editor.focus();
        return;
    }
    try {
        const lib = await loadVim();
        vimMode = lib.initVimMode(editor, el('vim'));
        editor.focus();
    } catch (e) {
        // 読めなければ素のまま。**入れられないことで書けなくなる理由は無い。**
        vimOn = false;
        document.body.classList.remove('vim');
        drawMarks();
        say('vim を読めません: ' + (e && e.message ? e.message : e));
    }
}

/* ── 書く面の絵 ── */

/// `![](attachments/…)` の行の下に、実物を小さく出す。
///
/// **字は消さない。** ファイルは Markdown のままで、行はそこにある ──
/// 消して絵に置き換えると、消した字を直す方法が無くなる（パスを一文字
/// 変えたいだけのときに困る）。Monaco の `view zone` は行と行のあいだに
/// 空きを作る仕掛けで、そこへ絵を置く。
///
/// 貼った直後に「本当にこれが入ったのか」を確かめる手が、いまは無かった。
let zones = [];
let zoneTimer = null;

function drawZones() {
    if (!editor || !state.open) return;
    const model = editor.getModel();
    if (!model) return;
    const dir = state.open.path.replace(/[^/]*$/, '');
    const want = [];
    for (let n = 1; n <= model.getLineCount(); n++) {
        const t = model.getLineContent(n).trim();
        // 行そのものが絵のときだけ。文の途中の絵は文の中に居る。
        const m = /^!\[([^\]]*)\]\(([^)\s]+)\)$/.exec(t);
        if (!m) continue;
        const src = m[2];
        if (/^[a-z][a-z0-9+.-]*:/i.test(src) && !/^file:/i.test(src)) continue;
        want.push({ line: n, alt: m[1], src });
    }

    editor.changeViewZones((acc) => {
        for (const z of zones) acc.removeZone(z);
        zones = [];
        for (const w of want) {
            const box = document.createElement('div');
            box.className = 'zoneimg';
            const img = document.createElement('img');
            img.src = /^file:/i.test(w.src) ? w.src
                : 'file://' + encodeURI(w.src.startsWith('/') ? w.src : dir + w.src);
            img.alt = w.alt;
            // **読めない絵は、黙って空けない。** 貼り間違いに気づけるように、
            // 何が読めなかったのかを出す。
            img.onerror = () => {
                box.classList.add('bad');
                box.textContent = 'この絵は読めません: ' + w.src;
            };
            box.append(img);
            zones.push(acc.addZone({
                afterLineNumber: w.line,
                heightInPx: 128,
                domNode: box,
            }));
        }
    });
}

/// 打っている間は数え直さない ── 一文字ごとに全行を見るのは高い。
function zonesSoon() {
    clearTimeout(zoneTimer);
    zoneTimer = setTimeout(drawZones, 350);
}

/* ── 読む面で書く ── */

/// **読む面は、読むだけの面ではない。**
///
/// 押して入力欄を開く、という一手を挟まない ── メモ帳と同じで、置いて
/// 打てる。記号は見えないまま、下の帯（太字・斜体…）がそのまま効く。
///
/// 仕掛けは三つ:
///
///   * 面ぜんぶを `contenteditable` にする。打った跡は DOM に付く
///   * 落ち着いたら **DOM を Markdown に戻して**、いつもの保存を通す
///   * **戻せないかたまりは、触らせない** ── 枠・表・図・注記・絵は
///     `contenteditable="false"` にして、押したら書く面へ送る。
///     打てるのに保存されない、が**いちばん悪い**
///
/// 戻せるのは `to_html` が出す札だけ ── 語彙はこちらが決めているので、
/// 逆に読むのも数十行で済む。**外から来た HTML は入ってこない**（貼り付けは
/// 字だけにしている）。
let readTimer = null;
/// 書き戻している間は、描き直しを止める（自分の保存で自分を消さない）。
let syncing = false;

/// 打った跡を拾う。**面ぜんぶが入力欄なので、`input` 一本で足りる。**
el('read').addEventListener('input', () => { readChanged(); tableBar(); });
document.addEventListener('selectionchange', () => {
    if (view !== 'write' && state.open) tableBar();
});
el('read').addEventListener('blur', () => { clearTimeout(readTimer); syncRead(); }, true);

/// 貼り付けは**字だけ**入れる。
///
/// よそから来た HTML をそのまま入れると、`inlineToMd` が知らない札が
/// 混ざり、字に戻したときに消える ── 貼ったつもりのものが無い、が
/// いちばん悪い。絵の貼り付けは別に拾っている。
el('read').addEventListener('paste', (e) => {
    if (!e.clipboardData) return;
    if ([...e.clipboardData.items].some((i) => i.kind === 'file' && i.type.startsWith('image/'))) return;
    e.preventDefault();
    document.execCommand('insertText', false, e.clipboardData.getData('text/plain'));
});

/// Enter で `<div>` ではなく `<p>` を作らせる。
///
/// **飾りは `style` にさせない**（`styleWithCSS` は偽のまま）── 真にすると
/// 太字が `<b>` ではなく `<span style="font-weight:bold">` になり、字に
/// 戻すときに飾りが落ちる。**太字にしたのに保存されない**、という形で出た。
/// 色だけは `<font color>` で来るので、そちらを読む（`inlineToMd`）。
try {
    document.execCommand('defaultParagraphSeparator', false, 'p');
    document.execCommand('styleWithCSS', false, false);
} catch { /* 古い呼び方なので、断られても書けなくはならない */ }

/// 触ってはいけないかたまりか。
function richBlock(node) {
    if (!node || node.nodeType !== 1) return false;
    // **表と注記は触れる。** 字に戻せる形をしているので、触らせない理由が
    // 無い ── 触れないままだと「読む面だけで完結できる」が嘘になる。
    // 枠（コード）と図と絵だけは、戻せないので書く面へ送る。
    if (['PRE', 'FIGURE'].includes(node.tagName)) return true;
    return node.classList.contains('mermaid');
}

/// 描いたあとの仕込み。
///
/// **書いてあった字を、かたまりごとに持たせておく**（`data-md`）── 戻せない
/// ものは、これをそのまま返す。行番号は `to_html` が差している。
function armRead() {
    const box = el('read');
    const open = !!state.open && view !== 'write';
    box.contentEditable = open ? 'true' : 'false';
    box.spellcheck = false;
    if (!open) return;
    const src = whole().split('\n');
    for (const node of [...box.children]) {
        const at = Number(node.dataset.line);
        const span = Number(node.dataset.span) || 1;
        if (!Number.isNaN(at)) node.dataset.md = src.slice(at, at + span).join('\n');
        if (richBlock(node)) {
            node.contentEditable = 'false';
            node.title = node.classList.contains('mermaid') || node.querySelector('code.language-mermaid')
                ? '押すと、図を見ながら直せます'
                : '押すと、書く面のその行へ';
        }
    }
    // 升は字ではなく操作 ── 中に caret が入ると、押せるものが打てるものに見える。
    for (const b of box.querySelectorAll('.box')) b.contentEditable = 'false';
    // 注記の種類の札は、中身ではなく `> [!NOTE]` の言い換え ── 打てると
    // 「注意」を「ちゅうい」に直せてしまい、それは記法を壊す。
    for (const h of box.querySelectorAll('.alert-h')) h.contentEditable = 'false';
}

/// DOM を Markdown に戻す。
function readToMd() {
    const out = [];
    for (const node of el('read').children) {
        if (richBlock(node)) {
            // **書いてあった字をそのまま返す。** 図や枠を読み解いて
            // 組み直すより、触らせないほうが失わない。
            //
            // 持っていないものが一つでもあれば、**書き戻さない** ──
            // 空を返すと、そのかたまりが黙って消える。消えたことに
            // 気づけるのは、たいてい何回か保存したあと。
            if (node.dataset.md === undefined) return null;
            out.push(node.dataset.md);
            continue;
        }
        const md = blockToMd(node);
        if (md !== null) out.push(md);
    }
    // 前書きの後ろに一行空ける ── 新しいノートがそう作られるので、
    // ここで詰めると、触っただけのノートが**同期先で差分**になる。
    const body = out.filter((s) => s !== '').join('\n\n') + '\n';
    return state.head ? '\n' + body : body;
}

function blockToMd(node, depth = 0) {
    if (node.nodeType === 3) return node.data.trim() ? node.data : null;
    if (node.nodeType !== 1) return null;
    const pad = '  '.repeat(depth);
    // 注記は `> [!NOTE]` に戻す。**種類の札は中身ではない**ので、
    // 見出しの一行（`.alert-h`）は書き出さず、class から取り直す。
    if (node.classList.contains('alert')) {
        const kind = [...node.classList].find((c) => c !== 'alert') || 'note';
        const body = [...node.children]
            .filter((c) => !c.classList.contains('alert-h'))
            .map((c) => blockToMd(c))
            .filter((x) => x !== null).join('\n\n');
        return ['> [!' + kind.toUpperCase() + ']',
                ...body.split('\n').map((l) => (l ? '> ' + l : '>'))].join('\n');
    }
    switch (node.tagName) {
        case 'H1': case 'H2': case 'H3': case 'H4': case 'H5': case 'H6':
            return '#'.repeat(Number(node.tagName[1])) + ' ' + inlineToMd(node);
        case 'UL': case 'OL': {
            // **字に戻すあいだ、画面には一切触らない。**
            //
            // 前はここで升（`.box`）と入れ子の箇条書きを `remove()` して、
            // 読み終えてから付け直していた。入れ子は戻していたが**升は
            // 戻していなかった** ── 一度書き戻すと升が画面から消え、次の
            // 書き戻しではただの `- やること` になって、チェックが
            // ファイルから消えた。「表示面で打っていたらチェックリストが
            // 消えた」はこれ。
            //
            // 読むだけで済むものを、動かして読む理由は無い。
            const rows = [];
            let n = 0;
            for (const li of node.children) {
                if (li.tagName !== 'LI') continue;
                n += 1;
                const mark = li.querySelector(':scope > .box');
                const head = mark
                    ? '- [' + (mark.getAttribute('aria-pressed') === 'true' ? 'x' : ' ') + '] '
                    : (node.tagName === 'OL' ? n + '. ' : '- ');
                rows.push(pad + head + inlineToMd(li).trim());
                // 入れ子は項目の中に居る。字の上では、その項目の下に付く。
                for (const x of li.children) {
                    if (['UL', 'OL'].includes(x.tagName)) rows.push(blockToMd(x, depth + 1));
                }
            }
            return rows.join('\n');
        }
        case 'TABLE': {
            const rows = [...node.querySelectorAll('tr')];
            if (!rows.length) return null;
            const cells = (tr) => [...tr.children]
                .map((c) => inlineToMd(c).trim().replace(/\|/g, '\\|') || '　');
            const aligns = [...rows[0].children].map((c) => {
                const a = c.getAttribute('style') || '';
                return a.includes('center') ? ':---:' : (a.includes('right') ? '---:' : '---');
            });
            const out = ['| ' + cells(rows[0]).join(' | ') + ' |',
                         '| ' + aligns.join(' | ') + ' |'];
            for (const tr of rows.slice(1)) out.push('| ' + cells(tr).join(' | ') + ' |');
            return out.join('\n');
        }
        case 'BLOCKQUOTE':
            return blockLines(node).map((l) => (l ? '> ' + l : '>')).join('\n');
        case 'HR':
            return '---';
        case 'BR':
            return null;
        default: {
            const t = inlineToMd(node).trim();
            return t === '' ? null : pad + t;
        }
    }
}

function blockLines(node) {
    const out = [];
    for (const c of node.children) {
        const md = blockToMd(c);
        if (md !== null) out.push(...md.split('\n'));
    }
    if (!out.length) {
        const t = inlineToMd(node).trim();
        if (t) out.push(t);
    }
    return out;
}

/// 一つのかたまりの中を、Markdown の字に戻す。
///
/// **札の語彙はこちらが決めている**（`to_html` が出すもの）ので、
/// 知らない札は中身だけ取る ── 貼り付けで紛れ込んだ札を、記号として
/// 書き出さないため。
function inlineToMd(node) {
    let out = '';
    for (const c of node.childNodes) {
        if (c.nodeType === 3) { out += c.data; continue; }
        if (c.nodeType !== 1) continue;
        // 升は字ではなく操作 ── 行頭の `- [ ] ` として既に書いてある。
        if (c.classList && c.classList.contains('box')) continue;
        // 入れ子の箇条書きは、かたまりとして別に書く。
        if (['UL', 'OL'].includes(c.tagName)) continue;
        const inner = inlineToMd(c);
        switch (c.tagName) {
            case 'STRONG': case 'B': out += inner.trim() ? '**' + inner + '**' : ''; break;
            case 'EM': case 'I': out += inner.trim() ? '*' + inner + '*' : ''; break;
            case 'DEL': case 'S': case 'STRIKE': out += inner.trim() ? '~~' + inner + '~~' : ''; break;
            case 'CODE': out += '`' + c.textContent + '`'; break;
            case 'A': out += '[' + inner + '](' + (c.getAttribute('href') || '') + ')'; break;
            case 'BR': out += '\n'; break;
            case 'IMG': out += ''; break;
            case 'FONT': case 'SPAN': {
                // 色だけは記法に戻す ── ほかの飾りは字だけ取る。
                //
                // `execCommand('foreColor')` は `<font color="#rrggbb">` を
                // 置く。`styleWithCSS` を真にすれば `<span style>` になるが、
                // そうすると太字まで `<span>` になって落ちるので、こちらで
                // 両方を読む。
                const raw = (c.getAttribute('color') || '') + ' ' + (c.getAttribute('style') || '');
                const hex = /#[0-9a-f]{6}/i.exec(raw);
                // 太字・斜体を `style` で持っている札も、拾えるだけ拾う。
                let t = inner;
                if (/font-weight:\s*(bold|[6-9]00)/i.test(raw) && t.trim()) t = '**' + t + '**';
                if (/font-style:\s*italic/i.test(raw) && t.trim()) t = '*' + t + '*';
                if (/line-through/i.test(raw) && t.trim()) t = '~~' + t + '~~';
                out += hex ? '<span style="color:' + hex[0].toLowerCase() + '">' + t + '</span>' : t;
                break;
            }
            default: out += inner;
        }
    }
    return out;
}

/// 打ったら、落ち着いてから書き戻す。
function readChanged() {
    if (syncing || view === 'write' || !state.open) return;
    state.dirty = true;
    el('state').textContent = '書きかけ';
    clearTimeout(readTimer);
    readTimer = setTimeout(syncRead, 700);
}

/// DOM を字に戻して、いつもの保存を通す。
///
/// **描き直さない。** 打っている最中に組み直すと、caret がどこかへ飛ぶ ──
/// 見た目は既に打った通りになっているので、組み直す理由も無い。
async function syncRead() {
    if (syncing || !state.open || !editor) return;
    const body = readToMd();
    if (body === null) {
        // **黙って止まらない。** 打った字が消えたように見えるのがいちばん悪い。
        say('保存できません ── 図かコード枠の元の字が取れません。'
            + '「コード」の面で直してください');
        el('state').textContent = '保存できません';
        return;
    }
    if (state.head + body === whole()) return;
    syncing = true;
    try {
        loading = true;
        editor.setValue(body);
        loading = false;
        state.dirty = true;
        await save();
        // 行番号がずれたので、持たせ直す（描き直さずに）。
        armRead();
        drawCount();
        if (tocOn) drawToc();
    } finally {
        syncing = false;
    }
}

/// 注記を入れる。**種類は選ばせる** ── `> [!WARNING]` を覚えている人は
/// 少なく、覚えていなければ無いのと同じ。
async function cmdAlert() {
    const kind = await askPick('どの注記', [
        { name: 'ノート', sub: '覚えておくこと', value: 'NOTE' },
        { name: 'こつ', sub: '知っていると楽なこと', value: 'TIP' },
        { name: '大事', sub: '見落とすと困ること', value: 'IMPORTANT' },
        { name: '注意', sub: '気をつけること', value: 'WARNING' },
        { name: '危険', sub: '取り返しがつかないこと', value: 'CAUTION' },
    ], 'GitHub でも同じ形で出ます');
    if (kind === null) return;
    const body = '> [!' + kind + ']\n> ';
    if (onRead()) await readPut(body);
    else put(body);
}

/* ── 表 ── */

/// caret が表の中にあるとき、そのすぐ上に道具を出す。
///
/// **縦棒を数えさせない。** 打ち込む表は縦棒を数え続ける表で、揃え方の行
/// （`:---` `---:`）の形は誰も覚えていない ── iPhone の表と同じ考えで、
/// 数えるのは機械の仕事にする。
function tableBar() {
    const bar = el('tablebar');
    const cell = caretCell();
    if (!cell) { bar.hidden = true; return; }
    const table = cell.closest('table');
    const box = table.getBoundingClientRect();
    bar.hidden = false;
    bar.style.left = box.left + 'px';
    bar.style.top = Math.max(box.top - 34, 8) + 'px';
}

function caretCell() {
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return null;
    let n = sel.getRangeAt(0).startContainer;
    if (n.nodeType === 3) n = n.parentNode;
    const cell = n && n.closest ? n.closest('th, td') : null;
    return cell && el('read').contains(cell) ? cell : null;
}

/// 表の道具。DOM をそのまま組み替えて、あとは `readToMd` が字に戻す。
function tableDo(what) {
    const cell = caretCell();
    if (!cell) return;
    const table = cell.closest('table');
    const row = cell.parentElement;
    const at = [...row.children].indexOf(cell);
    const body = table.querySelector('tbody') || table;
    const rows = [...table.querySelectorAll('tr')];

    const blank = (tag) => {
        const c = document.createElement(tag);
        // **空の升は全角空白で埋める。** 中身が空の升は描く側によっては
        // 消えてしまい、消えた表は「作れなかった」に見える。
        c.textContent = '　';
        return c;
    };

    if (what === 'row+') {
        const tr = document.createElement('tr');
        for (let i = 0; i < row.children.length; i++) tr.append(blank('td'));
        if (row.parentElement === body) row.after(tr);
        else body.prepend(tr);
    } else if (what === 'row-') {
        // 見出しの行は消さない ── 消すと表でなくなる。
        if (row.parentElement !== body || rows.length <= 2) { say('この行は消せません'); return; }
        row.remove();
    } else if (what === 'col+') {
        for (const tr of rows) {
            const isHead = tr.children[0] && tr.children[0].tagName === 'TH';
            tr.children[at].after(blank(isHead ? 'th' : 'td'));
        }
    } else if (what === 'col-') {
        if (rows[0].children.length <= 1) { say('最後の列は消せません'); return; }
        for (const tr of rows) tr.children[at]?.remove();
    } else if (what.startsWith('align')) {
        const how = what.slice(6);
        for (const tr of rows) {
            const c = tr.children[at];
            if (!c) continue;
            if (how === 'left') c.removeAttribute('style');
            else c.setAttribute('style', 'text-align:' + how);
        }
    }
    readChanged();
    setTimeout(tableBar, 0);
}

/* ── 読む面の道具 ── */

/// caret の居る、いちばん外のかたまり。
/// 最後に caret が居たかたまり。
///
/// **押した瞬間には、もう分からない。** 帯の釦を押すと焦点は釦へ移り、
/// 選択も消える ── そのとき `getSelection()` を訊いても「どこでもない」
/// としか返らず、末尾に落ちる（「図がノートのいちばん下に入った」はこれ）。
/// だから**動いたときに憶えておく**。
let caretAt = null;

document.addEventListener('selectionchange', () => {
    const box = el('read');
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return;
    let n = sel.getRangeAt(0).startContainer;
    if (n.nodeType === 3) n = n.parentNode;
    if (!n || !box.contains(n)) return;
    while (n && n.parentElement !== box) n = n.parentElement;
    if (n) caretAt = n;
});

function caretBlock() {
    const box = el('read');
    const sel = getSelection();
    if (sel && sel.rangeCount) {
        let n = sel.getRangeAt(0).startContainer;
        if (n.nodeType === 3) n = n.parentNode;
        if (n && box.contains(n)) {
            while (n && n.parentElement !== box) n = n.parentElement;
            if (n) return n;
        }
    }
    // 焦点が外れたあとは、最後に居た場所。まだ画面に居るときだけ。
    if (caretAt && box.contains(caretAt)) return caretAt;
    return box.lastElementChild;
}

/// 見た目をその場で変える道具（字の飾り）。
///
/// `execCommand` は古い呼び方だが、**`contenteditable` で選んだところに
/// 飾りを付ける道は、いまも実質これしかない**。付くのは `<b>` や `<i>` で、
/// 字に戻すときに `**` や `*` になる（`inlineToMd`）。
function readDress(cmd) {
    el('read').focus();
    const sel = getSelection();
    const had = sel && !sel.isCollapsed;
    document.execCommand(cmd);
    // **飾りの外へ caret を出す。**
    //
    // 選んだところに `<strong>` を掛けると、caret はその中に残る ──
    // 続けて打った字まで太字になる（「あああ だけ太字にしたいのに、
    // その後もずっと太字」）。選んでいたときだけ、掛けた札の**すぐ後ろ**へ
    // 出す。選んでいなければ「ここから太字」の意味なので、そのまま。
    if (!had) { readChanged(); return; }
    const now = getSelection();
    if (now && now.rangeCount) {
        let n = now.getRangeAt(0).endContainer;
        if (n.nodeType === 3) n = n.parentNode;
        const dress = n.closest ? n.closest('strong, b, em, i, del, s, strike, code') : null;
        if (dress && el('read').contains(dress)) {
            const r = document.createRange();
            r.setStartAfter(dress);
            r.collapse(true);
            now.removeAllRanges();
            now.addRange(r);
        }
    }
    readChanged();
}

/// かたまりの種類を変える道具（見出し・箇条書き・引用）。
function readBlockAs(what) {
    el('read').focus();
    if (what === 'ul') document.execCommand('insertUnorderedList');
    else if (what === 'ol') document.execCommand('insertOrderedList');
    else document.execCommand('formatBlock', false, what);
    readChanged();
}

/// 見出しは押すたびに深くなる ── 書く面と同じ（`#` → `##` → `###` → 無し）。
function readHeading() {
    const n = caretBlock();
    const now = n && /^H[1-6]$/.test(n.tagName) ? Number(n.tagName[1]) : 0;
    readBlockAs(now >= 3 ? 'p' : 'h' + (now + 1));
}

/// 字そのものを書き換える道具（チェック・リンク・表…）。
///
/// **いったん全部を字に戻してから直し、組み直す。** 見た目の上でやろうと
/// すると、升や表のような「札の形が決まっているもの」を DOM の上で組み立て
/// 直すことになり、そこだけ別の作り方が生える。
async function readSourceEdit(change, node) {
    const box = el('read');
    // 直すところは、たいてい caret のあるかたまり。**押して開く工房だけは
    // 別** ── 右押しは caret を動かさないので、押されたものを名指しで渡す。
    const at = [...box.children].indexOf(node || caretBlock());
    if (at < 0) return;
    const blocks = [...box.children].map((n) =>
        richBlock(n) ? n.dataset.md : (blockToMd(n) ?? ''));
    if (blocks.some((b) => b === undefined)) {
        say('この面からは書き戻せません（コードか図の元の字が取れません）');
        return;
    }
    try {
        blocks[at] = await change(blocks[at]);
    } catch (e) {
        say('置けません: ' + e.message);
        return;
    }
    const body = blocks.filter((s) => s !== '').join('\n\n') + '\n';
    loading = true;
    // 前書きの後ろの一行空きを、ここでも戻す ── `readToMd` はそうしていて、
    // ここだけ詰めると、道具で一つ直しただけのノートが**同期先で差分**になる。
    editor.setValue(state.head ? '\n' + body : body);
    loading = false;
    state.dirty = true;
    await save();
    await drawRead();
    // **入れたものの次に降りる。** 組み直しで caret は消えるので、
    // 置き直さないと次の一文字がどこへ行くか分からない。
    landAfter(at);
}

/// `n` 番目のかたまりの、次に caret を置く。
function landAfter(n) {
    const box = el('read');
    const kids = [...box.children];
    const to = kids[n + 1] || kids[kids.length - 1];
    if (!to) return;
    box.focus();
    const r = document.createRange();
    r.selectNodeContents(to);
    r.collapse(true);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
    caretAt = to;
    to.scrollIntoView({ block: 'nearest' });
}

const readMark = (kind, withWhat) => readSourceEdit((md) =>
    ask('mark', { kind, with: withWhat || '', text: md }).then((r) => r.text));

const readPut = (text) => readSourceEdit((md) => (md.trim() ? md + '\n\n' : '') + text);

/// マークダウンの書き方。**押せる形で出す** ── 選ぶとその場に入る。
///
/// 読むだけの一覧にすると、読んでから自分で打ち直すことになる。
/// vim の入切。**歯車の中だけ** ── 道具の帯からは外した（使うのは
/// たいてい一人で、毎日見る帯に居座る値打ちは無い）。選んだことは憶える。
async function cmdVim() {
    const to = await askPick('vimモード', [
        { name: 'vim で打つ', sub: 'ノーマル / 挿入 / ビジュアル、`:w` も', value: true },
        { name: '素のメモ帳で打つ', sub: 'ふつうの入力', value: false },
    ], vimOn ? 'いま: vim' : 'いま: 素のメモ帳');
    if (to === null || to === vimOn) return;
    setVim(to);
}

/// 行番号の入切。**「コード」の面だけの話。**
async function cmdLineNo() {
    const to = await askPick('行番号', [
        { name: '出す', sub: '「コード」の面の左に', value: true },
        { name: '出さない', value: false },
    ], lineNo ? 'いま: 出している' : 'いま: 出していない');
    if (to === null || to === lineNo) return;
    setLineNo(to);
}

async function cmdSyntax() {
    const rows = [
        ['見出し', '# 大きい見出し', '# から始める。## で一段小さく'],
        ['箇条書き', '- もの', '行の頭に - と空白'],
        ['番号つき', '1. ひとつめ', '1. 2. 3. と書く'],
        ['チェック', '- [ ] やること', '押すと入り切りできる升になる'],
        ['太字', '**ここが太字**', '前後を ** で挟む'],
        ['斜体', '*ここが斜体*', '前後を * で挟む'],
        ['取り消し線', '~~消した字~~', '前後を ~~ で挟む'],
        ['コード', '`コード`', '前後を ` で挟む'],
        ['コードの枠', '```\nここに何行でも\n```', '``` の行で挟む'],
        ['リンク', '[見せる字](https://)', '角括弧が字、丸括弧が行き先'],
        ['画像', '![説明](絵の場所)', '頭に ! を付けるとリンクではなく絵'],
        ['引用', '> 引いてきた字', '行の頭に > と空白'],
        ['注記', '> [!NOTE]\n> 覚えておくこと', 'NOTE / TIP / IMPORTANT / WARNING / CAUTION'],
        ['区切り線', '---', 'ハイフン三つだけの行'],
        ['表', '| a | b |\n|---|---|\n| 1 | 2 |', '縦棒で区切る'],
        ['図', '```mermaid\nflowchart LR\n  A --> B\n```', 'mermaid の書き方で図になる'],
    ];
    const pick = await askPick('マークダウンの書き方', rows.map(([name, ex, how]) => ({
        name, sub: how, key: ex.split('\n')[0], value: ex,
    })), '選ぶと、いま書いているところに入ります');
    if (pick === null) return;
    if (onRead()) readPut(pick);
    else if (editor) put(pick + '\n');
}

/* ── 書く道具 ── */

/// **押すと「自分で打ったはずの文字」が入る。** 書式ツールバーではない ──
/// ファイルは Markdown のままで、押した跡は打った跡と見分けがつかない。
///
/// 何が起きるかは `amber-core` の `markdown::marks` が決める。窓が自分で
/// `#` を数えはじめると、iPhone と押し心地が分かれる。
///
/// 並びは iPhone と同じ ── 上の一列が「ノートが実際にできているもの」で、
/// 残りは本当に時々のもの。**十四個の帯は、毎回使う五個ぶんの値段を取る。**
const MARKS = [
    [
        ['見出し', '⌘1', () => onRead() ? readHeading() : applyMark('heading')],
        ['箇条書き', '⌘⇧8', () => onRead() ? readBlockAs('ul') : applyMark('line', '- ')],
        ['チェックリスト', '⌘⇧9', () => onRead() ? readMark('line', '- [ ] ') : applyMark('line', '- [ ] ')],
        ['番号リスト', '⌘⇧7', () => onRead() ? readBlockAs('ol') : applyMark('line', '1. ')],
        ['太字', '⌘B', () => onRead() ? readDress('bold') : applyMark('wrap', '**')],
        ['画像', '', pickPicture],
        // **フローは画像の隣。** 図は「使う人は使う」もので、二列目に
        // 畳んでおくと、あることに気づかれない。
        ['フロー', '', cmdDiagram],
    ],
    [
        ['斜体', '⌘I', () => onRead() ? readDress('italic') : applyMark('wrap', '*')],
        ['取り消し線', '⌘⇧X', () => onRead() ? readDress('strikeThrough') : applyMark('wrap', '~~')],
        ['|'],
        ['リンク', '⌘K', () => onRead() ? readPut('[見せる字](https://)') : put('[](https://)', 1)],
        ['表', '', () => onRead()
            ? readPut('| 見出し | 見出し |\n| --- | --- |\n|  |  |')
            : put('| 見出し | 見出し |\n| --- | --- |\n|  |  |\n', 2)],
        ['水平線', '', () => onRead() ? readPut('---') : put('\n---\n\n')],
        ['|'],
        ['引用', '', () => onRead() ? readBlockAs('blockquote') : applyMark('line', '> ')],
        ['注記', '', cmdAlert],
    ],
];

/// いま打っているのは読む面か。
///
/// **見えている面ではなく、焦点で決める。** 並べているときは両方見えて
/// いるので、どちらの道具かは打っている場所が決める。
function onRead() {
    if (view === 'read') return true;
    if (view === 'write') return false;
    const at = getSelection()?.anchorNode;
    return !!at && el('read').contains(at.nodeType === 3 ? at.parentNode : at);
}
/// 二列目は畳んである。開いたままかどうかは憶えておく。
let moreMarks = false;

function drawMarks() {
    const box = el('marks');
    box.innerHTML = '';
    MARKS.forEach((row, n) => {
        if (n > 0 && !moreMarks) return;
        const r = document.createElement('div');
        r.className = 'r';
        for (const [name, key] of row) {
            if (name === '|') {
                const sep = document.createElement('div');
                sep.className = 'sep';
                r.append(sep);
                continue;
            }
            const b = document.createElement('button');
            b.textContent = name;
            b.title = key ? `${name}（${key}）` : name;
            // 押した瞬間に焦点を奪わない ── 奪うと、どこに入れるかを
            // 決める手がかり（選んだところ）が先に消える。
            b.onmousedown = (e) => e.preventDefault();
            b.onclick = () => {
                const found = MARKS.flat().find((m) => m[0] === name);
                if (found) found[2]();
            };
            r.append(b);
        }
        if (n === 0) {
            const sep = document.createElement('div');
            sep.className = 'sep';
            // **vim の釦は置かない。** 使うのはたいてい一人で、毎日見る
            // 帯に居座る値打ちは無い ── ⚙ の中にある。
            const more = document.createElement('button');
            more.textContent = moreMarks ? 'たたむ' : '…';
            more.title = moreMarks ? '畳む' : 'ほかの記号';
            more.classList.toggle('on', moreMarks);
            more.onclick = () => {
                moreMarks = !moreMarks;
                drawMarks();
                window.amber.remember({ moreMarks });
                if (editor) editor.layout();
            };
            r.append(sep, more);
        }
        box.append(r);
    });
}

/// 選んだところを core に渡し、返ってきた字で置き換える。
///
/// 名前が `mark` でないのは、**葉の印を描く `mark(size)` が既にいる**から ──
/// 一度そちらを覆ってしまい、窓の左上が `[object Promise]` になった。
///
/// **位置は渡さない。** JS は UTF-16 の桁で数え、Rust は文字で数えるので、
/// 絵文字が一つ混ざれば境目がずれる ── 選んだ字そのものを渡す。
async function applyMark(kind, withWhat) {
    if (!editor || view === 'read') return;
    const model = editor.getModel();
    let sel = editor.getSelection();
    // 行頭の印と見出しは、選んでいなくてもその行が相手。
    if (kind !== 'wrap') {
        sel = new monaco.Selection(
            sel.startLineNumber, 1,
            sel.endLineNumber, model.getLineMaxColumn(sel.endLineNumber));
    }
    const before = model.getValueInRange(sel);
    let after;
    try {
        after = (await ask('mark', { kind, with: withWhat || '', text: before })).text;
    } catch (e) {
        say('置けません: ' + e.message);
        return;
    }
    const start = sel.getStartPosition();
    const rows = after.split('\n');
    const endLine = start.lineNumber + rows.length - 1;
    const endCol = (rows.length === 1 ? start.column : 1) + rows[rows.length - 1].length;
    // 選んでいなかったのに挟んだときは、**印の中に入れる** ── そこで打ちたい。
    const inside = kind === 'wrap' && before === '';
    const end = inside
        ? new monaco.Selection(start.lineNumber, start.column + (withWhat || '').length,
                               start.lineNumber, start.column + (withWhat || '').length)
        : new monaco.Selection(start.lineNumber, start.column, endLine, endCol);
    editor.executeEdits('marks', [{ range: sel, text: after }], [end]);
    editor.focus();
}

/// 骨組みを置いて、打ちはじめる場所に入る。
///
/// `caret` は**置いた字の頭から数えた文字数** ── そこに入って打てるように
/// する。省くと、置いた字の末尾に出る。**縦棒を数えるのは cian の仕事**で、
/// 揃え方の行（`:---`）の形を人が覚えている必要は無い。
function put(text, caret) {
    if (!editor || view === 'read') return;
    const sel = editor.getSelection();
    const start = sel.getStartPosition();
    const at = (n) => {
        const head = text.slice(0, n);
        const rows = head.split('\n');
        const line = start.lineNumber + rows.length - 1;
        const col = (rows.length === 1 ? start.column : 1) + rows[rows.length - 1].length;
        return new monaco.Selection(line, col, line, col);
    };
    editor.executeEdits('marks', [{ range: sel, text }],
                        [at(caret === undefined ? text.length : caret)]);
    editor.focus();
}

/// 絵をノートの隣に置いて、リンクを打つ。
///
/// **ノートの隣に置く。** 同期しているフォルダを別の端末で開いたとき、
/// 絵だけが来ないノートは「消えたのか、元から無いのか」が分からない。
async function pickPicture() {
    if (!state.open) return;
    const file = await window.amber.pickFile(
        [{ name: '画像', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'heic'] }]);
    if (!file) return;
    const got = await window.amber.fileBytes(file);
    if (!got) { say('その絵は読めません'); return; }
    await attach(got.b64, got.ext);
}

async function attach(b64, ext) {
    try {
        const r = await ask('image', { note: state.open.path, b64, ext });
        put(`![](${r.link})\n`);
        zonesSoon();
    } catch (e) {
        say('絵を置けません: ' + e.message);
    }
}

/* ── 読む面 ── */

/// いま右に出しているもの。`write` / `read` / `split`。
///
/// **重ねずに横へ並べる。** 同じ場所に重ねて片方を `display:none` にすると、
/// Monaco の `automaticLayout` が幅ゼロのまま測り、戻したときに折り返しが
/// 直るまで一瞬崩れる。畳むのは片方だけで、並びは変えない。
/// **既定は「表示」。** 開いてまず見たいのは組んだ姿で、記号の並びではない
/// ── 書きたくなったらその場で打てる。
let view = 'read';
/// ノートだけを大きく（Inkdrop の distraction free）。
let zen = false;
/// 組み直しの世代。遅れて帰ってきた古い HTML で新しい画面を潰さない。
let readSeq = 0;
/// 組み直しを待たせる玉。**書き戻しの `readTimer` とは別物** ── 片方は
/// 「打ったので組み直す」、もう片方は「打ったので書き戻す」。
let drawTimer = null;

/// front matter を戻した、ファイルにある通りの一枚。
///
/// **チェックの行番号は、この数え方の行番号。** 本文だけを渡すと、
/// front matter の行数ぶんずれた升に印が付く。
function whole() {
    return state.head + (editor ? editor.getValue() : '');
}

function applyView() {
    const open = !!state.open;
    // **帯はいつも出す。** 設定（⚙）はノートを開いていなくても要る ──
    // 「置き場所を変える」はノートが一本も無いときにこそ押したい。
    el('top').hidden = false;
    for (const id of ['title', 'views', 'count2', 'state', 'dots']) el(id).hidden = !open;
    el('blank').hidden = open;
    el('work').hidden = !open;
    el('ed').hidden = !open || view === 'read';
    el('read').hidden = !open || view === 'write';
    el('toc').hidden = !open || !tocOn;
    // **読む面でも道具の帯は出す。** 記号を覚えていない人の道具なので、
    // 記号の見えない面でこそ要る。
    document.body.classList.toggle('reading', false);
    document.body.classList.toggle('split', view === 'split');
    document.body.classList.toggle('zen', zen);
    for (const b of document.querySelectorAll('#views button[data-view]')) {
        b.classList.toggle('on', b.dataset.view === view);
    }
    el('tocbtn').classList.toggle('on', tocOn);
    el('zenbtn').classList.toggle('on', zen);
    // 幅が変わったので測り直す。**畳みが効いた後で**（今のフレームでは
    // まだ古い幅しか見えない）。
    if (editor && view !== 'read') setTimeout(() => editor.layout(), 0);
    if (open && view !== 'write') drawRead();
    if (open && tocOn) drawToc();
}

function setView(v) {
    view = v;
    applyView();
    window.amber.remember({ view: v });
    if (v === 'read') document.activeElement?.blur();
    else if (editor) editor.focus();
}

/// 上の帯の三つ。**押せる形と、キーと、同じ一本の道を通す。**
for (const b of document.querySelectorAll('#views button[data-view]')) {
    b.onclick = () => setView(b.dataset.view);
}
el('tocbtn').onclick = () => toggleToc();
el('zenbtn').onclick = () => setZen(!zen);
el('gear').onclick = (e) => {
    if (el('more').hidden) openMenu(e.currentTarget.getBoundingClientRect(), 'app');
    else closeMenu();
};
el('dots').onclick = (e) => {
    if (el('more').hidden) openMenu(e.currentTarget.getBoundingClientRect());
    else closeMenu();
};

/// 字の大きさ。**書く面と読む面を一緒に動かす** ── 片方だけ動くと、
/// 並べたときに同じノートが二つの大きさで出る。
/// 「コード」の面に行番号を出すか。**既定は出さない** ── ノートは
/// 行で指す文書ではないので、ふだんは数字が一列ぶん余計。
let lineNo = false;

function setLineNo(on) {
    lineNo = !!on;
    window.amber.remember({ lineNo });
    if (editor) editor.updateOptions({ lineNumbers: lineNo ? 'on' : 'off' });
}

let fontStep = 0;
const FONT_BASE = 15;

function setFont(step, quiet) {
    fontStep = Math.max(-4, Math.min(8, step));
    window.amber.remember({ fontStep });
    const px = FONT_BASE + fontStep;
    if (editor) editor.updateOptions({ fontSize: px });
    el('read').style.fontSize = (px - 0.5) + 'px';
    // 起動して戻すときは黙る ── 開いた瞬間に札が出る理由は無い。
    if (!quiet && fontStep !== 0) say('字の大きさ ' + px + 'px（⌘0 で戻る）');
}

function toggleRead() { setView(view === 'read' ? 'write' : 'read'); }
function toggleSplit() { setView(view === 'split' ? 'write' : 'split'); }

function setZen(on) {
    zen = on;
    applyView();
    // 戻るときは黙る ── `say('')` は空の札を出してしまう。
    if (on) say('ノートだけを大きく（F12 か Esc で戻る）');
}

/// いま何字か。
///
/// **数えるのは本文だけ** ── 前書き（題・タグ・作った日）はノートが自分を
/// 説明する言葉で、書いた量ではない。語数ではなく字数にしたのは、日本語に
/// 語の切れ目が無いから ── 空白で切って数えると、一段落が「1 語」になる。
function drawCount() {
    if (!editor || !state.open) { el('count2').textContent = ''; return; }
    const t = editor.getValue();
    const chars = [...t.replace(/\s/g, '')].length;
    const lines = t ? t.split('\n').length : 0;
    el('count2').textContent = chars.toLocaleString() + ' 字 ・ ' + lines + ' 行';
}

/// 打っている間の組み直しは、止まってから。
function readSoon() {
    if (view === 'write') return;
    clearTimeout(drawTimer);
    drawTimer = setTimeout(drawRead, 260);
}

async function drawRead() {
    if (view === 'write' || !state.open) return;
    const seq = ++readSeq;
    let html;
    try {
        html = (await ask('html', { text: whole() })).html || '';
    } catch (e) {
        say('組めません: ' + e.message);
        return;
    }
    // 追い越されていたら捨てる。速く打つと、古い答えが後から着く。
    if (seq !== readSeq) return;
    // 空のノートでも打ちはじめられるように、空の段落を一つ置く ──
    // `contenteditable` は中身が無いと caret を置く先が無い。
    el('read').innerHTML = html.trim() ? html : '<p data-line="' + headLines() + '" data-span="1"><br></p>';
    // **末尾には、いつも降りられる一行を置く。**
    //
    // 表や水平線でノートが終わっていると、その下に caret を置く手が
    // 無い（表の外側は表の一部ではないので、矢印でも出られない）。
    // 空のままなら字に戻すときに落ちるので、増えも減りもしない。
    tailStop();
    // **札を配るのが先。** 絵や図はこのあと札を掛け替える（`<pre>` →
    // `<div class="mermaid">`、`<img>` → `<figure>`）ので、掛け替える前に
    // 元の字を持たせておかないと、引き継ぐものが無い ── 図を入れたノートで
    // 保存が黙って止まった。
    armRead();
    findPictures();
    paintCode();
    drawDiagrams();
}

/// 末尾に空の段落を一つ置く（もう空の段落で終わっているなら、置かない）。
function tailStop() {
    const box = el('read');
    const last = box.lastElementChild;
    if (last && last.tagName === 'P' && !last.textContent.trim()) return;
    const p = document.createElement('p');
    p.append(document.createElement('br'));
    box.append(p);
}

/// 前書きの行数。空のノートに置く段落の行番号に要る。
function headLines() {
    return state.head ? state.head.split('\n').length - 1 : 0;
}

/* ── 図（mermaid） ── */

/// **図のあるノートを開くまで、読み込まない。** 3.4MB あって、ほとんどの
/// ノートには図が無い ── 起動のたびに払う値段ではない。
let Mermaid = null;
let mermaidSeq = 0;

/// 図に使う色。**アプリのどこを見ても同じ色の家族にする** ── フォルダの
/// 色も、字の色も、円グラフの切れ端も、マインドマップの枝もこの十一色。
/// 図ごとに別の並びを持つと、同じノートの中で色の意味が変わる。
const FAMILY = [
    '#D07A2E', '#3D7FA8', '#5E8C42', '#9A6FB5', '#C2649A', '#2AA79B',
    '#B08A2E', '#6E7BC4', '#C4564E', '#0E93A8', '#7A7A7A',
];

/// 図の設定。**地の明暗に合わせる** ── 図だけ白いと、暗い画面で目を焼く。
function mermaidOpts() {
    // 既定の図は**紫と水色**で、ノートの地とも琥珀とも合わない ── 図だけ
    // 別のアプリから貼ってきたように見える。`base` に色を渡して、いま出て
    // いるテーマの色で描かせる（テーマを替えると図も替わる）。
    const css = getComputedStyle(document.documentElement);
    const v = (name, or_) => (css.getPropertyValue(name) || '').trim() || or_;
    const dark = isDark();
    return {
        startOnLoad: false,
        theme: 'base',
        themeVariables: {
            background: v('--paper', '#fffdf8'),
            primaryColor: v('--rail', '#f3ecdf'),
            primaryTextColor: v('--ink', '#2a2011'),
            primaryBorderColor: v('--amber', '#f0a52b'),
            secondaryColor: v('--list', '#f8f3e8'),
            tertiaryColor: v('--paper', '#fffdf8'),
            lineColor: v('--ink-3', '#9a8a6f'),
            textColor: v('--ink', '#2a2011'),
            mainBkg: v('--rail', '#f3ecdf'),
            nodeBorder: v('--amber', '#f0a52b'),
            clusterBkg: v('--list', '#f8f3e8'),
            clusterBorder: v('--line', '#e4d9c4'),
            edgeLabelBackground: v('--paper', '#fffdf8'),
            ...Object.fromEntries(FAMILY.map((c, n) => ['pie' + (n + 1), c])),
            pieStrokeColor: v('--paper', '#fffdf8'),
            pieOuterStrokeColor: v('--line', '#e4d9c4'),
            pieTitleTextColor: v('--ink', '#2a2011'),
            pieSectionTextColor: '#ffffff',
            pieLegendTextColor: v('--ink-2', '#6b5a41'),
            pieOpacity: '1',
            fontSize: '14px',
            // 節の角を丸める ── 既定の直角は、amber のどの面にも無い形。
            nodeTextColor: v('--ink', '#2a2011'),
            darkMode: dark,
        },
        // 円グラフの色。**既定の派手な12色は、琥珀の隣で喧嘩する** ──
        // フォルダと文字色に使っている11色と同じ並びを渡して、アプリの
        // どこを見ても同じ色の家族にする。
        themeCSS: '.pieTitleText{font-size:15px;font-weight:700}'
            + '.slice{font-size:13px;font-weight:600}'
            + '.pieCircle{stroke:' + v('--paper', '#fffdf8') + ';stroke-width:2px}'
            + '.pieOuterCircle{stroke:' + v('--line', '#e4d9c4') + '}'
            + '.legend text{font-size:13px}'
            // マインドマップだけは `themeVariables` を見ない ── 灰と藤色で
            // 描かれて、琥珀のノートの上で**そこだけ別のアプリ**に見える。
            // 枝は円グラフと同じ十一色にして、まん中は琥珀そのものに。
            + '.mindmap-node.section--1 circle.basic{fill:' + v('--amber', '#f0a52b')
                + ';stroke:' + v('--amber', '#f0a52b') + '}'
            + '.mindmap-node.section--1 .nodeLabel{color:#3a2408;font-weight:700}'
            + FAMILY.map((c, n) =>
                '.mindmap-node.section-' + n + ' .node-bkg{fill:color-mix(in srgb,' + c
                    + ' 14%,' + v('--paper', '#fffdf8') + ');stroke:' + c + '}'
                + '.mindmap-node.section-' + n + ' line{stroke:' + c + ';stroke-width:2px}'
                + '.edge.section-edge-' + n + '{stroke:' + c + ';stroke-width:2.5px}').join(''),
        flowchart: { curve: 'basis', padding: 14, nodeSpacing: 44, rankSpacing: 46, htmlLabels: true },
        pie: { textPosition: 0.62, useMaxWidth: true },
        sequence: { actorMargin: 44, mirrorActors: false },
        // 予定表は、既定だと細い帯に目盛りが詰まって日付が重なる（読めない）。
        // 横幅いっぱいまで伸ばし、棒と余白を広げて、目盛りの字を離す。
        gantt: {
            // **広く描いてから、入るところまで縮める。** 既定の幅だと目盛りが
            // 重なって日付が読めない（`09/1009/12…` になる）。広い紙に描けば
            // 字は離れ、`useMaxWidth` が枠に合わせて全体を縮めてくれる。
            useWidth: 980, useMaxWidth: true,
            barHeight: 22, barGap: 7,
            topPadding: 48, leftPadding: 88, gridLineStartPadding: 32,
            fontSize: 12, sectionFontSize: 12, numberSectionStyles: 4,
        },
        // ノートは人が書いたもの。図の札に書いた HTML を効かせない。
        securityLevel: 'strict',
        fontFamily: '-apple-system, "Hiragino Sans", "Yu Gothic UI", sans-serif',
    };
}

/// 図を、選んで作る。
///
/// **mermaid の書き方は覚えなくていい。** よく使う四つだけを出し、名前を
/// 訊いて骨組みを入れる ── 込み入った図は書き方を覚えた人が書けばよく、
/// そこまでを画面に載せると「難しいもの」に見えて誰も押さなくなる。
async function cmdDiagram() {
    const kind = await askPick('どんな図', [
        { name: '流れ図', sub: 'A → B → C。手順や段取りに', value: 'flow' },
        { name: '分かれ道', sub: '「はい / いいえ」で分かれる', value: 'branch' },
        { name: 'マインドマップ', sub: '一つの言葉から枝を広げる', value: 'mind' },
        { name: '年表', sub: 'いつ何があったか。日記や記録に', value: 'time' },
        { name: '予定表', sub: '棒で見る段取り（ガント）', value: 'gantt' },
        { name: '四象限', sub: '大事さと急ぎで、やることを置く', value: 'quad' },
        { name: 'やりとり', sub: '誰が誰に何を、の順番', value: 'seq' },
        { name: '円グラフ', sub: '割合を見せる', value: 'pie' },
    ], '選ぶと骨組みが入ります。中の言葉は、あとから書き換えられます');
    if (kind === null) return;

    const ask3 = async (title, foot, or_) => {
        const v = await askText(title, or_ || '', foot);
        return v === null ? null : (v.trim() || or_ || '');
    };
    // 読点でも矢印でも切れるようにする ── 打ちながら決めるので、
    // どちらで区切ったかを覚えていられない。
    const parts = (v) => v.split(/\s*(?:→|->|、|,)\s*/).filter(Boolean);
    const name = (n) => String.fromCharCode(65 + n);
    let md = '';

    if (kind === 'flow') {
        const v = await ask3('順に並べる言葉', '「→」か読点で区切ってください', '書く → 見直す → 出す');
        if (v === null) return;
        const step = parts(v);
        md = '```mermaid\nflowchart LR\n'
            + step.map((t, n) => '  ' + name(n) + '[' + t + ']').join('\n') + '\n'
            + step.slice(1).map((_, n) => '  ' + name(n) + ' --> ' + name(n + 1)).join('\n')
            + '\n```';
    } else if (kind === 'branch') {
        const q = await ask3('分かれ道の問い', '', '足りている？');
        if (q === null) return;
        const yes = await ask3('「はい」のとき', '', '出す');
        if (yes === null) return;
        const no = await ask3('「いいえ」のとき', '', '足す');
        if (no === null) return;
        md = '```mermaid\nflowchart LR\n  A{' + q + '}\n  B[' + yes + ']\n  C[' + no + ']\n'
            + '  A -->|はい| B\n  A -->|いいえ| C\n```';
    } else if (kind === 'mind') {
        const root = await ask3('まん中に置く言葉', '', '来年やること');
        if (root === null) return;
        const v = await ask3('そこから広げる言葉', '読点で区切ってください', '仕事、家、体、学び');
        if (v === null) return;
        md = '```mermaid\nmindmap\n  root((' + root + '))\n'
            + parts(v).map((t) => '    ' + t).join('\n') + '\n```';
    } else if (kind === 'time') {
        const title = await ask3('年表の題', '', '今年');
        if (title === null) return;
        const v = await ask3('できごと', '「いつ: なに」を読点で区切ってください',
            '4月: 引っ越し、7月: 新しい仕事、11月: 旅行');
        if (v === null) return;
        const rows = parts(v).map((x) => {
            const m = /^(.*?)\s*[:：]\s*(.*)$/.exec(x.trim());
            return m ? '  ' + m[1] + ' : ' + m[2] : '  ' + x.trim() + ' : ';
        });
        md = '```mermaid\ntimeline\n  title ' + title + '\n' + rows.join('\n') + '\n```';
    } else if (kind === 'gantt') {
        const title = await ask3('予定表の題', '', '段取り');
        if (title === null) return;
        const v = await ask3('やること', '「なに: 始まり, 何日」を読点で区切ってください',
            '下ごしらえ: 2026-09-10, 3d、本番: 2026-09-13, 5d');
        if (v === null) return;
        const rows = v.split(/\s*[、]\s*/).filter(Boolean).map((x, n) => {
            const m = /^(.*?)\s*[:：]\s*(.*)$/.exec(x.trim());
            return m ? '  ' + m[1] + ' :t' + n + ', ' + m[2] : '  ' + x.trim() + ' :t' + n + ', 1d';
        });
        md = '```mermaid\ngantt\n  title ' + title + '\n  dateFormat YYYY-MM-DD\n'
            + '  axisFormat %m/%d\n  section やること\n' + rows.join('\n') + '\n```';
    } else if (kind === 'quad') {
        const v = await ask3('置くもの', '「なに: 大事さ, 急ぎ」を 0〜1 で。読点で区切ってください',
            '週報: 0.8, 0.9、片付け: 0.3, 0.2、勉強: 0.9, 0.2');
        if (v === null) return;
        const rows = v.split(/\s*[、]\s*/).filter(Boolean).map((x) => {
            const m = /^(.*?)\s*[:：]\s*([\d.]+)\s*,\s*([\d.]+)/.exec(x.trim());
            return m ? '  "' + m[1] + '": [' + m[3] + ', ' + m[2] + ']' : null;
        }).filter(Boolean);
        md = '```mermaid\nquadrantChart\n  title やることの置きどころ\n'
            + '  x-axis いつでも --> いま\n  y-axis 小さい --> 大きい\n'
            + '  quadrant-1 すぐやる\n  quadrant-2 段取りする\n'
            + '  quadrant-3 あとで\n  quadrant-4 誰かに頼む\n' + rows.join('\n') + '\n```';
    } else if (kind === 'seq') {
        const a2 = await ask3('だれが', '', '私');
        if (a2 === null) return;
        const b2 = await ask3('だれに', '', '相手');
        if (b2 === null) return;
        const what = await ask3('なにを', '', 'お願いする');
        if (what === null) return;
        md = '```mermaid\nsequenceDiagram\n  participant ' + a2 + '\n  participant ' + b2 + '\n'
            + '  ' + a2 + '->>' + b2 + ': ' + what + '\n  ' + b2 + '-->>' + a2 + ': わかった\n```';
    } else {
        const v = await ask3('割合', '「名前 数」を読点で区切ってください', '仕事 5、家 3、ほか 2');
        if (v === null) return;
        const rows = v.split(/\s*[、,]\s*/).filter(Boolean).map((x) => {
            const m = /^(.*?)\s+([\d.]+)$/.exec(x.trim());
            return m ? '  "' + m[1] + '" : ' + m[2] : '  "' + x.trim() + '" : 1';
        });
        md = '```mermaid\npie showData\n' + rows.join('\n') + '\n```';
    }
    if (onRead()) await readPut(md);
    else put(md + '\n');
}

function loadMermaid() {
    if (Mermaid) return Promise.resolve(Mermaid);
    return new Promise((resolve, reject) => {
        // **Monaco のローダには渡さない。**
        //
        // mermaid の束ねの中には、`define.amd` を見て自分から名乗り出る
        // 小さな部品が入っている。ローダはその名乗りを「mermaid だ」と
        // 受け取るので、返ってくるのは `initialize` を持たない別物になる
        // （実際にそうなった。「mermaid が名乗りません」はそれ）。
        //
        // だから **`define` を伏せてから素の `<script>` で読む。** 束ねは
        // 最後に `globalThis.mermaid` へ自分を置く。
        const keep = window.define;
        window.define = undefined;
        const tag = document.createElement('script');
        tag.src = 'vendor/mermaid/mermaid.min.js';
        tag.onload = () => {
            window.define = keep;
            const lib = globalThis.mermaid;
            if (!lib || typeof lib.initialize !== 'function') {
                reject(new Error('mermaid が名乗りません'));
                return;
            }
            lib.initialize(mermaidOpts());
            Mermaid = lib;
            resolve(lib);
        };
        tag.onerror = () => {
            window.define = keep;
            reject(new Error('vendor/mermaid が置かれていません（node gui/vendor.js）'));
        };
        document.head.append(tag);
    });
}

async function drawDiagrams() {
    const blocks = [...el('read').querySelectorAll('pre > code.language-mermaid')];
    if (!blocks.length) return;
    const seq = readSeq;
    let lib;
    try {
        lib = await loadMermaid();
    } catch (e) {
        say('図を読めません: ' + (e && e.message ? e.message : e));
        return;
    }
    if (seq !== readSeq) return;
    for (const code of blocks) {
        const src = code.textContent;
        try {
            const { svg } = await lib.render('mmd' + (++mermaidSeq), src);
            if (seq !== readSeq) return;
            const box = document.createElement('div');
            box.className = 'mermaid';
            box.innerHTML = svg;
            // **元の字と行番号を引き継ぐ。** 引き継がないと、字に戻すとき
            // この図の中身がどこにも無く、**保存のたびに図が消える**
            // （実際に消えた）。組み直しで札を掛け替えるところは、
            // 掛け替えたぶんを必ず持っていく。
            keepMark(code.parentElement, box);
            code.parentElement.replaceWith(box);
        } catch (e) {
            // **描けない図は、書いた字のまま残す。** 消すと、直しようがない。
            if (seq !== readSeq) return;
            code.parentElement.classList.add('bad');
            code.parentElement.title = '図にできません: ' + (e && e.message ? e.message : e);
        }
    }
    // 掛け替えたあとの札にも、触れない印と元の字を。
    if (seq === readSeq) armRead();
}

/* ── 図の工房 ── 図を見ながら、表で直す ── */

/// **書き方を覚えなくても、作った図を直せるようにする。**
///
/// 作るところ（`cmdDiagram`）は選ぶだけで済むのに、**直すところが字だけ**
/// だった。`flowchart LR` も `A -->|はい| B` も覚えていない人にとって、一度
/// 作った図は「作り直すしか手が無いもの」で、それは作れると言えない。
///
/// ここは図を押すと工房が開く。左に表、右に図。表を直すとその場で描き直る
/// ので、**当たっているかを、保存する前に目で確かめられる**。
///
/// **読み戻せない図でも閉め出さない。** 手で書いた凝った図は表にならない
/// が、そのときは字の面が出る ── 字で直しながら、右で図を見られる。工房を
/// 開けない図は無い。

/// 図の種類を見分ける。
function mmdKind(src) {
    const head = src.split('\n').map((l) => l.trim())
        .find((l) => l && !l.startsWith('%%')) || '';
    if (/^(flowchart|graph)\b/.test(head)) return 'flow';
    if (/^pie\b/.test(head)) return 'pie';
    if (/^mindmap\b/.test(head)) return 'mind';
    if (/^timeline\b/.test(head)) return 'time';
    if (/^gantt\b/.test(head)) return 'gantt';
    if (/^quadrantChart\b/.test(head)) return 'quad';
    if (/^sequenceDiagram\b/.test(head)) return 'seq';
    return null;
}

/// 図の字を、表に読み戻す。読めなければ `null`（そのときは字で直す面になる）。
///
/// **読めなかった行を、黙って落とさない。** 半分だけ読めた表を出すと、直して
/// 保存した瞬間に読めなかった行が消える ── 消えたことに気づくのは何日も後に
/// なる。だから一行でも読めなければ、表そのものを諦める。
///
/// 表にしない行（`dateFormat`、`x-axis`、注釈…）は **`head` にそのまま
/// 取っておいて、書き戻すときに戻す**。読めた行だけ組み直して、あとは元の字。
function mmdParse(src) {
    const kind = mmdKind(src);
    if (!kind) return null;
    const live = src.split('\n').filter((l) => l.trim() !== '');
    if (!live.length) return null;

    // マインドマップだけは、**字下げが中身**（枝の深さ）なので別に読む。
    if (kind === 'mind') {
        const lines = live.slice(1).filter((l) => !l.trim().startsWith('%%'));
        const rootAt = lines.findIndex((l) => /^\s*root\(\(/.test(l));
        if (rootAt < 0) return null;
        const kids = lines.filter((_, i) => i !== rootAt);
        // 枝の枝と、凝った形（`枝[四角]`）は表にできない ── 平らに直すと
        // 形が変わってしまうので、そういう図は字で直してもらう。
        if (kids.some((l) => /[[({]/.test(l.trim()))) return null;
        const deep = (l) => /^\s*/.exec(l)[0].length;
        if (kids.length && kids.some((l) => deep(l) !== deep(kids[0]))) return null;
        return {
            kind, first: 'mindmap', head: [], edges: [],
            title: (/root\(\((.*)\)\)/.exec(lines[rootAt]) || ['', ''])[1],
            rows: kids.map((l) => ({ a: l.trim() })),
        };
    }

    const first = live[0].trim();
    const head = [];
    const rows = [];
    const edges = [];
    let title = '';
    // 題を表に出す種類だけ、題の行を抜き取る。出さない種類のものは
    // `head` に残す ── 触らないものを触ったことにしない。
    const wantsTitle = !!(DIAGRAM_FORM[kind] || {}).title;

    for (const line of live.slice(1)) {
        const l = line.trim();
        if (l.startsWith('%%')) { head.push('  ' + l); continue; }
        if (wantsTitle && /^title\b/.test(l)) { title = l.replace(/^title\s*/, ''); continue; }

        if (kind === 'pie') {
            const m = /^"(.*)"\s*:\s*([\d.]+)$/.exec(l);
            if (m) { rows.push({ a: m[1], b: m[2] }); continue; }
        } else if (kind === 'quad') {
            const m = /^"(.*)"\s*:\s*\[\s*([\d.]+)\s*,\s*([\d.]+)\s*\]$/.exec(l);
            if (m) { rows.push({ a: m[1], b: m[3], c: m[2] }); continue; }
            if (/^(x-axis|y-axis|quadrant-)/.test(l)) { head.push('  ' + l); continue; }
        } else if (kind === 'time') {
            const m = /^(.*?)\s*:\s*(.*)$/.exec(l);
            if (m) { rows.push({ a: m[1], b: m[2] }); continue; }
        } else if (kind === 'gantt') {
            if (/^(dateFormat|axisFormat|excludes|todayMarker|tickInterval|weekday)\b/.test(l)) {
                head.push('  ' + l);
                continue;
            }
            // **区切りは一つまで。** 二つある予定表を平らに読むと、書き戻し
            // たときに全部が一つの区切りへ移る ── 表には出ない引っ越し。
            if (/^section\b/.test(l)) {
                if (head.some((h) => /^\s*section\b/.test(h))) return null;
                head.push('  ' + l);
                continue;
            }
            const m = /^(.*?)\s*:\s*(?:[^,]*,\s*)?(\d{4}-\d{2}-\d{2})\s*,\s*(.+)$/.exec(l);
            if (m) { rows.push({ a: m[1], b: m[2], c: m[3] }); continue; }
        } else if (kind === 'seq') {
            // 出てくる人は行から組み直すので、名乗りの行は取っておかない
            // ── 残すと、名前を直したときに古い柱がもう一本立つ。
            if (/^participant\b/.test(l)) continue;
            const m = /^(\S+?)\s*(-->>|->>|-->|->)\s*(\S+?)\s*:\s*(.*)$/.exec(l);
            if (m) { rows.push({ a: m[1], b: m[3], c: m[4], dashed: m[2].startsWith('--') }); continue; }
        } else {
            const n = /^([A-Za-z_]\w*)\s*([[({])(.*?)[\])}]$/.exec(l);
            if (n) {
                rows.push({ id: n[1], a: n[3], shape: { '{': 'diamond', '(': 'round' }[n[2]] || 'box' });
                continue;
            }
            const e = /^([A-Za-z_]\w*)\s*-->\s*(?:\|(.*?)\|\s*)?([A-Za-z_]\w*)$/.exec(l);
            if (e) { edges.push({ from: e[1], b: e[2] || '', to: e[3] }); continue; }
        }
        return null;    // 読めない行が一つでもあれば、表にしない
    }
    if (!rows.length) return null;
    const dir = (/^(?:flowchart|graph)\s+(\w+)/.exec(first) || [])[1] || 'LR';
    return { kind, first, head, title, rows, edges, dir };
}

/// 表を、図の字に書き戻す。
function mmdBuild(d) {
    const q = (t) => String(t ?? '').trim().replace(/"/g, '”');
    // 節の名前に括弧や縦棒が混ざると、そこで形が終わったことになって
    // 図が壊れる ── 打てる場所なので、通り道で落としておく。
    const plain = (t) => String(t ?? '').trim().replace(/[[\]{}()|]/g, '');
    const live = d.rows.filter((r) => Object.values(r).some((v) => String(v ?? '').trim()));
    const head = d.head.join('\n');
    const body = (first, rows) =>
        [first, d.title ? '  title ' + d.title : '', head, rows.join('\n')]
            .filter((s) => s !== '').join('\n');

    if (d.kind === 'mind') {
        return 'mindmap\n  root((' + (plain(d.title) || 'まん中') + '))\n'
            + live.map((r) => '    ' + plain(r.a)).join('\n');
    }
    if (d.kind === 'pie') {
        return body(d.first, live.map((r) => '  "' + q(r.a) + '" : ' + (Number(r.b) || 0)));
    }
    if (d.kind === 'quad') {
        return body(d.first, live.map((r) =>
            '  "' + q(r.a) + '": [' + num(r.c) + ', ' + num(r.b) + ']'));
    }
    if (d.kind === 'time') {
        return body(d.first, live.map((r) => '  ' + (q(r.a) || '　') + ' : ' + q(r.b)));
    }
    if (d.kind === 'gantt') {
        return body(d.first, live.map((r, n) =>
            '  ' + q(r.a) + ' :t' + n + ', ' + (q(r.b) || '2026-01-01') + ', ' + (q(r.c) || '1d')));
    }
    if (d.kind === 'seq') {
        const who = [...new Set(live.flatMap((r) => [r.a, r.b]).map(plain).filter(Boolean))];
        return body(d.first, [
            ...who.map((w) => '  participant ' + w),
            ...live.filter((r) => r.a && r.b).map((r) =>
                '  ' + plain(r.a) + (r.dashed ? '-->>' : '->>') + plain(r.b) + ': ' + q(r.c)),
        ]);
    }
    const wrap = { box: ['[', ']'], round: ['(', ')'], diamond: ['{', '}'] };
    const seen = new Set(live.map((r) => r.id));
    return body('flowchart ' + (d.dir || 'LR'), [
        ...live.map((r) => {
            const [o, c] = wrap[r.shape] || wrap.box;
            return '  ' + r.id + o + (plain(r.a) || r.id) + c;
        }),
        // 消した節を指したままの線は、書き戻さない ── 残すと mermaid が
        // 名前だけの箱を勝手に立てて、消したはずのものが図に戻る。
        ...d.edges.filter((e) => seen.has(e.from) && seen.has(e.to)).map((e) =>
            '  ' + e.from + ' -->' + (e.b.trim() ? '|' + plain(e.b) + '|' : '') + ' ' + e.to),
    ]);
}

/// 0〜1 に収める。**打ち間違いで図が壊れないように** ── 四象限は枠の外に
/// 置かれると、点が消えたようにしか見えない。
function num(v) {
    const n = Number(String(v ?? '').trim());
    return Number.isFinite(n) ? Math.min(Math.max(n, 0), 1) : 0.5;
}

/// 種類ごとの、表の形。**列の名前がそのまま説明になる** ── 「なに」「いつ」
/// と書いてあれば、何を打てばいいかを別に書かなくていい。
const DIAGRAM_FORM = {
    flow: { name: '流れ図', add: '箱を足す' },
    pie: {
        name: '円グラフ', title: '題', add: '割合を足す',
        cols: [{ k: 'a', label: '名前', w: 3, ph: '仕事' },
               { k: 'b', label: '数', w: 1, ph: '5' }],
    },
    quad: {
        name: '四象限', title: '題', add: 'やることを足す',
        cols: [{ k: 'a', label: 'やること', w: 3, ph: '週報' },
               { k: 'b', label: '大事さ', w: 1, ph: '0.8', slide: true },
               { k: 'c', label: '急ぎ', w: 1, ph: '0.9', slide: true }],
    },
    time: {
        name: '年表', title: '題', add: 'できごとを足す',
        cols: [{ k: 'a', label: 'いつ', w: 1, ph: '4月' },
               { k: 'b', label: 'なに', w: 3, ph: '引っ越し' }],
    },
    gantt: {
        name: '予定表', title: '題', add: 'やることを足す',
        cols: [{ k: 'a', label: 'やること', w: 3, ph: '下ごしらえ' },
               { k: 'b', label: '始まり', w: 1, ph: '2026-09-10', date: true },
               { k: 'c', label: '長さ', w: 1, ph: '3d' }],
    },
    mind: {
        name: 'マインドマップ', title: 'まん中', add: '枝を足す',
        cols: [{ k: 'a', label: '枝', w: 1, ph: '仕事' }],
    },
    seq: {
        name: 'やりとり', title: '題', add: 'やりとりを足す',
        cols: [{ k: 'a', label: 'だれが', w: 2, ph: '私' },
               { k: 'b', label: 'だれに', w: 2, ph: '相手' },
               { k: 'c', label: 'なにを', w: 3, ph: 'お願いする' },
               { k: 'dashed', label: '返事', w: 0, check: true }],
    },
};

const FLOW_SHAPE = [['box', '四角'], ['round', '丸み'], ['diamond', 'ひし形']];
const FLOW_DIR = [['LR', '左から右'], ['TD', '上から下'], ['RL', '右から左'], ['BT', '下から上']];

let studio = null;
let studioTimer = 0;

/// 工房を開く。`node` は読む面の図（`.mermaid`）か、描けなかった枠。
async function studioOpen(node) {
    const md = node && node.dataset ? node.dataset.md : undefined;
    if (md === undefined) { say('この図の元の字が取れません'); return; }
    const src = fenceBody(md);
    if (src === null) { say('図ではありません'); return; }
    const data = mmdParse(src);
    studio = { node, data, text: src, raw: !data, good: '' };
    el('studio').hidden = false;
    studioDraw();
    studioShow();
}

/// ` ```mermaid ` の中身を取り出す。図でなければ `null`。
function fenceBody(md) {
    const m = /^`{3,}\s*mermaid\s*\n([\s\S]*?)\n?`{3,}\s*$/.exec(String(md).trim());
    return m ? m[1] : null;
}

/// いまの表（か字）から、図の字を作る。
function studioText() {
    return studio.raw ? studio.text : mmdBuild(studio.data);
}

function studioClose() {
    el('studio').hidden = true;
    clearTimeout(studioTimer);
    studio = null;
    el('read').focus();
}

/// 直したものを、ノートへ返す。
async function studioOk() {
    const md = '```mermaid\n' + studioText().trim() + '\n```';
    const node = studio.node;
    studioClose();
    await readSourceEdit(() => md, node);
}

/* ── 表を描く ── */

function studioDraw() {
    const box = el('studio');
    const spec = studio.data ? DIAGRAM_FORM[studio.data.kind] : null;
    box.querySelector('.kind').textContent = spec ? spec.name : '図';
    const swap = box.querySelector('#studioswap');
    swap.textContent = studio.raw ? '表で直す' : '字で直す';
    // 読み戻せなかった図は、表に戻れない ── 押せる顔をしておいて何も
    // 起きないより、押せないと見えているほうがいい。
    swap.disabled = studio.raw && !studio.data;
    swap.title = swap.disabled
        ? 'この図は表にできません（手で書いた形）。字で直してください'
        : '';

    const form = box.querySelector('.form');
    form.textContent = '';
    if (studio.raw) { form.append(studioRaw()); return; }
    const d = studio.data;

    if (spec.title !== undefined) {
        form.append(studioField(spec.title, d.title, (v) => { d.title = v; studioShow(); }));
    }
    if (d.kind === 'flow') {
        form.append(studioPick('向き', FLOW_DIR, d.dir, (v) => { d.dir = v; studioShow(); }));
        form.append(studioFlow());
        return;
    }
    form.append(studioRows(spec.cols, d.rows, spec.add, () =>
        Object.fromEntries(spec.cols.map((c) => [c.k, c.check ? false : '']))));
}

/// 題のような、一つきりの欄。
function studioField(label, value, set) {
    const wrap = tag('div', 'grp');
    wrap.append(tag('label', '', label));
    const i = document.createElement('input');
    i.type = 'text';
    i.value = value || '';
    i.oninput = () => set(i.value);
    wrap.append(i);
    return wrap;
}

/// 選ぶ欄（向き・形）。
function studioPick(label, opts, value, set) {
    const wrap = tag('div', 'grp');
    if (label) wrap.append(tag('label', '', label));
    const s = document.createElement('select');
    for (const [v, name] of opts) {
        const o = document.createElement('option');
        o.value = v;
        o.textContent = name;
        o.selected = v === value;
        s.append(o);
    }
    s.onchange = () => set(s.value);
    wrap.append(s);
    return wrap;
}

/// 行の並んだ表。上下に動かせて、消せて、足せる。
///
/// **並べ替えを引きずりで作らない。** 引きずりは掴む場所を探すところから
/// 始まって、外した時にどこへ落ちたか分からない ── ↑↓ なら一段ずつ、
/// 見ながら動かせる。
function studioRows(cols, rows, addName, blank) {
    const box = tag('div', 'rows');
    const head = tag('div', 'row hd');
    for (const c of cols) {
        const h = tag('span', '', c.label);
        h.style.flex = c.w ? c.w + ' 1 0' : '0 0 auto';
        head.append(h);
    }
    head.append(tag('span', 'sp'));
    box.append(head);

    rows.forEach((r, n) => {
        const line = tag('div', 'row');
        for (const c of cols) {
            const cell = studioCell(c, r);
            cell.style.flex = c.w ? c.w + ' 1 0' : '0 0 auto';
            line.append(cell);
        }
        const move = (to) => {
            if (to < 0 || to >= rows.length) return;
            rows.splice(to, 0, rows.splice(n, 1)[0]);
            studioDraw();
            studioShow();
        };
        line.append(studioBtn('↑', '一つ上へ', () => move(n - 1)));
        line.append(studioBtn('↓', '一つ下へ', () => move(n + 1)));
        line.append(studioBtn('✕', 'この行を消す', () => {
            rows.splice(n, 1);
            studioDraw();
            studioShow();
        }));
        box.append(line);
    });

    const add = tag('button', 'add', '＋ ' + addName);
    add.onclick = () => {
        rows.push(blank());
        studioDraw();
        studioShow();
        // 足した行の、最初の欄へ ── 足してから掴みに行かせない。
        [...el('studio').querySelectorAll('.rows .row:not(.hd)')].pop()
            ?.querySelector('input')?.focus();
    };
    const wrap = tag('div', 'grp rowsgrp');
    wrap.append(box, add);
    return wrap;
}

/// 一つの欄。数は 0〜1 のつまみ、日付は日付、あとは字。
function studioCell(c, r) {
    if (c.check) {
        const w = tag('label', 'chk');
        const i = document.createElement('input');
        i.type = 'checkbox';
        i.checked = !!r[c.k];
        i.onchange = () => { r[c.k] = i.checked; studioShow(); };
        w.append(i, tag('span', '', '点線'));
        w.title = '返事のような、点線の矢印にする';
        return w;
    }
    if (c.slide) {
        const w = tag('div', 'slide');
        const i = document.createElement('input');
        i.type = 'range';
        i.min = '0'; i.max = '1'; i.step = '0.05';
        i.value = String(num(r[c.k]));
        const n = tag('span', 'n', i.value);
        i.oninput = () => { r[c.k] = i.value; n.textContent = i.value; studioShow(); };
        w.append(i, n);
        return w;
    }
    const i = document.createElement('input');
    i.type = c.date ? 'date' : 'text';
    i.value = r[c.k] || '';
    i.placeholder = c.ph || '';
    i.oninput = () => { r[c.k] = i.value; studioShow(); };
    return i;
}

/// 流れ図。**箱の表と、線の表**の二つ。
///
/// 線の行き先は、箱の名前から選ぶ ── `A`、`B` のような合言葉を人に
/// 打たせない（打たせると、消した箱を指したままの線が残る）。
function studioFlow() {
    const d = studio.data;
    const wrap = document.createDocumentFragment();
    const box = tag('div', 'rows');
    const head = tag('div', 'row hd');
    head.append(tag('span', '', '箱の中の言葉'));
    head.querySelector('span').style.flex = '3 1 0';
    const sh = tag('span', '', '形');
    sh.style.flex = '0 0 92px';
    head.append(sh, tag('span', 'sp'));
    box.append(head);

    d.rows.forEach((r, n) => {
        const line = tag('div', 'row');
        const i = document.createElement('input');
        i.type = 'text';
        i.value = r.a || '';
        i.placeholder = '書く';
        i.style.flex = '3 1 0';
        i.oninput = () => { r.a = i.value; studioRelabel(); studioShow(); };
        line.append(i);
        const pick = studioPick('', FLOW_SHAPE, r.shape, (v) => { r.shape = v; studioShow(); });
        pick.classList.add('bare');
        pick.style.flex = '0 0 92px';
        line.append(pick);
        const move = (to) => {
            if (to < 0 || to >= d.rows.length) return;
            d.rows.splice(to, 0, d.rows.splice(n, 1)[0]);
            studioDraw(); studioShow();
        };
        line.append(studioBtn('↑', '一つ上へ', () => move(n - 1)));
        line.append(studioBtn('↓', '一つ下へ', () => move(n + 1)));
        line.append(studioBtn('✕', 'この箱を消す', () => {
            const gone = d.rows.splice(n, 1)[0];
            d.edges = d.edges.filter((e) => e.from !== gone.id && e.to !== gone.id);
            studioDraw(); studioShow();
        }));
        box.append(line);
    });
    const addNode = tag('button', 'add', '＋ 箱を足す');
    addNode.onclick = () => {
        const id = freshId(d.rows.map((r) => r.id));
        d.rows.push({ id, a: '', shape: 'box' });
        // 足した箱を、いま最後の箱につないでおく ── 置いただけの箱は
        // 図の隅に浮くので、たいてい「つなぐ」まで込みで一つの用事。
        const prev = d.rows[d.rows.length - 2];
        if (prev) d.edges.push({ from: prev.id, b: '', to: id });
        studioDraw(); studioShow();
        // 足した箱へ、そのまま打てるように ── 足してから掴みに行かせない。
        [...el('studio').querySelectorAll('.rowsgrp')][0]
            ?.querySelector('.row:last-of-type input')?.focus();
    };
    const g1 = tag('div', 'grp rowsgrp');
    g1.append(tag('label', '', '箱'), box, addNode);

    const ebox = tag('div', 'rows');
    const ehead = tag('div', 'row hd');
    for (const [t, w] of [['ここから', '2 1 0'], ['線の言葉', '2 1 0'], ['ここへ', '2 1 0']]) {
        const s = tag('span', '', t);
        s.style.flex = w;
        ehead.append(s);
    }
    ehead.append(tag('span', 'sp'));
    ebox.append(ehead);

    const pickable = () => d.rows.map((r) => [r.id, (r.a || '').trim() || r.id]);
    d.edges.forEach((e, n) => {
        const line = tag('div', 'row');
        const from = studioPick('', pickable(), e.from, (v) => { e.from = v; studioShow(); });
        from.classList.add('bare', 'nodepick'); from.style.flex = '2 1 0';
        const label = document.createElement('input');
        label.type = 'text';
        label.value = e.b || '';
        label.placeholder = '（なくてもいい）';
        label.style.flex = '2 1 0';
        label.oninput = () => { e.b = label.value; studioShow(); };
        const to = studioPick('', pickable(), e.to, (v) => { e.to = v; studioShow(); });
        to.classList.add('bare', 'nodepick'); to.style.flex = '2 1 0';
        line.append(from, label, to);
        line.append(studioBtn('✕', 'この線を消す', () => {
            d.edges.splice(n, 1);
            studioDraw(); studioShow();
        }));
        ebox.append(line);
    });
    const addEdge = tag('button', 'add', '＋ 線を足す');
    addEdge.onclick = () => {
        if (d.rows.length < 2) { say('線を引くには、箱が二つ要ります'); return; }
        d.edges.push({ from: d.rows[0].id, b: '', to: d.rows[1].id });
        studioDraw(); studioShow();
    };
    const g2 = tag('div', 'grp rowsgrp');
    g2.append(tag('label', '', '線'), ebox, addEdge);

    wrap.append(g1, g2);
    return wrap;
}

/// 箱の名前を変えたら、線の「ここから／ここへ」の見え方も変える。
///
/// **全部を描き直さない。** 描き直すと、いま打っている欄から caret が飛ぶ
/// ── 一文字ごとに欄を掴み直すことになる。動くのは名札だけなので、
/// 名札だけ書き換える。
function studioRelabel() {
    for (const pick of el('studio').querySelectorAll('.nodepick select')) {
        for (const o of pick.options) {
            const r = studio.data.rows.find((x) => x.id === o.value);
            if (r) o.textContent = (r.a || '').trim() || r.id;
        }
    }
}

/// まだ使っていない合言葉を一つ。`A`…`Z`、尽きたら `N1`、`N2`…
function freshId(used) {
    for (let n = 0; n < 26; n++) {
        const id = String.fromCharCode(65 + n);
        if (!used.includes(id)) return id;
    }
    for (let n = 1; ; n++) if (!used.includes('N' + n)) return 'N' + n;
}

/// 字で直す面。**表にできない図の逃げ道**であり、書き方を覚えた人の近道。
function studioRaw() {
    const wrap = tag('div', 'grp rawgrp');
    wrap.append(tag('label', '', 'mermaid の字'));
    const t = document.createElement('textarea');
    t.value = studio.text;
    t.spellcheck = false;
    t.oninput = () => { studio.text = t.value; studioShow(); };
    wrap.append(t);
    if (!studio.data) {
        wrap.append(tag('div', 'note',
            'この図は表にできない形（手で書いたか、amber の知らない書き方）です。'
            + '右の絵を見ながら、ここで直してください。'));
    }
    return wrap;
}

function studioBtn(text, title, go) {
    const b = tag('button', 'mini', text);
    b.title = title;
    b.onclick = go;
    return b;
}

function tag(name, cls, text) {
    const n = document.createElement(name);
    if (cls) n.className = cls;
    if (text !== undefined) n.textContent = text;
    return n;
}

/* ── 図を描く ── */

/// 打つたびに描き直す。**待たせない程度に間を置く** ── 一文字ごとに
/// 描くと、打っている最中の壊れた字で「図にできません」が点滅する。
function studioShow() {
    clearTimeout(studioTimer);
    studioTimer = setTimeout(studioRender, 170);
}

async function studioRender() {
    if (!studio) return;
    const src = studioText().trim();
    const view = el('studio').querySelector('.view');
    const err = el('studio').querySelector('.err');
    let lib;
    try {
        lib = await loadMermaid();
    } catch (e) {
        err.hidden = false;
        err.textContent = '図を読めません: ' + (e && e.message ? e.message : e);
        return;
    }
    if (!studio) return;
    try {
        const { svg } = await lib.render('studio' + (++mermaidSeq), src);
        if (!studio) return;
        view.innerHTML = svg;
        studio.good = svg;
        err.hidden = true;
    } catch (e) {
        if (!studio) return;
        // **直前の図を残したまま、言う。** 消してしまうと、打ち間違えた
        // 一文字のあいだ図が消えて、何を直していたのか分からなくなる。
        if (studio.good) view.innerHTML = studio.good;
        err.hidden = false;
        err.textContent = 'いまの字では図になりません: ' + (e && e.message ? e.message : e);
    }
}

/* ── 工房の受け口 ── */

el('studio').addEventListener('keydown', (e) => {
    // 中で打った字を、外の近道に取られない（`b` で太字、`e` で面替え…）。
    e.stopPropagation();
    if (e.isComposing || e.keyCode === 229) return;
    if (e.code === 'Escape') { e.preventDefault(); studioClose(); return; }
    if (e.code === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); studioOk(); }
});
el('studio').addEventListener('mousedown', (e) => {
    if (e.target.id === 'studio') studioClose();
});
el('studioswap').onclick = () => {
    if (studio.raw) {
        // 字 → 表。読めなければ、表にせず言う ── 読めない字を無理に
        // 表へ入れると、読めなかったところが消える。
        const back = mmdParse(studio.text);
        if (!back) { say('いまの字は表にできません。字のまま直してください'); return; }
        studio.data = back;
        studio.raw = false;
    } else {
        studio.text = studioText();
        studio.raw = true;
    }
    studioDraw();
    studioShow();
};
el('studiocancel').onclick = () => studioClose();
el('studiook').onclick = () => studioOk();

/// コード枠に色を付ける。
///
/// **Monaco の色付けをそのまま借りる。** ハイライタを別に持ってこない ──
/// エディタは既にこの機械の中で `rust` も `js` も色分けしていて、同じ言語に
/// 二つの色分けを持つと、書いている面と読める面で色が違うノートができる。
/// 語彙は `vendor/monaco/vs/basic-languages` にあるものがそのまま効く。
async function paintCode() {
    const seq = readSeq;
    for (const code of el('read').querySelectorAll('pre > code[class^="language-"]')) {
        const lang = code.className.replace('language-', '').trim();
        // mermaid は図として描くので、字に色を付けない。
        if (!lang || lang === 'mermaid') continue;
        try {
            const painted = await monaco.editor.colorize(code.textContent, lang, { tabSize: 4 });
            // 組み直しに追い越されていたら、もう別のノートを見ている。
            if (seq !== readSeq) return;
            code.innerHTML = painted;
        } catch {
            // 知らない語彙なら、素のまま。**色が付かないのは読めないことではない。**
        }
    }
}

/// ノートの隣にある絵を、ノートの隣から読む。
///
/// **`![](attachments/x.jpg)` はノートからの相対**で、窓の `index.html`
/// からの相対ではない。直さないと、貼った絵がぜんぶ欠けた四角で出る。
/// 掛け替えた札に、元の行と元の字を持たせる。
function keepMark(from, to) {
    for (const k of ['line', 'span', 'md']) {
        if (from.dataset[k] !== undefined) to.dataset[k] = from.dataset[k];
    }
}

function findPictures() {
    const dir = state.open ? state.open.path.replace(/[^/]*$/, '') : '';
    for (const img of el('read').querySelectorAll('img')) {
        const src = img.getAttribute('src') || '';
        if (src && !/^[a-z][a-z0-9+.-]*:/i.test(src) && !src.startsWith('//')) {
            img.src = 'file://' + encodeURI(src.startsWith('/') ? src : dir + src);
        }
        // **`alt` は書いた人の言葉。** 出せば説明になり、出さなければ
        // 読み上げにしか届かない字になる。書いていなければ何も足さない
        // ── 空の札は、説明の無いことを説明しているように見える。
        const alt = (img.getAttribute('alt') || '').trim();
        const box = document.createElement('figure');
        keepMark(img, box);
        img.replaceWith(box);
        box.append(img);
        if (alt) {
            const cap = document.createElement('figcaption');
            cap.textContent = alt;
            box.append(cap);
        }
    }
}

/// 読める形の中で押せるもの ── 升と、リンク。
el('read').addEventListener('click', async (e) => {
    const box = e.target.closest('.box');
    if (box) {
        // **打ち込みと同じ道を通す。** `check` は字を返すだけで、保存は
        // いつもの `save()` ── だから衝突の検査も同じものが効く。
        const line = Number(box.dataset.line);
        const done = box.textContent.trim() === '☐';
        try {
            const r = await ask('check', { text: whole(), line, done });
            const cut = await ask('split', { text: r.text });
            loading = true;
            editor.setValue(cut.body || '');
            loading = false;
            state.head = cut.head || '';
            state.dirty = true;
            await save();
            await drawRead();
        } catch (err) {
            say('直せません: ' + err.message);
        }
        return;
    }
    // 図は、押すと工房が開く ── 書く面へ送っても、そこにあるのは
    // `flowchart LR` で、直せる人はもう工房を要らない。描けなかった枠
    // （`pre.bad`）も同じ扉から ── **直したいのは、まさに壊れた図**。
    const art = diagramAt(e.target);
    if (art) { e.preventDefault(); studioOpen(art); return; }
    const a = e.target.closest('a');
    if (!a) {
        // **触れないかたまりは、書く面のその行へ送る。**
        // 打てるのに保存されない、を作らないための逃げ道。
        // **`richBlock()` と同じ顔ぶれにする。** ここだけ古いままだと、
        // 触れるようにしたはずの表を押した瞬間に書く面へ飛ぶ（実際に飛んだ）。
        const rich = e.target.closest('pre, figure, .mermaid');
        if (rich && el('read').contains(rich)) {
            const at = Number(rich.dataset.line);
            setView('split');
            if (!Number.isNaN(at) && editor) {
                const line = Math.max(at - headLines(), 0) + 1;
                editor.revealLineNearTop(line);
                editor.setPosition({ lineNumber: line, column: 1 });
                editor.focus();
            }
        }
        return;
    }
    // **窓の中では開かせない。** 踏んだ先に窓ごと持っていかれると、
    // 題字を窓の中に描いている以上、戻る道が無い。
    e.preventDefault();
    const href = a.getAttribute('href') || '';
    if (href.startsWith('#')) {
        let id = href.slice(1);
        try { id = decodeURIComponent(id); } catch { /* そのまま使う */ }
        el('read').querySelector(`[id="${CSS.escape(id)}"]`)
            ?.scrollIntoView({ behavior: 'smooth', block: 'start' });
        return;
    }
    if (!(await window.amber.openLink(href))) say('この行き先は開けません: ' + href);
});

// 右押しでも同じ扉。**押しても右押しでも開く** ── どちらだったかを
// 覚えている人はいないので、両方に置く。
el('read').addEventListener('contextmenu', (e) => {
    const art = diagramAt(e.target);
    if (!art) return;
    e.preventDefault();
    studioOpen(art);
});

/// 押されたところの図。描けた図（`.mermaid`）と、描けなかった枠のどちらも。
function diagramAt(target) {
    const box = el('read');
    const done = target.closest('.mermaid');
    if (done && box.contains(done)) return done;
    const pre = target.closest('pre');
    if (pre && box.contains(pre) && pre.querySelector('code.language-mermaid')) return pre;
    return null;
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
    // **読む面も打っている場所。** ここを数え忘れると、Enter が命令として
    // 拾われて焦点が書く面へ飛ぶ ── 読む面で改行できない、として出た。
    const inRead = el('read').contains(e.target);

    // **画面の形は、どこを打っていても効く。** 書いている最中に読みたく
    // なるのだから、エディタの中でこそ効かないと意味がない ── だから
    // 「打っている場所では素の一文字は文字」の線より手前に置く。
    if (e.code === 'F12') { e.preventDefault(); setZen(!zen); return; }
    if (e.code === 'Escape') {
        // **手前にあるものから閉じる。** 小窓が開いているのに大きい画面が
        // 戻ると、閉じたつもりのものが残る。
        if (!el('more').hidden) { e.preventDefault(); closeMenu(); return; }
        if (!el('veil').hidden) { e.preventDefault(); closeSheet(null); return; }
        if (zen) { e.preventDefault(); setZen(false); return; }
        // 読む面で打っている途中の Esc は、書き戻してから手を離す。
        if (el('read').contains(e.target)) { syncRead(); document.activeElement?.blur(); return; }
    }

    // **修飾キー付きは、素の一文字より先に。**
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.code === 'KeyP') {
        e.preventDefault(); palette(); return;
    }
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.code === 'KeyO') {
        e.preventDefault(); toggleToc(); return;
    }
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyD' && state.open) {
        e.preventDefault(); cmdStar(); return;
    }
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyE') { e.preventDefault(); toggleRead(); return; }
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyP') { e.preventDefault(); toggleSplit(); return; }
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyO') { e.preventDefault(); cmdOpenOutside(); return; }
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyS') { e.preventDefault(); save(); return; }
    // 字の大きさ。**`Equal` と `Minus` は位置のキー** ── JIS では `+` は
    // Shift を要り、`e.key` で当てると刻印どおりに打っても効かない。
    if ((e.metaKey || e.ctrlKey) && (e.code === 'Equal' || e.code === 'NumpadAdd')) {
        e.preventDefault(); setFont(fontStep + 1); return;
    }
    if ((e.metaKey || e.ctrlKey) && (e.code === 'Minus' || e.code === 'NumpadSubtract')) {
        e.preventDefault(); setFont(fontStep - 1); return;
    }
    if ((e.metaKey || e.ctrlKey) && e.code === 'Digit0') {
        e.preventDefault(); setFont(0); return;
    }
    // 書く道具。**帯のボタンと同じ一本の道を通す** ── 押した形と打った形で
    // 結果が違うと、どちらかが嘘になる。
    if (e.metaKey || e.ctrlKey) {
        const hit = markKey(e);
        if (hit) { e.preventDefault(); hit(); return; }
    }
    // 左の列を畳む（Inkdrop の ⌘/）。狭い画面では二列ぶんが効く。
    if ((e.metaKey || e.ctrlKey) && e.code === 'Slash') { e.preventDefault(); toggleRail(); return; }
    // 見たノートの前後（Inkdrop の ⌘← / ⌘→）。
    if ((e.metaKey || e.ctrlKey) && e.code === 'ArrowLeft') { e.preventDefault(); walk(-1); return; }
    if ((e.metaKey || e.ctrlKey) && e.code === 'ArrowRight') { e.preventDefault(); walk(1); return; }
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
    if (inField || inEditor || inRead) return;

    if (e.code === 'ArrowDown' || e.code === 'KeyJ') { e.preventDefault(); moveCursor(1); }
    else if (e.code === 'ArrowUp' || e.code === 'KeyK') { e.preventDefault(); moveCursor(-1); }
    else if (e.code === 'Enter') { e.preventDefault(); if (editor) editor.focus(); }
    else if (e.code === 'KeyN') { e.preventDefault(); newNote(); }
    else if (e.code === 'Slash') { e.preventDefault(); el('find').focus(); el('find').select(); }
});

/// 修飾キー付きの一打を、書く道具の一押しに。
///
/// **`e.code` で当てる。** JIS では `e.key` が `Process` になり、数字の
/// 段は配列で別の字になる ── `Digit8` は「8 の位置のキー」なので動く。
function markKey(e) {
    const c = e.code;
    if (e.shiftKey) {
        if (c === 'KeyX') return () => applyMark('wrap', '~~');
        if (c === 'KeyC') return () => applyMark('wrap', '`');
        if (c === 'Digit8') return () => applyMark('line', '- ');
        if (c === 'Digit9') return () => applyMark('line', '- [ ] ');
        if (c === 'Digit7') return () => applyMark('line', '1. ');
        return null;
    }
    if (c === 'KeyB') return () => applyMark('wrap', '**');
    if (c === 'KeyI') return () => applyMark('wrap', '*');
    if (c === 'KeyK') return () => put('[](https://)', 1);
    if (c === 'Digit1') return () => applyMark('heading');
    return null;
}

/// 貼り付けられたものが絵なら、ノートの隣に置いてリンクを打つ。
///
/// **捕まえるのは絵のときだけ。** 字の貼り付けはエディタの仕事で、
/// ここが横取りすると Monaco の取り消しが繋がらなくなる。
document.addEventListener('paste', async (e) => {
    if (!state.open || !editor || view === 'read') return;
    const items = [...(e.clipboardData?.items || [])];
    const pic = items.find((i) => i.kind === 'file' && i.type.startsWith('image/'));
    if (!pic) return;
    e.preventDefault();
    e.stopPropagation();
    const got = await window.amber.clipboardImage();
    if (!got) { say('その絵は読めません'); return; }
    await attach(got.b64, got.ext);
}, true);

/// 左の列を畳む。
let railOff = false;
function toggleRail() {
    railOff = !railOff;
    document.body.classList.toggle('norail', railOff);
    window.amber.remember({ railOff });
    if (editor) setTimeout(() => editor.layout(), 0);
}

/// 見たノートの前後をたどる。
///
/// **開いた順に積む。** 一覧の並び順ではない ── 「さっき見ていたもの」は
/// 並び順の隣ではなく、たどった道の隣にある。
const trail = [];
let trailAt = -1;
function trailPush(path) {
    if (trail[trailAt] === path) return;
    trail.splice(trailAt + 1);
    trail.push(path);
    trailAt = trail.length - 1;
}
function walk(step) {
    const to = trailAt + step;
    if (to < 0 || to >= trail.length) { say(step < 0 ? 'これより前はありません' : 'これより後はありません'); return; }
    trailAt = to;
    // たどっている間は積み直さない ── 積むと前へ戻れなくなる。
    openNote(trail[to], { walking: true });
}

/* ── 窓ができること、ひとつの表 ── */

/// **パレットも ⋯ の献立も、ここを見る。**
///
/// 二か所に書くと、片方にだけ増えた命令ができて、そのうち「あるはずなのに
/// 無い」になる。`need` は要るもの: `note` は開いているノート、`root` は
/// 置き場所（いつもある）。`menu` が真なら、⋯ の献立にも出る。
const CMDS = [
    { id: 'new', name: '新しいノート', key: '⌘N', run: () => newNote() },
    { id: 'outside', name: 'amberフォルダ以外のノートを開く', key: '⌘O', app: true,
      run: cmdOpenOutside },
    { id: 'save', name: '保存', key: '⌘S', need: 'note', run: () => save() },
    { id: 'read', name: '表示 / コードを入れ替え', key: '⌘E', need: 'note', run: () => toggleRead() },
    { id: 'split', name: '並べて表示', key: '⌘P', need: 'note', run: () => toggleSplit() },
    { id: 'zen', name: 'ノートだけを大きく', key: 'F12', need: 'note', run: () => setZen(!zen) },
    { id: 'toc', name: '目次', key: '⌘⇧O', need: 'note', run: () => toggleToc() },
    { id: 'rail', name: '左の列を畳む', key: '⌘/', run: () => toggleRail() },
    { id: 'back', name: '前に見たノート', key: '⌘←', run: () => walk(-1) },
    { id: 'fwd', name: '次に見たノート', key: '⌘→', run: () => walk(1) },
    { id: 'find', name: 'ノートを探す', key: '⌘F', run: () => { el('find').focus(); el('find').select(); } },
    { id: 'find2', name: 'フィルタ（タグ・フォルダ・期間）', app: true, run: cmdFind },

    // ── このノートにすること（⋯ と、ノートの右押し）
    { id: 'star', name: 'ブックマークに登録', key: '⌘D', need: 'note', menu: true, run: cmdStar },
    { id: 'tags', name: 'ノートのタグ設定', need: 'note', menu: true, run: cmdTags },
    { id: 'move', name: 'フォルダへ移動', need: 'note', menu: true, run: cmdMove },
    { id: 'remind', name: '通知設定', need: 'note', menu: true, run: cmdRemind },
    { id: 'export', name: 'エクスポート', need: 'note', menu: true, run: cmdExport },
    { id: 'delete', name: 'ゴミ箱へ入れる', need: 'note', menu: true, sep: true, run: cmdDelete },

    // ── amber のこと（⚙）
    { id: 'syntax', name: 'マークダウンの書き方', key: '⌘⇧/', app: true, run: cmdSyntax },
    { id: 'theme', name: 'テーマ', app: true, run: cmdTheme },
    { id: 'vim', name: 'vimモード', app: true, run: cmdVim },
    { id: 'lineno', name: '行番号', app: true, run: cmdLineNo },
    { id: 'backup', name: '一括バックアップ', app: true, sep: true, run: cmdBackup },
    { id: 'root', name: 'amber保存ディレクトリ変更', app: true, run: cmdRoot },
    { id: 'all', name: 'コマンド一覧', key: '⌘⇧P', app: true, sep: true, run: () => palette() },

    // ── 表には要るが、献立には出さないもの
    { id: 'mkbook', name: '新しいフォルダを作る', run: () => cmdMkBook() },
    { id: 'color', name: 'フォルダに色を付ける', sub: 'フォルダを右押しでも', run: () => cmdColor() },
    { id: 'bigger', name: '字を大きく', key: '⌘+', run: () => setFont(fontStep + 1) },
    { id: 'smaller', name: '字を小さく', key: '⌘−', run: () => setFont(fontStep - 1) },
    { id: 'font0', name: '字の大きさを戻す', key: '⌘0', run: () => setFont(0) },
];

const canRun = (c) => c.need !== 'note' || !!state.open;

/// 命令のパレット（⌘⇧P）。**名前で探せれば、覚えなくていい。**
async function palette() {
    const items = CMDS.filter(canRun).map((c) => ({
        name: c.name, key: c.key || '', value: c.id,
    }));
    const id = await askPick('何をしますか', items, '↑↓ で選び、Enter で実行');
    if (id === null) return;
    const c = CMDS.find((x) => x.id === id);
    if (c) await c.run();
}

/// ⋯ の献立。**パレットと同じ表の、ノートに関わるところだけ。**
/// `which` が `app` なら設定の献立、既定はノートの献立。
///
/// **「見た目」をノートの右押しに出さない。** ノートを右押しした人が
/// 訊いているのは「このノートをどうするか」で、アプリの色ではない。
function openMenu(at, which) {
    const box = el('more');
    const key = which === 'app' ? 'app' : 'menu';
    const items = CMDS.filter((c) => c[key] && canRun(c)).map((c) => {
        // **いまどうなっているかを、押す前に見せる。**
        if (c.id === 'theme') return { ...c, sub: 'いま: ' + themeName() };
        if (c.id === 'vim') return { ...c, sub: vimOn ? 'いま: vim' : 'いま: 素のメモ帳' };
        if (c.id === 'lineno') return { ...c, sub: lineNo ? 'いま: 出している' : 'いま: 出していない' };
        if (c.id === 'root') return { ...c, sub: shortPath(state.root) };
        return c;
    });
    if (!items.length) return;
    // 区切りは**印の付いた命令の手前**に置く ── 「最後の一つの前」に
    // すると、命令が増えた日に区切りが勝手に動く。
    box.innerHTML = items.map((c) =>
        (c.sep ? '<div class="sep"></div>' : '')
        + '<button data-id="' + c.id + '">' + escapeHtml(c.name)
        + (c.sub ? '<span class="sub">' + escapeHtml(c.sub) + '</span>' : '')
        + (c.key ? '<span class="k">' + escapeHtml(c.key) + '</span>' : '') + '</button>').join('');
    for (const b of box.querySelectorAll('button')) {
        b.onclick = async () => {
            closeMenu();
            const c = CMDS.find((x) => x.id === b.dataset.id);
            if (c) await c.run();
        };
    }
    box.hidden = false;
    // 画面の外へはみ出さない。**右端に置くものなので、右から測る。**
    const w = box.offsetWidth;
    box.style.left = Math.max(8, Math.min(at.right - w, innerWidth - w - 8)) + 'px';
    box.style.top = (at.bottom + 6) + 'px';
    setTimeout(() => document.addEventListener('mousedown', closeMenuOnce, { once: true }), 0);
}

function closeMenu() { el('more').hidden = true; }
function closeMenuOnce(e) { if (!el('more').contains(e.target)) closeMenu(); }

/* ── 目次 ── */

let tocOn = false;
let tocTimer = null;

function toggleToc() {
    tocOn = !tocOn;
    window.amber.remember({ tocOn });
    applyView();
    if (tocOn) drawToc();
}

/// 見出しを並べる。
///
/// **何が見出しかは core が決める**（`note::blocks`）── 窓が `#` を数え
/// はじめると、`#仕事` というタグの行が目次に出る（空白の有無で決まる）。
/// 行番号も core が持ってくるので、飛び先を数え直さなくていい。
async function drawToc() {
    if (!tocOn || !state.open || !editor) return;
    let heads;
    try {
        heads = ((await ask('blocks', { text: whole() })).blocks || [])
            .filter((b) => b.kind === 'heading');
    } catch {
        return;
    }
    const box = el('toc');
    if (!heads.length) {
        box.innerHTML = '<div class="none">見出しがありません</div>';
        return;
    }
    box.innerHTML = heads.map((h, n) =>
        '<button class="h" data-l="' + h.level + '" data-n="' + n + '">'
        + escapeHtml(h.text) + '</button>').join('');
    for (const b of box.querySelectorAll('.h')) {
        b.onclick = () => gotoHead(heads[Number(b.dataset.n)]);
    }
}

/// 見出しへ飛ぶ。書く面ならその行へ、読む面ならその見出しへ。
function gotoHead(h) {
    if (view !== 'read' && editor) {
        // core の行番号は前書きを含むファイルの行。エディタは本文だけを
        // 持っているので、前書きのぶんを引く。
        const cut = state.head ? state.head.split('\n').length - 1 : 0;
        const line = Math.max(h.line - cut, 0) + 1;
        editor.revealLineNearTop(line);
        editor.setPosition({ lineNumber: line, column: 1 });
        editor.focus();
    }
    if (view !== 'write') {
        const want = h.text.trim();
        for (const el2 of el('read').querySelectorAll('h1,h2,h3,h4,h5,h6')) {
            if (el2.textContent.trim() === want) {
                el2.scrollIntoView({ behavior: 'smooth', block: 'start' });
                break;
            }
        }
    }
}

/* ── 色 ── */

/// 選べるテーマ。三つ目は「暗いか」── `null` は OS に訊く。
///
/// **名前で選んだら、選んだとおりに出す。** 「ayu-dark にしたのに昼は
/// 明るい」は、選んだことにならない。既定（空）だけが OS に従う。
const THEMES = [
    ['', '琥珀 ── OS に合わせる', null],
    ['amber-light', '琥珀 ── 明るい', false],
    ['amber-dark', '琥珀 ── 暗い', true],
    ['ayu-light', 'ayu ── 明るい', false],
    ['ayu-mirage', 'ayu ── 中間（mirage）', true],
    ['ayu-dark', 'ayu ── 暗い', true],
    ['paper', '紙 ── 白と黒だけ', false],
];
let theme = '';

/// いま暗いか。**Monaco と mermaid にも同じ答えを渡す** ── 別々に訊くと、
/// テーマを替えた日にエディタだけ前の明暗で残る。
function isDark() {
    const t = THEMES.find(([k]) => k === theme);
    return t && t[2] !== null ? t[2] : matchMedia('(prefers-color-scheme: dark)').matches;
}

function setTheme(name) {
    theme = name || '';
    if (theme) document.documentElement.dataset.theme = theme;
    else delete document.documentElement.dataset.theme;
    window.amber.remember({ theme });
    if (window.monaco && editor) monaco.editor.setTheme(isDark() ? 'vs-dark' : 'vs');
    if (Mermaid) {
        Mermaid.initialize(mermaidOpts());
        // 図は初期化し直しただけでは色が変わらない ── 描き直す。
        if (view !== 'write') drawRead();
    }
}

/// いま出ているテーマの名前。献立に添える。
function themeName() {
    const t = THEMES.find(([k]) => k === theme);
    return t ? t[1].split(' ── ')[0] + (t[1].includes('──') ? '・' + t[1].split('── ')[1] : '') : '琥珀';
}

async function cmdTheme() {
    const at = await askPick('テーマ', THEMES.map(([k, n]) => ({
        name: n, sub: k === theme ? '● いま' : '', value: k,
    })), '琥珀は育ててきたもの。ayu は書く道具の定番。紙は刷るため');
    if (at === null) return;
    setTheme(at);
}

/* ── 左の列の、作ると消す ── */

/// 段の見出しに「＋」を添える。
///
/// **できることは前からあった。** フォルダもブックマークも階層に
/// なるし、タグもノートに付ければ増える ── ただ、それを言う場所が画面に
/// 無かった。使えないのと、あるのに見えないのは、使う人には同じこと。
function head(name, plus) {
    return '<div class="head">' + escapeHtml(name)
        + (plus ? '<button class="plus" data-plus="' + plus + '" title="増やす">＋</button>' : '')
        + '</div>';
}

async function railPlus(kind) {
    if (kind === 'book') { await cmdMkBook(); return; }
    if (kind === 'star') {
        const name = await askText('新しいブックマークの置き場所の名前', '', '仕事/週次 と書けば階層になります');
        if (name === null || !name.trim()) return;
        try {
            await ask('shelf', { path: state.root, name: name.trim() });
            await reload({ quiet: true });
            say('「' + name.trim() + '」を作りました');
        } catch (e) {
            say('作れません: ' + e.message);
        }
        return;
    }
    if (kind === 'tag') {
        // **タグはノートに付いて生まれる。** 空のタグを作れるようにすると、
        // どのノートにも付いていないタグが並ぶ列ができる。
        if (!state.open) { say('タグを付けるノートを、先に開いてください'); return; }
        await cmdTags();
    }
}

/// 行き先を右押ししたときの献立。**フォルダ・タグ・ブックマークを、名前ごと直す。**
function railMenu(kind, what, at) {
    if (!what) return;
    const box = el('more');
    const items = [];
    // **下の階層は、ここから作る。** 名前に「/」を打たせるのは、
    // 書き方を知っている人にしか通じない。
    if (kind === 'book') {
        items.push({ name: 'この中にフォルダを作る', run: () => cmdMkBook(what) });
        items.push({ name: 'フォルダに色を付ける', run: () => cmdColor(what) });
    }
    if (kind === 'star') {
        items.push({ name: 'この中に置き場所を作る', run: () => newShelf(what) });
    }
    items.push({ name: '名前を変える', sep: items.length > 0, run: () => railRename(kind, what) });
    items.push({
        name: kind === 'book' ? 'このフォルダをゴミ箱へ'
            : (kind === 'tag' ? 'このタグを全部のノートから外す' : 'この置き場所を全部のノートから外す'),
        run: () => railDrop(kind, what),
    });
    box.innerHTML = items.map((c, n) =>
        (c.sep ? '<div class="sep"></div>' : '')
        + '<button data-n="' + n + '">' + escapeHtml(c.name) + '</button>').join('');
    for (const b of box.querySelectorAll('button')) {
        b.onclick = () => { closeMenu(); items[Number(b.dataset.n)].run(); };
    }
    box.hidden = false;
    const w = box.offsetWidth;
    box.style.left = Math.max(8, Math.min(at.x, innerWidth - w - 8)) + 'px';
    box.style.top = (at.y + 4) + 'px';
    setTimeout(() => document.addEventListener('mousedown', closeMenuOnce, { once: true }), 0);
}

/// そのフォルダ・タグ・ブックマークに居るノート。
function underRail(kind, what) {
    if (kind === 'book') {
        return state.notes.filter((n) => n.book === what || (n.book || '').startsWith(what + '/'));
    }
    if (kind === 'tag') return state.notes.filter((n) => (n.tags || []).includes(what));
    return state.notes.filter((n) => n.star === what || (n.star || '').startsWith(what + '/'));
}

async function railRename(kind, what) {
    const to = await askText('新しい名前', what,
        kind === 'book' ? '仕事/2026 と書けば階層になります' : '');
    if (to === null || !to.trim() || to.trim() === what) return;
    const name = to.trim();
    const hit = underRail(kind, what);
    try {
        if (kind === 'book') {
            // **中のノートを一本ずつ移す。** フォルダはただのディレクトリで、
            // 名前を変えるのは中身を動かすこと ── 途中で止まっても、動いた
            // ぶんは新しい名前の下にちゃんと居る。
            await ask('mkbook', { dir: state.root + '/' + name });
            for (const n of hit) {
                const sub = (n.book || '').slice(what.length).replace(/^\//, '');
                const dir = state.root + '/' + name + (sub ? '/' + sub : '');
                await ask('mkbook', { dir });
                await ask('move', { path: n.path, dir });
            }
            await window.amber.trash(state.root + '/' + what);
        } else {
            for (const n of hit) await retagOne(n, kind, what, name);
        }
        state.dest = { kind, what: name };
        await reload({ quiet: true });
        say('「' + name + '」に変えました（' + hit.length + ' 件）');
    } catch (e) {
        say('変えられません: ' + e.message);
    }
}

async function railDrop(kind, what) {
    const hit = underRail(kind, what);
    const what2 = kind === 'book' ? 'フォルダ' : (kind === 'tag' ? 'タグ' : 'ブックマーク');
    const ask2 = kind === 'book'
        ? '「' + what + '」を、中の ' + hit.length + ' 件ごとゴミ箱へ入れますか'
        : '「' + what + '」の' + what2 + 'を ' + hit.length + ' 件から外しますか（ノートは残ります）';
    if (!await askYes(ask2)) return;
    try {
        if (kind === 'book') {
            if (!await window.amber.trash(state.root + '/' + what)) {
                say('ゴミ箱へ入れられません');
                return;
            }
        } else {
            for (const n of hit) await retagOne(n, kind, what, null);
        }
        state.dest = { kind: 'all', what: '' };
        state.open = null;
        applyView();
        await reload({ quiet: true });
        say(kind === 'book' ? 'ゴミ箱へ入れました' : '外しました（' + hit.length + ' 件）');
    } catch (e) {
        say('外せません: ' + e.message);
    }
}

/// 一本のノートのタグ（またはブックマーク）を、付け替える。`to` が `null` なら外す。
///
/// **開いているノートは、開いたまま直す。** 直接ファイルを書くと、窓が
/// 持っている字と食い違い、次の保存でどちらかが消える。
async function retagOne(n, kind, from, to) {
    const same = state.open && state.open.path === n.path;
    const text = same ? whole() : (await ask('read', { path: n.path })).text;
    let out;
    if (kind === 'tag') {
        const tags = (n.tags || []).filter((t) => t !== from);
        if (to) tags.push(to);
        out = (await ask('settags', { text, tags })).text;
    } else {
        const sub = (n.star || '').slice(from.length);
        out = (await ask('star', to === null ? { text } : { text, shelf: to + sub })).text;
    }
    if (same) {
        await putWhole(out);
    } else {
        const r = await ask('write', { path: n.path, text: out });
        if (r && r.conflict) throw new Error(n.path + ' は別のところから書き換えられています');
    }
}

/* ── 外から来た一本 ── */

/// amber の置き場所の外にある `.md` を、**単発で**開く。
///
/// 置き場所を入れ替えない ── 一本開くたびに一覧が丸ごと変わると、
/// 「さっきまでのノートが消えた」に見える。並べても持たない ── 「フォルダが
/// そのまま索引」という前提の外にあるものを索引に混ぜると、索引が索引で
/// なくなる。
///
/// **異例な開き方だと、画面が言う。** 左の二列を出さず、上に帯を出す ──
/// 出さないと、いつもの一本と見分けが付かないまま別のフォルダへ書く。
/// 閉じれば、さっきまで見ていた一覧とノートが戻る。
let guestBack = null;

/// 道を、読める長さに。**真ん中を落とす** ── 頭（どこの家か）と
/// 末尾（何というファイルか）が、どちらも効く。
function shortPath(at) {
    const home = (state.root || '').match(/^(\/Users\/[^/]+)/);
    let t = home && at.startsWith(home[1]) ? '~' + at.slice(home[1].length) : at;
    const part = t.split('/');
    if (part.length > 5) t = part.slice(0, 2).join('/') + '/…/' + part.slice(-2).join('/');
    return t;
}

async function openGuest(path) {
    if (!path || !/\.(md|markdown|txt)$/i.test(path)) {
        say('開けるのは .md / .markdown / .txt です');
        return;
    }
    let note;
    try {
        note = await ask('note', { path });
    } catch (e) {
        say('開けません: ' + e.message);
        return;
    }
    // 書きかけを置いていかない ── 戻ったときに消えている、を作らない。
    if (state.dirty) await save();
    // 戻り先を憶える。**開くより先に。** 途中で失敗しても道が残る。
    if (!state.guest) guestBack = { open: state.open, dest: state.dest, view };
    state.guest = true;
    document.body.classList.add('guest');
    el('guestbar').hidden = false;
    el('guestwhere').textContent = shortPath(path);
    await openNote(path, { guest: note });
}

function closeGuest() {
    if (!state.guest) return;
    state.guest = false;
    document.body.classList.remove('guest');
    el('guestbar').hidden = true;
    const back = guestBack;
    guestBack = null;
    state.dest = back?.dest || { kind: 'all', what: '' };
    if (back?.view) view = back.view;
    state.open = null;
    applyView();
    reload({ quiet: true }).then(() => {
        if (back && back.open) openNote(back.open.path);
        else drawList();
    });
}

/// `.md` を窓に落としたら、同じ開き方をする。
///
/// **Electron 32 から `File.path` は無い。** `webUtils.getPathForFile` で
/// 訊く（preload の向こう側）── 描く側が勝手にファイルの道を知れる口は
/// 作らない。
document.addEventListener('dragover', (e) => {
    if (!e.dataTransfer?.types?.includes('Files')) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
    document.body.classList.add('dropping');
});
document.addEventListener('dragleave', (e) => {
    if (e.relatedTarget) return;
    document.body.classList.remove('dropping');
});
document.addEventListener('drop', async (e) => {
    if (!e.dataTransfer?.files?.length) return;
    e.preventDefault();
    document.body.classList.remove('dropping');
    const at = window.amber.pathOf(e.dataTransfer.files[0]);
    if (!at) { say('この落としものの場所が分かりません'); return; }
    // 置き場所の中のものは、いつもの一本として開く ── 同じファイルが
    // 一覧と客の両方に居ると、どちらに書いたのか分からなくなる。
    if (at.startsWith(state.root + '/')) {
        if (state.guest) closeGuest();
        await reload({ quiet: true });
        const known = state.notes.find((n) => n.path === at);
        if (known) { await openNote(at); return; }
    }
    await openGuest(at);
});

/// 外から渡された一本を受け取る。**起動の途中でも来る**ので、
/// 立ち上がりきってから開く。
window.amber.onGuest((at) => {
    if (booted) openGuest(at);
    else pendingGuest = at;
});
let booted = false;
let pendingGuest = null;

async function cmdOpenOutside() {
    const at = await window.amber.pickFile(
        [{ name: 'ノート', extensions: ['md', 'markdown', 'txt'] }]);
    if (!at) return;
    await openGuest(at);
}

/* ── 命令 ── */

/// ノートの字を書き換える一本道。
///
/// **前書きを戻し、切り直し、いつもの保存を通す。** 星もタグも通知も
/// front matter の一行なので、同じ道を通れば衝突の検査も一度で済む ──
/// 押して付けた星と、打って書いた星が、別の扱いになる理由は無い。
async function editNote(change) {
    if (!state.open || !editor) return false;
    let text;
    try {
        text = await change(whole());
    } catch (e) {
        say('直せません: ' + e.message);
        return false;
    }
    if (text == null) return false;
    const cut = await ask('split', { text });
    loading = true;
    editor.setValue(cut.body || '');
    loading = false;
    state.head = cut.head || '';
    state.dirty = true;
    await save();
    await drawRead();
    return true;
}

/// いま開いているノートの、拡張子を外した名前。
function stem() {
    const f = (state.open?.path || 'note').split('/').pop();
    return f.replace(/\.[^.]*$/, '');
}

/// ブックマークに登録する。
///
/// **どこに置くかを、その場で選ぶ。** 前は「入れる／外す」と「置き場所を
/// 選ぶ」が別の命令になっていて、入れたあとにもう一度探して選ぶ形だった
/// ── フォルダへ移動と同じ一手で済む話。置き場所が無ければ、その場で作る。
async function cmdStar() {
    if (starred(state.open)) {
        const now = state.open.star;
        const off = await askPick('このノートはブックマーク済みです',
            [{ name: 'ブックマークから外す', value: 'off' },
             { name: '置き場所を変える', value: 'move' }],
            now && now !== 'true' ? 'いま: ' + now : 'いま: ブックマークの直下');
        if (off === null) return;
        if (off === 'off') {
            if (await editNote((t) => ask('star', { text: t }).then((r) => r.text))) {
                say('ブックマークから外しました');
            }
            return;
        }
    }
    const where = [{ name: '（ブックマークの直下）', value: '' },
        ...state.stars.map((x) => ({ name: x, value: x })),
        { name: '＋ 新しい置き場所を作る', value: ' new' }];
    let to = await askPick('どこに登録しますか', where);
    if (to === null) return;
    if (to === ' new') {
        const made = await newShelf();
        if (made === null) return;
        to = made;
    }
    if (await editNote((t) => ask('star', { text: t, shelf: to }).then((r) => r.text))) {
        say(to ? '「' + to + '」に登録しました' : 'ブックマークに登録しました');
    }
}

/// ブックマークの置き場所を一つ作る。`under` があれば、その下に。
///
/// **「/」を打たせない。** 階層は親を右押しして作る ── 一行に全部書かせる
/// のは、書き方を知っている人にしか通じない。
async function newShelf(under) {
    const name = await askText(under ? '「' + under + '」の下に作る名前' : '新しい置き場所の名前',
        '', under ? '' : '下の階層は、置き場所を右押しして作れます');
    if (name === null || !name.trim()) return null;
    const leaf = name.trim().replace(/\//g, '／');
    const full = under ? under + '/' + leaf : leaf;
    try {
        await ask('shelf', { path: state.root, name: full });
        await reload({ quiet: true });
        say('「' + full + '」を作りました');
        return full;
    } catch (e) {
        say('作れません: ' + e.message);
        return null;
    }
}

async function cmdTags() {
    let now = [...(state.open.tags || [])];
    for (;;) {
        const all = [...new Set([...tagsOf(state.notes).map(([t]) => t), ...now])]
            .sort((a, b) => a.localeCompare(b, 'ja'));
        const items = [
            ...all.map((t) => ({
                name: (now.includes(t) ? '☑  ' : '☐  ') + t,
                sub: now.includes(t) ? '' : (tagsOf(state.notes).find(([x]) => x === t)?.[1] || 0) + ' 件',
                value: t,
            })),
            { name: '＋ 新しいタグを作る', value: ' new' },
            { name: '── これで決まり', key: 'Enter', value: ' done' },
        ];
        const pick = await askPick('タグ（押すと付け外し）',
            items, 'いま: ' + (now.length ? '#' + now.join(' #') : 'なし'));
        // **やめたら、何も変えない。** 途中まで触っていても書き戻さない。
        if (pick === null) return;
        if (pick === ' done') break;
        if (pick === ' new') {
            const v = await askText('新しいタグの名前', '', '空白は使えません（`買い物` のように）');
            if (v && v.trim()) {
                const t = v.trim().replace(/^#/, '').replace(/\s+/g, '');
                if (t && !now.includes(t)) now.push(t);
            }
            continue;
        }
        now = now.includes(pick) ? now.filter((t) => t !== pick) : [...now, pick];
    }
    const tags = now;
    if (await editNote((t) => ask('settags', { text: t, tags }).then((r) => r.text))) {
        say(tags.length ? '#' + tags.join(' #') : 'タグを外しました');
    }
}

/* ── フィルタ ── */

/// タグ・フォルダ・期間で絞る。**打たせない。**
///
/// 言葉で探すのは上の「ノートを探す」の欄がやる ── ここに同じものを置くと、
/// 同じことを二か所で頼めることになり、どちらが効いているか分からなくなる。
/// タグもフォルダも**そのものを指す**（名前を打たせない）── 打たせると
/// 打ち間違いが「見つかりません」として返ってきて、間違いに見えない。
async function cmdFind() {
    const how = await askPick('フィルタ', [
        { name: 'タグ', sub: 'いくつでも選べます', value: 'tag' },
        { name: 'フォルダ', value: 'book' },
        { name: '期間', sub: '作った日・直した日で', value: 'when' },
        { name: 'フィルタをやめる', value: 'clear' },
    ], '言葉で探すのは、上の「ノートを探す」で');
    if (how === null) return;
    if (how === 'clear') {
        setFind('');
        state.when = null;
        drawFind();
        drawList();
        return;
    }
    if (how === 'tag') { await findTags(); return; }
    if (how === 'when') { await findWhen(); return; }
    const b = await askPick('どのフォルダ', state.books.map((x) => ({
        name: x,
        sub: state.notes.filter((n) => n.book === x || (n.book || '').startsWith(x + '/')).length + ' 件',
        value: x,
    })), state.books.length ? '' : 'フォルダがまだありません（左の「フォルダ ＋」から作れます）');
    if (b === null) return;
    addFind('book:' + b);
}

/// タグは**いくつでも**選べる。
///
/// 一つずつ小窓を開き直すのは、選んでいる途中が見えるから ── 「いま何を
/// 選んでいるか」を出さずに複数選ばせると、押した数を数えることになる。
async function findTags() {
    const picked = [];
    for (;;) {
        const all = tagsOf(state.notes);
        if (!all.length) { say('タグがまだありません'); return; }
        const items = [
            ...all.map(([t, n]) => ({
                name: (picked.includes(t) ? '☑  ' : '☐  ') + t,
                sub: n + ' 件', value: t,
            })),
            { name: '── これで絞る', key: 'Enter', value: ' done' },
        ];
        const pick = await askPick('タグ（押すと選び、いくつでも）', items,
            picked.length ? 'いま: #' + picked.join(' #') + '（全部付いたものだけ）' : '');
        if (pick === null) return;
        if (pick === ' done') break;
        const at = picked.indexOf(pick);
        if (at < 0) picked.push(pick);
        else picked.splice(at, 1);
    }
    if (!picked.length) return;
    addFind(picked.map((t) => 'tag:' + t).join(' '));
}

/// 期間で絞る。**日付は打たせない** ── 「先週」を `2026-08-30..` に直すのは
/// 人の仕事ではない。
async function findWhen() {
    const which = await askPick('どちらの日付で', [
        { name: '直した日', sub: 'いつ書き換えたか', value: 'updated' },
        { name: '作った日', sub: 'いつ作ったか', value: 'created' },
    ]);
    if (which === null) return;
    const span = await askPick('いつからのものを', [
        { name: '今日', value: 1 },
        { name: '3日以内', value: 3 },
        { name: '1週間以内', value: 7 },
        { name: '1か月以内', value: 30 },
        { name: '3か月以内', value: 90 },
        { name: '1年以内', value: 365 },
        { name: 'それより前のもの', value: -1 },
    ]);
    if (span === null) return;
    state.when = { which, days: span };
    drawFind();
    drawList();
}

/// 期間の札を出し直す。
function drawFind() {
    const box = el('whenchip');
    if (!state.when) { box.hidden = true; box.textContent = ''; return; }
    const name = state.when.which === 'created' ? '作った日' : '直した日';
    const days = state.when.days;
    const how = days < 0 ? '1年より前' : (days === 1 ? '今日' : days + '日以内');
    box.hidden = false;
    box.textContent = name + ': ' + how + '  ✕';
}

function setFind(v) {
    el('find').value = v;
    el('find').dispatchEvent(new Event('input'));
}

function addFind(add) {
    const box = el('find');
    setFind((box.value.trim() ? box.value.trim() + ' ' : '') + add);
    box.focus();
}

async function cmdMove() {
    const here = [{ name: '（いちばん上）', value: '' },
        ...state.books.map((b) => ({ name: b, value: b })),
        { name: '＋ 新しいフォルダを作る', value: ' new' }];
    let to = await askPick('どのフォルダへ', here);
    if (to === null) return;
    if (to === ' new') {
        const made = await cmdMkBook();
        if (!made) return;
        to = made;
    }
    const dir = to ? state.root + '/' + to : state.root;
    try {
        // 書きかけを置いていかない ── 移した先に古い字が残る。
        if (state.dirty) await save();
        const r = await ask('move', { path: state.open.path, dir });
        await reload({ quiet: true });
        await openNote(r.path);
        say(to ? '「' + to + '」へ移しました' : 'いちばん上へ移しました');
    } catch (e) {
        say('移せません: ' + e.message);
    }
}

/// フォルダを一つ作る。`under` があれば、その下に。
///
/// **「/」を打たせない。** 「仕事/2026」と書けば階層になる、は書き方を
/// 知っている人にしか通じない ── 下の階層は、親を右押しして作る。
async function cmdMkBook(under) {
    const name = await askText(under ? '「' + under + '」の下に作る名前' : '新しいフォルダの名前',
        '', under ? '' : '下の階層は、フォルダを右押しして作れます');
    if (name === null || !name.trim()) return null;
    const leaf = name.trim().replace(/\//g, '／');
    const full = under ? under + '/' + leaf : leaf;
    try {
        await ask('mkbook', { dir: state.root + '/' + full });
        await reload({ quiet: true });
        say('「' + full + '」を作りました');
        return full;
    } catch (e) {
        say('作れません: ' + e.message);
        return null;
    }
}

async function cmdDelete() {
    if (!await askYes('「' + (state.open.title || stem()) + '」をゴミ箱へ入れますか')) return;
    // **消さずに、ゴミ箱へ。** core の `delete` は消してしまう（電話には
    // ゴミ箱が無いので）。机の上では、戻せないのは強すぎる。
    const path = state.open.path;
    if (!await window.amber.trash(path)) {
        say('ゴミ箱へ入れられません');
        return;
    }
    state.open = null;
    state.dirty = false;
    applyView();
    await reload({ quiet: true });
    say('ゴミ箱へ入れました');
}

/// 通知。**仕掛けるのは窓でもできる。鳴らすのは電話。**
///
/// 窓は閉じている時間のほうが長く、閉じている間の時刻は誰も見ていない ──
/// ここで鳴るのは「開いているうちに来た分」だけ。同じフォルダを見ている
/// iPhone は、閉じていても鳴らす。
async function cmdRemind() {
    const kind = await askPick('いつ知らせるか', [
        { name: '日付と時刻を決める', value: 'once' },
        { name: '毎日', sub: '例: 09:00', value: 'daily' },
        { name: '毎週', sub: '例: 月 09:00', value: 'weekly' },
        { name: '毎月', sub: '例: 1 09:00', value: 'monthly' },
        { name: '（やめる）', value: 'off' },
    ], '仕掛けるのは窓、鳴らすのは iPhone（窓は開いている間だけ鳴ります）');
    if (kind === null) return;
    if (kind === 'off') {
        if (await editNote(async (t) => {
            const a = (await ask('setfield', { text: t, key: 'remind' })).text;
            return (await ask('setfield', { text: a, key: 'repeat' })).text;
        })) say('通知をやめました');
        return;
    }
    if (kind === 'once') {
        const v = await askText('いつ', ymdNow(), '2026-09-10 09:00 の形で');
        if (v === null || !v.trim()) return;
        if (await editNote((t) => ask('setfield', { text: t, key: 'remind', value: v.trim() })
            .then((r) => r.text))) say('通知を仕掛けました: ' + v.trim());
        return;
    }
    const hint = { daily: '09:00', weekly: '月 09:00', monthly: '1 09:00' }[kind];
    const v = await askText('繰り返し（' + kind + '）', hint,
        '毎日は 09:00、毎週は 月 09:00、毎月は 1 09:00');
    if (v === null || !v.trim()) return;
    if (await editNote((t) => ask('setfield', { text: t, key: 'repeat', value: kind + ' ' + v.trim() })
        .then((r) => r.text))) say('繰り返しを仕掛けました');
}

function ymdNow() {
    const d = new Date();
    const p = (n) => String(n).padStart(2, '0');
    return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) + ' 09:00';
}

async function cmdExport() {
    const how = await askPick('どの形で書き出すか', [
        { name: 'Markdown', sub: 'ノートそのまま（前書きも含む）', value: 'md' },
        { name: 'HTML', sub: '読める形、一枚で完結', value: 'html' },
        { name: 'PDF', sub: '読める形を刷る', value: 'pdf' },
    ]);
    if (how === null) return;
    const name = stem();
    try {
        if (how === 'md') {
            const at = await window.amber.saveText(name + '.md', whole());
            if (at) say('書き出しました: ' + at);
            return;
        }
        const body = (await ask('html', { text: whole() })).html || '';
        const page = onePage(state.open.title || name, body);
        const at = how === 'html'
            ? await window.amber.saveText(name + '.html', page)
            : await window.amber.savePDF(name + '.pdf', page);
        if (at) say('書き出しました: ' + at);
    } catch (e) {
        say('書き出せません: ' + e.message);
    }
}

/// 一枚で完結する HTML。
///
/// **外を参照しない。** 別の機械で開いても字の形が崩れないように、字体は
/// その機械にあるものだけ。絵はノートの隣から拾っているので、そこだけは
/// 付いてこない（それは書き出しではなく、束ねる話）。
function onePage(title, body) {
    return '<!doctype html><html lang="ja"><head><meta charset="utf-8">'
        + '<title>' + escapeHtml(title) + '</title><style>'
        + 'body{max-width:44rem;margin:3rem auto;padding:0 1.4rem;'
        + 'font:16px/1.9 -apple-system,BlinkMacSystemFont,"Hiragino Sans","Yu Gothic UI",sans-serif;'
        + 'color:#2a2011;background:#fffdf8}'
        + 'h1,h2,h3,h4{line-height:1.4;margin:1.6em 0 .5em}'
        + 'h2{padding-bottom:.2em;border-bottom:1px solid #efe6d4}'
        + 'code{font:.88em/1.6 ui-monospace,Menlo,monospace;background:#f3ecdf;'
        + 'border:1px solid #efe6d4;border-radius:5px;padding:.1em .35em}'
        + 'pre{padding:11px 14px;overflow-x:auto;background:#f3ecdf;'
        + 'border:1px solid #efe6d4;border-radius:9px}'
        + 'pre code{background:none;border:0;padding:0}'
        + 'blockquote{margin:.85em 0;padding:.1em 0 .1em 1em;border-left:3px solid #e4d9c4;color:#6b5a41}'
        + 'table{border-collapse:collapse}th,td{border:1px solid #e4d9c4;padding:5px 11px}'
        + 'th{background:#f3ecdf}img{max-width:100%;height:auto;border-radius:8px}'
        + 'hr{border:0;border-top:1px solid #e4d9c4;margin:1.6em 0}'
        + 'a{color:#b5760f}li.task{list-style:none;margin-left:-1.35em}'
        + '.box{display:inline-block;width:1.35em;background:none;border:0;'
        + 'color:#b5760f;font-size:1.05em}'
        + '</style></head><body>' + body + '</body></html>';
}

/// フォルダと文字色の11色。**iPhone の `Colouring.palette` と同じ並び。**
/// 色でしか区別できないノートは grep に映らず、読み上げにも伝わらないので、
/// 増やさない。
const PALETTE = [
    ['#0E93A8', 'シアン'], ['#2AA79B', 'みどり青'], ['#3D7FA8', '青'],
    ['#6E7BC4', '藤'], ['#9A6FB5', '紫'], ['#C2649A', '桃'],
    ['#C4564E', '赤'], ['#D07A2E', '橙'], ['#B08A2E', '金'],
    ['#5E8C42', '緑'], ['#7A7A7A', '灰'],
];

/// フォルダに色を付ける。**フォルダを右押ししたときだけ。**
///
/// 歯車の中に置いていた頃は「どのフォルダの話か」を画面が言っておらず、
/// いま選んでいるものが相手だと知っている人にしか使えなかった。
async function cmdColor(folder) {
    const what = folder || (state.dest.kind === 'book' ? state.dest.what : '');
    if (!what) {
        say('色を付けるフォルダを右押ししてください');
        return;
    }
    const hex = await askPick('「' + what + '」の色', [
        { name: '（色を外す）', value: '' },
        ...PALETTE.map(([h, n]) => ({ name: n, sub: h, value: h })),
    ]);
    if (hex === null) return;
    try {
        const r = await ask('color', { path: state.root, folder: what, color: hex || null });
        state.colors = r.colors || {};
        drawRail();
    } catch (e) {
        say('色を付けられません: ' + e.message);
    }
}

async function cmdBackup() {
    const into = await window.amber.pickFolder();
    if (!into) return;
    try {
        const r = await ask('backup', { path: state.root, scope: 'all', what: '', into });
        say('保存しました: ' + (r.zip || into));
    } catch (e) {
        say('保存できません: ' + e.message);
    }
}

async function cmdRoot() {
    // **いまどこかを先に見せる。** 「amber のディレクトリはどうやって
    // 決めるのか」が分からなかったのは、決める場所が無かったからではなく、
    // **いまどこを見ているのかが画面のどこにも出ていなかった**から。
    const go = await askPick('ノートの置き場所', [
        { name: '別の場所を選ぶ', sub: 'フォルダを一つ選びます', value: 'pick' },
    ], 'いま: ' + state.root);
    if (go === null) return;
    const dir = await window.amber.pickFolder();
    if (!dir) return;
    state.root = dir;
    window.amber.remember({ root: dir });
    state.open = null;
    applyView();
    await reload({});
    say('置き場所を変えました: ' + dir);
}

/* ── 小窓 ── */

/// 命令を選ぶ・字を打つ・一つ選ぶ、を一枚で賄う。
///
/// **三つ作らない。** 別々に書くと、微妙に違う「Esc で閉じる」が三つできて、
/// そのうち一つだけ閉じない日が来る。ここが唯一の閉じ方。
///
/// `items` があれば選ぶ小窓、無ければ字を打つ小窓。返すのは選んだ値
/// （または打った字）で、やめたときは `null`。
let sheetDone = null;

function sheet({ title, value, placeholder, items, foot }) {
    closeSheet(null);
    const veil = el('veil');
    const input = veil.querySelector('input');
    const list = veil.querySelector('.items');
    veil.querySelector('.hd').textContent = title || '';
    veil.querySelector('.foot').textContent = foot || '';
    veil.querySelector('.foot').hidden = !foot;
    input.value = value || '';
    input.placeholder = placeholder || '';
    veil.hidden = false;
    input.focus();
    input.select();

    let at = 0;
    const draw = () => {
        if (!items) { list.innerHTML = ''; return; }
        const q = input.value.trim().toLowerCase();
        // 打った字を、名前のどこかに含むもの。**部分一致** ── 覚えている
        // のはたいてい真ん中の一語で、頭ではない。
        const hit = items.filter((i) => !q || (i.name + ' ' + (i.sub || '')).toLowerCase().includes(q));
        at = Math.min(at, Math.max(hit.length - 1, 0));
        list.innerHTML = hit.map((i, n) =>
            '<div class="it' + (n === at ? ' on' : '') + '" data-n="' + n + '">'
            + '<span>' + escapeHtml(i.name) + '</span>'
            + (i.sub ? '<span class="sub">' + escapeHtml(i.sub) + '</span>' : '')
            + (i.key ? '<span class="k">' + escapeHtml(i.key) + '</span>' : '')
            + '</div>').join('');
        for (const row of list.querySelectorAll('.it')) {
            row.onclick = () => closeSheet(hit[Number(row.dataset.n)].value);
        }
        list.querySelector('.it.on')?.scrollIntoView({ block: 'nearest' });
        return hit;
    };
    draw();

    input.oninput = () => { at = 0; draw(); };
    input.onkeydown = (e) => {
        e.stopPropagation();
        // **変換中の Enter は、変換の確定であって返事ではない。**
        //
        // 「さかな」→「魚」を Enter で確定した瞬間に小窓まで閉じ、
        // 「魚の目」と打つつもりが「魚」というフォルダが出来る。
        // `isComposing` は変換中に真になり、それを見ない環境でも
        // `keyCode 229` が同じことを言う（古い実装が残っている）。
        if (e.isComposing || e.keyCode === 229) return;
        if (e.code === 'Escape') { e.preventDefault(); closeSheet(null); return; }
        if (!items) {
            if (e.code === 'Enter') { e.preventDefault(); closeSheet(input.value); }
            return;
        }
        const hit = items.filter((i) => {
            const q = input.value.trim().toLowerCase();
            return !q || (i.name + ' ' + (i.sub || '')).toLowerCase().includes(q);
        });
        if (e.code === 'ArrowDown') { e.preventDefault(); at = Math.min(at + 1, hit.length - 1); draw(); }
        else if (e.code === 'ArrowUp') { e.preventDefault(); at = Math.max(at - 1, 0); draw(); }
        else if (e.code === 'Enter') {
            e.preventDefault();
            if (hit[at]) closeSheet(hit[at].value);
        }
    };
    // 幕を押したらやめる。**中は押しても閉じない。**
    veil.onmousedown = (e) => { if (e.target === veil) closeSheet(null); };
    return new Promise((resolve) => { sheetDone = resolve; });
}

function closeSheet(v) {
    const veil = el('veil');
    if (!veil.hidden) {
        veil.hidden = true;
        if (editor && !veil.contains(document.activeElement)) editor.focus();
    }
    if (sheetDone) { const f = sheetDone; sheetDone = null; f(v); }
}

/// 字を打つ小窓。
const askText = (title, value, foot) => sheet({ title, value, foot });
/// 一つ選ぶ小窓。`items` は `{ name, sub, key, value }`。
const askPick = (title, items, foot) =>
    sheet({ title, items, placeholder: '絞り込む', foot });

/// はい／いいえ。**取り返しのつかないものだけに使う。**
function askYes(title) {
    return sheet({ title, items: [
        { name: 'はい', value: true },
        { name: 'やめる', value: false },
    ] }).then((v) => v === true);
}

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
    if (saved.view === 'read' || saved.view === 'split' || saved.view === 'write') {
        view = saved.view;
    }
    moreMarks = !!saved.moreMarks;
    drawMarks();
    // **帯を先に整える。** ノートを一本も開かないまま終わる起動もある
    // （初めて立ち上げた日がそう）── そのとき ⚙ が出ていないと、
    // 置き場所を決める道がどこにも無い。
    applyView();
    if (saved.railOff) { railOff = true; document.body.classList.add('norail'); }
    // エディタはまだ無い ── 開いたときに入る（`makeEditor` の末尾）。
    if (saved.vim) vimOn = true;
    if (typeof saved.fontStep === 'number') fontStep = saved.fontStep;
    if (saved.order) order = saved.order;
    if (saved.tocOn) tocOn = true;
    if (saved.theme) setTheme(saved.theme);
    if (saved.lineNo) lineNo = true;
    drawOrder();
    booted = true;
    if (pendingGuest) { const at = pendingGuest; pendingGuest = null; openGuest(at); }

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
