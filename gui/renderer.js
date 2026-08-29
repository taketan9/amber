'use strict';
// The listing, drawn. No engine logic here — this asks and paints, and every
// answer it paints came from cian-core.

/// The two panes as the engine last described them.
const state = { left: null, right: null, focus: 'left' };

const el = {
    left: document.querySelector('[data-pane="left"]'),
    right: document.querySelector('[data-pane="right"]'),
    status: document.getElementById('status'),
    ask: document.getElementById('ask'),
    find: document.getElementById('find'),
    findQ: document.getElementById('find-q'),
    findHits: document.getElementById('find-hits'),
    findFoot: document.getElementById('find-foot'),
    view: document.getElementById('view'),
    vName: document.getElementById('v-name'),
    vAbout: document.getElementById('v-about'),
    vBody: document.getElementById('v-body'),
    vFoot: document.getElementById('v-foot'),
};

/// The operation currently running, if any, so its progress has somewhere to
/// land and Esc has something to call off.
let running = null;

/// Ask before doing. Resolves true only on a deliberate yes.
///
/// Nothing in cian reaches the disk without passing through here: the terminal
/// build's whole promise is that a slip costs nothing, and a front end that
/// quietly skipped the asking would not be the same program.
function confirm(head, body) {
    el.ask.querySelector('.head').textContent = head;
    el.ask.querySelector('.body').textContent = body;
    el.ask.hidden = false;
    // The focus goes where Enter goes.
    //
    // It used to sit on the safe button, meaning to make leaning on the
    // keyboard harmless — but Enter answers yes here whatever has the focus,
    // so it was not protecting anything. All it did was put a ring around
    // やめる while the key labelled (Enter) did 実行, which reads as the
    // opposite of what happens. Being asked at all is the protection.
    el.ask.querySelector('[data-answer="yes"]').focus();
    return new Promise((resolve) => {
        const done = (answer) => {
            el.ask.hidden = true;
            el.ask.removeEventListener('click', onClick);
            document.removeEventListener('keydown', onKey, true);
            resolve(answer);
        };
        const onClick = (e) => {
            const a = e.target.dataset && e.target.dataset.answer;
            if (a) done(a === 'yes');
        };
        const onKey = (e) => {
            if (e.key === 'Escape') { e.stopPropagation(); done(false); }
            else if (e.key === 'Enter') { e.stopPropagation(); done(true); }
            else if (e.key !== 'Tab') { e.stopPropagation(); }
        };
        el.ask.addEventListener('click', onClick);
        // Captured, so the listing's own keys never see these.
        document.addEventListener('keydown', onKey, true);
    });
}

function say(text, bad = false) {
    el.status.textContent = text;
    el.status.classList.toggle('bad', bad);
}

/// Bytes, in the width a listing can spare. Directories show nothing rather
/// than `0`, which is a number that means "we did not look".
function size(row) {
    if (row.is_dir || row.parent) return '';
    const u = ['B', 'K', 'M', 'G', 'T'];
    let n = row.len, i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return (i === 0 ? n : n.toFixed(n < 10 ? 1 : 0)) + u[i];
}

function draw(which) {
    const pane = state[which];
    const root = el[which];
    root.classList.toggle('active', state.focus === which);
    if (!pane) return;
    root.querySelector('.crumb').textContent = pane.cwd;

    const rows = root.querySelector('.rows');
    // Rebuilt whole. A listing is a few hundred rows and Chromium does not
    // notice; the moment it does, this is where a windowed list goes.
    const frag = document.createDocumentFragment();
    pane.entries.forEach((row, i) => {
        const div = document.createElement('div');
        div.className = 'row'
            + (row.is_dir ? ' dir' : '')
            + (row.marked ? ' marked' : '')
            + (i === pane.cursor ? ' cursor' : '');
        const name = document.createElement('span');
        name.className = 'name';
        name.textContent = row.parent ? '..' : row.name;
        const len = document.createElement('span');
        len.className = 'size';
        len.textContent = size(row);
        div.append(name, len);
        div.addEventListener('mousedown', () => {
            state.focus = which;
            pane.cursor = i;
            draw('left'); draw('right');
        });
        div.addEventListener('dblclick', () => { state.focus = which; enter(); });
        frag.append(div);
    });
    rows.replaceChildren(frag);

    // Keep the cursor on screen without yanking the view about.
    const at = rows.children[pane.cursor];
    if (at) at.scrollIntoView({ block: 'nearest' });
}

async function ask(method, params) {
    try {
        // Every request that names a pane states where the cursor is. The
        // cursor moves here, on every `j`, without asking the engine — so the
        // engine's own copy went stale, and `r` after three presses of `j`
        // renamed a file three rows up. Stated once, here, rather than
        // remembered at each of the dozen call sites.
        const pane = params && params.pane && state[params.pane];
        if (pane && params.cursor === undefined) params = { ...params, cursor: pane.cursor };
        return await window.cian.call(method, params);
    } catch (e) {
        say(String(e.message || e), true);
        return null;
    }
}

async function refresh() {
    const s = await ask('state', {});
    if (!s) return;
    state.left = s.left;
    state.right = s.right;
    draw('left'); draw('right');
    say(`${state.left.entries.length} 件 / ${state.right.entries.length} 件`);
}

/// Mark the row under the cursor, or every row.
async function mark(all) {
    const which = state.focus;
    const next = await ask(all ? 'markall' : 'mark', { pane: which });
    if (!next) return;
    state[which] = next;
    draw(which);
    say(next.marked ? `${next.marked} 件マーク` : 'マークなし');
}

