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
    // The safe button has the focus. Leaning on the keyboard should not
    // delete anything.
    const no = el.ask.querySelector('[data-answer="no"]');
    no.focus();
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
    // Only directories go anywhere yet. Opening a file is the next milestone,
    // and pretending otherwise would be a click that silently does nothing.
    if (!row || (!row.is_dir && !row.parent)) {
        say(`${row ? row.name : ''} — ファイルを開くのは次の段階です`);
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
    if (toggles.on) drawToggles();
    say(r.showing ? '隠しファイルを表示' : '隠しファイルを非表示');
}

/// Walk the sort keys, and turn one round when it is already chosen.
const SORTS = ['name', 'size', 'date', 'ext'];
let sortAt = 0;
async function sortBy() {
    const which = state.focus;
    const key = SORTS[sortAt];
    const r = await ask('sort', { pane: which, key });
    if (!r) return;
    // Reversed means the same key again; otherwise move on to the next one.
    if (!r.reverse) sortAt = (sortAt + 1) % SORTS.length;
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
const toggles = { on: false, at: 0 };

function togglesRows() {
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
            run: () => { setLook(look + 1); drawToggles(); say(`配色: ${LOOKS[look][1]}`); },
        },
    ];
}

function openToggles() {
    toggles.on = true;
    toggles.at = 0;
    el.find.hidden = false;
    el.findQ.hidden = true;
    el.findFoot.textContent = '↑↓ 選ぶ  Enter 切替  Esc 閉じる';
    drawToggles();
}

function closeToggles() {
    toggles.on = false;
    el.find.hidden = true;
    el.findQ.hidden = false;
}

function drawToggles() {
    const rows = togglesRows();
    const frag = document.createDocumentFragment();
    rows.forEach((row, i) => {
        const div = document.createElement('div');
        div.className = 'hit' + (i === toggles.at ? ' on' : '');
        const l = document.createElement('span');
        l.className = 'p';
        l.textContent = row.label;
        const v = document.createElement('span');
        v.textContent = row.value;
        div.append(l, v);
        div.addEventListener('mousedown', () => { toggles.at = i; row.run(); });
        frag.append(div);
    });
    el.findHits.replaceChildren(frag);
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

/// Everything the engine says unasked.
window.cian.onEvent((msg) => {
    // The walk, which belongs to the finder rather than to an operation.
    if (msg.event === 'finding' || msg.event === 'found') {
        if (!finder.open) return;
        finder.walking = msg.event === 'finding';
        // Re-rank as more arrives: what was typed a moment ago should see the
        // files that turned up since.
        rankNow();
        return;
    }
    if (!running || msg.op !== running.op) return;
    if (msg.event === 'progress') {
        say(`${running.verb}中… ${msg.done} / ${msg.total}`);
    } else if (msg.event === 'done') {
        const was = running;
        running = null;
        // Both panes: one lost files, the other gained them.
        refresh().then(() => {
            const bits = [`${was.verb} ${msg.ok} 件`];
            if (msg.skipped) bits.push(`${msg.skipped} 件スキップ`);
            if (msg.cancelled) bits.push('中止しました');
            if (msg.errors.length) bits.push(`${msg.errors.length} 件失敗`);
            bits.push(`${msg.ms} ms`);
            say(bits.join(' · '), msg.errors.length > 0);
        });
    }
});

/// The switches' keys, while they are up.
document.addEventListener('keydown', (e) => {
    if (!toggles.on) return;
    e.stopPropagation();
    const rows = togglesRows();
    if (e.key === 'Escape' || e.key === 'T') closeToggles();
    else if (e.key === 'ArrowDown') { toggles.at = Math.min(rows.length - 1, toggles.at + 1); drawToggles(); }
    else if (e.key === 'ArrowUp') { toggles.at = Math.max(0, toggles.at - 1); drawToggles(); }
    else if (e.key === 'Enter' || e.key === ' ') rows[toggles.at].run();
    else return;
    e.preventDefault();
}, true);

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
    else if (k === 'ArrowLeft' || k === 'h') { state.focus = 'left'; draw('left'); draw('right'); }
    else if (k === 'ArrowRight' || k === 'l') { state.focus = 'right'; draw('left'); draw('right'); }
    else if (k === 'Tab') { state.focus = state.focus === 'left' ? 'right' : 'left'; draw('left'); draw('right'); }
    else if (k === 'Enter') enter();
    else if (k === 'Backspace') parent();
    else if (k === ' ') mark(false);
    else if (k === 'a' && (e.ctrlKey || e.metaKey)) mark(true);
    else if (k === 'c' && !e.ctrlKey && !e.metaKey) operate('copy');
    else if (k === 'm') operate('move');
    else if (k === 'd') operate('delete');
    else if (k === 'T') openToggles();
    else if (k === 'r') rename();
    else if (k === 'a' && !e.ctrlKey && !e.metaKey) create(false);
    else if (k === 'A') create(true);
    else if (k === 'u') undo();
    else if (k === '/') startFilter();
    else if (k === ',') sortBy();
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

refresh().then(() => {
    // Said once, on the status line, where it costs nothing and answers the
    // only question this milestone exists to answer.
    const face = resolvedFace();
    const size = getComputedStyle(document.body).fontSize;
    say(`${el.status.textContent}   ·   ${face} ${size}`);
});