/// Copy, move or delete whatever is marked — or the row under the cursor when
/// nothing is. The destination is the other pane, which is the whole idea of
/// two panes side by side.
async function operate(kind) {
    if (running) {
        say('実行中です。Esc で中止できます');
        return;
    }
    const which = state.focus;
    const pane = state[which];
    if (!pane) return;

    // What is about to happen, named, before anything happens. The count comes
    // from the same rule the engine will use, so the sheet cannot promise one
    // thing and the engine do another.
    const chosen = pane.entries.filter((r) => !r.parent && r.marked);
    const here = pane.entries[pane.cursor];
    const rows = chosen.length ? chosen : (here && !here.parent ? [here] : []);
    if (!rows.length) {
        say('対象がありません');
        return;
    }
    const dest = state[which === 'left' ? 'right' : 'left'];
    const verb = { copy: 'コピー', move: '移動', delete: '削除' }[kind];
    const head = kind === 'delete'
        ? `${rows.length} 件をゴミ箱へ`
        : `${rows.length} 件を${verb}: → ${dest.cwd}`;
    // Every name, not a summary. "12 件" tells you nothing about whether the
    // twelve are the ones you meant.
    const body = rows.map((r) => r.name).join('\n');
    if (!await confirm(head, body)) {
        say('やめました');
        return;
    }

    const started = await ask(kind, { pane: which });
    if (!started) return;
    running = { op: started.op, kind, verb, total: started.count };
    say(`${verb}中… 0 / ${started.count}`);
}

function move(delta) {
    const pane = state[state.focus];
    if (!pane || !pane.entries.length) return;
    const last = pane.entries.length - 1;
    pane.cursor = Math.min(last, Math.max(0, pane.cursor + delta));
    draw(state.focus);
}

async function enter() {
    const which = state.focus;
    const pane = state[which];
    if (!pane) return;
    const row = pane.entries[pane.cursor];
    if (!row) return;
    // A file is read here rather than handed to the desktop — the same
    // division the terminal build makes. Ctrl+Enter is the other one.
    if (!row.is_dir && !row.parent) {
        await lookInside();
        return;
    }
    const next = await ask('enter', { pane: which, cursor: pane.cursor });
    if (!next) return;
    state[which] = next;
    draw(which);
    say(next.cwd);
}

async function parent() {
    const which = state.focus;
    const next = await ask('parent', { pane: which });
    if (!next) return;
    state[which] = next;
    draw(which);
    say(next.cwd);
}

/// Ask for a line of text. Resolves to null when the answer is no answer.
///
/// The same sheet as the confirm, with a field in it: one dialog to know
/// rather than two, and the keys mean the same thing in both.
function askFor(head, initial = '') {
    const sheet = el.ask.querySelector('.sheet');
    el.ask.querySelector('.head').textContent = head;
    const body = el.ask.querySelector('.body');
    body.textContent = '';
    const input = document.createElement('input');
    input.type = 'text';
    input.value = initial;
    input.className = 'field';
    body.append(input);
    el.ask.hidden = false;
    input.focus();
    // The stem, not the suffix: renaming is nearly always about the name and
    // almost never about the `.txt`.
    const dot = initial.lastIndexOf('.');
    if (dot > 0) input.setSelectionRange(0, dot);
    else input.select();

    return new Promise((resolve) => {
        const done = (value) => {
            el.ask.hidden = true;
            body.textContent = '';
            el.ask.removeEventListener('click', onClick);
            document.removeEventListener('keydown', onKey, true);
            resolve(value);
        };
        const onClick = (e) => {
            const a = e.target.dataset && e.target.dataset.answer;
            if (a) done(a === 'yes' ? input.value : null);
        };
        const onKey = (e) => {
            if (e.key === 'Escape') { e.stopPropagation(); done(null); }
            else if (e.key === 'Enter') { e.stopPropagation(); done(input.value); }
            else e.stopPropagation();
        };
        el.ask.addEventListener('click', onClick);
        document.addEventListener('keydown', onKey, true);
    });
}

/// Rename what the cursor is on.
async function rename() {
    const which = state.focus;
    const pane = state[which];
    const row = pane && pane.entries[pane.cursor];
    if (!row || row.parent) {
        say('対象がありません');
        return;
    }
    const name = await askFor(`${row.name} の新しい名前`, row.name);
    if (name === null || name === row.name) {
        say('やめました');
        return;
    }
    const next = await ask('rename', { pane: which, name });
    if (!next) return;
    state[which] = next;
    draw(which);
    say(`${row.name} → ${name}`);
}

/// A new file, or a new directory.
async function create(dir) {
    const which = state.focus;
    const name = await askFor(dir ? '新しいディレクトリの名前' : '新しいファイルの名前');
    if (name === null || !name.trim()) {
        say('やめました');
        return;
    }
    const next = await ask('create', { pane: which, name, dir });
    if (!next) return;
    state[which] = next;
    draw(which);
    say(`${name} を作りました`);
}

/// One step back, whatever it was.
async function undo() {
    const r = await ask('undo', {});
    if (!r) return;
    state.left = r.left;
    state.right = r.right;
    draw('left'); draw('right');
    say(r.said);
}

/// Show or hide the dotfiles.
async function toggleHidden() {
    const which = state.focus;
    const r = await ask('hidden', { pane: which });
    if (!r) return;
    state[which] = r.pane;
    draw(which);
    if (menu.spec === TOGGLES) drawMenu();
    say(r.showing ? '隠しファイルを表示' : '隠しファイルを非表示');
}

/// `,` shows the four keys and lets you choose — it does not walk them.
///
/// It had walked them, which is a different thing: the terminal build opens a
/// picker on whichever key is in force, with n/s/d/e as direct picks, and
/// choosing the key already in force flips the direction. Walking meant `,`
/// took two presses to leave `name`, because the first one only reversed it.
const SORTS = [['name', '名前', 'n'], ['size', 'サイズ', 's'],
               ['date', '日付', 'd'], ['ext', '拡張子', 'e']];
let sortKey = 'name';

async function applySort(key) {
    const which = state.focus;
    const r = await ask('sort', { pane: which, key });
    if (!r) return;
    sortKey = r.by;
    state[which] = r.pane;
    draw(which);
    say(`並び: ${r.by}${r.reverse ? ' ↓' : ' ↑'}`);
}

/// `/` narrows what is here. A second `/`, with nothing typed yet, looks
/// underneath instead — one slash for this listing, two for the tree. The
/// terminal build settled on that and it reads itself.
const filter = { on: false };

function startFilter() {
    filter.on = true;
    el.find.hidden = false;
    el.findQ.value = '';
    el.findQ.placeholder = 'この一覧を絞り込み（もう一度 / で下を探す）';
    el.findHits.replaceChildren();
    el.findFoot.textContent = '';
    el.findQ.focus();
}

function endFilter(keep) {
    filter.on = false;
    el.find.hidden = true;
    el.findQ.placeholder = '/ で絞り込み';
    if (!keep) applyFilter('');
}

async function applyFilter(text) {
    const which = state.focus;
    const next = await ask('filter', { pane: which, text });
    if (!next) return;
    state[which] = next;
    draw(which);
    say(text ? `絞り込み: ${text} — ${next.entries.length} 件` : `${next.entries.length} 件`);
}

/// The file finder: `//` opens it, typing narrows it, Enter goes there.
///
/// Ranking is the engine's — one fuzzy matcher rather than two that would
/// drift — and the round trip costs less than the ranking, because the engine
/// is a pipe away and not a network away.
const finder = { open: false, rows: [], at: 0, walking: false };

async function openFinder() {
    const which = state.focus;
    finder.open = true;
    finder.rows = [];
    finder.at = 0;
    finder.walking = true;
    el.find.hidden = false;
    el.findQ.value = '';
    el.findFoot.textContent = '探しています…';
    el.findHits.replaceChildren();
    el.findQ.focus();
    // Asked for before the walk has found anything, on purpose: the picker is
    // usable from the first keystroke and the tree arrives underneath it.
    await ask('find', { pane: which });
    rankNow();
}

function closeFinder() {
    finder.open = false;
    el.find.hidden = true;
}

async function rankNow() {
    if (!finder.open) return;
    const r = await ask('rank', { query: el.findQ.value, limit: 200 });
    if (!r || !finder.open) return;
    finder.rows = r.rows;
    finder.at = Math.min(finder.at, Math.max(0, r.rows.length - 1));
    drawHits(r.of);
}

function drawHits(of) {
    const frag = document.createDocumentFragment();
    finder.rows.forEach((row, i) => {
        const div = document.createElement('div');
        div.className = 'hit' + (row.is_dir ? ' d' : '') + (i === finder.at ? ' on' : '');
        const p = document.createElement('span');
        p.className = 'p';
        p.textContent = row.rel;
        div.append(p);
        div.addEventListener('mousedown', () => { finder.at = i; goToHit(); });
        frag.append(div);
    });
    el.findHits.replaceChildren(frag);
    const on = el.findHits.children[finder.at];
    if (on) on.scrollIntoView({ block: 'nearest' });
    el.findFoot.textContent = finder.walking
        ? `${finder.rows.length} / ${of} 件（まだ探しています）`
        : `${finder.rows.length} / ${of} 件`;
}

async function goToHit() {
    const row = finder.rows[finder.at];
    if (!row) return;
    const which = state.focus;
    closeFinder();
    const next = await ask('reveal', { pane: which, path: row.path });
    if (!next) return;
    state[which] = next;
    draw(which);
    say(row.rel);
}

/// The looks, in the order `T` walks them.
///
/// 白磁 leads because it is the default, and the default is chosen for the
/// person opening this for the first time rather than for the person who
/// built it — the same reasoning that made notepad the default grammar.
/// Taketan's own is solarized-light, one press away.
const LOOKS = [
    ['', '白磁'],
    ['solarized-light', 'Solarized Light'],
    ['inei', '陰翳'],
    ['terminal', '端末譲り'],
];

/// Which look is showing. Not yet written anywhere — where a preference
/// lives is still open, and guessing at a file now would mean two places to
/// read it from later.
let look = 0;

function setLook(i) {
    look = (i + LOOKS.length) % LOOKS.length;
    const [value] = LOOKS[look];
    if (value) document.documentElement.dataset.look = value;
    else delete document.documentElement.dataset.look;
}

/// The switches, on `T` — the key the terminal build puts them on.
///
/// Not a key each. cian-tui gathers the live settings into one menu rather
/// than spending a letter on every one of them, and a front end that scattered
/// them would be a second set of habits to learn.
const TOGGLES = {
    key: 'T',
    foot: '↑↓ 選ぶ  Enter 切替  Esc 閉じる',
    stay: true,
    rows: () => {
        const pane = state[state.focus];
        return [
            {
                label: '隠しファイル',
                value: pane && pane.hidden_shown ? '表示' : '非表示',
                run: () => toggleHidden(),
            },
            {
                label: '配色',
                value: LOOKS[look][1],
                run: () => { setLook(look + 1); drawMenu(); say(`配色: ${LOOKS[look][1]}`); },
            },
            {
                label: 'エディタの流儀',
                value: STYLES[style][1],
                run: () => { setStyle(style + 1); drawMenu(); say(`エディタ: ${STYLES[style][1]}`); },
            },
        ];
    },
};

const SORT_MENU = {
    key: ',',
    foot: '↑↓ 選ぶ  Enter 決定  n s d e で直接  Esc 閉じる',
    stay: false,
    at: () => SORTS.findIndex(([k]) => k === sortKey),
    rows: () => SORTS.map(([k, label, letter]) => ({
        label,
        value: k === sortKey ? '●' : letter,
        run: () => applySort(k),
    })),
    // The letters, so the picker is skippable once it is in the fingers —
    // the terminal build has the same four.
    letters: Object.fromEntries(SORTS.map(([k, , letter]) => [letter, () => applySort(k)])),
};

/// One menu driver, not one per menu.
///
/// The switches and the sort picker are the same object with different rows,
/// and a third near-copy of "draw a list, move a cursor, run the row" is how
/// they would start behaving differently from each other.
const menu = { spec: null, at: 0 };

function openMenu(spec) {
    menu.spec = spec;
    menu.at = Math.max(0, spec.at ? spec.at() : 0);
    el.find.hidden = false;
    el.findQ.hidden = true;
    el.findFoot.textContent = spec.foot;
    drawMenu();
}

function closeMenu() {
    menu.spec = null;
    el.find.hidden = true;
    el.findQ.hidden = false;
}

function drawMenu() {
    const rows = menu.spec.rows();
    const frag = document.createDocumentFragment();
    rows.forEach((row, i) => {
        const div = document.createElement('div');
        div.className = 'hit' + (i === menu.at ? ' on' : '');
        const l = document.createElement('span');
        l.className = 'p';
        l.textContent = row.label;
        const v = document.createElement('span');
        v.textContent = row.value;
        div.append(l, v);
        div.addEventListener('mousedown', () => {
            menu.at = i;
            row.run();
            if (!menu.spec.stay) closeMenu();
        });
        frag.append(div);
    });
    el.findHits.replaceChildren(frag);
}

document.addEventListener('keydown', (e) => {
    if (!menu.spec) return;
    e.stopPropagation();
    const spec = menu.spec;
    const rows = spec.rows();
    const pick = spec.letters && spec.letters[e.key];
    if (e.key === 'Escape' || e.key === spec.key) closeMenu();
    else if (e.key === 'ArrowDown' || e.key === 'j') { menu.at = (menu.at + 1) % rows.length; drawMenu(); }
    else if (e.key === 'ArrowUp' || e.key === 'k') { menu.at = (menu.at + rows.length - 1) % rows.length; drawMenu(); }
    else if (pick) { closeMenu(); pick(); }
    else if (e.key === 'Enter' || e.key === ' ') {
        rows[menu.at].run();
        if (!spec.stay) closeMenu();
    } else return;
    e.preventDefault();
}, true);

/// What `?` shows.
///
/// **Taken from cian's own key table, not written afresh.** Two keys in this
/// front end had drifted — sorting had wandered off `,`, and the look cycle had
/// taken `T`, which is the switches — and both were found by reading the
/// terminal build's list rather than by anyone noticing while using it. A help
/// screen written from memory would have recorded the drift as if it were the
/// design.
const HELP = [
    ['移動', [
        ['j / k / ↑ ↓', 'ひとつ下 / 上'],
        ['Enter', 'ディレクトリへ入る / ファイルを読む'],
        ['Ctrl+Enter', 'ディレクトリは反対ペインへ / ファイルは既定のアプリで'],
        ['Backspace', '親ディレクトリへ'],
        ['z', '入力したパスへ移動'],
        ['Tab', '反対のペインへ'],
        ['← → / Ctrl+h / Ctrl+l', '左 / 右のペインにフォーカス'],
        ['F5', '読み直す'],
    ]],
    ['探す', [
        ['/', 'この一覧を絞り込み'],
        ['/ /', 'この下のどこかにあるファイルをあいまい検索'],
        [',', 'ソート：名前／サイズ／日付／拡張子（n s d e で直接、同じキーで昇降反転）'],
        ['T', 'トグルメニュー：隠しファイル・配色・エディタの流儀'],
    ]],
    ['読み書き（F3・Enter）', [
        ['Ctrl+S', '保存（元の文字コード・改行・BOM のまま）'],
        ['Esc ×3', '閉じる ── 3回連続（未保存なら3回目で確認）'],
        ['Backspace ×3', '同じ。vim 流儀でノーマルモードのときだけ'],
        ['F3', '1回で閉じる'],
        ['流儀', 'メモ帳流（既定）／ vim ── 一覧に戻って T のメニューの中'],
        ['  vim のとき', 'ノーマルモードで開く。:w 保存 :q 閉じる :wq 両方'],
        ['  メモ帳流のとき', 'Ctrl+C/V/Z/F など Windows の手が効く'],
    ]],
    ['マークと操作', [
        ['Space', 'マーク切替して下へ'],
        ['Ctrl+A', '全マーク（もう一度で解除）'],
        ['V', '全マークを反転'],
        ['c / m / d', '反対ペインへコピー / 移動 / 削除（ゴミ箱へ）'],
        ['Ctrl+C / Ctrl+X', 'ファイルを保持（コピー / 切り取り）'],
        ['Ctrl+V / y', '保持したファイルをここへ貼り付け'],
        ['r', 'リネーム'],
        ['a / A', '新規ファイル / 新規ディレクトリ'],
        ['p', 'パス文字列をクリップボードへ'],
        ['o / O', 'このペインを反対側へ / 反対側をここへ'],
        ['u', '直前のリネーム／作成／移動／ディレクトリ移動を取り消し'],
        ['Esc', '実行中の操作を中止'],
    ]],
];

const help = { on: false };

function openHelp() {
    help.on = true;
    el.find.hidden = false;
    el.find.classList.add('help');
    el.findQ.hidden = true;
    el.findFoot.textContent = 'Esc か ? で閉じる  ── 端末版の cian と同じキーです';
    const frag = document.createDocumentFragment();
    for (const [group, rows] of HELP) {
        const h = document.createElement('div');
        h.className = 'group';
        h.textContent = group;
        frag.append(h);
        for (const [keys, what] of rows) {
            const div = document.createElement('div');
            div.className = 'hit';
            const l = document.createElement('span');
            l.className = 'k';
            l.textContent = keys;
            const v = document.createElement('span');
            v.className = 'w';
            v.textContent = what;
            div.append(l, v);
            frag.append(div);
        }
    }
    el.findHits.replaceChildren(frag);
    el.findHits.scrollTop = 0;
}

function closeHelp() {
    help.on = false;
    el.find.classList.remove('help');
    el.find.hidden = true;
    el.findQ.hidden = false;
}

/// Help's keys. It scrolls, because the terminal build's help did not and
/// the bottom of it could not be read.
document.addEventListener('keydown', (e) => {
    if (!help.on) return;
    e.stopPropagation();
    if (e.key === 'Escape' || e.key === '?') closeHelp();
    else if (e.key === 'ArrowDown' || e.key === 'j') el.findHits.scrollTop += 40;
    else if (e.key === 'ArrowUp' || e.key === 'k') el.findHits.scrollTop -= 40;
    else if (e.key === 'PageDown' || e.key === ' ') el.findHits.scrollTop += el.findHits.clientHeight - 40;
    else if (e.key === 'PageUp') el.findHits.scrollTop -= el.findHits.clientHeight - 40;
    else return;
    e.preventDefault();
}, true);

function focusPane(which) {
    state.focus = which;
    draw('left');
    draw('right');
}

async function invert() {
    const which = state.focus;
    const pane = await ask('invert', { pane: which });
    if (!pane) return;
    state[which] = pane;
    draw(which);
    say(pane.marked ? `${pane.marked} 件をマーク` : 'マークなし');
}

/// `o` brings this pane to the other one; `O` sends the other one here.
///
/// The pair is easy to get backwards, so the message names the direction
/// rather than saying "done" — the same reason `u` names what it undid.
async function syncPane(pullToHere) {
    const here = state.focus;
    const there = here === 'left' ? 'right' : 'left';
    const [to, from] = pullToHere ? [here, there] : [there, here];
    const path = state[from].cwd;
    const pane = await ask('list', { pane: to, path });
    if (!pane) return;
    state[to] = pane;
    draw(to);
    say(`${to === 'left' ? '左' : '右'}を ${path} へ`);
}

async function goToPath() {
    const path = await askFor('移動先', state[state.focus].cwd);
    if (!path) return;
    const which = state.focus;
    const pane = await ask('list', { pane: which, path });
    if (!pane) return;
    state[which] = pane;
    draw(which);
    say(pane.cwd);
}

/// `p` puts the paths on the clipboard — the marked ones, or the one under
/// the cursor. The text, not the files: copying the files is Ctrl+C, and
/// conflating the two is how you paste a path into a folder.
async function copyPaths() {
    const pane = state[state.focus];
    const marked = pane.entries.filter((x) => x.marked);
    const rows = marked.length ? marked : [pane.entries[pane.cursor]].filter(Boolean);
    if (!rows.length) return;
    const text = rows.map((x) => x.path).join('\n');
    await navigator.clipboard.writeText(text);
    say(`${rows.length} 件のパスをコピー`);
}

/// F5 goes back to the disk. `refresh()` above asks the engine what it
/// already holds, which is right at startup and wrong here — the point of
/// the key is that something changed underneath us.
/// Hold the selection for a later paste.
///
/// The pair to `c`/`m`, which go straight to the other pane. This one is for
/// when the destination is not on screen yet: hold here, walk there, paste.
/// The Windows letters, because that is what the hands do.
async function hold(op) {
    const r = await ask('clip', { pane: state.focus, op });
    if (!r) return;
    say(`${r.held} 件を${r.op === 'cut' ? '切り取り' : 'コピー'}`);
}

async function paste() {
    const r = await ask('paste', { pane: state.focus });
    if (!r) return;
    // The engine decides whether this is a copy or a move — it is holding the
    // register — so the verb comes back with the job rather than being
    // guessed here from which key was pressed.
    const verb = r.kind === 'move' ? '移動' : 'コピー';
    running = { op: r.op, kind: r.kind, verb, total: r.count };
    say(`${verb}中… 0 / ${r.count}`);
}

async function reread() {
    for (const which of ['left', 'right']) {
        const pane = await ask('list', { pane: which, path: state[which].cwd });
        if (!pane) return;
        state[which] = pane;
        draw(which);
    }
    say('読み直しました');
}

/// The filter's keys, while it is up.
document.addEventListener('keydown', (e) => {
    if (!filter.on) return;
    e.stopPropagation();
    if (e.key === 'Escape') { endFilter(false); say('絞り込みを解除'); }
    else if (e.key === 'Enter') { endFilter(true); }
    else if (e.key === '/' && el.findQ.value === '') {
        // Two slashes: this listing was not it, so look underneath.
        endFilter(true);
        openFinder();
    }
    else return;
    e.preventDefault();
}, true);

document.addEventListener('input', (e) => {
    if (filter.on && e.target === el.findQ) applyFilter(el.findQ.value);
});

/// The finder's own keys, while it is up.
document.addEventListener('keydown', (e) => {
    if (!finder.open) return;
    e.stopPropagation();
    if (e.key === 'Escape') { closeFinder(); say('やめました'); }
    else if (e.key === 'Enter') goToHit();
    else if (e.key === 'ArrowDown' || (e.key === 'n' && e.ctrlKey)) {
        finder.at = Math.min(finder.rows.length - 1, finder.at + 1);
        drawHits(finder.rows.length);
    }
    else if (e.key === 'ArrowUp' || (e.key === 'p' && e.ctrlKey)) {
        finder.at = Math.max(0, finder.at - 1);
        drawHits(finder.rows.length);
    }
    else return;   // everything else is typing, and belongs to the field
    e.preventDefault();
}, true);

// Each keystroke re-ranks. Not debounced: the answer comes from a pipe, and
// waiting on a timer to save a round trip that costs nothing would only make
// the picker feel slower than it is.
document.addEventListener('input', (e) => {
    if (finder.open && e.target === el.findQ) rankNow();
});

document.addEventListener('keydown', (e) => {
    // Not while a file is open. The editor no longer stops every key on its
    // way past — it cannot, or its own bindings never fire — so the listing's
    // keys have to decline for themselves.
    if (viewer.on) return;
    // cian's own keys first; anything not claimed here is left to Chromium,
    // which is what makes Ctrl+C and friends work without being written out.
    const k = e.key;
    if (k === 'ArrowDown' || k === 'j') move(1);
    else if (k === 'ArrowUp' || k === 'k') move(-1);
    else if (k === 'PageDown') move(20);
    else if (k === 'PageUp') move(-20);
    else if (k === 'Home') { state[state.focus].cursor = 0; draw(state.focus); }
    else if (k === 'End') {
        const p = state[state.focus];
        p.cursor = Math.max(0, p.entries.length - 1);
        draw(state.focus);
    }
    else if (k === 'ArrowLeft' || (k === 'h' && e.ctrlKey)) focusPane('left');
    else if (k === 'ArrowRight' || (k === 'l' && e.ctrlKey)) focusPane('right');
    else if (k === 'Tab') { state.focus = state.focus === 'left' ? 'right' : 'left'; draw('left'); draw('right'); }
    else if (k === 'Enter' && (e.ctrlKey || e.metaKey)) openOut();
    else if (k === 'Enter') enter();
    else if (k === 'Backspace') parent();
    else if (k === ' ') mark(false);
    else if (k === 'a' && (e.ctrlKey || e.metaKey)) mark(true);
    else if (k === 'c' && !e.ctrlKey && !e.metaKey) operate('copy');
    else if (k === 'm') operate('move');
    else if (k === 'd') operate('delete');
    else if (k === 'T') openMenu(TOGGLES);
    else if (k === 'r') rename();
    else if (k === 'a' && !e.ctrlKey && !e.metaKey) create(false);
    else if (k === 'A') create(true);
    else if (k === 'u') undo();
    else if (k === 'V') invert();
    else if (k === 'o') syncPane(true);
    else if (k === 'O') syncPane(false);
    else if (k === 'z') goToPath();
    else if (k === 'p' && !e.ctrlKey && !e.metaKey) copyPaths();
    else if (k === 'F5') reread();
    else if (k === '?') openHelp();
    else if (k === 'F3') lookInside();
    else if (k === 'c' && (e.ctrlKey || e.metaKey)) hold('copy');
    else if (k === 'x' && (e.ctrlKey || e.metaKey)) hold('cut');
    else if ((k === 'v' && (e.ctrlKey || e.metaKey)) || k === 'y') paste();
    else if (k === '/') startFilter();
    else if (k === ',') openMenu(SORT_MENU);
    else if (k === 'Escape' && running) {
        window.cian.call('cancel', { op: running.op });
        say('中止しています…');
    }
    else {
        // Nothing claimed it. Said out loud rather than swallowed: a key that
        // does nothing and a key that is not bound look identical from the
        // outside, and the terminal build grew `:key` for exactly this.
        if (k.length === 1 || k.startsWith('Arrow') || k.startsWith('F')) {
            console.log(`key: ${JSON.stringify(k)} code=${e.code}`
                + (e.ctrlKey ? ' ctrl' : '') + (e.metaKey ? ' meta' : '')
                + (e.shiftKey ? ' shift' : '') + (e.altKey ? ' alt' : ''));
        }
        return;
    }
    e.preventDefault();
});

/// Which face the listing actually got.
///
/// Naming a font is a request, not an instruction: the browser walks the list
/// and takes the first one installed, and if none is, the answer is whatever
/// the machine calls `sans-serif` — or worse, its default, which on Japanese
/// Windows is 明朝. That happened, and it took a person at the machine to
/// notice. A guess about type is not worth having when the answer can be
/// measured in one line.
function resolvedFace() {
    const asked = getComputedStyle(document.body).fontFamily.split(',');
    for (const raw of asked) {
        const name = raw.trim().replace(/^["']|["']$/g, '');
        if (document.fonts.check(`16px "${name}"`)) return name;
    }
    return '(none of them — the browser chose)';
}

/// The editor's runtime, loaded once and only when a file is opened.
///
/// **One component reads and writes.** The terminal build has no separate
/// editor either — its viewer becomes editable where you stand — and a
/// hand-written viewer beside a real editor would be two implementations of
/// the same motions and the same search, which is the pair that always drifts.
///
/// It is not in the repository. `node gui/vendor.js` puts it there out of
/// node_modules, trimmed to what actually runs; the release builds carry it.
let monacoLoading = null;

function loadMonaco() {
    if (monacoLoading) return monacoLoading;
    monacoLoading = new Promise((ok, no) => {
        const s = document.createElement('script');
        s.src = 'vendor/monaco/vs/loader.js';
        s.onerror = () => no(new Error(
            'gui/vendor がありません — gui/ で `node vendor.js` を実行してください'));
        s.onload = () => {
            // Absolute, because the editor starts a worker and a worker has
            // no idea what this page's directory was. A relative path sent it
            // looking for `file:///vendor/...` — the root of the disk — and
            // the editor came up as an exception with an empty window behind
            // it.
            const vs = new URL('vendor/monaco/vs', document.baseURI).href;
            // eslint-disable-next-line no-undef
            require.config({
                paths: { vs },
                'vs/nls': { availableLanguages: { '*': 'ja' } },
            });
            // eslint-disable-next-line no-undef
            require(['vs/editor/editor.main'], () => withVim().then(() => ok(window.monaco), no), no);
        };
        document.head.append(s);
    });
    return monacoLoading;
}

/// Reading a file, without leaving cian.
///
/// A window of lines is drawn rather than the whole file. The terminal build
/// works this way because a terminal has no choice; here it is a choice, and
/// the right one — a hundred thousand rows is a hundred thousand elements
/// otherwise, and the files worth opening in a viewer are exactly the long
/// ones. Which lines are on screen is arithmetic either way.
/// Add the vim grammar, which will not load itself.
///
/// monaco-vim ships a UMD bundle that checks for `define.amd` first — and
/// Monaco's own loader defines one, so it takes the AMD branch and goes
/// looking for `monaco-editor/esm/vs/editor/editor.api`, which is not in the
/// trimmed runtime and would not be the same copy of Monaco if it were. With
/// `define` out of sight for the length of the load it takes the plain-global
/// branch instead and picks up the `monaco` already on the window: one copy of
/// the editor, which is the only arrangement in which the vim keys reach the
/// editor the file is open in.
function withVim() {
    return new Promise((ok, no) => {
        const saved = window.define;
        window.define = undefined;
        const s = document.createElement('script');
        s.src = 'vendor/monaco-vim.js';
        const restore = () => { window.define = saved; };
        s.onload = () => { restore(); ok(); };
        s.onerror = () => { restore(); no(new Error('vendor/monaco-vim.js がありません')); };
        document.head.append(s);
    });
}

/// Reading and writing a file, without leaving cian.
///
/// **One component does both.** The terminal build has no separate editor —
/// its viewer becomes editable where you stand — and a hand-written viewer
/// beside a real editor would be two implementations of the same motions and
/// the same search. That pair always drifts; it is the reason the clipboard
/// rules and the copy guard live in cian-core.
const viewer = {
    on: false, opening: false, ed: null, vim: null,
    name: '', about: '', dirty: false,
    /// The model's version at the last read or write.
    ///
    /// Dirtiness is this compared against the current version, not a flag set
    /// by the first edit. A flag says "changed" for ever, including after the
    /// edits have been undone back to what is on disk — and it also went up
    /// the moment the file was loaded, because filling an editor is a change
    /// like any other. Monaco's alternative version id is exactly this
    /// question already answered.
    base: 0,
};

/// Which grammar the editor speaks.
///
/// notepad by default, because that is what a Windows desktop expects and
/// what was decided for this build. vim for Taketan, and for anyone else who
/// would rather. Where the choice is remembered is still open — the same
/// question as the look, and answering it in two places would be worse than
/// leaving it unanswered in one.
const STYLES = [['notepad', 'メモ帳流'], ['vim', 'vim']];
let style = 0;

/// The four looks are two grounds. Monaco ships a light and a dark theme, and
/// the editor sitting in the wrong one is the sort of thing that reads as
/// broken rather than as unstyled.
function editorTheme() {
    return LOOKS[look][0] === 'inei' || LOOKS[look][0] === 'terminal' ? 'vs-dark' : 'vs';
}

const MONACO_LANG = {
    Rust: 'rust', TypeScript: 'typescript', JavaScript: 'javascript', Python: 'python',
    Json: 'json', Toml: 'ini', Yaml: 'yaml', Markdown: 'markdown', Html: 'html',
    Css: 'css', Shell: 'shell', C: 'c', Cpp: 'cpp', Go: 'go', Java: 'java',
    Lua: 'lua', Sql: 'sql', Xml: 'xml', Ruby: 'ruby', Php: 'php',
};

async function lookInside() {
    const which = state.focus;
    const pane = state[which];
    const row = pane && pane.entries[pane.cursor];
    if (!row || row.parent) return;
    if (row.is_dir) { await enter(); return; }
    // Opening takes a second the first time — the editor's runtime has to load
    // — and `viewer.on` is not true until it has. Enter followed by F3 in that
    // second started a second open, and the second one's setValue landed on
    // the first one's editor.
    if (viewer.opening || viewer.on) return;
    viewer.opening = true;
    try {
        await openInEditor(which);
    } finally {
        viewer.opening = false;
    }
}

async function openInEditor(which) {
    const f = await ask('view', { pane: which });
    if (!f) return;

    let monaco;
    try {
        monaco = await loadMonaco();
    } catch (e) {
        say(e.message, true);
        return;
    }

    const enc = { Utf8: 'UTF-8', ShiftJis: 'Shift_JIS', Utf16Le: 'UTF-16LE', Utf16Be: 'UTF-16BE' };
    // Named, always. Which encoding it turned out to be is the question a
    // Japanese Windows machine asks of every file it did not write, and the
    // answer decides whether saving it is safe.
    viewer.about = [
        enc[f.encoding] || f.encoding,
        f.bom ? 'BOM' : null,
        f.eol.toUpperCase(),
        `${f.lines.length} 行`,
    ].filter(Boolean).join('  ·  ');
    viewer.name = f.name;
    viewer.dirty = false;
    viewer.on = true;
    el.view.hidden = false;

    const text = f.lines.join('\n');
    const lang = MONACO_LANG[f.lang] || 'plaintext';
    if (!viewer.ed) {
        viewer.ed = monaco.editor.create(el.vBody, {
            value: text,
            language: lang,
            theme: editorTheme(),
            automaticLayout: true,
            fontFamily: getComputedStyle(document.body).fontFamily,
            fontSize: parseFloat(getComputedStyle(document.body).fontSize),
            minimap: { enabled: false },
            // The one place this build differs from a code editor's defaults:
            // a file manager opens files it did not write, and reformatting
            // them on the way past is not its business.
            renderWhitespace: 'selection',
            scrollBeyondLastLine: false,
        });
        viewer.ed.onDidChangeModelContent(() => {
            const now = viewer.ed.getModel().getAlternativeVersionId();
            const dirty = now !== viewer.base;
            if (dirty === viewer.dirty) return;
            viewer.dirty = dirty;
            drawViewFoot();
        });
        viewer.ed.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => saveFile());
        viewer.ed.onDidChangeCursorPosition(drawViewFoot);
    } else {
        viewer.ed.updateOptions({ theme: editorTheme() });
        monaco.editor.setModelLanguage(viewer.ed.getModel(), lang);
        viewer.ed.setValue(text);
    }
    // After the text is in, not before: loading it is a change to the model,
    // and a file is not modified by having been opened.
    viewer.base = viewer.ed.getModel().getAlternativeVersionId();
    viewer.dirty = false;
    setStyle(style);
    el.vName.textContent = f.name;
    el.vAbout.textContent = viewer.about;
    viewer.ed.setPosition({ lineNumber: 1, column: 1 });
    viewer.ed.focus();
    drawViewFoot();
}

/// Attach or drop the vim grammar. Called on open and whenever the switch is
/// flipped, so the running editor changes under you rather than needing to be
/// closed and reopened.
function setStyle(i) {
    style = (i + STYLES.length) % STYLES.length;
    if (!viewer.ed) return;
    if (viewer.vim) { viewer.vim.dispose(); viewer.vim = null; }
    if (STYLES[style][0] === 'vim') {
        // eslint-disable-next-line no-undef
        viewer.vim = MonacoVim.initVimMode(viewer.ed, el.vFoot);
        // `:w` and `:q` where the fingers put them. Without these, vim style
        // would still need Ctrl+S and Esc — which is exactly the seam that
        // makes a vim mode feel like a costume.
        // eslint-disable-next-line no-undef
        const ex = MonacoVim.VimMode.Vim;
        ex.defineEx('write', 'w', saveFile);
        ex.defineEx('quit', 'q', () => closeView(false));
        ex.defineEx('wq', 'wq', async () => { if (await saveFile()) closeView(false); });
    }
    drawViewFoot();
}

function drawViewFoot() {
    // In vim style the footer is vim's own — its mode line and its `:` prompt
    // live there, and writing over them would take the command line away.
    if (viewer.vim) return;
    const at = viewer.ed && viewer.ed.getPosition();
    const where = at ? `${at.lineNumber} : ${at.column}` : '';
    el.vFoot.textContent = [
        where,
        viewer.dirty ? '未保存' : null,
        STYLES[style][1],
        'Ctrl+S 保存   Esc ×3 閉じる',
    ].filter(Boolean).join('   ·   ');
}

async function saveFile() {
    if (!viewer.ed) return false;
    const lines = viewer.ed.getValue().split(/\r?\n/);
    const r = await ask('save', { lines });
    if (!r) return false;
    viewer.base = viewer.ed.getModel().getAlternativeVersionId();
    viewer.dirty = false;
    drawViewFoot();
    say(`${r.saved} を保存しました（${r.lines} 行）`);
    // The listing shows a size and a date; both just changed.
    await reread();
    return true;
}

/// Leaving. An unsaved file asks first — the only door out of an editor that
/// can lose work.
async function closeView(ask_first = true) {
    if (ask_first && viewer.dirty) {
        if (!await confirm(`${viewer.name} は未保存です`, '閉じると編集は失われます')) return;
    }
    viewer.on = false;
    viewer.dirty = false;
    if (viewer.vim) { viewer.vim.dispose(); viewer.vim = null; }
    el.view.hidden = true;
    el.status.focus?.();
}

/// Three of the same key in a row is the way out.
///
/// The terminal build's rule, taken rather than invented — and the reason it
/// exists is worth keeping with it. One press must not close a file with
/// unsaved work in it, and Esc is pressed by reflex, so a single Esc closing
/// the editor would make it the most dangerous key on the keyboard in the one
/// grammar where it is hit without thinking. Three in a row is not a stray
/// keystroke.
///
/// It counts silently. A tally along the bottom, raised by a key pressed in
/// error, is noise exactly when it is least wanted; `?` says how to leave.
const wayOut = { key: null, times: 0 };

/// Whether vim is taking text right now.
///
/// Read off the mode line, because that is where vim itself says so — and
/// because this listener runs in the capture phase, before monaco-vim has
/// seen the key, the line still reads INSERT on the press that leaves insert
/// mode. Which is what is wanted: that press is leaving insert, not asking to
/// leave the file.
function vimTyping() {
    return !!viewer.vim && /INSERT|REPLACE/.test(el.vFoot.textContent || '');
}

document.addEventListener('keydown', (e) => {
    if (!viewer.on) return;
    // Not while the question is up. Esc answers it — and counting those
    // presses toward another way out would mean declining to close three
    // times and being asked a fourth.
    if (!el.ask.hidden) { wayOut.key = null; wayOut.times = 0; return; }

    // F3 is nobody's editing key, so it is the one door that opens on a single
    // press. Esc and Backspace are both, which is why they take three.
    if (e.key === 'F3') {
        e.stopPropagation();
        e.preventDefault();
        wayOut.key = null;
        wayOut.times = 0;
        closeView();
        return;
    }

    // Backspace deletes in notepad style, so it is not offered as a way out
    // there. In vim style it is, but not while insert mode has the keyboard.
    const doors = viewer.vim
        ? (vimTyping() ? [] : ['Escape', 'Backspace'])
        : ['Escape'];
    if (!doors.includes(e.key)) {
        wayOut.key = null;
        wayOut.times = 0;
        return;
    }
    // The same key three times. Esc, Backspace, Esc is three presses and no
    // intent — it is a hand looking for something.
    if (wayOut.key !== e.key) {
        wayOut.key = e.key;
        wayOut.times = 0;
    }
    wayOut.times += 1;
    if (wayOut.times < 3) return;
    wayOut.key = null;
    wayOut.times = 0;
    // Not stopped on the way through: the editor still gets its Esc, which is
    // how vim leaves whatever it was in the middle of. The third press asks
    // only when there is something to lose.
    closeView();
}, true);

/// Ctrl+Enter: a folder to the other pane, a file to your own application.
///
/// One key with two answers, because that is the terminal build's — and the
/// two share a question ("open this somewhere other than here") rather than
/// being two features that happen to sit on one chord.
async function openOut() {
    const r = await ask('open', { pane: state.focus });
    if (!r) return;
    if (r.view) {
        state[r.pane] = r.view;
        draw(r.pane);
        say(`${r.name} を${r.pane === 'left' ? '左' : '右'}で開きました`);
    } else {
        say(`${r.opened} を既定のアプリで開きました`);
    }
}

/// What the engine says unasked.
///
/// **Nothing was listening.** The bridge has offered `onEvent` since the spine
/// went in, and the renderer never subscribed — so a copy said "0 / 1" and
/// then sat there for ever, the panes did not reload when it finished, and a
/// job that failed reported nothing at all. The engine had been refusing to
/// copy a directory into itself, correctly and in silence.
window.cian.onEvent(async (msg) => {
    switch (msg.event) {
        case 'progress':
            if (!running || msg.op !== running.op) return;
            say(`${running.verb}中… ${msg.done} / ${msg.total}  ${base(msg.path)}`);
            return;

        case 'done': {
            if (!running || msg.op !== running.op) return;
            const verb = running.verb;
            running = null;
            // Awaited, because the listings speak too — and whichever of the
            // two says its piece last is the one that stays on screen.
            await reread();
            if (msg.cancelled) say(`${verb}を中止しました（${msg.ok} 件は済み）`, true);
            // Every failure, named. A count of them tells you something went
            // wrong without telling you what, which is the worst of both.
            else if (msg.errors.length) say(msg.errors.join('  /  '), true);
            else if (msg.skipped) say(`${verb} ${msg.ok} 件、${msg.skipped} 件は飛ばしました`);
            else say(`${verb} ${msg.ok} 件（${msg.ms} ms）`);
            return;
        }

        case 'finding':
            if (finder.open) el.findFoot.textContent = `${msg.found} 件を見ています…`;
            return;

        case 'found':
            if (!finder.open) return;
            el.findFoot.textContent = msg.capped
                ? `${msg.total} 件で打ち切り — 絞り込んでください`
                : `${msg.total} 件`;
            rankNow();
            return;
    }
});

function base(p) {
    return String(p).split(/[\\/]/).pop();
}

refresh().then(() => {
    // Said once, on the status line, where it costs nothing and answers the
    // only question this milestone exists to answer.
    const face = resolvedFace();
    const size = getComputedStyle(document.body).fontSize;
    say(`${el.status.textContent}   ·   ${face} ${size}`);
});
