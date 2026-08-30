'use strict';
// The listing, drawn. No engine logic here — this asks and paints, and every
// answer it paints came from cian-core.

/// The two panes as the engine last described them.
const state = { left: null, right: null, focus: 'left' };

const el = {
    hints: document.getElementById('hints'),
    work: document.getElementById('work'),
    panes: document.getElementById('panes'),
    left: document.querySelector('[data-pane="left"]'),
    right: document.querySelector('[data-pane="right"]'),
    status: document.getElementById('status'),
    stBadge: document.getElementById('st-badge'),
    stChips: document.getElementById('st-chips'),
    stMsg: document.getElementById('st-msg'),
    stShell: document.getElementById('st-shell'),
    ask: document.getElementById('ask'),
    find: document.getElementById('find'),
    findHits: document.getElementById('find-hits'),
    findFoot: document.getElementById('find-foot'),
    fbar: document.getElementById('fbar'),
    fSign: document.getElementById('f-sign'),
    fInput: document.getElementById('f-input'),
    fCount: document.getElementById('f-count'),
    prog: document.getElementById('prog'),
    pHead: document.getElementById('p-head'),
    pNow: document.getElementById('p-now'),
    pFill: document.getElementById('p-fill'),
    pNum: document.getElementById('p-num'),
    shell: document.getElementById('shell'),
    sTabs: document.getElementById('s-tabs'),
    sTitle: document.getElementById('s-title'),
    sAbout: document.getElementById('s-about'),
    sPanes: document.getElementById('s-panes'),
    report: document.getElementById('report'),
    rName: document.getElementById('r-name'),
    rAbout: document.getElementById('r-about'),
    rQ: document.getElementById('r-q'),
    rRows: document.getElementById('r-rows'),
    rFoot: document.getElementById('r-foot'),
    view: document.getElementById('view'),
    vName: document.getElementById('v-name'),
    vAbout: document.getElementById('v-about'),
    vBody: document.getElementById('v-body'),
    vPic: document.getElementById('v-pic'),
    vRead: document.getElementById('v-read'),
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
function confirm(head, body, choices = {}) {
    el.ask.querySelector('.head').textContent = head;
    el.ask.querySelector('.body').textContent = body;
    const yesBtn = el.ask.querySelector('[data-answer="yes"]');
    // The plain answer can be renamed per question — a transfer's Enter means
    // "skip what already exists", a delete's means "to the trash" — and the
    // stronger variants ride on their own letter, as the terminal build has
    // them: `a` is the "I really mean it" key, `r` renames on the way.
    yesBtn.textContent = `${choices.yes ?? '実行'}  (Enter)`;
    const extras = choices.extras ?? [];
    for (const x of extras) {
        const b = document.createElement('button');
        b.dataset.answer = x.key;
        b.textContent = `${x.label}  (${x.key})`;
        yesBtn.before(b);
    }
    el.ask.hidden = false;
    // The focus goes where Enter goes.
    //
    // It used to sit on the safe button, meaning to make leaning on the
    // keyboard harmless — but Enter answers yes here whatever has the focus,
    // so it was not protecting anything. All it did was put a ring around
    // やめる while the key labelled (Enter) did 実行, which reads as the
    // opposite of what happens. Being asked at all is the protection.
    yesBtn.focus();
    return new Promise((resolve) => {
        const done = (answer) => {
            el.ask.hidden = true;
            el.ask.removeEventListener('click', onClick);
            document.removeEventListener('keydown', onKey, true);
            for (const b of el.ask.querySelectorAll('.buttons button')) {
                if (b.dataset.answer !== 'yes' && b.dataset.answer !== 'no') b.remove();
            }
            yesBtn.textContent = '実行  (Enter)';
            resolve(answer);
        };
        const onClick = (e) => {
            const a = e.target.dataset && e.target.dataset.answer;
            if (a === 'yes') done(true);
            else if (a === 'no') done(false);
            else if (a) done(a);
        };
        const onKey = (e) => {
            if (e.key === 'Escape' || e.key === 'n') { e.stopPropagation(); done(false); }
            else if (e.key === 'Enter' || e.key === 'y') { e.stopPropagation(); done(true); }
            else if (extras.some((x) => x.key === e.key)) { e.stopPropagation(); done(e.key); }
            else if (e.key !== 'Tab') { e.stopPropagation(); }
        };
        el.ask.addEventListener('click', onClick);
        // Captured, so the listing's own keys never see these.
        document.addEventListener('keydown', onKey, true);
    });
}

/// The transient half of the status line. The chips beside it are rebuilt by
/// drawStatus(); this is the one thing that changes because something was
/// *said* rather than because something *is*.
const status = { msg: '', bad: false };

function say(text, bad = false) {
    status.msg = text;
    status.bad = bad;
    drawStatus();
    // Every state change in this program passes through here on its way to
    // saying what happened, which makes it the one place the hint bar can be
    // kept honest without threading a call through eighty functions.
    drawHints();
}

/// Free space per pane, fetched when the pane lands somewhere new. Cached by
/// path: the status line redraws on every keystroke and a statvfs per `j`
/// would be a disk question asked two hundred times for one answer.
const disk = { left: { at: null, v: null }, right: { at: null, v: null } };

async function freshenDisk(which) {
    const pane = state[which];
    const d = disk[which];
    if (!pane || pane.remote || pane.cwd === d.at) return;
    d.at = pane.cwd;
    try {
        // Straight through the bridge, not ask(): a pane that cannot answer
        // (an archive, a listing mid-change) is a chip that stays blank, not
        // a dialog.
        d.v = await window.cian.call('df', { pane: which });
    } catch { d.v = null; }
    drawStatus();
}

/// The terminal build's status row (render.rs draw_status), chip for chip:
/// badge → counts → marks → the file under the cursor → the filter → the
/// disk → the running operation → the message. The badge and the message are
/// never dropped; the chips clip from the left (CSS does the dropping).
function drawStatus() {
    const which = state.focus;
    const pane = state[which];
    // The badge: which surface has the keys, and in what mode.
    const mode = term.on && term.focused ? ['S', '']
        : visual.on ? [which === 'left' ? 'L' : 'R', ' VISUAL']
        : filter.on ? [which === 'left' ? 'L' : 'R', ' FILTER']
        : [which === 'left' ? 'L' : 'R', ''];
    el.stBadge.textContent = mode[0] + mode[1];
    el.stBadge.className = mode[1] === ' VISUAL' ? 'visual' : mode[1] === ' FILTER' ? 'filter' : '';
    const chips = [];
    const chip = (cls, text) => {
        const s = document.createElement('span');
        s.className = cls;
        s.textContent = text;
        chips.push(s);
    };
    if (pane) {
        chip('n', `${pane.entries.length} 件`);
        if (pane.marked > 0) chip('mk', `マーク ${pane.marked}`);
        const row = pane.entries[pane.cursor];
        if (row && !row.parent) chip('cur', row.name);
        if (pane.filter) chip('flt', `フィルタ /${pane.filter} (${pane.entries.length} 件)`);
    }
    const d = disk[which]?.v;
    if (d && d.total > 0) {
        const usedPct = (d.total - d.available) / d.total;
        chip(`disk${usedPct >= 0.95 ? ' crit' : usedPct >= 0.8 ? ' warn' : ''}`,
            `空き ${human(d.available)} / ${human(d.total)}`);
    }
    if (running) {
        // Per cent where the bytes are known — the same number the bar shows,
        // so the chip left behind by `b` is not a different measurement.
        const pct = running.bytesTotal > 0
            ? `${Math.round((running.bytes / running.bytesTotal) * 100)}%`
            : `${running.done ?? 0} / ${running.total}`;
        chip('op', `↻ ${running.verb} ${pct}`);
    }
    // Something is being asked of the engine and is taking a moment. It has
    // no percentage to report — `:du` cannot know how deep the tree is until
    // it has walked it — so this says only that the key landed.
    if (busy.n > 0) chip('op', '⋯ 実行中');
    el.stChips.replaceChildren(...chips);
    el.stMsg.textContent = status.msg ? `◂ ${status.msg}` : '';
    el.stMsg.classList.toggle('bad', status.bad);
    // The active shell's own title on the right, the terminal build's rule:
    // suppressed while a message is showing — the message wins the space.
    el.stShell.textContent = !status.msg && term.on ? el.sTitle.textContent : '';
}

/// Bytes, in the width a listing can spare. A directory shows `—` as the
/// terminal build has it — a dash is "not a number here", where a blank reads
/// as a cell that failed to load. `..` alone shows nothing.
function size(row) {
    if (row.parent) return '';
    if (row.is_dir) return '—';
    const u = ['B', 'K', 'M', 'G', 'T', 'P', 'E'];
    let n = row.len, i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return (i === 0 ? n : n.toFixed(n < 10 ? 1 : 0)) + u[i];
}

/// Which surface the keys are actually in, said on `#work` so the stylesheet
/// can tell "this is the current pane" from "this is where you are typing".
/// The file pane wore the accent frame while the shell had the keyboard.
function markFocus() {
    el.work.dataset.focus = term.on && term.focused ? 'shell' : 'files';
}

/// The one owner of "the keys are in the shell now".
///
/// Ten places set `term.focused` and nine of them also had to remember the
/// class beside it — so the frame and the flag drifted apart, and the tenth
/// site somebody adds next month would have drifted too. Same shape as the
/// undo stacks: if two things must move together, one function moves them.
function setShellFocus(on) {
    term.focused = on;
    el.shell.classList.toggle('on', on);
    markFocus();
}

function draw(which) {
    const pane = state[which];
    const root = el[which];
    root.classList.toggle('active', state.focus === which);
    markFocus();
    if (!pane) return;
    root.classList.toggle('remote', !!pane.remote);
    // What this pane is showing, said in the one line people actually read.
    // A server's rows look exactly like a local directory's, and mistaking
    // somebody's server for your own disk is worth a word and a frame.
    // The tab strip, drawn only when there is a second tab: a row of chrome
    // showing one tab is a row of chrome saying nothing.
    const strip = root.querySelector('.tabs');
    if (!pane.tabs || pane.tabs.length < 2) {
        strip.replaceChildren();
    } else {
        strip.replaceChildren(...pane.tabs.map((name, i) => {
            const t = document.createElement('span');
            t.textContent = name;
            if (i === pane.tab) t.className = 'on';
            t.addEventListener('mousedown', () => goTab(which, { at: i }));
            return t;
        }));
    }
    const where = pane.remote
        ? `${pane.remote}:${pane.cwd}`
        : pane.archive
            ? `${pane.archive.split(/[\\/]/).pop()} の中`
            : (pane.flat ? `${pane.flat} — ${pane.cwd}` : pane.cwd);
    // Which side you are on, but only when the other side is not on screen.
    // With both panes visible the highlight says it; with one, Tab would
    // otherwise change everything and announce nothing.
    const lead = ONE_PANE.includes(viewMode) ? `[${which === 'left' ? '左' : '右'}] ` : '';
    // A breadcrumb rather than a grey string: the parents dim, the folder you
    // are *in* in the text colour. It is the most-read line in the window and
    // it was set smaller than the date column and in the same ink as the
    // things nobody reads.
    const crumb = root.querySelector('.crumb');
    const cut = Math.max(where.lastIndexOf('/'), where.lastIndexOf('\\'));
    const here = cut >= 0 && cut < where.length - 1 ? where.slice(cut + 1) : '';
    const parents = here ? where.slice(0, cut + 1) : where;
    // One isolated run holding both parts, so the right-to-left box clips the
    // head without reordering anything inside it.
    const path = document.createElement('span');
    path.className = 'path';
    const tail = document.createElement('span');
    tail.className = 'here';
    tail.textContent = here;
    path.append(document.createTextNode(lead + parents), tail);
    crumb.replaceChildren(path);

    const rows = root.querySelector('.rows');
    // Rebuilt whole. A listing is a few hundred rows and Chromium does not
    // notice; the moment it does, this is where a windowed list goes.
    const frag = document.createDocumentFragment();
    rows.classList.toggle('icons', viewMode === 'icons');
    rows.classList.toggle('details', viewMode === 'details');
    rows.classList.toggle('classic', viewMode === 'classic');
    // The columns that fit, decided by the pane's real width — the terminal
    // build's progressive drop (render.rs: the date needs ~52 columns, the
    // size ~34), translated through the half-width cell of the current size.
    const ch = FONT.at / 2;
    root.classList.toggle('no-when', viewMode !== 'details' && root.clientWidth < 52 * ch);
    root.classList.toggle('no-size', viewMode !== 'details' && root.clientWidth < 34 * ch);
    // The ☁ column only where something is actually in the cloud, as the
    // terminal build allocates it — two blank cells on every row otherwise.
    root.classList.toggle('no-cloud', !pane.entries.some((e) => e.cloud));
    // The column header, in classic and details both — the terminal build
    // draws one in every list view, and a table without its sort marker is a
    // table you have to remember about. Clicking a heading sorts by it.
    const head = root.querySelector('.dhead');
    head.hidden = viewMode === 'icons';
    if (!head.hidden && head.dataset.built !== viewMode) {
        head.dataset.built = viewMode;
        head.replaceChildren();
        const cols = viewMode === 'details'
            ? [['glyph', '', null], ['name', '名前', 'name'], ['cloud', '', null],
               ['size', 'サイズ', 'size'], ['kind', '種類', 'ext'], ['when', '更新日時', 'date']]
            : [['cloud', '', null], ['mark', '', null], ['glyph', '', null],
               ['name', '名前', 'name'], ['size', 'サイズ', 'size'], ['when', '日時', 'date']];
        for (const [cls, label, key] of cols) {
            const c = document.createElement('span');
            c.className = cls;
            c.textContent = label;
            c.dataset.key = key || '';
            if (key) {
                c.classList.add('sortable');
                c.addEventListener('mousedown', () => applySort(key));
            }
            head.append(c);
        }
    }
    if (!head.hidden) {
        // Which column the listing is actually sorted by. Explorer marks it,
        // and a table that does not is a table you have to remember about.
        // The pane's own order, not a global — sorting is per pane, and a
        // remembered "current sort" described the wrong pane after a Tab.
        for (const c of head.children) {
            c.textContent = c.textContent.replace(/ [↑↓]$/, '');
            if (c.dataset.key && c.dataset.key === pane.sort_key) {
                c.textContent += pane.sort_reverse ? ' ↓' : ' ↑';
            }
        }
    }
    pane.entries.forEach((row, i) => {
        const div = document.createElement('div');
        div.className = 'row'
            + (row.is_dir ? ' dir' : '')
            + kindClassOf(row)
            + (row.marked ? ' marked' : '')
            + (i === pane.cursor ? ' cursor' : '');
        const name = document.createElement('span');
        name.className = 'name';
        name.textContent = row.parent ? '..' : row.name;
        if (viewMode === 'icons') {
            const g = document.createElement('span');
            g.className = 'glyph';
            g.textContent = glyphFor(row);
            div.append(g, name);
        } else if (viewMode === 'details') {
            // Explorer's details, in Explorer's order: icon, name, size,
            // kind, date. The icon belongs here too — a details list with no
            // picture in it reads as a table of strings, and the kind of a
            // file is the first thing the eye wants.
            const g = document.createElement('span');
            g.className = 'glyph';
            g.textContent = glyphFor(row);
            const cl = document.createElement('span');
            cl.className = 'cloud';
            cl.textContent = row.cloud ? '☁' : '';
            const len = document.createElement('span');
            len.className = 'size';
            len.textContent = size(row);
            const kind = document.createElement('span');
            kind.className = 'kind';
            kind.textContent = kindOf(row);
            const w = document.createElement('span');
            w.className = 'when';
            w.textContent = when(row);
            div.append(g, name, cl, len, kind, w);
        } else {
            // Classic, in the terminal build's column order: mark, icon,
            // name, then the numbers. The ● column is what makes ten marked
            // rows readable at a glance — a colour on the name alone
            // vanishes into whichever row is the cursor.
            const mk = document.createElement('span');
            mk.className = 'mark';
            mk.textContent = row.marked ? '●' : '';
            const g = document.createElement('span');
            g.className = 'glyph';
            g.textContent = iconFor(row);
            const cl = document.createElement('span');
            cl.className = 'cloud';
            cl.textContent = row.cloud ? '☁' : '';
            const len = document.createElement('span');
            len.className = 'size';
            len.textContent = size(row);
            const w = document.createElement('span');
            w.className = 'when';
            w.textContent = when(row);
            // The terminal build's column order: ☁, mark, icon, name, then
            // the numbers.
            div.append(cl, mk, g, name, len, w);
        }
        div.addEventListener('mousedown', () => {
            state.focus = which;
            pane.cursor = i;
            draw('left'); draw('right');
        });
        div.addEventListener('dblclick', () => { state.focus = which; enter(); });
        // Right-click opens the same menu Shift+Enter does, on the row you
        // pointed at. A file manager where the right button does nothing is a
        // file manager that feels broken before you have tried anything.
        div.addEventListener('contextmenu', (e) => {
            e.preventDefault();
            state.focus = which;
            pane.cursor = i;
            draw('left'); draw('right');
            openMenu(CONTEXT);
        });
        frag.append(div);
    });
    rows.replaceChildren(frag);

    // Keep the cursor on screen without yanking the view about.
    const at = rows.children[pane.cursor];
    if (at) at.scrollIntoView({ block: 'nearest' });

    // The chip row follows every repaint — the counts, the marks and the
    // name under the cursor are all things a repaint may have changed — and
    // the disk chip refreshes itself only when the pane landed somewhere new.
    drawStatus();
    freshenDisk(which);
}

/// How many engine calls are outstanding and have been slow enough to be
/// worth admitting to.
///
/// `:du` over a big tree takes seconds, and the window said nothing at all
/// while it did — no way to tell "working" from "the key did not land".
/// Anything under a quarter of a second stays silent, because a chip that
/// flashes on every keystroke is worse than no chip.
const busy = { n: 0 };
const BUSY_AFTER_MS = 250;

async function ask(method, params) {
    let slow = null;
    let counted = false;
    slow = setTimeout(() => {
        counted = true;
        busy.n += 1;
        drawStatus();
    }, BUSY_AFTER_MS);
    try {
        // Every request states where both cursors are. The cursor moves here,
        // on every `j`, without asking the engine — so the engine's own copy
        // went stale, and `r` after three presses of `j` renamed a file three
        // rows up. Both, not just the focused one, because `=` compares what
        // the two of them are pointing at. Stated once, here, rather than
        // remembered at each of the dozen call sites.
        if (state.left && state.right) {
            params = {
                cursors: { left: state.left.cursor, right: state.right.cursor },
                ...params,
            };
        }
        return await window.cian.call(method, params);
    } catch (e) {
        say(String(e.message || e), true);
        return null;
    } finally {
        clearTimeout(slow);
        if (counted) { busy.n -= 1; drawStatus(); }
    }
}

/// `Shift+P` — the files themselves, for Finder or Explorer to paste.
///
/// `p` puts the path text on the clipboard. These are two different things and
/// the terminal build keeps them on two keys for a reason: pasting a path into
/// a folder is not pasting a file into it.
async function clipFiles() {
    const r = await ask('clipfiles', { pane: state.focus });
    if (!r) return;
    say(`${r.count} 件をクリップボードへ（Finder で貼り付けられます）`);
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
/// Mark, and step. `Space` steps down and `Shift+Space` up — the terminal
/// build has both, because marking a run upwards is as common as downwards
/// and doing it with Space and k is two hands' worth of keys.
async function mark(all, step = 1) {
    const which = state.focus;
    const next = await ask(all ? 'markall' : 'mark', { pane: which });
    if (!next) return;
    if (!all && step < 0) {
        // The engine always steps down after marking; going up is two steps
        // back from where it left the cursor.
        next.cursor = Math.max(0, next.cursor - 2);
    }
    state[which] = next;
    draw(which);
    say(next.marked ? `${next.marked} 件マーク` : 'マークなし');
}

/// The bar, while an operation is running.
///
/// Shown for as long as the work takes, as the terminal build shows it, and
/// dismissed with `b` *without cancelling* — the status chip carries the rest.
/// `op_bar_hidden` there, `prog.hidden` here.
const prog = { hidden: false, stalledAt: 0 };

/// A path shortened from the middle, keeping both ends — the terminal
/// build's `truncate_middle`. Both ends carry: the head says which volume or
/// project, the tail says which file. Cutting either off answers half the
/// question the line exists to answer.
function truncateMiddle(text, max = 68) {
    if (text.length <= max) return text;
    const keep = max - 1;
    const head = Math.ceil(keep / 2);
    return `${text.slice(0, head)}…${text.slice(text.length - (keep - head))}`;
}

function drawProg() {
    if (!running || prog.hidden) { el.prog.hidden = true; return; }
    el.prog.hidden = false;
    el.pHead.textContent = `${running.verb}中`;
    el.pNow.textContent = truncateMiddle(running.path || '');
    // By bytes where they are known, by files otherwise — cian-core's own
    // rule (Progress::fraction), so the bar and the numbers cannot disagree.
    const frac = running.bytesTotal > 0
        ? running.bytes / running.bytesTotal
        : (running.total > 0 ? running.done / running.total : 0);
    el.pFill.style.width = `${Math.round(Math.min(1, frac) * 100)}%`;
    const secs = Math.round((running.ms ?? 0) / 1000);
    const elapsed = secs >= 60 ? `${Math.floor(secs / 60)}分${secs % 60}秒` : `${secs}秒`;
    const bytes = running.bytesTotal > 0
        ? `${human(running.bytes)} / ${human(running.bytesTotal)}   `
        : '';
    // Stalled: the work has not reported for a while. Said plainly, because a
    // bar that has not moved in twenty seconds is otherwise indistinguishable
    // from a program that has died.
    const still = prog.stalledAt && performance.now() - prog.stalledAt > 8000
        ? `   ⚠ ${Math.round((performance.now() - prog.stalledAt) / 1000)} 秒動いていません`
        : '';
    el.pNum.textContent =
        `${Math.round(Math.min(1, frac) * 100)}%   ${bytes}(${running.done} / ${running.total} 件)   ·   ${elapsed}${still}`;
}

/// Copy, move or delete whatever is marked — or the row under the cursor when
/// nothing is. The destination is the other pane, which is the whole idea of
/// two panes side by side.
async function operate(kind) {
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
    // The terminal build's three answers. The plain yes *skips* what already
    // exists — it used to overwrite, silently, which is the one outcome a
    // confirmation exists to prevent. `a` overwrites on purpose; `r` renames
    // a single item on the way over.
    const answer = kind === 'delete'
        ? await confirm(head, body, { yes: 'ゴミ箱へ', extras: [{ key: 'a', label: '完全削除' }] })
        : await confirm(head, body, {
            yes: `${verb}（同名はスキップ）`,
            extras: [
                { key: 'a', label: '上書き' },
                ...(rows.length === 1 ? [{ key: 'r', label: '名前を変えて' }] : []),
            ],
        });
    if (!answer) {
        say('やめました');
        return;
    }
    if (answer === 'r') {
        // A single-item move/copy can be renamed on the way, seeded with the
        // name it arrived with.
        const name = await askFor(`${verb}先の名前`, rows[0].name);
        if (!name) { say('やめました'); return; }
        const r = await ask('transferas', {
            src: rows[0].path, dest: dest.cwd, name, move: kind === 'move',
        });
        if (r) { say(`${verb} → ${name}`); await reread(); }
        return;
    }
    const started = await ask(kind, {
        pane: which,
        conflict: answer === 'a' ? 'overwrite' : 'skip',
        mode: answer === 'a' ? 'permanent' : 'trash',
    });
    if (!started) return;
    beginOp(started, kind, verb);
}

/// Take up an operation the engine has just accepted — running now, or in
/// line behind one that is. A queued job gets no bar: there is nothing to
/// show yet, and the one on screen belongs to the job actually working.
function beginOp(started, kind, verb) {
    if (started.queued) {
        say(`キューに追加 — ${started.queued} 件待ち（:queue で一覧）`);
        return;
    }
    running = {
        op: started.op, kind, verb,
        total: started.count, done: 0, bytes: 0, bytesTotal: 0, ms: 0, path: '',
    };
    prog.hidden = false;
    prog.stalledAt = performance.now();
    drawProg();
    say(`${verb}中… 0 / ${started.count}`);
}

/// Land the cursor on a row. Every jump goes through here so that a jump
/// made mid-visual extends the selection — `G` in visual means "to the end",
/// and a `G` that moved the cursor without re-painting silently didn't.
function jumpTo(at) {
    const pane = state[state.focus];
    if (!pane || !pane.entries.length) return;
    pane.cursor = Math.max(0, Math.min(pane.entries.length - 1, at));
    draw(state.focus);
    if (visual.on) paintVisual();
    if (preview.on) showPreview();
}

async function clearMarksAndFilter() {
    const which = state.focus;
    if (state[which].filter) {
        const p = await ask('filter', { pane: which, text: '' });
        if (p) state[which] = p.pane ?? p;
    }
    if (state[which].marked > 0) {
        const p = await ask('unmarkall', { pane: which });
        if (p) state[which] = p;
    }
    draw(which);
    say('マークとフィルタを解除しました');
}

/// How many tiles sit on one visual row of the icon grid, measured off the
/// live layout — the grid is `auto-fill` and only the browser knows.
function iconCols() {
    const grid = el[state.focus].querySelector('.rows');
    const tiles = grid.children;
    if (tiles.length < 2) return 1;
    const top = tiles[0].offsetTop;
    let n = 1;
    while (n < tiles.length && tiles[n].offsetTop === top) n += 1;
    return n;
}

function move(delta) {
    const pane = state[state.focus];
    if (!pane || !pane.entries.length) return;
    // In the icon grid, a step up or down is a visual row, not a file — the
    // keys follow the eyes.
    if (viewMode === 'icons' && Math.abs(delta) === 1) delta *= iconCols();
    const last = pane.entries.length - 1;
    pane.cursor = Math.min(last, Math.max(0, pane.cursor + delta));
    draw(state.focus);
    if (visual.on) paintVisual();
    if (preview.on) showPreview();
}

async function enter() {
    const which = state.focus;
    const pane = state[which];
    if (!pane) return;
    const row = pane.entries[pane.cursor];
    if (!row) return;
    // Over the network the rows' paths are the server's, not this disk's —
    // opening one locally would look for a directory that is not here.
    if (pane.remote) {
        if (row.parent) { await remoteStep({ up: true }); return; }
        // A file opens — downloaded and read, Ctrl+S uploads it back. Enter
        // means "read it" on a server the same as it does on this disk.
        if (!row.is_dir) { await lookInside(); return; }
        await remoteStep({});
        return;
    }
    // Inside an archive, Enter on a file reads it — the same thing Enter
    // means on a file everywhere else. It used to fall through to the engine,
    // which refused with "まだ開けません" long after F3 could open it: the
    // message had outlived the limitation it described.
    if (pane.archive && !row.is_dir && !row.parent) {
        await lookInside();
        return;
    }
    // An archive is a directory you can walk into, which is what the terminal
    // build does with Enter — reading a zip as a list of names is `:lsar`, and
    // it is a different question.
    if (!row.is_dir && !row.parent && /\.(zip|tar|gz|tgz|7z|rar|jar)$/i.test(row.name)) {
        const r = await ask('enterarchive', { pane: which });
        if (!r) return;
        state[which] = r.pane;
        draw(which);
        say(`${r.archive.split(/[\\/]/).pop()} の中`);
        return;
    }
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
function askFor(head, initial = '', opts = {}) {
    const sheet = el.ask.querySelector('.sheet');
    el.ask.querySelector('.head').textContent = head;
    const body = el.ask.querySelector('.body');
    body.textContent = '';
    const input = document.createElement('input');
    // A password is never shown, and never pre-filled. cian has nowhere to
    // keep one that would be better than not keeping one, so it is asked for
    // each time and held only until the connection is made.
    input.type = opts.secret ? 'password' : 'text';
    input.value = opts.secret ? '' : initial;
    input.className = 'field';
    // A prompt inside the field, when the command it takes has one — `:` is
    // information about what you type, not about what the box is.
    if (opts.hint) input.placeholder = opts.hint;
    body.append(input);
    el.ask.hidden = false;
    input.focus();
    // The stem, not the suffix: renaming is nearly always about the name and
    // almost never about the `.txt`.
    const dot = opts.secret ? -1 : initial.lastIndexOf('.');
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
async function applySort(key) {
    const which = state.focus;
    const r = await ask('sort', { pane: which, key });
    if (!r) return;
    state[which] = r.pane;
    draw(which);
    say(`並び: ${r.by}${r.reverse ? ' ↓' : ' ↑'}`);
}

/// `/` narrows what is here. A second `/`, with nothing typed yet, looks
/// underneath instead — one slash for this listing, two for the tree. The
/// terminal build settled on that and it reads itself.
/// The prompt row at the foot, and which of the three things is typing into it.
///
/// `/` narrows the listing, `//` searches the tree below it, `:` takes a
/// command — three different questions, one place to type them, which is
/// where cian-tui puts all three (its prompt line, above the hints). The
/// command line used to raise a modal sheet in the middle of the window and
/// the finder a full-screen scrim over the very listing it was searching.
///
/// The colour says which: green for the two that search, purple for the one
/// that runs — cian-tui's own, because they take the same letters and the
/// only thing telling them apart is the frame.
const filter = { on: false, mode: null, resolve: null };

const PROMPT_SIGN = { filter: '/', find: '//', cmd: ':' };

function openPrompt(mode, seed = '', note = '') {
    filter.on = true;
    filter.mode = mode;
    el.fbar.dataset.mode = mode === 'cmd' ? 'cmd' : 'search';
    el.fSign.textContent = PROMPT_SIGN[mode];
    el.fbar.hidden = false;
    el.fInput.value = seed;
    el.fCount.textContent = note;
    el.fInput.focus();
    el.fInput.select();
    drawHints();
}

function closePrompt() {
    filter.on = false;
    filter.mode = null;
    el.fbar.hidden = true;
    el.fInput.blur();
    drawHints();
}

function startFilter() {
    // Seeded with what is already narrowing this pane, as the terminal build
    // seeds its box — reopening the filter to adjust it should not clear it.
    openPrompt('filter', state[state.focus]?.filter ?? '');
}

function endFilter(keep) {
    closePrompt();
    if (!keep) applyFilter('');
}

async function applyFilter(text) {
    const which = state.focus;
    const next = await ask('filter', { pane: which, text });
    if (!next) return;
    state[which] = next;
    draw(which);
    const n = next.entries.length;
    el.fCount.textContent = `${n} 件`;
    say(text ? `絞り込み: ${text} — ${n} 件` : `${n} 件`);
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
    el.findFoot.textContent = '探しています…';
    el.findHits.replaceChildren();
    // Typed at the foot like everything else; the sheet above holds the hits.
    openPrompt('find');
    // Asked for before the walk has found anything, on purpose: the picker is
    // usable from the first keystroke and the tree arrives underneath it.
    await ask('find', { pane: which });
    rankNow();
}

function closeFinder() {
    finder.open = false;
    el.find.hidden = true;
    if (filter.mode === 'find') closePrompt();
}

async function rankNow() {
    if (!finder.open) return;
    const r = await ask('rank', { query: el.fInput.value, limit: 200 });
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
/// The window's own three, hand-made for this window: 白磁 is the default,
/// 陰翳 its dark counterpart, 端末譲り the one that looks like the terminal
/// build. The eighteen named palettes cian-tui ships arrive beside them at
/// startup, from cian-core's table — one list, one key, and a theme chosen
/// here is the theme the terminal opens with.
const LOOKS = [
    ['', '白磁'],
    ['inei', '陰翳'],
    ['terminal', '端末譲り'],
];

/// The palettes from cian-core, once they have arrived.
const palettes = new Map();

/// A spec becomes CSS custom properties.
///
/// The eleven the window uses are derived from the seventeen a palette
/// publishes — and the arithmetic that had to match the terminal build's
/// (which ink reads on the accent, how far to pull a colour toward the page)
/// is done in the engine, so there is one answer rather than two.
function paintPalette(t) {
    const r = document.documentElement.style;
    const set = {
        '--bg': t.bg,
        '--pane': t.bg,
        '--pane-off': t.popup,
        '--line': t.border,
        '--text': t.fg,
        '--dim': t.dim,
        '--dir': t.blue,
        '--accent': t.accent,
        '--accent-dim': t.accent_dim,
        '--on-accent': t.on_accent,
        '--mark': t.mark,
        // The file-kind colours, mapped as the terminal build maps its
        // FilePalette from the same Spec (theme.rs from_spec): code=yellow,
        // config=cyan, document=doc, image=magenta, media=cyan, archive=red,
        // executable=green. Without these the 白磁 quiet tones stayed put
        // under every one of the eighteen palettes.
        '--k-code': t.yellow,
        '--k-config': t.cyan,
        '--k-doc': t.doc,
        '--k-image': t.magenta,
        '--k-media': t.cyan,
        '--k-archive': t.red,
        '--k-exec': t.green,
    };
    for (const [k, v] of Object.entries(set)) r.setProperty(k, v);
    document.documentElement.dataset.dark = t.light ? '' : '1';
}

function clearPalette() {
    const r = document.documentElement.style;
    for (const k of ['--bg', '--pane', '--pane-off', '--line', '--text', '--dim',
        '--dir', '--accent', '--accent-dim', '--on-accent', '--mark',
        '--k-code', '--k-config', '--k-doc', '--k-image', '--k-media',
        '--k-archive', '--k-exec']) r.removeProperty(k);
    delete document.documentElement.dataset.dark;
}

/// How the listing is laid out: the terminal build's `:view`, which it
/// could only ask for — ":view icons" in a terminal answers "window only",
/// because this is the feature a window exists to have.
const VIEWS = ['classic', 'details', 'icons'];
const VIEW_NAMES = { classic: 'クラシック', details: '詳細一覧', icons: 'アイコン' };
/// The two that take the whole window. Both are the Explorer arrangement,
/// where the listing is the thing you are looking at; classic keeps the two
/// panes, which is what cian is for.
const ONE_PANE = ['details', 'icons'];
let viewMode = 'classic';

function setView(mode, remember = true) {
    if (!VIEWS.includes(mode)) { say(`${mode}? — :view details | icons | classic`, true); return; }
    viewMode = mode;
    // Icons take the whole window; the other two keep the two panes. A wall
    // of tiles split down the middle is two narrow columns of icons, which is
    // not what either half of that arrangement is for.
    el.panes.classList.toggle('one', ONE_PANE.includes(mode));
    // `draw` already paints `active` on the focused pane, which is what
    // decides who is at the front here — so the two views need no second
    // notion of focus.
    draw('left');
    draw('right');
    if (remember) ask('remember', { key: 'gui_view', value: mode });
}

/// The lower-cased extension, or ''. Four functions asked this question with
/// the same regex on four lines — the audit's "same line four times" — and
/// four copies of one rule is how one of them starts answering differently.
function extOf(row) {
    return (row.name.match(/\.([a-z0-9]+)$/i) || [, ''])[1].toLowerCase();
}

/// What kind of thing this is, said in a word. Explorer's "種類" column —
/// which is more useful than the extension it is derived from, because the
/// extension is already right there in the name.
function kindOf(row) {
    if (row.parent) return '';
    if (row.is_dir) return 'フォルダー';
    const ext = extOf(row);
    if (!ext) return 'ファイル';
    const known = {
        md: 'Markdown', txt: 'テキスト', log: 'ログ', json: 'JSON', toml: 'TOML',
        yml: 'YAML', yaml: 'YAML', csv: 'CSV', tsv: 'TSV', xml: 'XML', html: 'HTML',
        css: 'CSS', js: 'JavaScript', ts: 'TypeScript', rs: 'Rust', py: 'Python',
        go: 'Go', c: 'C', h: 'C ヘッダ', cpp: 'C++', java: 'Java', lua: 'Lua',
        sh: 'シェル', bat: 'バッチ', ps1: 'PowerShell', sql: 'SQL',
        pdf: 'PDF', zip: 'ZIP', tar: 'TAR', gz: 'GZIP', '7z': '7-Zip', rar: 'RAR',
        png: 'PNG 画像', jpg: 'JPEG 画像', jpeg: 'JPEG 画像', gif: 'GIF 画像',
        webp: 'WebP 画像', svg: 'SVG 画像', bmp: 'BMP 画像', ico: 'アイコン',
        mp3: '音声', wav: '音声', flac: '音声', mp4: '動画', mov: '動画', mkv: '動画',
        xlsx: 'Excel', xls: 'Excel', docx: 'Word', doc: 'Word', pptx: 'PowerPoint',
        ttf: 'フォント', otf: 'フォント', woff2: 'フォント',
        exe: 'アプリケーション', dll: 'ライブラリ', so: 'ライブラリ', dylib: 'ライブラリ',
    };
    return known[ext] || `${ext.toUpperCase()} ファイル`;
}

/// What kind of file this is, as a row class — the terminal build's
/// `kind_for` (render.rs), extension for extension. The class picks the
/// name's colour from the palette; a dotfile recedes to muted.
function kindClassOf(row) {
    if (row.parent || row.is_dir) return '';
    if (row.name.startsWith('.')) return ' k-muted';
    const ext = extOf(row);
    if (/^(rs|py|js|mjs|cjs|ts|tsx|jsx|go|c|h|cpp|cc|cxx|hpp|java|rb|php|lua|swift|kt|kts|vue|svelte|html|htm|css|scss|sass|less)$/.test(ext)) return ' k-code';
    if (/^(toml|ini|conf|cfg|yaml|yml|json|jsonc|xml|env)$/.test(ext)) return ' k-config';
    if (/^(md|markdown|txt|log|pdf|docx?|xlsx?|pptx?|rtf|csv|tsv)$/.test(ext)) return ' k-doc';
    if (/^(png|jpe?g|gif|bmp|svg|webp|ico|tiff?)$/.test(ext)) return ' k-image';
    if (/^(mp3|wav|flac|ogg|m4a|aac|mp4|mov|mkv|avi|webm|wmv)$/.test(ext)) return ' k-media';
    if (/^(zip|tar|gz|7z|rar|bz2|xz|zst|tgz)$/.test(ext)) return ' k-archive';
    if (/^(exe|msi|bat|cmd|ps1|sh|bash|zsh|fish|app|dll|so|dylib)$/.test(ext)) return ' k-exec';
    return '';
}

/// The row's small leading icon — the terminal build's `icon_for`
/// (render.rs), codepoint for codepoint, drawn from the bundled Nerd font.
/// Written as escapes and copied from that table, not from memory: a glyph
/// remembered wrong renders as some other picture, silently. The emoji set
/// below stays for the icon tiles, where a big picture is the point.
function iconFor(row) {
    if (row.parent) return '\u{f062}';
    if (row.is_dir) {
        return {
            '.git': '\u{e702}', '.github': '\u{f408}', node_modules: '\u{e5fa}',
            src: '\u{f121}', tests: '\u{f0c3}', test: '\u{f0c3}',
            docs: '\u{f02d}', doc: '\u{f02d}',
            target: '\u{f1c6}', build: '\u{f1c6}', dist: '\u{f1c6}', out: '\u{f1c6}',
            '.vscode': '\u{e7c5}', '.idea': '\u{e7c5}',
        }[row.name] ?? '\u{f07b}';
    }
    const whole = {
        'cargo.toml': '\u{e7a8}', 'cargo.lock': '\u{e7a8}',
        dockerfile: '\u{f308}', '.dockerignore': '\u{f308}',
        makefile: '\u{e779}', 'readme.md': '\u{f48a}', readme: '\u{f48a}',
        license: '\u{f02d}', 'license.md': '\u{f02d}',
        '.gitignore': '\u{f1d3}', '.gitattributes': '\u{f1d3}', '.gitmodules': '\u{f1d3}',
        '.env': '\u{f462}', '.env.local': '\u{f462}',
        'package.json': '\u{e60b}', 'package-lock.json': '\u{e60b}', 'yarn.lock': '\u{e60b}',
    }[row.name.toLowerCase()];
    if (whole) return whole;
    const ext = extOf(row);
    const map = {
        rs: '\u{e7a8}', py: '\u{e73c}',
        js: '\u{f2ee}', mjs: '\u{f2ee}', cjs: '\u{f2ee}',
        ts: '\u{e628}', tsx: '\u{e628}', jsx: '\u{e628}', go: '\u{e627}',
        c: '\u{e61e}', h: '\u{e61e}',
        cpp: '\u{e61d}', cc: '\u{e61d}', cxx: '\u{e61d}', hpp: '\u{e61d}',
        java: '\u{e738}', rb: '\u{e21e}', php: '\u{e608}', lua: '\u{e620}',
        swift: '\u{e755}', kt: '\u{e634}', kts: '\u{e634}',
        md: '\u{f48a}', markdown: '\u{f48a}',
        json: '\u{e60b}', jsonc: '\u{e60b}', yaml: '\u{f481}', yml: '\u{f481}',
        toml: '\u{f013}', ini: '\u{f013}', conf: '\u{f013}', cfg: '\u{f013}',
        xml: '\u{f72d}', html: '\u{f13b}', htm: '\u{f13b}',
        css: '\u{f13c}', scss: '\u{f13c}', sass: '\u{f13c}', less: '\u{f13c}',
        vue: '\u{fd42}', svelte: '\u{e697}',
        sh: '\u{f489}', bash: '\u{f489}', zsh: '\u{f489}', fish: '\u{f489}',
        png: '\u{f1c5}', jpg: '\u{f1c5}', jpeg: '\u{f1c5}', gif: '\u{f1c5}',
        bmp: '\u{f1c5}', svg: '\u{f1c5}', webp: '\u{f1c5}', ico: '\u{f1c5}',
        tif: '\u{f1c5}', tiff: '\u{f1c5}',
        mp3: '\u{f001}', wav: '\u{f001}', flac: '\u{f001}', ogg: '\u{f001}',
        m4a: '\u{f001}', aac: '\u{f001}',
        mp4: '\u{f03d}', mov: '\u{f03d}', mkv: '\u{f03d}', avi: '\u{f03d}',
        webm: '\u{f03d}', wmv: '\u{f03d}',
        pdf: '\u{f1c1}',
        zip: '\u{f1c6}', tar: '\u{f1c6}', gz: '\u{f1c6}', '7z': '\u{f1c6}',
        rar: '\u{f1c6}', bz2: '\u{f1c6}', xz: '\u{f1c6}',
        txt: '\u{f0f6}', log: '\u{f0f6}',
        exe: '\u{f013}', dll: '\u{f013}', so: '\u{f013}', dylib: '\u{f013}',
    };
    return map[ext] ?? '\u{f15c}';
}

/// What an icon tile shows for a file. Deliberately coarse: a dozen kinds a
/// glance can tell apart, not a catalogue. Anything unknown is a plain page,
/// which is honest — the name below it is the real information.
function glyphFor(row) {
    if (row.parent) return '↩';
    if (row.is_dir) return '📁';
    const ext = extOf(row);
    if (/^(png|jpe?g|gif|webp|bmp|svg|avif|ico)$/.test(ext)) return '🖼️';
    if (/^(zip|tar|gz|tgz|7z|rar|jar)$/.test(ext)) return '📦';
    if (/^(pdf)$/.test(ext)) return '📕';
    if (/^(md|txt|log)$/.test(ext)) return '📝';
    if (/^(rs|js|ts|py|lua|c|h|cpp|go|java|sh|bat|ps1|toml|ya?ml|json|html|css)$/.test(ext)) return '📜';
    if (/^(xlsx?|csv)$/.test(ext)) return '📊';
    if (/^(docx?|pptx?)$/.test(ext)) return '📄';
    if (/^(mp[34]|wav|mov|mkv|flac|m4a)$/.test(ext)) return '🎞️';
    return '📄';
}

/// A modified time, the way a listing shows one.
///
/// This year gets `MM-DD HH:MM`; anything older gets the year instead of the
/// clock — `ls -l` and Finder both do this, and for good reason. The year is
/// the same four digits on almost every row, so printing it everywhere spends
/// the width of the widest column saying the least. What the eye wants from
/// this column is "recently, or long ago".
function when(row) {
    if (!row.modified) return '';
    const d = new Date(row.modified * 1000);
    const p = (n) => String(n).padStart(2, '0');
    const md = `${p(d.getMonth() + 1)}-${p(d.getDate())}`;
    return d.getFullYear() === new Date().getFullYear()
        ? `${md} ${p(d.getHours())}:${p(d.getMinutes())}`
        : `${d.getFullYear()}-${md}`;
}

/// Which look is showing, and it *is* written down now.
///
/// The question was open for months because the answer looked expensive:
/// reading `init.lua` needs Lua, which needs a C compiler. It turned out the
/// terminal build already carries one — mlua, vendored, built green on
/// Windows every release — so the property being protected had been spent long
/// ago. The engine reads and writes the terminal build's own state file, so a
/// look chosen here is the look `cian` opens with, and the other way round.
let look = 0;

function setLook(i, remember = true) {
    look = (i + LOOKS.length) % LOOKS.length;
    const [value] = LOOKS[look];
    // At the look's own base size, follow the new look's base — 端末譲り is
    // 14px on purpose. An explicit Ctrl+= choice survives the switch.
    const wasBase = FONT.at === baseFont();
    clearPalette();
    if (value) document.documentElement.dataset.look = value;
    else delete document.documentElement.dataset.look;
    if (wasBase) setFont(baseFont(), false);
    if (viewer.ed) viewer.ed.updateOptions({ theme: editorTheme() });
    if (remember) ask('remember', { key: 'gui_look', value: LOOKS[look][0] || 'hakuji' });
}

/// One of the eighteen. Same key, same list, same file the terminal build
/// reads its own choice out of.
function setPalette(name, remember = true) {
    const t = palettes.get(name);
    if (!t) { say(`${name}? — :theme で一覧`, true); return; }
    delete document.documentElement.dataset.look;
    paintPalette(t);
    palette = name;
    if (viewer.ed) viewer.ed.updateOptions({ theme: editorTheme() });
    if (remember) {
        ask('remember', { key: 'theme', value: name });
        // Choosing a palette *is* choosing the window's ground. Left in
        // place, a `gui_look` of 陰翳 from last month silently overrode this
        // palette — and the terminal build's — on every startup after.
        look = 0;
        ask('remember', { key: 'gui_look', value: 'hakuji' });
    }
}

/// Which named palette is on, or null when one of the window's own looks is.
let palette = null;

/// What the keys do here, right now.
///
/// **Taken from cian-tui's own hint table, translated key for key.** The
/// terminal build carries this bar and the window did not, which is most of
/// why the two felt like different programs: cian tells you what you can
/// press, continuously, and a window that stayed silent made you remember
/// instead. Ordered by how often each is reached for, so a narrow window
/// drops from the end.
function hintsNow() {
    if (viewer.on) {
        if (style === 1) {
            return [['Ctrl+S', '保存'], ['Shift+←→', '選択'], ['Ctrl+C / V', 'コピー / 貼付'],
                ['Ctrl+F', '検索'], ['Esc ×3', '閉じる'], ['Shift+Enter', 'メニュー — キー操作切替']];
        }
        return [['Ctrl+S', '保存'], ['Esc', '編集終了'], ['/', '検索'], ['i', '編集'],
            ['v', '選択'], ['y', 'コピー'], ['d c y', '＋モーション'], [':q', '閉じる'],
            [':notepad', 'メモ帳ふうに'], ['?', 'キー一覧']];
    }
    if (term.on && term.focused) {
        // Dynamic, as the terminal build's shell hints are: ^C only while a
        // drag selection exists (otherwise it names the gesture that makes
        // one), the pane keys only while there is a second pane to go to.
        const sel = window.getSelection();
        const hasSel = sel && !sel.isCollapsed && el.sPanes.contains(sel.anchorNode);
        const split = el.sPanes.querySelectorAll('.sgrid').length > 1;
        return [['Esc', 'ファイル'],
            hasSel ? ['Ctrl+C', '選択をコピー'] : ['ドラッグ選択', '= コピー'],
            ...(split ? [['Shift+F1/F2', '前/次のペイン']] : []),
            ['F9', '新規タブ'], ['F10', 'タブを閉じる'], ['Shift+F8', '左右分割'],
            ['Shift+F9', '上下分割'],
            ...(split ? [['Shift+F10', '分割を閉じる']] : []),
            ['F12', 'ズーム']];
    }
    if (visual.on) {
        return [['j/k', '伸ばす'], ['a', '全選択'], ['gg/G', '先頭/末尾'],
            ['Enter', '確定'], ['Esc', '取消']];
    }
    if (filter.on) {
        if (filter.mode === 'cmd') return [['打つ', 'コマンド'], ['Enter', '実行'], ['Esc', 'やめる'], ['C', '一覧から選ぶ']];
        if (filter.mode === 'find') return [['打つ', '絞込'], ['↑↓', '選ぶ'], ['Enter', 'そこへ'], ['Esc', 'やめる']];
        return [['打つ', '絞込'], ['↑↓', 'カーソル'], ['Enter', '適用'], ['Esc', '解除'], ['/', 'この下を探す']];
    }
    const pane = state[state.focus];
    if (pane && pane.archive) {
        return [['Enter/l', '入る'], ['Bksp', '戻る'], ['F3', 'メンバー閲覧'],
            ['Space', 'マーク'], ['c', '展開 →'], ['?', 'ヘルプ']];
    }
    if (pane && pane.remote) {
        return [['Esc', '切断'], ['Space', 'マーク'], ['c', '転送'], ['r', 'リネーム'],
            ['d', '削除'], ['Enter', '開く'], ['?', 'ヘルプ']];
    }
    if (pane && pane.flat) {
        return [['b/Esc', '戻る'], ['Space', 'マーク'], ['/', '絞込'],
            ['Enter', '開く'], ['F3', '閲覧'], ['?', 'ヘルプ']];
    }
    return [['←→', 'ペイン'], ['Shift+J', 'シェル'], ['Space', 'マーク'], ['/', '絞込'],
        [',', '並替'], ['Shift+F', '検索'], ['Ctrl+F', 'grep'], ['b', 'ブランチ'],
        ['F3', '閲覧'], ['Shift+Enter', 'メニュー'], ['F1/F2', '前/次タブ'],
        ['F9', '新規タブ'], ['F10', 'タブを閉じる'], ['=', '差分'], ['?', 'ヘルプ']];
}

let hintsOn = true;

/// Tell the layout how tall the two fixed foot bars actually are.
///
/// Measured, not declared: their text is set at a size the person changes
/// with Ctrl+=, so a number in the stylesheet would be right until the first
/// press. A listing whose last row sits under the status bar is a row you
/// cannot see or reach.
function measureFoot() {
    const r = document.documentElement.style;
    r.setProperty('--status-h', `${el.status.offsetHeight}px`);
    r.setProperty('--hints-h', `${hintsOn ? el.hints.offsetHeight : 0}px`);
}

function drawHints() {
    el.hints.hidden = !hintsOn;
    if (!hintsOn) return;
    el.hints.replaceChildren(...hintsNow().map(([k, what]) => {
        const s = document.createElement('span');
        const b = document.createElement('b');
        b.textContent = k;
        s.append(b, document.createTextNode(what));
        return s;
    }));
    // A narrow window gives up hints from just before the end, so the last
    // one — `? ヘルプ`, the door to all the others — is never the one lost.
    // It used to clip from the right, which dropped exactly that one first.
    while (el.hints.scrollWidth > el.hints.clientWidth && el.hints.children.length > 2) {
        el.hints.children[el.hints.children.length - 2].remove();
    }
    measureFoot();
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
                label: 'キーヒント',
                value: hintsOn ? '出す' : '出さない',
                run: () => {
                    hintsOn = !hintsOn;
                    drawHints();
                    drawMenu();
                    ask('remember', { key: 'gui_hints', value: hintsOn ? '1' : '0' });
                },
            },
            {
                label: '隠しファイル',
                value: pane && pane.hidden_shown ? '表示' : '非表示',
                run: () => toggleHidden(),
            },
            // Put where it can be found. A view you can only leave by knowing
            // the words `:view classic` is a view you are stuck in — and
            // icons is the one that hides the listing you would have read the
            // help from.
            {
                label: '一覧の見せ方',
                value: VIEW_NAMES[viewMode],
                run: () => {
                    const next = VIEWS[(VIEWS.indexOf(viewMode) + 1) % VIEWS.length];
                    setView(next);
                    drawMenu();
                    say(`一覧: ${VIEW_NAMES[next]}`);
                },
            },
            {
                label: '配色',
                value: palette || LOOKS[look][1],
                // Opens the gallery rather than cycling: there are twenty-one
                // of them now, and stepping through twenty-one with one key is
                // not choosing, it is waiting.
                run: () => { closeMenu(); cmdTheme(); },
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
    at: () => SORTS.findIndex(([k]) => k === (state[state.focus]?.sort_key ?? 'name')),
    rows: () => SORTS.map(([k, label, letter]) => ({
        label,
        value: k === (state[state.focus]?.sort_key ?? 'name') ? '●' : letter,
        run: () => applySort(k),
    })),
    // The letters, so the picker is skippable once it is in the fingers —
    // the terminal build has the same four.
    letters: Object.fromEntries(SORTS.map(([k, , letter]) => [letter, () => applySort(k)])),
};

/// `M` — everything you can do to the row under the cursor.
///
/// Built fresh each time from what the row actually is, so a directory is not
/// offered "extract" and a plain file is not offered it either. The terminal
/// build's menu does the same, and it is the discoverable half of a program
/// whose other half is a hundred and forty keys.
/// The context menu, built the way cian-tui builds it.
///
/// **Taken from the terminal build's own tree, group for group.** It was
/// twelve flat items here and about a hundred in five zones there, which is
/// most of what "全然違う" meant: in the terminal, `M` is how you reach
/// everything cian does without remembering a key for it, and a short flat
/// list is not that. The zones are its zones — launchers, then the frequent
/// file operations, then the groups, then the OS, then quit — so items sit
/// where the hand already expects them.
///
/// A group with nothing in it is not offered: an entry that can only refuse
/// is worse than no entry.
function contextRows() {
    const pane = state[state.focus];
    const row = pane && pane.entries[pane.cursor];
    const has = row && !row.parent;
    const inShell = term.on && term.focused;
    const v = [];

    // ── launchers ──
    v.push(group('AI ▸', aiRows));
    v.push({ label: '保存したコマンド', value: 'Ctrl+Shift+Enter', run: cmdSnippets });
    v.push({ label: 'マクロ', value: '@', run: cmdMacros });
    v.push({ label: 'ブックマーク', value: 's', run: cmdShortcuts });
    v.push({ label: 'コマンド入力', value: ':', run: () => commandLine() });

    if (inShell) {
        // The shell's own menu: what can be done to a terminal, not to a file.
        v.push({ label: '貼り付け（シェルへ）', value: 'Ctrl+V', run: () => document.execCommand('paste') });
        v.push(group('セッション ▸', () => [
            { label: '記録を取る／止める', value: ':sessionlog', run: cmdShellLog },
            { label: '文字コード', value: ':enc', run: () => cmdEncoding() },
        ]));
        v.push(group('ウィンドウ ▸', () => [
            { label: '左右に分割', value: 'Shift+F8', run: () => splitShell(false) },
            { label: '上下に分割', value: 'Shift+F9', run: () => splitShell(true) },
            { label: '新しいタブ', value: 'F9', run: shellTab },
            { label: '分割を閉じる', value: 'Shift+F10', run: () => closePane() },
            { label: 'ズーム', value: 'F12', run: zoomFocused },
            { label: 'このペインだけ', value: 'Shift+F12', run: () => ask('shellpanezoom', {}).then((r) => r && takeShell(r)) },
        ]));
        v.push({ label: 'このシェルに名前を付ける', value: ':shellname', run: cmdShellName });
        v.push({ label: '全ペインに同時入力', value: 'Ctrl+S', run: cmdSync });
        v.push({ label: '閉じる', value: 'Esc', run: () => { setShellFocus(false); say('ファイル'); } });
        v.push({ label: 'キー一覧', value: '?', run: openHelp });
        return v;
    }

    // ── the frequent file operations ──
    if (has) {
        v.push({ label: '開く', value: 'Enter', run: enter });
        v.push({ label: 'コピー（保持）', value: 'Ctrl+C', run: () => hold('copy') });
        v.push({ label: '切り取り（保持）', value: 'Ctrl+X', run: () => hold('cut') });
        v.push({ label: 'ここに貼り付け', value: 'Ctrl+V', run: paste });
        v.push({ label: 'パスをコピー', value: 'p', run: copyPaths });
        v.push({ label: 'ファイルとしてコピー', value: 'P', run: clipFiles });
        v.push({ label: '名前を変える', value: 'r', run: rename });
        v.push({ label: '削除（ゴミ箱へ）', value: 'd', run: () => operate('delete') });
        v.push({ label: '新しいタブで開く', value: 't', run: tabNew });
        v.push(group('ファイル ▸', () => [
            { label: '反対ペインへコピー', value: 'c', run: () => operate('copy') },
            { label: '反対ペインへ移動', value: 'm', run: () => operate('move') },
            { label: '行き先を指定してコピー', value: ':cp', run: () => commandLine('cp ') },
            { label: 'まとめてリネーム', value: ':renamelist', run: cmdRenameList },
        ]));
        v.push(group('圧縮・展開 ▸', () => {
            const rows = [
                { label: 'zip にまとめる', value: ':zip', run: () => cmdCompress('zip') },
                { label: 'パスワード付き zip', value: ':zipenc', run: () => cmdCompress('zipenc') },
                { label: 'tar.gz にまとめる', value: ':targz', run: () => cmdCompress('targz') },
            ];
            if (isArchive(row)) {
                rows.push({ label: '中身を見る', value: ':lsar', run: cmdArchiveList });
                rows.push({ label: 'ここに展開', value: ':unzip', run: cmdExtract });
            }
            return rows;
        }));
    }

    v.push(group('調べる ▸', () => [
        { label: '属性', value: ':attr', run: cmdAttr },
        { label: 'チェックサム', value: ':hash', run: () => cmdHash('') },
        { label: '差分をとる', value: '=', run: cmdCompare },
        { label: 'ファイル数と行数', value: ':count', run: cmdCount },
        { label: '容量分析', value: ':du', run: cmdDu },
        { label: '重複を探す', value: ':dup', run: cmdDedup },
    ]));
    v.push(group('git ▸', () => [
        { label: 'ステージ', value: ':stage', run: () => cmdVcs('stage') },
        { label: 'ステージ解除', value: ':unstage', run: () => cmdVcs('unstage') },
        { label: '変更を破棄', value: ':discard', run: () => cmdVcs('discard') },
        { label: '差分', value: ':gitdiff', run: () => cmdVcsDiff(null) },
        { label: 'このファイルの履歴', value: ':filelog', run: () => cmdLog(true) },
        { label: 'ブランチ', value: 'b', run: cmdBranch },
    ]));
    v.push(group('svn ▸', () => [
        { label: '追加', value: ':svnadd', run: () => cmdSvn('stage') },
        { label: '取り消す', value: ':svnrevert', run: () => cmdSvn('discard') },
        { label: '衝突を解決', value: ':svnresolve', run: () => cmdSvn('resolve') },
        { label: '差分', value: ':svndiff', run: () => cmdVcsDiff('svn') },
        { label: '履歴', value: ':svnlog', run: () => cmdLog(false, 'svn') },
        { label: '更新', value: ':svnupdate', run: () => cmdSvn('update') },
        { label: 'コミット', value: ':svncommit', run: () => cmdSvn('commit') },
    ]));
    v.push({ label: 'サーバへつなぐ', value: 'Shift+S', run: cmdSshPicker });
    v.push({ label: '動いている処理', value: ':queue', run: cmdQueue });
    v.push(group('表示 ▸', () => [
        { label: 'クラシック', value: ':view classic', run: () => { setView('classic'); say('一覧: クラシック'); } },
        { label: '詳細一覧', value: ':view details', run: () => { setView('details'); say('一覧: 詳細一覧'); } },
        { label: 'アイコン', value: ':view icons', run: () => { setView('icons'); say('一覧: アイコン'); } },
        { label: '隠しファイル', value: 'T', run: toggleHidden },
        { label: '配色', value: ':theme', run: cmdTheme },
        { label: 'トグル', value: 'T', run: () => openMenu(TOGGLES) },
    ]));
    v.push(group('OS ▸', () => [
        { label: '既定のアプリで開く', value: 'Ctrl+Enter', run: openOut },
        { label: '外部エディタで開く', value: ':edit', run: cmdEditExternal },
        { label: 'Finder で表示', value: ':revealos', run: cmdRevealOs },
    ]));
    v.push({ label: 'キー一覧', value: '?', run: openHelp });
    v.push({ label: '閉じる', value: ':q', run: cmdQuit });
    return v;
}

/// A row that opens a submenu instead of doing something.
///
/// `rows` is a function, not a list: what a group offers depends on what is
/// under the cursor at the moment it is opened, and a list built when the
/// parent was drawn would be a list about the file you were on then.
function group(label, rows) {
    return { label, value: '▸', group: rows };
}

function isArchive(row) {
    return row && !row.is_dir && /\.(zip|tar|gz|tgz|bz2|xz|7z|rar|jar)$/i.test(row.name);
}

function aiRows() {
    if (viewer.on) {
        return [
            { label: 'この文章について訊く', value: ':ai', run: () => cmdAiAsk('') },
            { label: 'コマンドの書き方', value: ':aicmd', run: () => cmdAiCmd('') },
            { label: 'このコードを直す', value: ':aifix', run: () => commandLine('ai この選択を直して ') },
        ];
    }
    if (term.on && term.focused) {
        return [
            { label: 'コマンドを作る', value: ':aicmd', run: () => cmdAiCmd('') },
            { label: '直近のエラーを説明', value: ':aierror', run: cmdAiError },
        ];
    }
    return [
        { label: '自由に訊く', value: ':ai', run: () => cmdAiAsk('') },
        { label: 'ログを診断', value: ':ailog', run: cmdAiLog },
        { label: '不要さがし', value: ':aijunk', run: () => cmdAiScan('aijunk') },
        { label: '畳み方の案', value: ':aistructure', run: () => cmdAiScan('aistructure') },
        { label: '意味で探す', value: ':aisearch', run: () => commandLine('aisearch ') },
        { label: '改名案', value: ':airename', run: () => commandLine('airename ') },
        { label: 'コミットメッセージ', value: ':aicommit', run: cmdAiCommit },
    ];
}

const CONTEXT = {
    key: 'M',
    foot: '↑↓ 選ぶ   Enter 実行   ← / Esc 戻る',
    stay: false,
    rows: contextRows,
};

/// One menu driver, not one per menu.
///
/// The switches and the sort picker are the same object with different rows,
/// and a third near-copy of "draw a list, move a cursor, run the row" is how
/// they would start behaving differently from each other.
const menu = { spec: null, at: 0 };

/// Where a submenu came from, so ← and Esc go back one level rather than
/// dropping you out of the menu entirely — which in a tree this size means
/// starting the whole search again.
const menuStack = [];

function openMenu(spec) {
    if (!spec.child) menuStack.length = 0;
    menu.spec = spec;
    menu.at = Math.max(0, spec.at ? spec.at() : 0);
    el.find.hidden = false;
    el.findFoot.textContent = spec.foot;
    drawMenu();
}

/// Do what a row says: open its submenu, or run it and close.
function runMenuRow(row, spec) {
    if (!row) return;
    if (row.group) {
        const rows = row.group();
        if (!rows.length) { say(`${row.label} — できることがありません`, true); return; }
        menuStack.push(spec);
        openMenu({
            key: spec.key,
            foot: '↑↓ 選ぶ   Enter 実行   ← / Esc 戻る',
            child: true,
            rows: () => rows,
        });
        return;
    }
    row.run();
    if (!spec.stay) closeMenu();
}

function closeMenu() {
    menuStack.length = 0;
    menu.spec = null;
    el.find.hidden = true;
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
            runMenuRow(row, menu.spec);
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
    if ((e.key === 'Escape' || e.key === 'ArrowLeft' || e.key === 'h') && menuStack.length) {
        // Up one level, not out. The terminal build's `Back` row, on the key
        // a vi user's hand is already on.
        openMenu(menuStack.pop());
    }
    else if (e.key === 'Escape' || e.key === spec.key) closeMenu();
    else if (e.key === 'ArrowDown' || e.key === 'j') { menu.at = (menu.at + 1) % rows.length; drawMenu(); }
    else if (e.key === 'ArrowUp' || e.key === 'k') { menu.at = (menu.at + rows.length - 1) % rows.length; drawMenu(); }
    else if (pick) { closeMenu(); pick(); }
    else if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowRight' || e.key === 'l') {
        runMenuRow(rows[menu.at], spec);
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
        ['Shift+D / Shift+U', '10行ずつ'],
        ['gg / G', '先頭 / 末尾'],
        ['Enter', 'ディレクトリへ入る / ファイルを読む / アーカイブの中へ'],
        ['アーカイブの中で F3', '中のファイルを読む・直す。Ctrl+S で書き戻す'],
        ['Ctrl+Enter', 'ディレクトリは反対ペインへ / ファイルは既定のアプリで'],
        ['Backspace', '親ディレクトリへ'],
        ['z', '入力したパスへ移動'],
        ['Tab', '反対のペインへ'],
        ['t / F9', '新しいタブ（いまの場所で開く）'],
        ['w / F10', 'タブを閉じる'],
        ['F1 / F2', '前 / 次のタブ（Shift+Tab でも次へ）'],
        ['← → / Ctrl+h / Ctrl+l', '左 / 右のペインにフォーカス'],
        ['Shift+H / Shift+L', '同じ（端末版と同じ綴り）'],
        ['F5', '読み直す'],
        [':view', '一覧の見せ方 — classic（2画面） / details（詳細一覧） / icons（アイコン）。T でも'],
        ['Ctrl+= / Ctrl+- / Ctrl+0', '文字を大きく / 小さく / 元に戻す'],
    ]],
    ['探す', [
        ['f  →  n / N', 'この一覧を検索・次・前'],
        ['/', 'この一覧を絞り込み'],
        ['/ /', 'この下のどこかにあるファイルをあいまい検索'],
        ['Shift+F', '名前で探す（この下すべて）── :find'],
        ['Ctrl+F / Ctrl+G', 'ファイルの中を探す（:grep）'],
        ['  結果で p', '一覧に読み込んで、いつものキーで操作する'],
        ['  結果で r', 'マッチした全ファイルを一括置換（1行ずつ確認）'],
        ['  Ctrl+N / Ctrl+Shift+N', 'ファイルを開いたまま次 / 前のヒットへ'],
        ['b', 'この配下を1ファイル1行に平坦化（b か Esc で戻る）'],
        ['h', 'このペインの履歴'],
        ['Z', '登録した場所と行った場所へ飛ぶ'],
        ['s', 'ショートカット（登録した場所）'],
        [':bookmark', 'いまの場所を登録する'],
        ['ドラッグして落とす', 'デスクトップからペインへ ── 移動します（先に確認）'],
        ['Alt+← / Alt+→', '前 / 先のディレクトリへ'],
        [',', 'ソート：名前／サイズ／日付／拡張子（n s d e で直接、同じキーで昇降反転）'],
        ['T', 'トグルメニュー：隠しファイル・一覧の見せ方・配色・エディタの流儀'],
    ]],
    ['コマンド', [
        [':', 'コマンドを打つ（:count :du :grep …）'],
        ['C', 'コマンド一覧をあいまい検索'],
        [':count', 'ファイル数とステップ数'],
        [':du', '容量分析 — 何が大きいか（Enter で中へ）'],
        [':attr / :chmod / :readonly', '属性を見る・変える'],
        [':hash', 'チェックサム（既定 sha256、:hash md5 も）'],
        ['=  /  :diff', '左右を比較 — ファイル同士は行差分、ディレクトリ同士は再帰'],
        ['  比較で Enter', '並べて開く — 左右とも編集でき、Ctrl+S で両方保存'],
        ['  F7 / Shift+F7', '次 / 前の相違へ'],
        ['  > / <', 'ディレクトリ比較：そのエントリを反対側へコピー'],
        ['  c / w', '比較結果をクリップボードへ / ファイルに保存'],
        [':renamepattern', '一括リネーム {name}_{n3}.{ext}（先にプレビュー）'],
        [':renamelist', '名前の一覧を編集してリネーム（Ctrl+S で適用）'],
        [':zip / :tar / :targz', 'マークをアーカイブにまとめる'],
        [':unzip / :lsar', 'ここに展開 / 中身を見る'],
        [':log / :filelog', 'コミットログ / このファイルの履歴（git・svn）'],
        [':gitdiff', '選択ファイルの差分'],
        [':stage / :unstage / :discard', 'git add / reset / 変更の破棄'],
        [':svnupdate :svncommit :svnresolve', 'svn の3つ'],
        [':dup', '中身が同じファイルを探す（:duplicate でも）'],
        [':df / :wc / :stat', '空き容量 / 行・単語・バイト / 属性'],
        [':mark *.rs  :unmark *', 'ワイルドカードでマーク'],
        [':copyto / :moveto', '反対ペイン以外の場所へ'],
        [':edit', '外部エディタ（$EDITOR）で開く'],
        [':where', '設定ファイルがどこにあるか'],
        [':key', '押したキーをそのまま表示（効かないキーの調査に）'],
        [':reload', 'init.lua を読み直す'],
        [':office / :officelink', 'Office 文書のクラウド側を開く / .url を作る'],
    ]],
    ['サーバ（SFTP）', [
        ['Shift+S', 'SSHピッカー — init.lua の cian.ssh から選ぶ'],
        [':remote  /  :ssh', '手で打つなら — user@host[:port][:/path]'],
        ['Enter / Backspace', 'サーバの中を移動'],
        ['c', '反対ペインへ — 立っている側でアップロードか転送かが決まる'],
        ['a / A / r / d', 'サーバ上でも同じキー（削除はゴミ箱なし＝戻せません）'],
        ['Enter / F3', 'サーバのファイルを開く — Ctrl+S でサーバへ書き戻す'],
        ['Ctrl+V / ドロップ', 'ローカルのファイルをアップロード'],
        [':local', 'サーバを閉じてローカルへ戻る'],
        ['枠が変わります', 'サーバを表示しているペインは色の違う枠になります'],
    ]],
    ['AI（init.lua で設定したとき）', [
        [':aicmd 説明', 'コマンドを作ってシェルに置く ── 実行はしません'],
        [':ailog', '選択したログを診断（末尾を読みます）'],
        [':aijunk / :aistructure', '不要さがし / 畳み方の案（中身は送らない・実行前に全部見せる）'],
        [':airename 指示', '改名案（例 :airename snake_case に）'],
        [':aisearch 探しもの', '意味で探す'],
        [':aierror', 'シェルの直近のエラーを説明'],
        [':aicommit', 'ステージ済み差分からコミットメッセージ（Enter で署名）'],
        [':ime', 'vim のノーマルモードで IME を自動オフ（init.lua の cian.ime）'],
        [':ai 質問', '自由に訊く'],
        [':aidiff', '表示中の差分を説明する'],
    ]],
    ['シェル', [
        ['Shift+J  /  :shell', 'シェルパネル（下半分に出る）'],
        ['Esc', 'ファイルへ戻る（Esc 2回でシェルへ渡る）'],
        ['Shift+PgUp / PgDn', '流れた出力を遡る'],
        [':!コマンド', 'シェルで実行 — % 選択、%f ファイル、%d ディレクトリ'],
        ['Ctrl+Shift+Enter / :snip', '保存したコマンドを選んでシェルへ（cian.snippets）'],
        [':vi / :vim / :nvim', 'そのエディタを新しいシェルタブで開く'],
        [':each コマンド', 'マーク各ファイルに実行 — {} がパス'],
        ['F9 / F10', 'シェルのタブを開く / 閉じる（パネルにいるとき）'],
        ['F1 / F2', '前 / 次のシェルタブ'],
        ['Shift+F8 / Shift+F9', '左右 / 上下に分割'],
        ['Shift+F10', '分割したペインを閉じる'],
        ['Shift+F1 / Shift+F2', '前 / 次のペインへ'],
        ['F1〜F8', 'そのシェルタブへ直接'],
        ['Ctrl+Shift+矢印', '分割の境界を動かす'],
        ['Ctrl+S  /  :sync', '全ペインに同時入力（同じコマンドを4台へ）'],
        ['F12  /  :zoom', 'シェルパネルを広げる／戻す'],
        ['Shift+F12', 'いまのペインだけを表示／分割に戻す'],
        [':sessionlog', 'シェルの写しをファイルに取る（もう一度で止める）'],
        ['ドラッグで選択', '放した瞬間にクリップボードへ'],
        [':preview', 'カーソルのファイルを追って表示（もう一度で止める）'],
        ['@  /  :macro', 'マクロを実行 ── レイアウトどおりに分割して開きます'],
    ]],
    ['読み書き（F3・Enter）', [
        ['画像・PDF', 'F3 か Enter でそのまま表示（寸法も出ます）'],
        ['バイナリ', '16進で表示。i で編集 — 0-9 a-f で上書き、Ctrl+S 保存（.bak を残す）'],
        ['  上書きのみ', 'ずれないので、ファイルの大きさは変わりません'],
        ['Ctrl+S', '保存（元の文字コード・改行・BOM のまま）'],
        ['Esc ×3', '閉じる ── 3回連続（未保存なら3回目で確認）'],
        ['Backspace ×3', '同じ。vim 流儀でノーマルモードのときだけ'],
        ['F3', '1回で閉じる'],
        ['F3（マーク中）', 'マークした全部を開く'],
        ['F2 / Shift+F2', '次 / 前の開いているファイル'],
        ['Ctrl+Shift+O', '見出し一覧から飛ぶ（vim 流儀は :outline）'],
        ['Ctrl+Shift+B', '各行を最後に変えた人（vim 流儀は :blame、もう一度で消す）'],
        ['── 以下は vim 流儀のコマンド行から ──', 'T でエディタの流儀を vim に'],
        [':sort :rsort :uniq', '行をソート / 逆順 / 重複を落とす'],
        [':s/古い/新しい/g', '開いているファイルを置換'],
        [':han :zen', '全角ASCII→半角 / 半角カナ→全角'],
        [':expand :unexpand :reindent', 'タブ↔スペース、インデントを揃える'],
        [':lf :crlf', '改行コードを変える（保存時に反映）'],
        ['流儀', 'メモ帳流（既定）／ vim ── 一覧に戻って T のメニューの中'],
        ['  vim のとき', 'ノーマルモードで開く。:w 保存 :q 閉じる :wq 両方'],
        ['  % ', '対応する括弧へ（monaco-vim のもの）'],
        ['  ]] / [[', '次 / 前の見出しへ'],
        ['  za', '折り畳む・開く'],
        ['  :enc', '文字コードを変えて読み直す（引数なしで順に）'],
        ['  :ws / :ruler', '見えない文字 / 桁の目盛り'],
        ['Ctrl+E', 'Markdown を組んで表示 / ソースへ戻る（:render・vim は :preview）'],
        ['  :s/古い/新しい/g', 'このファイルを置換'],
        ['  :g/re/d  :v/re/d', '一致した行を削除 / 一致した行だけ残す'],
        ['  :combine [n][!]', '次の行を連結（! は空白なし）'],
        ['矩形', 'Alt+Shift+矢印 で選び、Alt+Shift+I/A/C/D で 左端/右端/置換/削除'],
        ['Ctrl+] / Ctrl+[', '見出し移動（メモ帳流でも使えます）'],
        ['  メモ帳流のとき', 'Ctrl+C/V/Z/F など Windows の手が効く'],
    ]],
    ['マークと操作', [
        ['Space', 'マーク切替して下へ'],
        ['Shift+Space', 'マーク切替して上へ'],
        ['v', 'ビジュアル選択（Enter 確定・Esc 取消）'],
        [':nobom', 'UTF-8 BOM を除去（UTF-16 は触らない）'],
        ['Ctrl+A', '全マーク（もう一度で解除）'],
        ['V', '全マークを反転'],
        ['c / m / d', '反対ペインへコピー / 移動 / 削除（ゴミ箱へ）'],
        ['Ctrl+C / Ctrl+X', 'ファイルを保持（コピー / 切り取り）'],
        ['Ctrl+V / y', '保持したファイルをここへ貼り付け'],
        ['r', 'リネーム'],
        ['a / A', '新規ファイル / 新規ディレクトリ'],
        ['p', 'パス文字列をクリップボードへ'],
        ['Shift+P', 'ファイルそのものをクリップボードへ（Finder/エクスプローラで貼れます）'],
        ['o / O', 'このペインを反対側へ / 反対側をここへ'],
        ['u / Ctrl+R', '取り消し / やり直し'],
        ['M / Shift+Enter / 右クリック', 'このエントリにできること'],
        ['Esc', 'マーク・フィルタ解除 → 実行中の操作を中止'],
        [':queue', '実行中の操作を見る — x で1つだけ止める'],
    ]],
];

const help = { on: false };

function openHelp() {
    help.on = true;
    el.find.hidden = false;
    el.find.classList.add('help');
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

async function goToPath(given) {
    const path = given || await askFor('移動先', state[state.focus].cwd);
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
    beginOp(r, r.kind, verb);
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
    const k = e.key;
    const mode = filter.mode;
    if (k === 'Escape') {
        if (mode === 'filter') { endFilter(false); say('絞り込みを解除'); }
        else if (mode === 'find') { closeFinder(); say('やめました'); }
        else { closePrompt(); say('やめました'); }
    }
    else if (k === 'Enter') {
        if (mode === 'filter') endFilter(true);
        else if (mode === 'find') goToHit();
        else { const line = el.fInput.value; closePrompt(); runTypedCommand(line); }
    }
    else if (k === '/' && mode === 'filter' && el.fInput.value === '') {
        // Two slashes: this listing was not it, so look underneath.
        endFilter(true);
        openFinder();
    }
    // The cursor still walks while the box is open — the terminal build's
    // filter mode does the same, and it is what makes "type three letters,
    // arrow down, Enter" one motion. In the finder the arrows walk the hits.
    else if (k === 'ArrowDown' || (e.ctrlKey && k === 'n')) {
        if (mode === 'find') { finder.at = Math.min(finder.rows.length - 1, finder.at + 1); drawHits(finder.rows.length); }
        else if (mode === 'filter') move(1);
        else return;
    }
    else if (k === 'ArrowUp' || (e.ctrlKey && k === 'p')) {
        if (mode === 'find') { finder.at = Math.max(0, finder.at - 1); drawHits(finder.rows.length); }
        else if (mode === 'filter') move(-1);
        else return;
    }
    else return;
    e.preventDefault();
}, true);

document.addEventListener('input', (e) => {
    if (!filter.on || e.target !== el.fInput) return;
    if (filter.mode === 'filter') applyFilter(el.fInput.value);
    else if (filter.mode === 'find') rankNow();
});

// The finder's keys are the prompt row's now — it types there like the other
// two, so a second handler for the same keystrokes would be a second answer.

// Each keystroke re-ranks. Not debounced: the answer comes from a pipe, and
// waiting on a timer to save a round trip that costs nothing would only make
// the picker feel slower than it is.


/// While the bar is up it owns the keyboard — two keys, the terminal build's
/// (keys.rs, the progress popup): Esc stops the work, `b` stops only the
/// screen. Registered before the listing's handler so neither reaches it.
document.addEventListener('keydown', (e) => {
    if (!running || prog.hidden) return;
    if (e.key === 'Escape') {
        e.stopPropagation();
        e.preventDefault();
        window.cian.call('cancel', { op: running.op });
        say('中止しています…');
        return;
    }
    if (e.key === 'b' || e.key === 'Enter') {
        e.stopPropagation();
        e.preventDefault();
        prog.hidden = true;
        drawProg();
        say('バックグラウンドで実行中 — :queue で管理');
        return;
    }
    // Everything else is swallowed rather than passed on. A `d` typed at a
    // bar is a `d` meant for the listing behind it, and that one deletes.
    e.stopPropagation();
    e.preventDefault();
}, true);

/// The 49 action names `cian.set_keymap` accepts, each pointing at what this
/// build already does for that key.
///
/// The terminal build resolves the same names out of the same init.lua; a
/// binding that worked in one and not the other would be two programs wearing
/// one name. `unbind` is the one that does nothing on purpose — it exists so
/// a key can be made to shadow its own default.
const ACTIONS = {
    cursor_down: () => move(1),
    cursor_up: () => move(-1),
    cursor_top: () => jumpTo(0),
    cursor_bottom: () => jumpTo(state[state.focus].entries.length - 1),
    page_up: () => move(-20),
    page_down: () => move(20),
    parent: () => parent(),
    enter: () => enter(),
    quit: () => cmdQuit(),
    search: () => searchHere(),
    search_next: () => hopHere(1),
    search_prev: () => hopHere(-1),
    history: () => cmdHistory(),
    shortcuts: () => cmdShortcuts(),
    copy: () => operate('copy'),
    move: () => operate('move'),
    paste: () => paste(),
    cut: () => hold('cut'),
    delete: () => operate('delete'),
    rename: () => rename(),
    new_file: () => create(false),
    new_dir: () => create(true),
    open_other: () => openOut(),
    open_other_tab: () => tabNew(),
    sync_from_other: () => syncPane(true),
    sync_to_other: () => syncPane(false),
    // One function, because Ctrl+Enter is one key with two answers here: a
    // folder to the other pane, a file to your own application. Both names
    // land on it, and which half happens is decided by what is under the
    // cursor — as it is in the terminal build.
    open_external: () => openOut(),
    copy_path: () => copyPaths(),
    copy_file_ref: () => clipFiles(),
    mark_down: () => mark(false),
    mark_up: () => mark(false, -1),
    invert_marks: () => invert(),
    select_all: () => mark(true),
    visual: () => startVisual(),
    command: () => commandLine(),
    filter: () => startFilter(),
    find_recursive: () => runCommand(findCommand('find'), ''),
    grep_recursive: () => runCommand(findCommand('grep'), ''),
    sort: () => openMenu(SORT_MENU),
    jump_path: () => goToPath(),
    view: () => lookInsideAll(),
    diff: () => cmdCompare(),
    refresh: () => reread(),
    menu: () => openMenu(CONTEXT),
    ssh: () => cmdSshPicker(),
    new_tab: () => tabNew(),
    close_tab: () => tabClose(),
    manual: () => openHelp(),
    unbind: () => {},
};

/// What init.lua bound, keyed the way a keydown arrives: "ctrl+alt+x".
const bound = new Map();

/// `cian.set_keymap("alt+g", …)` → the string a keydown makes.
///
/// The terminal build's own spec parser, in the terminal build's order:
/// modifiers before the key, `shift` folded into an upper-case letter rather
/// than carried as a flag — because that is what a terminal actually
/// delivers, and a window that disagreed would need its own documentation.
function keySpec(spec) {
    const parts = String(spec).trim().split('+');
    let key = parts.pop();
    if (!key || [...key].length !== 1) return null;
    let ctrl = false;
    let alt = false;
    for (const m of parts) {
        const w = m.trim().toLowerCase();
        if (w === 'ctrl' || w === 'control' || w === 'c') ctrl = true;
        else if (w === 'alt' || w === 'opt' || w === 'option' || w === 'meta' || w === 'm') alt = true;
        else if (w === 'shift' || w === 's') key = key.toUpperCase();
        else return null;
    }
    return (ctrl ? 'ctrl+' : '') + (alt ? 'alt+' : '') + key;
}

function pressSpec(e) {
    return (e.ctrlKey || e.metaKey ? 'ctrl+' : '') + (e.altKey ? 'alt+' : '') + e.key;
}

/// Take what init.lua bound. Names that are not actions and keys that are not
/// keys are said out loud — a binding that silently does nothing is worse than
/// no binding, because the person goes looking in the wrong place.
function applyKeymaps(list) {
    bound.clear();
    const bad = [];
    for (const { key, action } of list || []) {
        const spec = keySpec(key);
        if (!spec) { bad.push(`${key}（キーとして読めません）`); continue; }
        if (!ACTIONS[action]) { bad.push(`${action}（そんな動作はありません）`); continue; }
        bound.set(spec, action);
    }
    keymapErrors = bad;
}

/// What was wrong with the bindings, held until the opening line is out of
/// the way. A config error that scrolls past in 200ms is a config error
/// nobody sees, and a binding that silently does nothing sends the person
/// looking in the wrong place.
let keymapErrors = [];

document.addEventListener('keydown', (e) => {
    if (keyEcho.on) {
        // Swallowed, not just reported. The point of this mode is to try the
        // key that "does nothing" — and the first one anybody tries is a
        // chord, half of which cut, delete or overwrite. Showing Ctrl+X and
        // *also* cutting the file would be the worst possible answer.
        if (e.key === 'Escape') { toggleKeyEcho(); return; }
        e.stopPropagation();
        e.preventDefault();
        const bits = [
            e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Meta',
        ].filter(Boolean);
        say(`${[...bits, e.key].join('+')}   code=${e.code}   keyCode=${e.keyCode}`
            + '   — Esc で止める');
        return;
    }
    // Not while a file is open. The editor no longer stops every key on its
    // way past — it cannot, or its own bindings never fire — so the listing's
    // keys have to decline for themselves.
    if (viewer.on) return;
    // What init.lua bound comes before what cian ships: rebinding a key is
    // saying "not the default", and a default that still fired would make the
    // binding a suggestion.
    const mine = bound.get(pressSpec(e));
    if (mine) {
        e.preventDefault();
        e.stopPropagation();
        ACTIONS[mine]();
        return;
    }
    // cian's own keys first; anything not claimed here is left to Chromium,
    // which is what makes Ctrl+C and friends work without being written out.
    const k = e.key;
    // Every bare letter is guarded with !ctrl && !meta. Four chords in this
    // chain were dead because their plain letter matched first — and Ctrl+D
    // *deleted*, because `d` did not care about its modifiers. An unclaimed
    // chord now falls through to the report at the bottom instead of quietly
    // running the letter it happens to contain.
    const bare = !e.ctrlKey && !e.metaKey;
    // The dividers, and *first*: the plain arrows below carry no modifier
    // guard, so tested later this key would have moved the cursor instead —
    // the same shape as the four chords that were dead in this chain before.
    if (e.ctrlKey && e.shiftKey && k.startsWith('Arrow')) resizeSplit(k);
    else if (k === 'ArrowDown' || (k === 'j' && bare)) move(1);
    else if (k === 'ArrowUp' || (k === 'k' && bare)) move(-1);
    else if (k === 'PageDown') move(20);
    else if (k === 'PageUp') move(-20);
    else if (k === 'D' && bare) move(10);
    else if (k === 'U' && bare) move(-10);
    else if (k === 'G' && bare) jumpTo(state[state.focus].entries.length - 1);
    else if (k === 'g' && bare) {
        // `gg`, two keystrokes and therefore a small state machine — a lone
        // `g` means nothing here, as in vim.
        const now = performance.now();
        if (now - lastGG < 1000) { lastGG = 0; jumpTo(0); }
        else lastGG = now;
    }
    else if (k === ' ' && e.shiftKey && bare) mark(false, -1);
    else if (k === 'Home') jumpTo(0);
    else if (k === 'End') jumpTo(state[state.focus].entries.length - 1);
    // Shift+H / Shift+L cross the panes, as in the terminal build.
    else if (k === 'H' && bare) focusPane('left');
    else if (k === 'L' && bare) focusPane('right');
    else if (k === 'ArrowLeft' && !e.altKey && viewMode === 'icons') { const p = state[state.focus]; p.cursor = Math.max(0, p.cursor - 1); draw(state.focus); }
    else if (k === 'ArrowRight' && !e.altKey && viewMode === 'icons') { const p = state[state.focus]; p.cursor = Math.min(p.entries.length - 1, p.cursor + 1); draw(state.focus); }
    else if (k === 'ArrowLeft' && !e.altKey) focusPane('left');
    else if (k === 'h' && e.ctrlKey) focusPane('left');
    else if (k === 'ArrowRight' && !e.altKey) focusPane('right');
    else if (k === 'l' && e.ctrlKey) focusPane('right');
    // Shift+Tab before Tab, which swallowed it — the same shape as Enter below.
    else if (k === 'Tab' && e.shiftKey) goTab(state.focus, { step: 1 });
    else if (k === 'Tab') { state.focus = state.focus === 'left' ? 'right' : 'left'; draw('left'); draw('right'); }
    // The most-modified Enter first: Ctrl+Shift lands on the Ctrl arm if it
    // is tested second, and the snippet launcher was unreachable for it.
    else if (k === 'Enter' && (e.ctrlKey || e.metaKey) && e.shiftKey) cmdSnippets();
    else if (k === 'Enter' && (e.ctrlKey || e.metaKey)) openOut();
    // Before the plain Enter, which used to swallow it: the menu was written,
    // listed in the help, and never once opened from this key. A modified key
    // has to be tested before the key it modifies.
    else if (k === 'Enter' && e.shiftKey) openMenu(CONTEXT);
    // Visual selection first: Enter there means "keep these", and entering a
    // directory in the middle of choosing files is never what was meant.
    else if (k === 'Enter' && visual.on) endVisual(true);
    else if (k === 'Enter') enter();
    else if (k === 'Backspace' && state[state.focus].remote) remoteStep({ up: true });
    else if (k === 'Backspace') parent();
    else if (k === ' ' && bare) mark(false);
    else if (k === 'a' && (e.ctrlKey || e.metaKey)) mark(true);
    // `c` is `c` either way: whether it is a copy or an upload is decided by
    // which pane you are standing in, which the program already knows.
    else if (k === 'c' && !e.ctrlKey && !e.metaKey
             && (state.left.remote || state.right.remote)) transfer();
    else if (k === 'c' && !e.ctrlKey && !e.metaKey) operate('copy');
    else if (k === 'm' && bare) {
        // Moving across the network is a download that then deletes the
        // original, and nothing here does the second half yet. `c` copies;
        // saying so beats an error per file from a move that never could.
        if (state[state.focus].remote) say('サーバとの移動はまだです — c でコピーしてください', true);
        else operate('move');
    }
    else if (k === 'd' && bare) {
        if (state[state.focus].remote) remoteOp('delete'); else operate('delete');
    }
    else if (k === 'T' && bare) openMenu(TOGGLES);
    else if (k === 'M' && bare) openMenu(CONTEXT);
    else if (k === 'Z' && bare) cmdJump();
    else if (k === 's' && bare) cmdShortcuts();
    else if (k === 'S' && bare) cmdSshPicker();
    else if (k === '@' && bare) cmdMacros();
    else if (k === 'F12') zoomFocused();
    else if ((k === '=' || k === '+') && (e.ctrlKey || e.metaKey)) { setFont(FONT.at + 1); say(`文字の大きさ ${FONT.at}px`); }
    else if (k === '-' && (e.ctrlKey || e.metaKey)) { setFont(FONT.at - 1); say(`文字の大きさ ${FONT.at}px`); }
    else if (k === '0' && (e.ctrlKey || e.metaKey)) { setFont(baseFont()); say('文字の大きさを戻しました'); }
    else if ((k === 't' && bare) || k === 'F9') tabNew();
    else if ((k === 'w' && bare) || k === 'F10') tabClose();
    else if (k === 'F1') goTab(state.focus, { step: -1 });
    else if (k === 'F2') goTab(state.focus, { step: 1 });
    else if (k === 'J' && bare) { if (term.on) { setShellFocus(true); say('シェル'); } else openShell(); }
    else if (k === 'r' && bare) {
        if (state[state.focus].remote) remoteOp('rename'); else rename();
    }
    else if (k === 'a' && bare) {
        if (state[state.focus].remote) remoteOp('touch'); else create(false);
    }
    else if (k === 'A' && bare) {
        if (state[state.focus].remote) remoteOp('mkdir'); else create(true);
    }
    else if (k === 'u' && bare) undo();
    else if (k === 'V' && bare) invert();
    else if (k === 'v' && bare) startVisual();
    else if (k === 'o' && bare) syncPane(true);
    else if (k === 'O' && bare) syncPane(false);
    else if (k === 'z' && bare) goToPath();
    else if (k === 'P' && bare) clipFiles();
    else if (k === 'p' && bare) copyPaths();
    else if (k === 'F5') reread();
    else if (k === '?' && bare) openHelp();
    else if (k === 'F3') lookInsideAll();
    else if (k === ':' && bare) commandLine();
    else if (k === 'C' && bare) openPalette();
    // The modified ones first: `f` on its own would otherwise swallow Ctrl+F.
    else if ((k === 'f' || k === 'g') && (e.ctrlKey || e.metaKey)) runCommand(findCommand('grep'), '');
    else if (k === 'f' && bare) searchHere();
    else if (k === 'F' && bare) runCommand(findCommand('find'), '');
    else if (k === 'n' && bare) hopHere(1);
    else if (k === 'N' && bare) hopHere(-1);
    else if (k === 'b' && bare) cmdBranch();
    else if (k === '=' && bare) cmdCompare();
    else if ((k === 'r' || k === 'y') && (e.ctrlKey || e.metaKey)) redo();
    else if (k === 'h' && bare) cmdHistory();
    else if (k === 'ArrowLeft' && e.altKey) step('back');
    else if (k === 'ArrowRight' && e.altKey) step('forward');
    // The file clipboard holds *local* paths. A remote row's path names a
    // place on the server, and holding it would paste a path that exists
    // nowhere on this disk — quietly, later, somewhere else.
    else if ((k === 'c' || k === 'x') && (e.ctrlKey || e.metaKey) && state[state.focus].remote) {
        say('サーバ上のファイルはクリップボードに持てません — c で転送してください', true);
    }
    // Pasting into a server pane uploads what the register holds. The
    // register's paths never travel to the window — the engine owns both
    // halves of the gesture.
    else if (((k === 'v' && (e.ctrlKey || e.metaKey)) || (k === 'y' && bare)) && state[state.focus].remote) {
        uploadHeld();
    }
    else if (k === 'c' && (e.ctrlKey || e.metaKey)) hold('copy');
    else if (k === 'x' && (e.ctrlKey || e.metaKey)) hold('cut');
    else if ((k === 'v' && (e.ctrlKey || e.metaKey)) || (k === 'y' && bare)) paste();
    else if (k === '/' && bare) startFilter();
    else if (k === ',' && bare) openMenu(SORT_MENU);
    // Esc backs out of whatever the listing is showing that is not a
    // directory. A branch view and a panelized search are both "here is a set
    // of files"; leaving them is the same gesture.
    else if (k === 'Escape' && visual.on) endVisual(false);
    // A preview is showing but the keys are here, so Esc has to reach it from
    // the listing — `viewer.on` is false precisely so that j and k still move.
    else if (k === 'Escape' && preview.on) togglePreview();
    else if (k === 'Escape' && state[state.focus] && state[state.focus].flat) leaveFlat();
    // Leaving a server is Esc, as the terminal build has it (":remote … Esc
    // leaves"). :local stays as the spoken form of the same thing.
    else if (k === 'Escape' && state[state.focus] && state[state.focus].remote) cmdDisconnect();
    // The terminal build's listing Esc: clear marks and filter. Both at once,
    // because "get me back to the plain listing" is one intention.
    else if (k === 'Escape' && state[state.focus]
             && (state[state.focus].marked > 0 || state[state.focus].filter)) clearMarksAndFilter();
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

// ─────────────────────────────────────────────────────────────────────────
// Anything that answers with a list.
//
// A search, disk usage, checksums, the history: one screen, not one per
// question. They differ in what Enter does and in nothing else, so the screen
// takes a `pick` and the rest is the same rows, the same j/k, the same Esc.
// Written the other way, cian's twenty-odd reports would be twenty-odd
// almost-identical lists, and they would stop agreeing within a month.
// ─────────────────────────────────────────────────────────────────────────
const report = { on: false, rows: [], at: 0, pick: null, act: null, move: null, leave: null };

/// Show a list. `rows` are `{ n, label, sub, path }` — `n` is the right-aligned
/// left column (a size, a line number, nothing), `label` the thing itself,
/// `sub` the dimmed remainder.
function show(title, about, rows, opts = {}) {
    report.on = true;
    report.rows = rows;
    // The unfiltered set, kept so narrowing is reversible by backspacing —
    // a filter that discards what it hides can only ever be typed forwards.
    report.all = rows;
    report.about = about;
    report.query = !!opts.filter;
    report.at = 0;
    report.pick = opts.pick || null;
    report.act = opts.act || null;
    // Called as the cursor passes, not on Enter. For a list whose rows *are*
    // the thing — the palettes — where reading a name is no substitute for
    // seeing it.
    report.move = opts.move || null;
    // Called when the list is dismissed rather than chosen from — for a list
    // that has been changing things while you looked at it.
    report.leave = opts.leave || null;
    el.rName.textContent = title;
    el.rAbout.textContent = about;
    el.rFoot.textContent = opts.foot
        || (report.query ? '打って絞る   ↑↓ 選ぶ   Enter 開く   Esc 閉じる'
            : rows.length ? '↑↓ 選ぶ   Enter 開く   Esc 閉じる' : 'Esc 閉じる');
    el.rQ.hidden = !report.query;
    el.rQ.value = '';
    el.rQ.placeholder = opts.hint || '打って絞り込み';
    el.report.hidden = false;
    drawReport();
    if (report.query) el.rQ.focus();
}

/// Narrow the list to what was typed.
///
/// A plain case-insensitive substring, over the label and whatever is beside
/// it. Deliberately *not* fuzzy: the file finder's ranking lives in Rust so
/// there is only one of it, and a second matcher written here would drift
/// from it within a month. These lists are a hundred-odd known names, where
/// "contains what I typed" is both predictable and enough.
function filterReport() {
    const q = el.rQ.value.trim().toLowerCase();
    report.rows = q
        ? report.all.filter((r) => `${r.label} ${r.sub || ''}`.toLowerCase().includes(q))
        : report.all;
    report.at = 0;
    el.rAbout.textContent = q
        ? `${report.rows.length} / ${report.all.length} 件`
        : report.about;
    drawReport();
    // The preview follows the narrowing, not just the arrows: with the
    // palettes, the top row of what you have typed *is* the answer you are
    // looking at.
    if (report.move && report.rows[report.at]) report.move(report.rows[report.at]);
}

function closeReport(abandoned = false) {
    if (abandoned && report.leave) report.leave();
    report.on = false;
    report.move = null;
    report.leave = null;
    report.rows = [];
    report.all = [];
    report.query = false;
    el.rQ.hidden = true;
    el.rQ.blur();
    el.report.hidden = true;
}

function drawReport() {
    const frag = document.createDocumentFragment();
    report.rows.forEach((row, i) => {
        const div = document.createElement('div');
        div.className = 'hit' + (i === report.at ? ' on' : '');
        if (row.n !== undefined && row.n !== null) {
            const n = document.createElement('span');
            n.className = 'n';
            n.textContent = row.n;
            div.append(n);
        }
        const l = document.createElement('span');
        l.className = 'p';
        l.textContent = row.label;
        div.append(l);
        if (row.sub) {
            const sub = document.createElement('span');
            sub.className = 'sub';
            sub.textContent = row.sub;
            div.append(sub);
        }
        div.addEventListener('mousedown', () => {
            report.at = i;
            drawReport();
            if (report.pick) report.pick(row);
        });
        frag.append(div);
    });
    el.rRows.replaceChildren(frag);
    const on = el.rRows.children[report.at];
    if (on) on.scrollIntoView({ block: 'nearest' });
}

document.addEventListener('keydown', (e) => {
    if (!report.on) return;
    e.stopPropagation();
    const last = report.rows.length - 1;
    const go = (to) => {
        report.at = Math.max(0, Math.min(last, to));
        drawReport();
        if (report.move && report.rows[report.at]) report.move(report.rows[report.at]);
    };
    const k = e.key;
    const ctrl = e.ctrlKey || e.metaKey;
    // What means the same thing whether or not there is a box to type in.
    // Ctrl+n / Ctrl+p are here because with a filter the letters are text,
    // and the terminal build's palette takes exactly these (keys.rs:813).
    if (k === 'Escape') closeReport(true);
    else if (k === 'ArrowDown' || (ctrl && k === 'n')) go(report.at + 1);
    else if (k === 'ArrowUp' || (ctrl && k === 'p')) go(report.at - 1);
    else if (k === 'PageDown') go(report.at + 20);
    else if (k === 'PageUp') go(report.at - 20);
    else if (k === 'Enter' && report.pick && report.rows[report.at]) report.pick(report.rows[report.at]);
    else if (report.query) {
        // Everything else is text. Not swallowed and not acted on: the box
        // has the focus and the character belongs to it.
        return;
    }
    else if (k === 'q') closeReport(true);
    else if (k === 'j') go(report.at + 1);
    else if (k === 'k') go(report.at - 1);
    else if (k === 'g') go(0);
    else if (k === 'G') go(last);
    else if (report.act && report.act[k]) report.act[k]();
    else return;
    e.preventDefault();
}, true);

document.addEventListener('input', (e) => {
    if (report.on && report.query && e.target === el.rQ) filterReport();
});

/// Bytes, the way a person reads them.
function human(n) {
    if (n < 1024) return `${n} B`;
    const u = ['KB', 'MB', 'GB', 'TB'];
    let v = n / 1024;
    let i = 0;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i += 1; }
    return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${u[i]}`;
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
/// The files open in the viewer, and which one is showing.
///
/// More than one because the answer to "which of these has the error" is found
/// by opening several and stepping between them, and closing one to look at
/// the next loses your place in the first.
const openFiles = { list: [], at: 0 };

const viewer = {
    on: false, opening: false, ed: null, vim: null,
    name: '', about: '', dirty: false, readOnly: false,
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

/// Every look is one of two grounds. Monaco ships a light and a dark theme, and
/// the editor sitting in the wrong one is the sort of thing that reads as
/// broken rather than as unstyled.
function editorTheme() {
    if (palette) return palettes.get(palette).light ? 'vs' : 'vs-dark';
    return LOOKS[look][0] === 'inei' || LOOKS[look][0] === 'terminal' ? 'vs-dark' : 'vs';
}

/// Is the window dark right now? Asked by mermaid and the preview, which have
/// to choose a diagram theme and a code theme before they draw.
function isDark() {
    if (palette) return !palettes.get(palette).light;
    return LOOKS[look][0] === 'inei' || LOOKS[look][0] === 'terminal';
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
        // Inside an archive the row names nothing on this disk, so the member
        // is extracted first and read from there.
        if (pane.remote) {
            await openRemoteMember(which);
        } else if (pane.archive) {
            await openArchiveMember(which);
        } else {
            await openInEditor(which);
        }
    } finally {
        viewer.opening = false;
    }
}

/// Something the window can draw but not read: a picture, a PDF.
///
/// Tried before the text read, not after it fails. `read_text` refuses a PNG
/// with "looks binary", which is true and unhelpful — the answer to opening a
/// picture is the picture.
async function openAsPicture(which) {
    const r = await ask('bytes', { pane: which });
    if (!r || !r.kind) return false;
    viewer.on = true;
    viewer.name = r.name;
    el.view.hidden = false;
    el.vBody.hidden = true;
    el.vPic.hidden = false;
    const node = document.createElement(r.kind === 'application/pdf' ? 'embed' : 'img');
    node.src = `data:${r.kind};base64,${r.b64}`;
    if (node.tagName === 'EMBED') { node.type = r.kind; node.style.cssText = 'width:100%;height:100%'; }
    node.addEventListener('load', () => {
        el.vAbout.textContent = node.naturalWidth
            ? `${node.naturalWidth} × ${node.naturalHeight}   ${human(r.len)}`
            : human(r.len);
    });
    el.vPic.replaceChildren(node);
    el.vName.textContent = r.name;
    el.vAbout.textContent = human(r.len);
    el.vFoot.textContent = 'Esc ×3 閉じる';
    return true;
}

/// Make the editor once, or reuse it.
///
/// Extracted because two things open it now — a file, and the list of names
/// `:renamelist` edits — and the second was reaching it by opening a file and
/// closing it again, which worked exactly as badly as it sounds.
function makeEditor(monaco, text, lang) {
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
    // The outline needs a key of its own in here: `:` belongs to the editor
    // once a file is open, so the command line cannot reach it.
    viewer.ed.addCommand(
        monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyO,
        () => cmdOutline(),
    );
    viewer.ed.addCommand(
        monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyB,
        () => cmdBlame(),
    );
    // The rectangle's verbs. Monaco selects one; these are what vim does to
    // it, and they are the reason to select one at all.
    viewer.ed.addCommand(monaco.KeyMod.Alt | monaco.KeyMod.Shift | monaco.KeyCode.KeyI,
        () => blockEdit('insert'));
    viewer.ed.addCommand(monaco.KeyMod.Alt | monaco.KeyMod.Shift | monaco.KeyCode.KeyA,
        () => blockEdit('append'));
    viewer.ed.addCommand(monaco.KeyMod.Alt | monaco.KeyMod.Shift | monaco.KeyCode.KeyC,
        () => blockEdit('replace'));
    viewer.ed.addCommand(monaco.KeyMod.Alt | monaco.KeyMod.Shift | monaco.KeyCode.KeyD,
        () => blockEdit('delete'));
    // The rendered document, and back. The terminal build's key for it.
    viewer.ed.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyE, () => togglePreview2());
    viewer.ed.onDidChangeCursorPosition(drawViewFoot);
    } else {
    viewer.ed.updateOptions({ theme: editorTheme() });
    monaco.editor.setModelLanguage(viewer.ed.getModel(), lang);
    viewer.ed.setValue(text);
    }

}

/// `F3` on a marked set opens all of them; `F2` and `Shift+F2` step between.
async function lookInsideAll() {
    const pane = state[state.focus];
    const marked = pane.entries.filter((x) => x.marked && !x.is_dir);
    if (marked.length < 2) { await lookInside(); return; }
    openFiles.list = marked.map((x) => x.path);
    openFiles.at = 0;
    await openNth(0);
    say(`${marked.length} 件を開きました（F2 / Shift+F2 で行き来）`);
}

async function openNth(at) {
    const n = openFiles.list.length;
    if (!n) return;
    openFiles.at = ((at % n) + n) % n;
    // The cursor has to move too: everything downstream — save, blame,
    // outline — asks the engine about "the selected file", and a viewer
    // showing one file while the engine holds another is the kind of
    // disagreement that writes to the wrong place.
    if (!await landOn(openFiles.list[openFiles.at])) return;
    if (viewer.on) await closeView(false);
    await lookInside();
    if (openFiles.list.length > 1) {
        el.vName.textContent = `[${openFiles.at + 1}/${openFiles.list.length}] ${viewer.name}`;
    }
}

/// Reading a file that only exists inside an archive.
///
/// It is extracted first, because everything downstream — the viewer, the
/// editor, the encoding switch — works on a path. Ctrl+S puts it back, and
/// the engine remembers which member it came from: a temporary file with no
/// idea where it came from is a file that can only be lost.
const member = { on: false };

/// A file on the server: downloaded, opened, and Ctrl+S uploads it back.
/// The same shape as an archive member, for the same reason — everything
/// downstream works on a path.
const remoteMember = { on: false };

async function openRemoteMember(which) {
    say('落としています…');
    const r = await ask('remoteview', { pane: which });
    if (!r) return false;
    remoteMember.on = true;
    const f = await ask('viewpath', { path: r.path });
    if (!f) { remoteMember.on = false; return false; }
    await showFile(f);
    el.vName.textContent = `${r.name}（サーバ上）`;
    el.vFoot.textContent = 'Ctrl+S でサーバへ書き戻す   ·   Esc ×3 閉じる';
    return true;
}

async function openArchiveMember(which) {
    const r = await ask('archiveview', { pane: which });
    if (!r) return false;
    member.on = true;
    member.writable = !!r.writable;
    const at = { path: r.path, name: r.name };
    // The listing is inside the archive, so the ordinary read cannot find the
    // file; it is opened from the temporary by path instead.
    const f = await ask('viewpath', { path: at.path });
    if (!f) { member.on = false; return false; }
    await showFile(f);
    el.vName.textContent = `${at.name}（アーカイブの中）`;
    el.vFoot.textContent = member.writable
        ? 'Ctrl+S でアーカイブに書き戻す   ·   Esc ×3 閉じる'
        : '読むだけ（tar への書き戻しはまだ）   ·   Esc ×3 閉じる';
    return true;
}

async function openInEditor(which) {
    const pane = state[which];
    const row = pane && pane.entries[pane.cursor];
    if (row && /\.(png|jpe?g|gif|webp|bmp|svg|avif|ico|pdf)$/i.test(row.name)) {
        if (await openAsPicture(which)) return;
    }
    const f = await ask('view', { pane: which });
    if (!f) return;
    await showFile(f);
}

/// Put a file the engine has read into the editor.
///
/// Split out because two things reach here now — a file in the listing,
/// and a member extracted from an archive — and the second was going to
/// need a copy of all of it.
async function showFile(f) {

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
        f.binary ? 'バイナリ（16進）' : (enc[f.encoding] || f.encoding),
        f.bom ? 'BOM' : null,
        f.binary ? null : f.eol.toUpperCase(),
        `${f.lines.length} 行`,
        human(f.bytes),
        f.truncated ? '※先頭のみ' : null,
    ].filter(Boolean).join('  ·  ');
    // A hex dump is a rendering of the file, not the file. Saving one back
    // would write the dump, so it opens read-only and says so.
    viewer.readOnly = !!f.binary;
    viewer.name = f.name;
    viewer.dirty = false;
    viewer.on = true;
    if (f.path) noteRecent(f.path, f.name);
    el.view.hidden = false;

    const text = f.lines.join('\n');
    const lang = MONACO_LANG[f.lang] || 'plaintext';
    makeEditor(monaco, text, lang);
    viewer.ed.updateOptions({ readOnly: viewer.readOnly });
    // After the text is in, not before: loading it is a change to the model,
    // and a file is not modified by having been opened.
    viewer.base = viewer.ed.getModel().getAlternativeVersionId();
    viewer.dirty = false;
    // A different file has different sections.
    sections = null;
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
function setStyle(i, remember = true) {
    style = (i + STYLES.length) % STYLES.length;
    if (remember) ask('remember', { key: 'gui_editor', value: STYLES[style][0] });
    if (!viewer.ed) return;
    if (viewer.vim) { viewer.vim.dispose(); viewer.vim = null; }
    // Cleared before vim takes the line. Otherwise the footer keeps whatever
    // was last written into it and vim's mode line appends to it — two status
    // lines in one, which is how it looked the first time.
    el.vFoot.textContent = '';
    if (STYLES[style][0] === 'vim') {
        // eslint-disable-next-line no-undef
        viewer.vim = MonacoVim.initVimMode(viewer.ed, el.vFoot);
        // The IME follows the mode: off when keys are commands, back when
        // they are text. monaco-vim announces every change, and syncIme reads
        // the mode back out of the footer — one rule for the whole window
        // rather than one for the editor and none for anywhere else.
        viewer.vim.on('vim-mode-change', () => queueMicrotask(syncIme));
        // `:w` and `:q` where the fingers put them. Without these, vim style
        // would still need Ctrl+S and Esc — which is exactly the seam that
        // makes a vim mode feel like a costume.
        // eslint-disable-next-line no-undef
        const ex = MonacoVim.VimMode.Vim;
        ex.defineEx('write', 'w', saveFile);
        ex.defineEx('quit', 'q', () => closeView(false));
        ex.defineEx('wq', 'wq', async () => { if (await saveFile()) closeView(false); });
        ex.defineEx('outline', 'outline', () => cmdOutline());
        ex.defineEx('blame', 'blame', () => cmdBlame());
        ex.defineEx('enc', 'enc', (_cm, params) => cmdEncoding((params.args || [])[0]));
        ex.defineEx('ws', 'ws', () => toggleWs());
        ex.defineEx('ruler', 'ruler', () => toggleRuler());
        ex.defineEx('preview', 'preview', () => togglePreview2());
        ex.defineEx('combine', 'combine', (_cm, p) => cmdCombine((p.args || []).join(' ') + (p.argString || '')));
        // The line operations, which until now could not be reached at all:
        // each needs a file open, and cian's own `:` belongs to the listing,
        // which declines every key while a file is open. They were in the
        // command table, in the help, and unreachable — found by measuring
        // the code for twins, not by anybody using it. Here is where a vim
        // user would look for them anyway.
        for (const op of ['sort', 'rsort', 'uniq', 'han', 'zen', 'expand', 'unexpand', 'reindent']) {
            ex.defineEx(op, op, () => textOp(op));
        }
        // `:s/old/new/g`. monaco-vim has its own substitute, but it does not
        // know cian's — the engine holds the same one the terminal build
        // uses, so the two builds agree on what a pattern means.
        ex.defineEx('subst', 's', (_cm, p) => cmdSubstitute('s' + (p.argString || '')));
        // `:g/re/d` and `:v/re/d`, spelled as vim spells them.
        ex.defineEx('global', 'g', (_cm, p) => runGlobal(p, false));
        ex.defineEx('vglobal', 'v', (_cm, p) => runGlobal(p, true));
        // `]]` and `[[`, which monaco-vim does not have. `%` it does — it is
        // `moveToMatchedSymbol` and it works; the first version of this
        // replaced it with a worse one, which is what comes of adding a
        // feature without checking whether it is already there.
        // eslint-disable-next-line no-undef
        const vim = MonacoVim.VimMode.Vim;
        vim.defineAction('cianNextSection', () => hopSection(1));
        vim.defineAction('cianPrevSection', () => hopSection(-1));
        vim.mapCommand(']]', 'action', 'cianNextSection', {}, { isJump: true });
        vim.mapCommand('[[', 'action', 'cianPrevSection', {}, { isJump: true });
        // Folding, which monaco-vim also leaves out. Monaco does the folding;
        // this is only the key.
        vim.defineAction('cianFold', () => viewer.ed.trigger('cian', 'editor.toggleFold'));
        vim.mapCommand('za', 'action', 'cianFold');
        vim.mapCommand('zA', 'action', 'cianFold');
    }
    // Sections, in both grammars: `]]` and `[[` walk the outline the way they
    // walk headings in vim.
    if (viewer.ed) {
        viewer.ed.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.BracketRight,
            () => hopSection(1));
        viewer.ed.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.BracketLeft,
            () => hopSection(-1));
    }
    drawViewFoot();
}

/// `]]` / `[[` — the next or previous section.
///
/// The outline decides what a section is, which is the same answer `:outline`
/// gives. Two ideas of "section" in one editor would be one too many.
let sections = null;

async function hopSection(step) {
    if (!viewer.on || !viewer.ed) return;
    if (!sections) {
        const r = await ask('outline', {});
        sections = r ? r.items.map((i) => i.line) : [];
    }
    if (!sections.length) { say('見出しがありません'); return; }
    const now = viewer.ed.getPosition().lineNumber - 1;
    const to = step > 0
        ? sections.find((n) => n > now)
        : [...sections].reverse().find((n) => n < now);
    if (to === undefined) { say(step > 0 ? '最後の見出しです' : '最初の見出しです'); return; }
    viewer.ed.setPosition({ lineNumber: to + 1, column: 1 });
    viewer.ed.revealLineInCenter(to + 1);
}

/// `:g/re/d` from vim's command line. Only `d` is supported as the action,
/// which is the one everybody means — `:g/re/s/…` is `:s` with a filter and
/// that is a different command.
function runGlobal(params, keep) {
    const raw = (params.argString || (params.args || []).join(' ') || '').trim();
    const m = raw.match(/^\/(.*)\/\s*d\s*$/);
    if (!m) { say(':g/正規表現/d の形で書いてください', true); return; }
    cmdLineFilter(m[1], keep);
}

function drawViewFoot() {
    // The list of names is not a file, so the footer must not offer to save
    // one — it applies a rename, and saying 保存 there would describe
    // something that is not about to happen.
    if (renameList.on) {
        el.vFoot.textContent = `${renameList.paths.length} 件   1行に1つ、順番は変えないこと`
            + '   ·   Ctrl+S 適用   Esc ×3 やめる';
        return;
    }
    if (viewer.readOnly) {
        el.vFoot.textContent = hex.editing
            ? `16進編集 — 0-9 a-f で上書き   ·   ${hex.at.toString(16).padStart(8, '0')} 番地`
              + `${hex.half ? '（下位けた待ち）' : ''}   ·   Ctrl+S 保存（.bak を残します）   Esc 戻る`
            : '16進表示 — i で編集   ·   Esc ×3 閉じる';
        return;
    }
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
    if (pair.on) { await savePair(); return true; }
    if (scratch.on) { return saveScratch(); }
    if (remoteMember.on) {
        say('サーバへ送っています…');
        const r = await ask('remotesave', { lines: viewer.ed.getValue().split(/\r?\n/) });
        if (!r) return false;
        viewer.base = viewer.ed.getModel().getAlternativeVersionId();
        viewer.dirty = false;
        drawViewFoot();
        say(`${r.saved} をサーバへ書き戻しました`);
        return true;
    }
    if (member.on) {
        if (!member.writable) { say('tar への書き戻しはまだです', true); return false; }
        const r = await ask('archivesave', { lines: viewer.ed.getValue().split(/\r?\n/) });
        if (!r) return false;
        viewer.base = viewer.ed.getModel().getAlternativeVersionId();
        viewer.dirty = false;
        drawViewFoot();
        say(`${r.saved} を ${r.archive} に書き戻しました`);
        return true;
    }
    if (viewer.readOnly) { say('16進表示は保存できません', true); return false; }
    // The editor is holding a list of names rather than a file's contents.
    if (renameList.on) {
        const ok = await applyRenameList();
        if (ok) { renameList.on = false; await closeView(false); }
        return ok;
    }
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
    renameList.on = false;
    stopHex();
    reading = false;
    el.vRead.hidden = true;
    el.vRead.replaceChildren();
    member.on = false;
    remoteMember.on = false;
    scratch.on = false;
    if (pair.ed) { pair.ed.dispose(); pair.ed = null; }
    pair.on = false;
    // Only when the door is being used, not when stepping between files.
    if (ask_first) openFiles.list = [];
    if (viewer.vim) { viewer.vim.dispose(); viewer.vim = null; }
    el.vPic.replaceChildren();
    el.vPic.hidden = true;
    el.vBody.hidden = false;
    el.view.hidden = true;
    el.status.focus?.();
}

/// Editing a binary, one byte at a time.
///
/// **Overwrite only.** Offsets never shift and the file cannot change size,
/// which is the difference between editing a binary and corrupting one: an
/// inserted byte moves everything after it, and in a binary those offsets are
/// usually written down inside the file itself.
///
/// Two hex digits make a byte, so the first one is remembered and shown as
/// pending rather than applied — half a byte written is a byte nobody meant.
const hex = { editing: false, at: 0, half: null };

function startHex() {
    if (!viewer.readOnly) return;
    hex.editing = true;
    hex.at = 0;
    hex.half = null;
    viewer.ed.updateOptions({ readOnly: true });
    markHexByte();
    drawViewFoot();
    say('16進編集 — 0-9 a-f で上書き、Ctrl+S で保存');
}

function stopHex() {
    hex.editing = false;
    hex.half = null;
    if (viewer.ed) viewer.ed.deltaDecorations(hexMark, []);
    hexMark = [];
    drawViewFoot();
}

let hexMark = [];

/// Show which byte the next digit lands on. A hex editor with no cursor is a
/// hex editor you overwrite the wrong byte with.
function markHexByte() {
    if (!viewer.ed) return;
    const line = Math.floor(hex.at / 16) + 1;
    // The dump is `oooooooo  xx xx …` — two hex digits per byte, a space
    // between, and an extra space after the eighth.
    const col = 11 + (hex.at % 16) * 3 + (hex.at % 16 >= 8 ? 1 : 0);
    hexMark = viewer.ed.deltaDecorations(hexMark, [{
        range: new (window.monaco.Range)(line, col, line, col + 2),
        options: { inlineClassName: 'hexcur' },
    }]);
    viewer.ed.revealLineInCenterIfOutsideViewport(line);
}

async function hexDigit(ch) {
    const v = parseInt(ch, 16);
    if (Number.isNaN(v)) return;
    if (hex.half === null) {
        hex.half = v;
        drawViewFoot();
        return;
    }
    const byte = (hex.half << 4) | v;
    hex.half = null;
    const r = await ask('hexset', { at: hex.at, byte });
    if (!r) return;
    // One line back, not the file: a dump of a large binary is a lot of text
    // to resend because two digits changed.
    // Through the model, not the editor: `executeEdits` is a no-op while the
    // editor is read-only, and the editor has to stay read-only or the digits
    // would be typed into the dump as text. The first version saved the right
    // bytes and showed the old ones.
    const model = viewer.ed.getModel();
    const line = r.line + 1;
    model.applyEdits([{
        range: new (window.monaco.Range)(line, 1, line, model.getLineMaxColumn(line)),
        text: r.text,
    }]);
    viewer.dirty = true;
    hex.at += 1;
    markHexByte();
    drawViewFoot();
}

async function saveHex() {
    const r = await ask('hexsave', {});
    if (!r) return;
    viewer.dirty = false;
    await reread();
    say(`${r.saved} を保存しました（元は ${r.backup} に残しました）`);
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
    // The hex editor owns its keys while it is on.
    if (hex.editing) {
        if (e.key === 'Escape') { e.stopPropagation(); e.preventDefault(); stopHex(); return; }
        if (e.key === 's' && (e.ctrlKey || e.metaKey)) {
            e.stopPropagation();
            e.preventDefault();
            saveHex();
            return;
        }
        if (/^[0-9a-fA-F]$/.test(e.key) && !e.ctrlKey && !e.metaKey) {
            e.stopPropagation();
            e.preventDefault();
            hexDigit(e.key);
            return;
        }
        // Moving between bytes, so a mistake is walked back to rather than
        // restarted from the top.
        const step = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: 16, ArrowUp: -16 }[e.key];
        if (step) {
            e.stopPropagation();
            e.preventDefault();
            hex.at = Math.max(0, hex.at + step);
            hex.half = null;
            markHexByte();
            drawViewFoot();
            return;
        }
    }
    if (viewer.readOnly && !hex.editing && e.key === 'i') {
        e.stopPropagation();
        e.preventDefault();
        startHex();
        return;
    }
    // Not while the question is up. Esc answers it — and counting those
    // presses toward another way out would mean declining to close three
    // times and being asked a fourth.
    if (!el.ask.hidden) { wayOut.key = null; wayOut.times = 0; return; }

    // F3 is nobody's editing key, so it is the one door that opens on a single
    // press. Esc and Backspace are both, which is why they take three.
    // Between the differences, when two files are side by side.
    if (e.key === 'F7' && pair.ed) {
        e.stopPropagation();
        e.preventDefault();
        pair.ed.trigger('cian', e.shiftKey ? 'editor.action.diffReview.prev' : 'editor.action.diffReview.next');
        return;
    }
    // Reading the rendered document: Esc goes back to the source rather than
    // out of the file, because "back one step" is what Esc means everywhere
    // else in here.
    if (reading && (e.key === 'Escape' || (e.key === 'e' && (e.ctrlKey || e.metaKey)))) {
        e.stopPropagation();
        e.preventDefault();
        togglePreview2();
        return;
    }
    if (e.key === 'F3') {
        e.stopPropagation();
        e.preventDefault();
        wayOut.key = null;
        wayOut.times = 0;
        closeView();
        return;
    }
    // Between the open files, when there is more than one.
    // The grep's hits, from inside the file one of them opened.
    if (e.key === 'n' && (e.ctrlKey || e.metaKey)) {
        e.stopPropagation();
        e.preventDefault();
        hopHit(e.shiftKey ? -1 : 1);
        return;
    }
    if ((e.key === 'F2') && openFiles.list.length > 1) {
        e.stopPropagation();
        e.preventDefault();
        openNth(openFiles.at + (e.shiftKey ? -1 : 1));
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

// ─────────────────────────────────────────────────────────────────────────
// The commands, and the two ways to reach them.
//
// `:` for the name you already know, `C` for fuzzy-finding the one you don't.
// Both run the same table, which is the point: a command added here gets a
// prompt, a palette entry and a help line without any of them being written.
// The terminal build reached the same arrangement, and for the same reason —
// it has far more commands than it has keys.
// ─────────────────────────────────────────────────────────────────────────
const COMMANDS = [
    { name: 'count', about: 'ファイル数と行数を数える', run: cmdCount },
    { name: 'du', alias: ['diskusage'], about: '容量分析 — 何が大きいか', run: cmdDu },
    { name: 'attr', about: '属性を見る', run: cmdAttr },
    { name: 'chmod', about: 'モードを変える（例 :chmod 644）', arg: 'モード', run: cmdChmod },
    { name: 'readonly', about: '読み取り専用にする / 解除（既定 on）', run: cmdReadonly },
    { name: 'hash', about: 'チェックサム（既定 sha256、:hash md5 も）', run: cmdHash },
    { name: 'find', about: '名前で探す（この下すべて）', arg: '名前', run: (a) => cmdSearch('name', a) },
    { name: 'grep', about: 'ファイルの中を探す（この下すべて）', arg: '文字列か /正規表現/', run: (a) => cmdSearch('content', a) },
    { name: 'branch', about: 'この配下を1ファイル1行に平坦化', run: cmdBranch },
    { name: 'diff', about: '左右を比較（= でも）', run: cmdCompare },
    { name: 'diffedit', about: '左右のファイルを並べて、どちらも編集できる形で開く', run: cmdDiffEdit },
    { name: 'renamepattern', about: '一括リネーム: {name}_{n3}.{ext}', arg: 'パターン', run: cmdRenamePattern },
    { name: 'zip', about: 'マークを zip に（:zip -e でパスワード付き）', arg: '-e', optional: true, run: (a) => cmdCompress('zip', /-e/.test(a || '')) },
    { name: 'tar', about: 'マークを tar にまとめる', run: () => cmdCompress('tar') },
    { name: 'targz', about: 'マークを tar.gz にまとめる', run: () => cmdCompress('targz') },
    { name: 'unzip', alias: ['extract'], about: 'カーソルのアーカイブをここに展開', run: cmdExtract },
    { name: 'lsar', about: 'アーカイブの中身を見る', run: cmdArchiveList },
    { name: 'log', about: 'コミットログ（git / svn）', run: () => cmdLog(false) },
    { name: 'filelog', about: 'このファイルの履歴', run: () => cmdLog(true) },
    { name: 'gitdiff', about: '選択ファイルの差分（git / svn）', run: () => cmdVcsDiff(null) },
    { name: 'stage', alias: ['add', 'svnadd'], about: 'git add', run: () => cmdVcs('stage') },
    { name: 'unstage', alias: ['reset'], about: 'git reset', run: () => cmdVcs('unstage') },
    { name: 'discard', alias: ['revert', 'svnrevert'], about: '作業ツリーの変更を破棄', run: () => cmdVcs('discard') },
    { name: 'dup', alias: ['duplicate', 'dedup'], about: '中身が同じファイルを探す', run: cmdDedup },
    { name: 'redo', about: 'u で取り消した操作をやり直す', run: redo },
    { name: 'image', about: '画像の表示方式（窓では常に描画されます）', run: () => say('窓では画像は常に表示されます — F3 でどうぞ') },
    // `finder` is NOT an alias here: it is `:files`'s, and a spelling that
    // lives on two commands reaches only the first — the fuzzy finder its own
    // about-text promised could never open. `:view finder` still works as an
    // argument (cmdView maps it to details).
    { name: 'view', alias: ['grid', 'icons', 'details', 'classic'], about: '一覧の見せ方 — :view details | icons | classic', arg: 'details / icons / classic', optional: true, run: cmdView },
    { name: 'shell', about: 'シェルパネルを開く（Shift+J でも）', run: openShell },
    { name: 'remote', alias: ['sftp'], about: 'このペインでサーバを開く（SFTP）', run: cmdConnect },

    { name: 'ssh', about: 'SSHピッカー（Shift+S でも）', run: cmdSshPicker },
    { name: 'paste', about: '保持したファイルをここへ貼り付け（Ctrl+V / y でも）', run: paste },
    { name: 'local', about: 'サーバを閉じてローカルへ戻る', run: cmdDisconnect },
    { name: 'aicmd', about: 'AI: 説明からシェルコマンドを作る', arg: 'やりたいこと', run: cmdAiCmd },
    { name: 'ailog', alias: ['logtriage', 'triage'], about: 'AI: 選択したログを診断する', run: cmdAiLog },
    { name: 'ai', alias: ['chat'], about: 'AI: 自由に訊く', arg: '訊きたいこと', run: cmdAiAsk },
    { name: 'aidiff', alias: ['explain', 'explaindiff'], about: 'AI: 表示中の差分を説明する', run: cmdAiDiff },
    { name: 'office', about: 'Office 文書のクラウド側を開く', run: () => cmdOffice('office') },
    { name: 'officelink', about: 'クラウド側への .url を作る（メールに貼るのはこれ）', run: () => cmdOffice('officelink') },
    { name: 'reload', about: 'init.lua を読み直す', run: cmdReload },
    { name: 'key', about: '受け取ったキーをそのまま表示（もう一度で止める）', run: toggleKeyEcho },
    { name: 'bookmark', about: 'いまの場所を登録する', arg: '名前', optional: true, run: cmdBookmark },
    { name: 'macro', about: 'マクロを実行（@ でも）', run: cmdMacros },
    { name: 'sync', alias: ['broadcast'], about: 'シェル: 全ペインに同時入力（Ctrl+S でも）', run: cmdSync },
    { name: 'snip', alias: ['snippet'], about: '保存したコマンドをシェルへ（Ctrl+Shift+Enter でも）', run: cmdSnippets },
    { name: 'sessionlog', alias: ['log2'], about: 'シェルの写しをファイルに取る／止める', run: cmdShellLog },
    { name: 'shellname', alias: ['tabname'], about: 'このシェルタブに名前を付ける（タブを二度押しでも）', arg: '名前', optional: true, run: cmdShellName },
    { name: 'zoom', about: 'いま操作している面を広げる／戻す（F12 でも）', run: zoomFocused },
    { name: 'df', about: 'ディスクの空き容量', run: cmdDf },
    { name: 'wc', about: '行／単語／バイト数', run: cmdWc },
    { name: 'head', about: '先頭だけ見る（:head -n 20）', arg: '-n 数', optional: true, run: (a) => cmdPeek(a, false) },
    { name: 'tail', about: '末尾だけ見る（:tail -n 20）', arg: '-n 数', optional: true, run: (a) => cmdPeek(a, true) },
    { name: 'recent', alias: ['oldfiles'], about: '最近開いたファイル', run: cmdRecent },
    { name: 'version', alias: ['about'], about: '版と居場所', run: cmdVersion },
    { name: 'man', about: 'キー一覧（:help と同じ）', run: openHelp },
    { name: 'goto', about: '入力したパスへ移動（:cd と同じ）', arg: 'パス', run: cmdCd },
    { name: 'jump', about: '登録した場所と履歴へ飛ぶ（Z でも）', run: cmdJump },
    { name: 'palette', about: 'コマンド一覧（C でも）', run: openPalette },
    { name: 'selectall', alias: ['markall'], about: '全部マーク（Ctrl+A でも）', run: () => mark(true) },
    { name: 'ren', alias: ['rename'], about: 'リネーム（r でも）', run: rename },
    { name: 'untar', about: 'ここに展開（:unzip と同じ）', run: cmdExtract },
    { name: 'gdiff', alias: ['svndiff'], about: '選択ファイルの差分（:gitdiff と同じ）', run: () => cmdVcsDiff(null) },
    { name: 'step', about: 'ファイル数とステップ数（:count と同じ）', run: cmdCount },
    { name: 'files', alias: ['finder'], about: 'この下のファイルをあいまい検索（// でも）', run: openFinder },
    { name: 'where', alias: ['config'], about: 'cian が読み書きする設定ファイルの場所', run: cmdWhere },
    { name: 'mark', about: 'ワイルドカードでマーク（:mark *.rs）', arg: 'パターン', run: (a) => cmdMarkGlob(a, true) },
    { name: 'unmark', alias: ['deselect'], about: 'ワイルドカードでマークを外す', arg: 'パターン', run: (a) => cmdMarkGlob(a, false) },
    { name: 'copyto', about: '指定した場所へコピー', arg: '行き先', run: (a) => cmdTo('copyto', a) },
    { name: 'moveto', about: '指定した場所へ移動', arg: '行き先', run: (a) => cmdTo('moveto', a) },
    { name: 'revealos', alias: ['showinfinder'], about: 'Finder / エクスプローラで表示', run: cmdRevealOs },
    { name: 'edit', alias: ['e'], about: '外部エディタで開く（$EDITOR）', run: cmdEditExternal },
    { name: 'vi', alias: ['vim', 'nvim'], about: 'そのエディタを新しいシェルタブで開く', run: cmdEditorTab },
    { name: 'editstyle', alias: ['notepad', 'vimkey'], about: 'エディタの流儀 — :editstyle vim / :notepad', arg: 'vim / notepad', optional: true, run: cmdEditStyle },
    { name: 'scratch', alias: ['new'], about: '下書きを開く（:w で名前を付けて保存）', run: cmdScratch },
    { name: 'limit', alias: ['speed', 'ratelimit'], about: '転送の速さの上限 — :limit 2m / 500k / off', arg: '2m / 500k / off', optional: true, run: cmdLimit },
    { name: 'aicommit', alias: ['commitmsg'], about: 'AI: ステージ済みの差分からコミットメッセージを作る', run: cmdAiCommit },
    { name: 'aijunk', alias: ['junk'], about: 'AI: 消してよさそうなものを探す（中身は送らない）', run: () => cmdAiScan('aijunk') },
    { name: 'aistructure', alias: ['organize', 'aiorganize'], about: 'AI: 散らかりを畳む案を作る（実行前に全部見せる）', run: () => cmdAiScan('aistructure') },
    { name: 'airename', about: 'AI: 指示どおりの改名案を作る（:airename snake_case に）', arg: 'どう変えるか', run: cmdAiRename },
    { name: 'aisearch', alias: ['ask', 'semsearch'], about: 'AI: 意味で探す（:aisearch 先月の請求書）', arg: '探しもの', run: cmdAiSearch },
    { name: 'aierror', about: 'AI: シェルの直近のエラーを説明する', run: cmdAiError },
    { name: 'ime', alias: ['inputmethod'], about: 'IME 連携 — vim のノーマルモードで自動オフ（cian.ime）', run: cmdIme },
    { name: 'stat', about: '属性（:attr と同じ）', run: cmdAttr },
    { name: 'blame', about: '各行を最後に変えた人（開いているファイル）', run: cmdBlame },
    { name: 'enc', about: '開いているファイルの文字コードを変えて読み直す', arg: 'utf8 / sjis / utf16le / utf16be', optional: true, run: cmdEncoding },
    { name: 'ws', about: 'タブ・行末の空白などを見せる／隠す', run: toggleWs },
    { name: 'ruler', about: '桁の目盛りを出す／消す', run: toggleRuler },
    { name: 's', about: '開いているファイルを置換 s/古い/新しい/g', arg: 's/…/…/', run: cmdSubstitute },
    { name: 'g', about: '一致した行を削除（:g/re/d）', arg: '正規表現', run: (a) => cmdLineFilter(a, false) },
    { name: 'v', about: '一致した行だけ残す（:v/re/d）', arg: '正規表現', run: (a) => cmdLineFilter(a, true) },
    { name: 'combine', about: '次の行を連結（:combine 3 で3行、:combine! は空白なし）', arg: '行数', optional: true, run: cmdCombine },
    { name: 'theme', alias: ['colorscheme', 'colourscheme'], about: '配色 21 種 — 選ぶだけで着せ替わります（T のメニューにも）', arg: '名前', optional: true, run: cmdTheme },
    { name: 'redraw', about: '画面を描き直す', run: () => { draw('left'); draw('right'); say('描き直しました'); } },
    { name: 'preview', about: 'カーソルのファイルを追って表示（もう一度で止める）', run: togglePreview },
    { name: 'render', alias: ['source'], about: 'Markdown を組んで表示（Ctrl+E でも）', run: togglePreview2 },
    { name: 'queue', about: '実行中の操作を見る・止める', run: cmdQueue },
    { name: 'tab', about: '新しいタブ（t / F9 でも）', run: () => tabNew() },
    { name: 'tabclose', about: 'タブを閉じる（w / F10 でも）', run: () => tabClose() },
    // The short ones the terminal build has, spelled the same way. A person who
    // knows `:mkdir -p` should not have to find out that this one is different.
    { name: 'mkdir', alias: ['md'], about: 'ディレクトリを作る（:mkdir -p a/b/c）', arg: '名前', run: cmdMkdir },
    { name: 'touch', about: 'ファイルを作る／時刻を更新', arg: '名前', run: cmdTouch },
    { name: 'cp', alias: ['copy'], about: 'コピー — 引数なしで反対ペインへ、:cp <行き先> でそこへ', arg: '行き先', optional: true, run: (a) => a ? cmdTo('copyto', a) : operate('copy') },
    { name: 'mv', alias: ['move'], about: '移動 — 引数なしで反対ペインへ、:mv <行き先> でそこへ', arg: '行き先', optional: true, run: (a) => a ? cmdTo('moveto', a) : operate('move') },
    { name: 'rm', alias: ['del', 'delete'], about: '削除（ゴミ箱へ）', run: () => operate('delete') },
    { name: 'pwd', about: 'いまの場所を表示してクリップボードへ', run: cmdPwd },
    { name: 'ls', alias: ['dir'], about: '読み直す（:ls -a で隠しファイル切替）', run: cmdLs },
    { name: 'q', alias: ['quit'], about: '閉じる（確認します）', run: cmdQuit },
    { name: 'each', about: 'マーク各ファイルにコマンド — {} がパス', arg: 'コマンド', run: cmdEach },
    { name: 'nobom', alias: ['stripbom'], about: 'UTF-8 BOM を除去（UTF-16 は触らない）', run: cmdNoBom },
    { name: 'renamelist', about: '名前の一覧を編集してリネーム', run: cmdRenameList },
    { name: 'outline', about: '開いているファイルの見出し一覧', run: cmdOutline },
    { name: 'sort', about: '開いているファイルの行をソート', run: () => textOp('sort') },
    { name: 'rsort', about: '行を逆順ソート', run: () => textOp('rsort') },
    { name: 'uniq', about: '重複行を落とす', run: () => textOp('uniq') },
    { name: 'han', about: '全角ASCII → 半角', run: () => textOp('han') },
    { name: 'zen', about: '半角カナ → 全角', run: () => textOp('zen') },
    { name: 'expand', about: '行頭のタブ → スペース', run: () => textOp('expand') },
    { name: 'unexpand', about: '行頭のスペース → タブ', run: () => textOp('unexpand') },
    { name: 'reindent', about: 'インデントを揃える', run: () => textOp('reindent') },
    { name: 'lf', about: '改行を LF にする', run: () => setEol('lf') },
    { name: 'crlf', about: '改行を CRLF にする', run: () => setEol('crlf') },
    { name: 'svnupdate', about: 'svn update', run: () => cmdSvn('update') },
    { name: 'svncommit', about: 'svn commit（メッセージを訊きます）', run: () => cmdSvn('commit') },
    { name: 'svnresolve', alias: ['resolve'], about: 'svn resolve --accept working', run: () => cmdSvn('resolve') },
    { name: 'visual', alias: ['select'], about: 'ビジュアル選択（v でも）', run: startVisual },
    { name: 'compare', about: '左右を比較（= でも）', run: cmdCompare },
    { name: 'back', about: 'ひとつ前のディレクトリへ', run: () => step('back') },
    { name: 'forward', about: 'ひとつ先のディレクトリへ', run: () => step('forward') },
    { name: 'history', about: 'このペインの履歴', run: cmdHistory },
    { name: 'cd', about: ':cd <パス> / :cd .. / :cd - / :cd ~', arg: 'パス', run: cmdCd },
    { name: 'hidden', about: '隠しファイルの表示切替', run: toggleHidden },
    { name: 'refresh', alias: ['rescan'], about: '読み直す', run: reread },
    { name: 'undo', about: '直前の操作を取り消す', run: undo },
    { name: 'menu', alias: ['toggle'], about: 'トグルメニュー', run: () => openMenu(TOGGLES) },
    { name: 'help', alias: ['h'], about: 'キー一覧', run: openHelp },
];

/// `:q` — with the question, as the terminal build asks it. A window's ✕
/// button exists, so anyone typing :q is a person whose hands close things
/// by keyboard — and a typo away from :w.
/// `:cd`, the four ways the terminal build spells it. `-` is the previous
/// directory — the pane's own history already remembers it, so it is `back`
/// by another name. `~` and relatives resolve in the engine, against the
/// pane rather than against wherever the engine process was started.
async function cmdCd(dest) {
    if (dest.trim() === '-') { await step('back'); return; }
    await goToPath(dest.trim());
}

async function cmdQuit() {
    if (await confirm('cian を閉じます', '')) window.close();
    else say('やめました');
}

async function cmdMkdir(spec) {
    // `-p a/b/c` makes the whole chain; without it, one directory here.
    const deep = /^-p\s+/.test(spec);
    const name = spec.replace(/^-p\s+/, '').trim();
    if (!name) { say('名前がありません', true); return; }
    const r = await ask('create', { pane: state.focus, name, dir: true, deep });
    if (!r) return;
    state[state.focus] = r.pane ?? r;
    draw(state.focus);
    say(`${name} を作りました`);
}

async function cmdTouch(name) {
    if (!name) { say('名前がありません', true); return; }
    const r = await ask('create', { pane: state.focus, name, dir: false, touch: true });
    if (!r) return;
    state[state.focus] = r.pane ?? r;
    draw(state.focus);
    say(`${name} を作りました`);
}

async function cmdPwd() {
    const cwd = state[state.focus].cwd;
    await navigator.clipboard.writeText(cwd);
    say(`${cwd} — クリップボードへ`);
}

async function cmdLs(arg) {
    if (/-a/.test(arg || '')) { await toggleHidden(); return; }
    await reread();
}

/// Where the grep results are, so the viewer can walk them.
///
/// Kept after the report closes: opening a hit and then stepping to the next
/// is the whole point of a grep, and it cannot be done from a screen that had
/// to be closed to open the file.
const hits = { list: [], at: -1, needle: '' };

/// `Ctrl+N` / `Ctrl+Shift+N` — the next or previous grep hit, opened and
/// scrolled to its line.
async function hopHit(step) {
    if (!hits.list.length) { say('grep の結果がありません', true); return; }
    hits.at = (hits.at + step + hits.list.length) % hits.list.length;
    const h = hits.list[hits.at];
    if (!await landOn(h.path)) return;
    if (viewer.on) await closeView(false);
    await lookInside();
    if (viewer.ed && h.line) {
        viewer.ed.setPosition({ lineNumber: h.line, column: 1 });
        viewer.ed.revealLineInCenter(h.line);
    }
    say(`${hits.needle}   ${hits.at + 1} / ${hits.list.length}   ${h.path.split(/[\\/]/).pop()}`);
}

/// `:queue` — what is running, and a way to stop one without stopping the
/// rest. A file manager copying ten thousand files should be able to say which
/// ten thousand.
async function cmdQueue() {
    const r = await ask('queue', {});
    if (!r) return;
    if (!r.jobs.length) { say('動いている操作はありません'); return; }
    const verb = { copy: 'コピー', move: '移動', delete: '削除' };
    const waiting = r.jobs.filter((j) => j.state === 'waiting').length;
    show('操作キュー', waiting ? `実行中 1 件、待ち ${waiting} 件` : '実行中 1 件',
        r.jobs.map((j) => ({
            // The runner is marked, because "what is happening now" and "what
            // is about to" are different questions and the list answers both.
            n: j.state === 'running' ? '▶' : `#${j.op}`,
            label: `${verb[j.kind] || j.kind}  ${j.total} 件`,
            sub: j.stopping ? '止めています…'
                : j.state === 'waiting' ? `待機中   ${j.dest || ''}` : (j.dest || ''),
            op: j.op,
        })), {
            foot: 'x 中止（待機中なら取り消し）   b 動かしたまま閉じる   Esc 閉じる',
            act: {
                x: async () => {
                    const row = report.rows[report.at];
                    if (!row) return;
                    await ask('cancel', { op: row.op });
                    say(`#${row.op} を中止しています`);
                    closeReport();
                },
                // `b` puts it out of the way and leaves it running — the
                // terminal build's word for it. Nothing is cancelled; the
                // screen is. The bar goes with it, for the same reason.
                b: () => {
                    prog.hidden = true;
                    drawProg();
                    closeReport();
                    say('操作は動いたままです（:queue で戻れます）');
                },
            },
        });
}

/// `@` — the macros in `macro.lua`.
///
/// **A layout macro builds a grid of shell panes in the terminal build, and
/// there are no splits here yet — so each pane becomes a tab.** The shells,
/// their commands and their scripted steps all run; only the arrangement is
/// lost. Said out loud in the list rather than discovered.
async function cmdMacros() {
    const r = await ask('macros', {});
    if (!r) return;
    if (!r.macros.length) {
        say(`マクロがありません（${r.where || 'macro.lua'}）`);
        return;
    }
    show('マクロ', r.where || '', r.macros.map((m) => ({
        n: m.script ? 'script' : `${m.panes}枚`,
        label: m.name,
        sub: m.script ? '（スクリプトはまだ動きません）' : 'タブとして開きます',
        name: m.name,
        script: m.script,
    })), {
        foot: 'Enter 実行   Esc 閉じる',
        pick: async (row) => {
            closeReport();
            if (!term.on) await openShell();
            const done = await ask('macrorun', {
                pane: state.focus, name: row.name, ...shellSize(),
            });
            if (!done) return;
            takeShell(done);
            setShellFocus(true);
            say(`${done.name} — ${done.opened} 枚をタブで開きました`);
        },
    });
}

// ---- Tabs ----
//
// One list per side, and the active tab *is* that pane. A tab opens where you
// are standing, which is what makes it useful: the reason to open one is
// nearly always "keep this, and go somewhere else for a moment".

async function tabNew() {
    const which = state.focus;
    const pane = await ask('tabnew', { pane: which });
    if (!pane) return;
    state[which] = pane;
    draw(which);
    say(`タブ ${pane.tab + 1} / ${pane.tabs.length}`);
}

async function tabClose() {
    const which = state.focus;
    const pane = await ask('tabclose', { pane: which });
    if (!pane) return;
    state[which] = pane;
    draw(which);
    say(pane.cwd);
}

async function goTab(which, how) {
    const pane = await ask('tabgo', { pane: which, ...how });
    if (!pane) return;
    state[which] = pane;
    state.focus = which;
    draw('left');
    draw('right');
    say(pane.cwd);
}

/// `s` — the folders worth going back to.
///
/// The terminal build's own `shortcuts.lua`, read and written through the same
/// renderer. A second bookmark list would be the worst kind of two-programs
/// problem: which folders you had saved would depend on which one you saved
/// them from.
async function cmdShortcuts() {
    const r = await ask('shortcuts', {});
    if (!r) return;
    if (!r.rows.length) {
        say('登録がありません — :bookmark でいまの場所を登録できます');
        return;
    }
    show('ショートカット', r.where || '', r.rows.map((x) => ({
        n: x.group ? '▸' : '',
        label: '  '.repeat(x.depth) + x.name,
        sub: x.target || '',
        target: x.target,
    })), {
        foot: 'Enter そこへ   Esc 閉じる',
        pick: (row) => { if (row.target) { closeReport(); revealPath(row.target, true); } },
    });
}

async function cmdBookmark(name) {
    const r = await ask('bookmark', { pane: state.focus, name });
    if (!r) return;
    say(`${r.name} を登録しました`);
}

// ---- The AI, where a site has configured one ----
//
// The prompts live in the engine, word for word the terminal build's. Two
// front ends asking the same model differently would give two different
// answers to the same question, which is the kind of difference nobody can
// debug.

/// `:aicmd` — a description in, a command out, into the shell's prompt but
/// **not run**. A model that guesses wrong is a model that guesses wrong; the
/// person presses Enter, not the program.
/// What to do with the answer when it arrives. The engine runs the model on a
/// worker — it waits on a python process talking to somebody else's network —
/// so this is a question asked and an answer heard, not a call.
let aiWaiting = null;

async function cmdAiCmd(want) {
    const r = await ask('ai', { pane: state.focus, what: 'cmd', text: want });
    if (!r) return;
    say('考えています…');
    aiWaiting = async (answer) => {
        // Into the prompt, **not run**. A model that guesses wrong guesses
        // wrong; the person presses Enter, not the program.
        const line = answer.trim().split('\n')[0].replace(/^[$#>]\s*/, '');
        if (!term.on) await openShell();
        await ask('shellinput', { text: line });
        setShellFocus(true);
        say('Enter で実行、Ctrl+C で捨てる — 実行はしていません');
    };
}

async function cmdAiLog() {
    const pane = state[state.focus];
    const name = pane.entries[pane.cursor]?.name || '';
    const r = await ask('ai', { pane: state.focus, what: 'log' });
    if (!r) return;
    say('ログを読んでいます…');
    aiWaiting = (answer) => {
        show(`${name} の診断`, 'AI の答え — 確かめてから使ってください',
            answer.split('\n').map((t) => ({ label: t })),
            { foot: 'Esc 閉じる' });
    };
}

async function cmdAiAsk(question) {
    const r = await ask('ai', { pane: state.focus, what: 'text', text: question });
    if (!r) return;
    say('考えています…');
    aiWaiting = (answer) => {
        show('AI', question, answer.split('\n').map((t) => ({ label: t })), { foot: 'Esc 閉じる' });
    };
}

/// `:aidiff` — explain what is on the comparison screen.
///
/// Only from a comparison, because "explain the diff" with no diff up is a
/// question about nothing.
async function cmdAiDiff() {
    if (!report.on || !report.rows.length) {
        say('先に = で比較してください', true);
        return;
    }
    const text = report.rows.map((x) => `${x.n || ''} ${x.label} ${x.sub || ''}`).join('\n');
    const r = await ask('ai', {
        pane: state.focus, what: 'text',
        system: 'You explain a diff to the person who is about to act on it. '
            + 'Say what changed and, where it is clear, why it matters. '
            + 'Be concise; plain text, no markdown headings.',
        text: text.slice(0, 16000),
    });
    if (!r) return;
    say('差分を読んでいます…');
    aiWaiting = (answer) => {
        show('差分の説明', 'AI の答え — 確かめてから使ってください',
            answer.split('\n').map((t) => ({ label: t })), { foot: 'Esc 閉じる' });
    };
}

async function cmdOffice(what) {
    const r = await ask(what, { pane: state.focus });
    if (!r) return;
    if (r.made) {
        state[state.focus] = r;
        draw(state.focus);
        say(`${r.made} を作りました`);
        return;
    }
    say(`${r.opened} をクラウドで開きました`);
}

async function cmdReload() {
    const r = await ask('reload', {});
    if (!r) return;
    // Said plainly rather than "reloaded": some of init.lua is read once, at
    // startup, and claiming otherwise sends people looking for a bug.
    say(`init.lua を読み直しました（AI ${r.ai ? 'あり' : 'なし'}、`
        + `同期 ${r.sync_maps} 件、SSH ${r.ssh_hosts} 件）— 枠線などは再起動が要ります`);
}

/// `:key` — show what the window actually received.
///
/// The first thing to ask when a key "does nothing": did it arrive, and as
/// what? On this build it also answers whether a menu accelerator ate it,
/// which is a Windows-only failure invisible from a Mac.
const keyEcho = { on: false };

function toggleKeyEcho() {
    keyEcho.on = !keyEcho.on;
    say(keyEcho.on
        ? 'キー表示: 押したキーを出すだけで、何も実行しません（Esc で止める）'
        : 'キー表示を止めました');
}

// ---- A server, in this pane ----
//
// Not a transfer dialog. The rows are rows, `Enter` walks into a directory,
// `..` climbs, and `c` across to the other pane is an upload or a download
// depending on which side you are standing on. That is the terminal build's
// arrangement, and the reason it is worth having at all: nothing new to learn.

/// `Shift+S` — the hosts init.lua declares, picked rather than typed.
///
/// Whether a password is stored comes over as a yes or a no; the password
/// itself never leaves the engine, which resolves it (or runs password_cmd)
/// at connect time.
async function cmdSshPicker() {
    const r = await ask('sshhosts', {});
    if (!r) return;
    if (!r.hosts.length) { await cmdConnect(); return; }
    const rows = [];
    for (const h of r.hosts) {
        for (const u of h.users) {
            rows.push({
                n: u.stored ? '鍵あり' : '',
                label: `${u.name}@${h.name}`,
                sub: `${h.host}:${h.port}`,
                host: h.at,
                user: u.at,
                stored: u.stored,
                who: `${u.name}@${h.host}`,
            });
        }
    }
    show('SSH', `${rows.length} 件（init.lua の cian.ssh）`, rows, {
        foot: 'Enter 接続   Esc 閉じる',
        pick: async (row) => {
            closeReport();
            let password;
            if (!row.stored) {
                password = await askFor(`${row.who} のパスワード`, '', { secret: true });
                if (password === null) return;
            }
            say(`${row.who} に繋いでいます…`);
            const c = await ask('connect', {
                pane: state.focus, preset_host: row.host, preset_user: row.user, password,
            });
            if (!c) return;
            state[state.focus] = c.pane;
            draw(state.focus);
            say(`${c.host}  ${c.path}`);
        },
    });
}

async function cmdConnect() {
    const spec = await askFor('user@host[:port][:/path]', '');
    if (spec === null || !spec.trim()) return;
    const m = spec.trim().match(/^([^@]+)@([^:/]+)(?::(\d+))?(?::?(\/.*))?$/);
    if (!m) { say('user@host の形で書いてください', true); return; }
    const [, user, host, port, path] = m;
    // Asked for, never stored. cian has nowhere to keep a password that would
    // be better than not keeping one.
    const password = await askFor(`${user}@${host} のパスワード`, '', { secret: true });
    if (password === null) return;
    say(`${user}@${host} に繋いでいます…`);
    const r = await ask('connect', {
        pane: state.focus, user, host,
        port: port ? Number(port) : 22,
        path: path || '.',
        password,
    });
    if (!r) return;
    state[state.focus] = r.pane;
    draw(state.focus);
    say(`${r.host}  ${r.path}`);
}

async function cmdDisconnect() {
    const pane = await ask('disconnect', { pane: state.focus });
    if (!pane) return;
    state[state.focus] = pane;
    draw(state.focus);
    say(pane.cwd);
}

/// The same keys, over the network.
///
/// `a`, `A`, `r`, `d` behave as they do locally — the rows look the same, so
/// they should act the same. The one difference is said out loud rather than
/// discovered: a remote delete is a delete, because SFTP has no trash.
async function remoteOp(what) {
    const which = state.focus;
    const pane = state[which];
    let name;
    if (what === 'mkdir' || what === 'touch') {
        name = await askFor(what === 'mkdir' ? '新しいディレクトリの名前' : '新しいファイルの名前', '');
        if (name === null || !name) return;
    } else if (what === 'rename') {
        const row = pane.entries[pane.cursor];
        if (!row || row.parent) return;
        name = await askFor(`${row.name} の新しい名前`, row.name);
        if (name === null || !name) return;
    } else if (what === 'delete') {
        const marked = pane.entries.filter((x) => x.marked);
        const rows = marked.length ? marked : [pane.entries[pane.cursor]].filter((x) => x && !x.parent);
        if (!rows.length) { say('対象がありません', true); return; }
        if (!await confirm(
            `${rows.length} 件をサーバから削除します`,
            'ゴミ箱はありません — 元に戻せません\n\n' + rows.map((x) => x.name).join('\n'),
        )) { say('やめました'); return; }
    }
    const r = await ask('remoteop', { pane: which, what, name });
    if (!r) return;
    state[which] = r;
    draw(which);
    say(r.said);
}

async function remoteStep(opts) {
    const which = state.focus;
    const r = await ask('remotelist', { pane: which, ...opts });
    if (!r) return;
    state[which] = r.pane;
    draw(which);
    say(r.path);
}

async function uploadHeld() {
    const which = state.focus;
    say('アップロード中…');
    const r = await ask('uploadclip', { pane: which });
    if (!r) return;
    state[which] = r;
    draw(which);
    if (r.errors.length) say(r.errors.join('  /  '), true);
    else say(`${r.ok} 件をアップロードしました`);
}

async function transfer() {
    const which = state.focus;
    const other = which === 'left' ? 'right' : 'left';
    const pane = state[which];
    const rows = pane.entries.filter((x) => x.marked);
    const what = rows.length ? rows : [pane.entries[pane.cursor]].filter((x) => x && !x.parent);
    if (!what.length) { say('対象がありません', true); return; }
    const up = !!state[other].remote;
    const head = `${what.length} 件を ${up ? 'アップロード' : 'ダウンロード'}`;
    if (!await confirm(head, what.map((x) => x.name).join('\n'))) { say('やめました'); return; }
    say(`${head}中…`);
    const r = await ask('transfer', { pane: which });
    if (!r) return;
    state.left = r.left;
    state.right = r.right;
    draw('left');
    draw('right');
    if (r.errors.length) say(r.errors.join('  /  '), true);
    else say(`${r.ok} 件を${r.direction === 'up' ? 'アップロード' : 'ダウンロード'}しました`);
}

/// `:!cmd` — run it in the shell, in this pane's directory. `%` is the
/// selection, `%f` the file, `%d` the directory; the engine substitutes them,
/// quoted, because a path with a space in it is the common case.
async function cmdBang(line) {
    if (!term.on) await openShell();
    const r = await ask('run', { pane: state.focus, line });
    if (!r) return;
    say(r.sent);
}

/// `:renamelist` — edit the names as a list, apply the list.
///
/// The other bulk rename. `:renamepattern` is for a rule; this is for the
/// hundred names that follow no rule, which is most of them. Editing them as
/// text is the only way that is not a hundred prompts, and it is what every
/// filer that has this feature does.
async function cmdRenameList() {
    const pane = state[state.focus];
    const rows = pane.entries.filter((x) => x.marked);
    const what = rows.length ? rows : pane.entries.filter((x) => !x.parent);
    if (!what.length) { say('対象がありません', true); return; }

    let monaco;
    try {
        monaco = await loadMonaco();
    } catch (e) { say(e.message, true); return; }

    // The editor, on a list rather than a file. Nothing is written until it is
    // closed, and the line count has to still match — a list one line short is
    // a rename that would pair the wrong names together, silently.
    renameList.on = true;
    renameList.paths = what.map((x) => x.path);
    viewer.on = true;
    viewer.name = '名前の一覧';
    el.view.hidden = false;
    el.vBody.hidden = false;
    el.vPic.hidden = true;
    makeEditor(monaco, what.map((x) => x.name).join('\n'), 'plaintext');
    viewer.base = viewer.ed.getModel().getAlternativeVersionId();
    viewer.dirty = false;
    setStyle(style);
    el.vName.textContent = '名前の一覧を編集';
    el.vAbout.textContent = `${what.length} 件   1行に1つ、順番は変えないこと`;
    el.vFoot.textContent = 'Ctrl+S 適用   Esc ×3 やめる';
    viewer.ed.focus();
}

const renameList = { on: false, paths: [] };

async function applyRenameList() {
    const names = viewer.ed.getValue().split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
    if (names.length !== renameList.paths.length) {
        say(`行数が合いません（${names.length} 行 / ${renameList.paths.length} 件）`, true);
        return false;
    }
    const rows = renameList.paths
        .map((path, i) => ({ path, to: names[i] }))
        .filter((x, i) => x.to !== state[state.focus].entries.find((e) => e.path === x.path)?.name
            || names[i] !== names[i]);
    const changing = rows.filter((x) => x.to !== x.path.split(/[\\/]/).pop());
    if (!changing.length) { say('変わる名前がありません'); return true; }
    if (!await confirm(`${changing.length} 件の名前を変えます`,
        changing.map((x) => `${x.path.split(/[\\/]/).pop()}  →  ${x.to}`).join('\n'))) {
        say('やめました');
        return false;
    }
    const done = await ask('renameapply', { rows: changing });
    if (!done) return false;
    await reread();
    if (done.errors.length) say(done.errors.join('  /  '), true);
    else say(`${done.renamed} 件の名前を変えました`);
    return true;
}

/// The open file's headings, and a way to land on one.
///
/// Closing the list rather than keeping it beside the text: a file manager's
/// editor is a place you go to change one thing, and a permanent outline
/// column would be a third of the width spent on navigation you use twice.
async function cmdOutline() {
    if (!viewer.on) { say('先にファイルを開いてください', true); return; }
    const r = await ask('outline', {});
    if (!r) return;
    if (!r.items.length) { say('見出しが見つかりません'); return; }
    show(`${viewer.name} の見出し`, `${r.items.length} 件`,
        r.items.map((i) => ({
            n: String(i.line + 1),
            label: '  '.repeat(i.level) + i.text,
            line: i.line,
        })),
        {
            foot: 'Enter そこへ   Esc 閉じる',
            pick: (row) => {
                closeReport();
                viewer.ed.revealLineInCenter(row.line + 1);
                viewer.ed.setPosition({ lineNumber: row.line + 1, column: 1 });
                viewer.ed.focus();
            },
        });
}

/// A line operation on whatever the editor is holding.
///
/// The lines go down to cian-core and come back changed. `:han` and `:zen`
/// alone are a table of Japanese width mappings, and nobody should own two
/// copies of that.
/// Hand the whole buffer to the engine and take back what it returns.
///
/// The line work — sort, uniq, the substitutions — belongs on the engine's
/// side, where cian-core already holds it and the terminal build already
/// calls it. What is left here is the same six lines every time, and putting
/// the answer back **through the editor's own edit stack rather than
/// setValue** is the part that matters: it has to be undoable with the key
/// that undoes everything else in here.
async function rewriteBuffer(method, params, said) {
    if (!viewer.on || !viewer.ed) { say('先にファイルを開いてください', true); return null; }
    const lines = viewer.ed.getValue().split(/\r?\n/);
    const r = await ask(method, { ...params, lines });
    if (!r) return null;
    const model = viewer.ed.getModel();
    viewer.ed.executeEdits('cian', [{
        range: model.getFullModelRange(),
        text: r.lines.join('\n'),
    }]);
    viewer.ed.pushUndoStop();
    say(said(r, lines));
    return r;
}

async function textOp(op) {
    await rewriteBuffer('textop', { op },
        (r, lines) => `:${op}   ${lines.length} 行 → ${r.lines.length} 行`);
}

async function setEol(kind) {
    const r = await ask('eol', { kind });
    if (!r) return;
    say(`改行を ${r.eol.toUpperCase()} にしました（保存時に反映）`);
}

async function cmdSvn(what) {
    let message;
    if (what === 'commit') {
        message = await askFor('コミットメッセージ', '');
        if (message === null || !message.trim()) return;
    }
    const r = await ask('svn', { pane: state.focus, what, message });
    if (!r) return;
    state[state.focus] = r.pane;
    draw(state.focus);
    say(r.said);
}

async function cmdNoBom() {
    const pane = state[state.focus];
    const rows = pane.entries.filter((x) => x.marked);
    const what = rows.length ? rows : [pane.entries[pane.cursor]].filter((x) => x && !x.parent);
    if (!what.length) { say('対象がありません', true); return; }
    if (!await confirm(`${what.length} 件から UTF-8 BOM を除去します`,
        what.map((x) => x.name).join('\n'))) { say('やめました'); return; }
    const r = await ask('nobom', { pane: state.focus });
    if (!r) return;
    state[state.focus] = r.pane;
    draw(state.focus);
    const parts = [`BOM除去 ${r.stripped} 件`];
    if (r.none) parts.push(`もともと無し ${r.none} 件`);
    if (r.utf16) parts.push(`UTF-16 は据置 ${r.utf16} 件`);
    if (r.failed) parts.push(`失敗 ${r.failed} 件`);
    say(parts.join('   '), r.failed > 0);
}

async function cmdEach(line) {
    if (!term.on) await openShell();
    const r = await ask('each', { pane: state.focus, line });
    if (!r) return;
    say(`${r.ran} 件に実行しました`);
}

function findCommand(name) {
    // Aliases carry the terminal build's other spellings (`:duplicate`,
    // `:dup`) without a second palette entry per spelling.
    return COMMANDS.find((c) => c.name === name || (c.alias || []).includes(name));
}

/// `:` — the name, then whatever it takes.
function commandLine(initial = '') {
    // On the prompt row at the foot, where cian-tui puts its command line —
    // not in a sheet in the middle of the window. Purple, because `/` above
    // it is green and the two take the same letters.
    openPrompt('cmd', initial);
}

/// Run whatever was typed on the command line.
async function runTypedCommand(line) {
    const text = line.trim();
    if (!text) return;
    // `!` is a prefix, not a name: everything after it is the command line
    // itself, spaces and all.
    if (text.startsWith('!')) {
        await cmdBang(text.slice(1).trim());
        return;
    }
    const at = text.indexOf(' ');
    const name = at < 0 ? text : text.slice(0, at);
    const arg = at < 0 ? '' : text.slice(at + 1).trim();
    const cmd = findCommand(name);
    if (cmd && cmd.name !== name) {
        // Called by an alias: the spelling used is information (`:icons` is
        // `:view icons`, `:nvim` names its editor), so it rides along.
        await runCommand(cmd, arg, name);
        return;
    }
    if (!cmd) {
        // Named, not "unknown command": the name typed is the one thing the
        // person can compare against the list.
        say(`:${name} は知りません — C でコマンド一覧`, true);
        return;
    }
    await runCommand(cmd, arg);
}

async function runCommand(cmd, arg, invokedAs) {
    let a = arg;
    // Only where there is no sensible default, and only where there is no
    // sensible *nothing*: `:theme` with no name shows the list, which is a
    // better answer than a prompt. `:hash` means sha256 and `:readonly` means
    // on; stopping to ask would be a prompt with one likely answer, which is
    // the kind of question that trains people to hit Enter.
    if (cmd.arg && !a && !cmd.optional) {
        a = await askFor(`:${cmd.name}`, '');
        if (a === null) return;
    }
    try {
        await cmd.run(a, invokedAs);
    } catch (e) {
        say(String(e.message || e), true);
    }
}

/// `C` — every command, fuzzy.
function openPalette() {
    const rows = COMMANDS.map((c) => ({ label: `:${c.name}`, sub: c.about, cmd: c }));
    show('コマンド', `${rows.length} 個`, rows, {
        // The one the help has always called あいまい検索 and which walked
        // a hundred and thirty rows with j and k until now.
        filter: true,
        hint: '打って絞り込み（:name か説明）',
        foot: '打って絞る   ↑↓ 選ぶ   Enter 実行   Esc 閉じる',
        pick: (row) => { closeReport(); runCommand(row.cmd, ''); },
    });
}

// ---- The commands themselves ----

async function cmdCount() {
    const r = await ask('count', { pane: state.focus });
    if (!r) return;
    const rows = r.by_ext.map((e) => ({
        n: e.steps.toLocaleString(),
        label: e.ext,
        sub: `${e.files} ファイル`,
    }));
    show('ファイル数とステップ数',
        `${r.files.toLocaleString()} ファイル   ${r.steps.toLocaleString()} ステップ`
        + `   （実行 ${r.steps.toLocaleString()} / 空白 ${r.blank.toLocaleString()} / コメント ${r.comments.toLocaleString()}）`
        + (r.truncated ? '   ※上限で打ち切り' : ''),
        rows, { foot: 'Esc 閉じる' });
}

async function cmdDu(path) {
    const r = await ask('du', { pane: state.focus, ...(path ? { path } : {}) });
    if (!r) return;
    const rows = r.rows.map((x) => ({
        n: human(x.size),
        label: x.is_dir ? `${x.name}/` : x.name,
        path: x.path,
        is_dir: x.is_dir,
    }));
    const total = r.rows.reduce((n, x) => n + x.size, 0);
    show('容量分析', `${r.cwd}   合計 ${human(total)}`, rows, {
        foot: 'Enter ディレクトリへ入る   Esc 閉じる',
        pick: (row) => { if (row.is_dir) cmdDu(row.path); },
    });
}

async function cmdAttr() {
    const r = await ask('attr', { pane: state.focus });
    if (!r) return;
    const rows = [
        { label: '種類', sub: r.is_dir ? 'ディレクトリ' : 'ファイル' },
        { label: 'モード', sub: r.mode || '(なし)' },
        { label: '読み取り専用', sub: r.readonly ? 'はい' : 'いいえ' },
        { label: '所有者', sub: r.owner || '(なし)' },
        { label: '大きさ', sub: r.size === null ? '—' : `${human(r.size)}（${r.size.toLocaleString()} バイト）` },
        { label: '場所', sub: r.path },
    ];
    show('属性', r.name, rows, { foot: 'Esc 閉じる' });
}

async function cmdChmod(spec) {
    const r = await ask('chmod', { pane: state.focus, spec });
    if (!r) return;
    await reread();
    say(`${r.changed} 件を ${r.spec} にしました`);
}

async function cmdReadonly(onOff) {
    const on = !/^(off|no|false|0|解除)$/i.test((onOff || '').trim());
    const r = await ask('readonly', { pane: state.focus, on });
    if (!r) return;
    await reread();
    say(`${r.changed} 件を${on ? '読み取り専用に' : '書き込み可に'}しました`);
}

async function cmdHash(kind) {
    const k = /md5/i.test(kind || '') ? 'md5' : 'sha256';
    say(`${k} を計算中…`);
    const r = await ask('hash', { pane: state.focus, kind: k });
    if (!r) return;
    show(`チェックサム（${r.kind}）`, `${r.rows.length} 件`,
        r.rows.map((x) => ({ label: x.name, sub: x.sum })),
        { foot: 'Esc 閉じる' });
}

async function cmdSearch(mode, needle) {
    if (!needle) return;
    say(`${mode === 'content' ? '中を' : '名前を'}探しています…`);
    const r = await ask('search', { pane: state.focus, needle, mode });
    if (!r) return;
    const rows = r.hits.map((h) => ({
        n: h.line ? String(h.line.n) : null,
        label: h.rel + (h.is_dir ? '/' : ''),
        sub: h.line ? h.line.text.trim() : '',
        path: h.path,
        is_dir: h.is_dir,
    }));
    // Remembered, so Ctrl+N can walk them after this screen is gone.
    hits.list = r.hits.map((h) => ({ path: h.path, line: h.line ? h.line.n : 0 }));
    hits.at = -1;
    hits.needle = needle;
    show(mode === 'content' ? `grep ${needle}` : `find ${needle}`,
        `${r.root}   ${rows.length} 件${r.truncated ? '（打ち切り）' : ''}`,
        rows, {
            foot: 'Enter そこへ   p 一覧に読み込む   r 一括置換   Esc 閉じる',
            pick: (row) => {
                closeReport();
                hits.at = rows.indexOf(row) - 1;
                if (row.n) hopHit(1);
                else revealPath(row.path, row.is_dir);
            },
            act: {
                r: async () => {
                    // Replace across every file the grep matched. The plan
                    // first, every line of it: this writes to files that are
                    // not open and `u` cannot take it back.
                    const spec = await askFor('置換 s/古い/新しい/g', `s/${needle}//g`);
                    if (spec === null || !spec.trim()) return;
                    const paths = [...new Set(rows.map((x) => x.path))];
                    const plan = await ask('replaceplan', { paths, spec });
                    if (!plan) return;
                    if (!plan.changes.length) { say('変わる行がありません'); return; }
                    closeReport();
                    showReplacePlan(spec, plan);
                },
                p: async () => {
                    const paths = rows.map((x) => x.path);
                    const which = state.focus;
                    const pane = await ask('panelize', {
                        pane: which, paths, label: `${mode === 'content' ? 'grep' : 'find'} ${needle}`,
                    });
                    if (!pane) return;
                    closeReport();
                    state[which] = pane;
                    draw(which);
                    say(`${paths.length} 件を一覧に読み込みました（Esc で戻る）`);
                },
            },
        });
}

/// Put the cursor on a path, entering its directory if need be.
/// Put the cursor on this path, reading its directory only if we are not
/// standing in it already.
///
/// Three things wanted this — jumping to a search hit, landing on a row
/// picked out of a report, stepping through the files opened at once — and
/// each had written its own. Staying put when the directory is already the
/// right one is not only faster: a re-read would drop the marks and the
/// filter, which is a visible loss for a gesture that means "look over
/// there", not "start again".
async function landOn(path, isDir = false) {
    const which = state.focus;
    const dir = isDir ? path : path.replace(/[\\/][^\\/]*$/, '');
    if (state[which].cwd !== dir) {
        const pane = await ask('list', { pane: which, path: dir });
        if (!pane) return false;
        state[which] = pane;
    }
    if (!isDir) {
        const at = state[which].entries.findIndex((x) => x.path === path);
        if (at >= 0) state[which].cursor = at;
    }
    draw(which);
    return true;
}

async function revealPath(path, isDir) {
    if (await landOn(path, isDir)) say(state[state.focus].cwd);
}

async function cmdBranch() {
    const which = state.focus;
    if (state[which].flat) { await leaveFlat(); return; }
    say('この配下を集めています…');
    const r = await ask('branch', { pane: which });
    if (!r) return;
    state[which] = r.pane;
    draw(which);
    say(`${r.found} 件（b か Esc で戻る）`);
}

async function leaveFlat() {
    const which = state.focus;
    const pane = await ask('leaveflat', { pane: which });
    if (!pane) return;
    state[which] = pane;
    draw(which);
    say(pane.cwd);
}

async function step(dir) {
    const which = state.focus;
    const pane = await ask(dir, { pane: which });
    if (!pane) return;
    state[which] = pane;
    draw(which);
    say(pane.cwd);
}

async function cmdHistory() {
    const r = await ask('history', { pane: state.focus });
    if (!r) return;
    const rows = [
        ...r.back.map((p) => ({ n: '←', label: p })),
        { n: '', label: r.cwd, sub: 'いまここ' },
        ...r.forward.map((p) => ({ n: '→', label: p })),
    ];
    show('履歴', r.cwd, rows, {
        foot: 'Enter そこへ   Esc 閉じる',
        pick: (row) => { closeReport(); revealPath(row.label, true); },
    });
}

let lastGG = 0;

/// `v` — mark a run without pressing Space down it.
///
/// The anchor is where it started; every move re-marks from there, so
/// overshooting is corrected by moving back rather than by starting again.
/// `Enter` or a second `v` keeps it, `Esc` puts the marks back as they were.
const visual = { on: false, from: 0, was: null };

async function startVisual() {
    const pane = state[state.focus];
    if (!pane) return;
    if (visual.on) { await endVisual(true); return; }
    visual.on = true;
    visual.from = pane.cursor;
    visual.was = pane.entries.filter((x) => x.marked).map((x) => x.path);
    await paintVisual();
    say('ビジュアル選択 — Enter で確定、Esc で取消');
}

async function paintVisual() {
    const which = state.focus;
    const pane = state[which];
    const lo = Math.min(visual.from, pane.cursor);
    const hi = Math.max(visual.from, pane.cursor);
    const want = new Set(visual.was);
    pane.entries.forEach((x, i) => { if (i >= lo && i <= hi && !x.parent) want.add(x.path); });
    const next = await ask('setmarks', { pane: which, paths: [...want] });
    if (!next) return;
    next.cursor = pane.cursor;
    state[which] = next;
    draw(which);
    say(`ビジュアル: ${next.marked} 件`);
}

async function endVisual(keep) {
    if (!visual.on) return;
    visual.on = false;
    if (!keep) {
        const which = state.focus;
        const next = await ask('setmarks', { pane: which, paths: visual.was });
        if (next) { state[which] = next; draw(which); }
        say('取り消しました');
    } else {
        say(`${state[state.focus].marked} 件をマーク`);
    }
    visual.was = null;
}

/// `f` looks in *this* listing, and `n`/`N` walk the matches.
///
/// Not the same as `/`, which narrows the listing to what matches, and not the
/// same as `Shift+F`, which walks the whole tree below here. The terminal
/// build keeps all three, and they answer three different questions: where is
/// it, show me only those, and is it anywhere under here.
let here = { needle: '', at: -1 };

async function searchHere() {
    const needle = await askFor('この一覧を検索', here.needle);
    if (needle === null || !needle) return;
    here.needle = needle;
    here.at = -1;
    hopHere(1);
}

function hopHere(step) {
    const pane = state[state.focus];
    if (!pane || !here.needle) return;
    const q = here.needle.toLowerCase();
    const hits = [];
    pane.entries.forEach((x, i) => {
        if (!x.parent && x.name.toLowerCase().includes(q)) hits.push(i);
    });
    if (!hits.length) { say(`${here.needle} — 見つかりません`, true); return; }
    if (here.at < 0) {
        // The first hop starts from where the eye is, not from the top.
        const ahead = hits.findIndex((n) => n > pane.cursor);
        here.at = step > 0 ? (ahead < 0 ? 0 : ahead) : (ahead <= 0 ? hits.length - 1 : ahead - 1);
    } else {
        here.at = (here.at + step + hits.length) % hits.length;
    }
    pane.cursor = hits[here.at];
    draw(state.focus);
    say(`${here.needle}   ${here.at + 1} / ${hits.length}`);
}

// ---- Left against right, bulk rename, archives ----

/// `=` — one key, and what the two cursors point at decides the answer.
async function cmdCompare() {
    say('比べています…');
    const r = await ask('compare', {});
    if (!r) return;
    if (r.kind === 'dirs') {
        const mark = { left: '◀ 左だけ', right: '右だけ ▶', differ: '≠ 違う' };
        const roots = { left: r.left, right: r.right };
        show('ディレクトリ比較', `${r.left}   ↔   ${r.right}   ${r.rows.length} 件${r.truncated ? '（打ち切り）' : ''}`,
            r.rows.map((x) => ({
                n: mark[x.status],
                label: x.rel + (x.is_dir ? '/' : ''),
                rel: x.rel,
                status: x.status,
            })),
            {
                foot: '> 右へコピー   < 左へコピー   c 一覧をコピー   w 保存   Esc 閉じる',
                act: {
                    '>': () => copyAcross(roots, 'left', 'right'),
                    '<': () => copyAcross(roots, 'right', 'left'),
                    c: () => copyReport('ディレクトリ比較'),
                    w: () => saveReport(`${r.left} ↔ ${r.right}`),
                },
            });
        return;
    }
    if (r.kind === 'files') {
        // Nothing to look at when they are the same, and a screen saying
        // "identical" over an empty list is a screen that wasted a keystroke.
        if (!r.added && !r.removed && !r.changed) {
            say(`${r.left} と ${r.right} は同じ内容です`);
            return;
        }
    }
    // A difference is read by its differences, so the identical runs between
    // them are folded away — the engine did that; here they are one row
    // saying how many went past.
    const glyph = { same: ' ', changed: '~', removed: '-', added: '+', skipped: '⋯' };
    const rows = r.rows.map((x) => {
        if (x.kind === 'skipped') return { n: '⋯', label: `── 同じ ${x.lines} 行 ──`, sub: '' };
        return {
            n: `${glyph[x.kind]} ${x.ln ?? ''}`.trim(),
            label: x.left ?? '',
            sub: x.right ?? '',
        };
    });
    show('ファイル比較', `${r.left}   ↔   ${r.right}   ${r.summary}`, rows, {
        foot: 'Enter 並べて編集   c 一覧をコピー   w 保存   Esc 閉じる',
        pick: () => { closeReport(); cmdDiffEdit(); },
        act: {
            c: () => copyReport(`${r.left} ↔ ${r.right}`),
            w: () => saveReport(`${r.left} ↔ ${r.right}`),
        },
    });
}

/// `>` / `<` in a directory comparison — put this entry on the other side.
///
/// The row knows where it is missing from, so the direction is checked rather
/// than assumed: copying a file that only exists on the right *to* the right
/// is a no-op that would still ask for a confirmation, and copying the wrong
/// way over a newer file is the mistake this screen exists to prevent.
async function copyAcross(roots, from, to) {
    const row = report.rows[report.at];
    if (!row) return;
    if (row.status === (from === 'left' ? 'right' : 'left')) {
        say(`${row.rel} は${from === 'left' ? '左' : '右'}にありません`, true);
        return;
    }
    const src = `${roots[from]}/${row.rel}`;
    const destDir = `${roots[to]}/${row.rel}`.replace(/[\\/][^\\/]*$/, '');
    if (!await confirm(`${row.rel} を${to === 'right' ? '右' : '左'}へコピー`, `${src}\n  →  ${destDir}`)) {
        say('やめました');
        return;
    }
    const r = await ask('copyone', { src, dest: destDir });
    if (!r) return;
    say(`${row.rel} をコピーしました`);
}

/// `c` / `w` on any report — the list as text, to the clipboard or to a file.
///
/// A comparison is something people paste into a ticket. Reading it off the
/// screen and retyping it is the alternative, and that is where the typos in
/// change requests come from.
async function copyReport(title) {
    const text = report.rows
        .map((x) => [x.n, x.label, x.sub].filter(Boolean).join('\t'))
        .join('\n');
    await navigator.clipboard.writeText(`${title}\n${text}`);
    say(`${report.rows.length} 行をクリップボードへ`);
}

async function saveReport(title) {
    const name = await askFor('保存する名前', 'compare.txt');
    if (name === null || !name) return;
    const text = report.rows
        .map((x) => [x.n, x.label, x.sub].filter(Boolean).join('\t'))
        .join('\n');
    const r = await ask('writefile', { pane: state.focus, name, text: `${title}\n${text}\n` });
    if (!r) return;
    await reread();
    say(`${r.wrote} に保存しました`);
}

/// `=` in the comparison, or `:diffedit` — the two files side by side, both
/// editable.
///
/// The report screen answers "what differs"; this answers "let me fix it".
/// Same two files, a different question — and fixing a difference by reading
/// it in one window and typing in another is how the wrong half gets edited.
const pair = { on: false, ed: null };

async function cmdDiffEdit() {
    const r = await ask('twofiles', {});
    if (!r) return;
    let monaco;
    try {
        monaco = await loadMonaco();
    } catch (e) { say(e.message, true); return; }

    if (viewer.on) await closeView(false);
    if (report.on) closeReport();
    viewer.on = true;
    pair.on = true;
    viewer.name = `${r.left.name} ↔ ${r.right.name}`;
    el.view.hidden = false;
    el.vBody.hidden = false;
    el.vPic.hidden = true;
    el.vName.textContent = viewer.name;
    el.vAbout.textContent = '左右とも編集できます — Ctrl+S でどちらも保存';
    el.vFoot.textContent = 'F7 / Shift+F7 次 / 前の相違   ·   Ctrl+S 保存   ·   Esc ×3 閉じる';

    const lang = MONACO_LANG[r.lang] || 'plaintext';
    // A fresh diff editor each time: reusing one across different file pairs
    // means old models hanging on to files nobody has open.
    if (pair.ed) pair.ed.dispose();
    el.vBody.replaceChildren();
    pair.ed = monaco.editor.createDiffEditor(el.vBody, {
        theme: editorTheme(),
        automaticLayout: true,
        fontFamily: getComputedStyle(document.body).fontFamily,
        fontSize: FONT.at,
        originalEditable: true,
        renderSideBySide: true,
        minimap: { enabled: false },
    });
    pair.ed.setModel({
        original: monaco.editor.createModel(r.left.lines.join('\n'), lang),
        modified: monaco.editor.createModel(r.right.lines.join('\n'), lang),
    });
    say(`${r.left.name} ↔ ${r.right.name}`);
}

async function savePair() {
    if (!pair.ed) return;
    const m = pair.ed.getModel();
    const l = await ask('save', { lines: m.original.getValue().split(/\r?\n/) });
    const r = await ask('savepair', { lines: m.modified.getValue().split(/\r?\n/) });
    if (!l && !r) return;
    await reread();
    say(`${[l && l.saved, r && r.saved].filter(Boolean).join('  と  ')} を保存しました`);
}

/// The plan first, always — the hundred new names before any of them exists.
async function cmdRenamePattern(pattern) {
    const r = await ask('renameplan', { pane: state.focus, pattern });
    if (!r) return;
    const changing = r.rows.filter((x) => !x.same);
    if (!changing.length) { say('変わる名前がありません'); return; }
    const clashes = changing.filter((x) => x.clash);
    show(`一括リネーム   ${r.pattern}`,
        `${changing.length} 件が変わります` + (clashes.length ? `   ★ ${clashes.length} 件は既にある名前` : ''),
        r.rows.map((x) => ({
            n: x.clash ? '★' : (x.same ? '=' : '→'),
            label: x.from,
            sub: x.to,
        })),
        {
            foot: clashes.length
                ? '★ の名前は既にあります — Enter で残りだけ実行   Esc やめる'
                : 'Enter 実行   Esc やめる',
            pick: async () => {
                closeReport();
                const rows = changing.filter((x) => !x.clash);
                if (!rows.length) { say('実行できる行がありません', true); return; }
                if (!await confirm(`${rows.length} 件の名前を変えます`,
                    rows.map((x) => `${x.from}  →  ${x.to}`).join('\n'))) { say('やめました'); return; }
                const done = await ask('renameapply', { rows });
                if (!done) return;
                await reread();
                if (done.errors.length) say(done.errors.join('  /  '), true);
                else say(`${done.renamed} 件の名前を変えました`);
            },
        });
}

async function cmdCompress(kind, encrypted = false) {
    const pane = state[state.focus];
    const rows = pane.entries.filter((x) => x.marked);
    const what = rows.length ? rows : [pane.entries[pane.cursor]].filter((x) => x && !x.parent);
    if (!what.length) { say('対象がありません', true); return; }
    const name = await askFor('アーカイブの名前（拡張子なし）', what[0].name.replace(/\.[^.]*$/, ''));
    if (name === null || !name) return;
    let password;
    if (encrypted) {
        password = await askFor('zip のパスワード', '', { secret: true });
        if (password === null || !password) return;
    }
    say(`${kind} を作っています…`);
    const r = await ask('compress', { pane: state.focus, kind, name, password });
    if (!r) return;
    state[state.focus] = r.pane;
    draw(state.focus);
    if (r.errors.length) say(r.errors.join('  /  '), true);
    else say(`${r.made} を作りました（${r.ok} 件）`);
}

async function cmdExtract() {
    const r = await ask('extract', { pane: state.focus });
    if (!r) return;
    state[state.focus] = r.pane;
    draw(state.focus);
    if (r.errors.length) say(r.errors.join('  /  '), true);
    else say(`${r.from} を展開しました（${r.ok} 件）`);
}

async function cmdArchiveList() {
    const r = await ask('archivelist', { pane: state.focus });
    if (!r) return;
    show(r.name, `${r.members.length} 件`,
        r.members.map((m) => ({
            n: m.is_dir ? '' : human(m.size),
            label: m.name,
            sub: m.is_dir ? '' : `圧縮後 ${human(m.compressed)}`,
        })),
        { foot: 'Esc 閉じる' });
}

// ---- Version control, duplicates, redo ----

async function cmdLog(justThisFile) {
    const r = await ask('log', { pane: state.focus, file: justThisFile });
    if (!r) return;
    show(r.of ? `${r.of} の履歴` : 'コミットログ',
        `${r.kind}   ${r.commits.length} 件`,
        r.commits.map((c) => ({ n: c.date, label: c.subject, sub: `${c.author}  ${c.hash}`, hash: c.hash })),
        {
            foot: 'Enter そのコミットの差分   Esc 閉じる',
            pick: (row) => cmdVcsDiff(row.hash),
        });
}

/// A diff, shown the way a diff reads: the sign in its own column, so `+` and
/// `-` line up down the page instead of hiding at the start of the text.
async function cmdVcsDiff(hash) {
    const r = await ask('vcsdiff', { pane: state.focus, ...(hash ? { hash } : {}) });
    if (!r) return;
    show(hash ? `差分 ${hash}` : '差分', `${r.lines.length} 行`,
        r.lines.map((t) => ({
            n: t.startsWith('+') ? '+' : t.startsWith('-') ? '-' : t.startsWith('@') ? '@' : '',
            label: t,
        })),
        { foot: 'Esc 閉じる' });
}

async function cmdVcs(what) {
    const r = await ask(what, { pane: state.focus });
    if (!r) return;
    state[state.focus] = r.pane;
    draw(state.focus);
    const verb = { stage: 'git add', unstage: 'git reset', discard: '破棄' }[what];
    say(`${r.count} 件を ${verb} しました`);
}

async function cmdDedup() {
    say('中身を突き合わせています…');
    const r = await ask('dedup', { pane: state.focus });
    if (!r) return;
    if (!r.groups.length) { say('同じ中身のファイルはありません'); return; }
    const rows = [];
    r.groups.forEach((g, i) => {
        g.forEach((p, j) => rows.push({ n: j === 0 ? `${i + 1}` : '', label: p }));
    });
    show('中身が同じファイル', `${r.groups.length} 組`, rows, { foot: 'Esc 閉じる' });
}

/// `:view`, and the terminal build's aliases for it — `:icons` on its own
/// means `:view icons`, which is how fingers actually type it.
async function cmdView(arg, invokedAs) {
    const mode = (arg || invokedAs || '').trim();
    const map = { grid: 'icons', finder: 'details' };
    if (!mode || mode === 'view') {
        setView(VIEWS[(VIEWS.indexOf(viewMode) + 1) % VIEWS.length]);
    } else {
        setView(map[mode] || mode);
    }
    say(`表示: ${viewMode}`);
}

async function redo() {
    const r = await ask('redo', {});
    if (!r) return;
    state.left = r.left;
    state.right = r.right;
    draw('left');
    draw('right');
    say(r.said);
}

/// `Z` — the places this session has been, newest first.
///
/// The terminal build's jump list is history plus bookmarks; bookmarks need
/// somewhere to live, which is the same open question as the look and the
/// editor style, so this is the half that needs nothing written down.
async function cmdJump() {
    // Recents and bookmarks together, which is what the terminal build's `Z`
    // is: "fuzzy-jump to a recent / bookmarked directory". Bookmarks first —
    // a place worth naming outranks a place merely visited.
    const rows = [];
    const seen = new Set();
    const marks = await ask('shortcuts', {});
    if (marks) {
        for (const x of marks.rows) {
            if (x.target && !seen.has(x.target)) {
                seen.add(x.target);
                rows.push({ n: '★', label: x.name, sub: x.target, target: x.target });
            }
        }
    }
    for (const which of ['left', 'right']) {
        const r = await ask('history', { pane: which });
        if (!r) continue;
        for (const p of [r.cwd, ...r.back, ...r.forward]) {
            if (!seen.has(p)) {
                seen.add(p);
                rows.push({ n: '', label: p, target: p });
            }
        }
    }
    if (!rows.length) { say('まだどこにも行っていません'); return; }
    show('行き先', `${rows.length} 件（★ = 登録済み）`, rows, {
        // The terminal build calls `Z` a *fuzzy* jump, and a list of paths is
        // exactly the list where typing three letters beats arrowing.
        filter: true,
        hint: '打って絞り込み（パスの一部）',
        foot: '打って絞る   Enter そこへ   Esc 閉じる',
        pick: (row) => { closeReport(); revealPath(row.target, true); },
    });
}

// ─────────────────────────────────────────────────────────────────────────
// The shell.
//
// **The terminal is in the engine.** Electron's usual answer is node-pty — a
// native module wanting a C++ toolchain and a rebuild against Electron's ABI,
// which is the several gigabytes this project already refused once. cian-pty
// is portable-pty and vt100, both plain Rust, and it is the same emulator the
// terminal build reads its shell through. So the window here knows nothing
// about escape sequences: it is handed a grid and it draws it. Interpreting
// them is a job with twenty years of edge cases in it, and a second answer to
// any of them is how two front ends stop looking like one program.
// ─────────────────────────────────────────────────────────────────────────
const term = { on: false, focused: false, rows: 24, cols: 80, tabs: 1, tab: 0, showing: null, names: [] };

/// How many cells fit. Measured from a real character rather than assumed:
/// the font is whatever the machine had, and three of the four looks disagree
/// about the size.
/// How big one character cell is, measured in the box the shell is drawn in.
///
/// The probe used to be a bare span dropped into the panel, which inherited
/// the *listing's* line height (--cell-h, made for rows you click on) and was
/// then multiplied by a hopeful 1.25 — so the panel was told it had seven
/// rows where twelve fitted, and two thirds of the shell was empty. It wears
/// `.sgrid` now, so it is measured under the rules the real thing is drawn
/// under, and ten lines are measured rather than one so rounding cannot
/// accumulate.
function measureCell() {
    const probe = document.createElement('div');
    probe.className = 'sgrid';
    probe.textContent = Array.from({ length: 10 }, () => 'M'.repeat(100)).join('\n');
    probe.style.cssText = 'position:absolute;visibility:hidden;left:-9999px;top:0;'
        + 'width:auto;height:auto;padding:0';
    el.sPanes.append(probe);
    const box = probe.getBoundingClientRect();
    const w = box.width / 100;
    const h = box.height / 10;
    probe.remove();
    return { w: w || 8, h: h || 20 };
}

/// The whole panel in cells. The engine divides this by each pane's share, so
/// a pane knows the width it actually has rather than the panel's — a shell
/// that thinks it is full width wraps at the wrong column, which is the
/// classic broken-split look.
function shellSize() {
    const { w, h } = measureCell();
    const box = el.sPanes.getBoundingClientRect();
    return {
        cols: Math.max(20, Math.floor((box.width - 16) / w)),
        rows: Math.max(4, Math.floor((box.height - 8) / h)),
    };
}

async function openShell(opts = {}) {
    const takeKeys = opts.focus !== false;
    el.shell.hidden = false;
    term.on = true;
    setShellFocus(takeKeys);
    const size = shellSize();
    term.rows = size.rows;
    term.cols = size.cols;
    const r = await ask('shellopen', { pane: state.focus, ...size });
    if (!r) { closeShell(); return; }
    takeShell(r);
    if (takeKeys) say('シェル — Esc でファイルへ戻る');
    else draw('left');
}

function closeShell() {
    term.on = false;
    setShellFocus(false);
    el.shell.hidden = true;
}

/// Focus without closing. The panel stays visible while the files have the
/// keys — which is the point of docking it rather than opening it instead.
function blurShell() {
    setShellFocus(false);
    say('ファイルへ戻りました（Shift+J でシェルへ）');
}

/// A reply that carries a screen and the strip that belongs beside it.
function takeShell(r) {
    if (r.gone) { closeShell(); return; }
    term.tabs = r.tabs ?? 1;
    term.tab = r.tab ?? 0;
    term.showing = r.showing ?? null;
    term.sync = !!r.sync;
    if (r.names) term.names = r.names;
    el.shell.classList.toggle('sync', term.sync);
    if (r.panes) layoutShell(r.panes);
}

/// Place the panes where the engine said, and draw each one's screen.
///
/// Absolute positions from fractions, because the layout is a tree the engine
/// has already turned into rectangles. Deriving it again here out of nested
/// boxes would be the same arithmetic written twice.
function layoutShell(panes) {
    const have = new Map([...el.sPanes.children].map((n) => [Number(n.dataset.id), n]));
    const want = new Set(panes.map((p) => p.id));
    for (const [id, node] of have) if (!want.has(id)) node.remove();
    for (const p of panes) {
        let node = have.get(p.id);
        if (!node) {
            node = document.createElement('div');
            node.className = 'sgrid';
            node.dataset.id = p.id;
            node.addEventListener('mousedown', () => focusPaneOf(p.id));
            el.sPanes.append(node);
        }
        node.style.left = `${p.x * 100}%`;
        node.style.top = `${p.y * 100}%`;
        node.style.width = `${p.w * 100}%`;
        node.style.height = `${p.h * 100}%`;
        node.classList.toggle('on', p.focused && term.focused);
        if (p.screen) drawShell(p.screen, node);
    }
}

async function focusPaneOf(id) {
    // Step until it lands: the engine owns the order, and asking it to move
    // one at a time is cheaper than teaching the window the tree.
    for (let i = 0; i < 8 && term.showing !== id; i++) {
        const r = await ask('shellfocus', { step: 1 });
        if (!r) return;
        takeShell(r);
    }
    setShellFocus(true);
}

/// `:theme` — pick a look by name, rather than cycling to it.
///
/// The switches menu walks them, which is right when there are four. Naming
/// one is what you want when you know which.
/// Every palette, the window's own three and cian-tui's eighteen, in one
/// list — because from where a person stands there is one question here.
function themeRows() {
    const rows = LOOKS.map(([, label], i) => ({
        n: !palette && i === look ? '●' : '',
        label,
        look: i,
    }));
    for (const name of palettes.keys()) {
        // No light/dark column: the gallery is live, so which way round a
        // palette goes is on the screen behind the list. A word saying so
        // would be describing what you can see.
        rows.push({ n: palette === name ? '●' : '', label: name, palette: name });
    }
    return rows;
}

/// Wear this one. `keep` is false while the cursor is only passing over it —
/// trying on eighteen palettes should not write eighteen settings.
function pickTheme(row, keep = true) {
    if (row.palette) {
        setPalette(row.palette, keep);
        if (keep) say(`配色: ${row.palette}`);
    } else {
        palette = null;
        setLook(row.look, keep);
        if (keep) say(`配色: ${LOOKS[row.look][1]}`);
    }
    // Move the ● with the choice. It used to be drawn once, when the list
    // opened, and then sat on whatever had been chosen before — pointing at
    // the wrong row of a list whose whole point is which row is on.
    if (report.on) {
        for (const r of report.rows) {
            r.n = (r.palette ? palette === r.palette : !palette && r.look === look) ? '●' : '';
        }
        drawReport();
    }
}

async function cmdTheme(name) {
    if (name) {
        const want = name.toLowerCase();
        if (palettes.has(want)) { setPalette(want); say(`配色: ${want}`); return; }
        const at = LOOKS.findIndex(([v, label]) =>
            v === name || label === name || (v || 'hakuji').startsWith(want));
        if (at >= 0) { setLook(at); say(`配色: ${LOOKS[at][1]}`); return; }
        // Named, not "no such theme": what was typed is the one thing the
        // person knows about, so the near misses are worth more than the
        // refusal.
        const near = [...palettes.keys()].filter((k) => k.includes(want)).slice(0, 4);
        say(near.length ? `${name}? — ${near.join('  ')}` : `${name} という配色はありません`, true);
        return;
    }
    // What was on before the gallery opened, so Esc can put it back. The
    // foot says "Esc 戻す" and a promise on the screen has to be kept.
    const was = palette ? { palette } : { look };
    const rows = themeRows();
    show('配色', `${rows.length} 種 — 上の ${LOOKS.length} つは窓のもの、あとは cian-tui のもの`,
        rows, {
            filter: true,
            hint: '打って絞り込み（dracula, light, …）',
            foot: '打って絞る   ↑↓ 選ぶだけで着せ替わります   Enter 決定   Esc 戻す',
            // Live, as the terminal build's gallery is: a palette is a thing
            // you look at, and choosing one from a list of names without
            // seeing it is choosing by memory.
            move: (row) => pickTheme(row, false),
            pick: (row) => { closeReport(); pickTheme(row); },
            leave: () => pickTheme(was, false),
        });
}

/// `:blame` — who last changed each line, in the gutter.
///
/// In the gutter rather than as a list, because the question is always about a
/// *particular* line: "when did this become like this". A separate window with
/// the same line numbers is the same information at arm's length.
let blameOn = false;
let blameMarks = [];

async function cmdBlame() {
    if (!viewer.on || !viewer.ed) { say('先にファイルを開いてください', true); return; }
    if (blameOn) {
        blameMarks = viewer.ed.deltaDecorations(blameMarks, []);
        blameOn = false;
        say('blame を消しました');
        return;
    }
    const r = await ask('blame', { pane: state.focus });
    if (!r) return;
    // One decoration per line, each carrying its own text: Monaco draws the
    // gutter, so the width sorts itself out and the code does not move.
    blameMarks = viewer.ed.deltaDecorations(blameMarks, r.lines.map((b, i) => ({
        range: new (window.monaco.Range)(i + 1, 1, i + 1, 1),
        options: {
            isWholeLine: true,
            before: {
                content: `${b.date} ${b.author}`.slice(0, 22).padEnd(22),
                inlineClassName: 'blame',
            },
        },
    })));
    blameOn = true;
    say(`${r.lines.length} 行の blame（もう一度 :blame で消えます）`);
}

/// `:enc` — read the open file again in another encoding.
///
/// The bytes are already in the engine, so this decodes rather than re-reads.
/// That matters for a log something is still writing to: re-reading would show
/// a different file than the one being looked at.
async function cmdEncoding(name) {
    if (!viewer.on || !viewer.ed) { say('先にファイルを開いてください', true); return; }
    const r = await ask('encoding', { as: name || undefined });
    if (!r) return;
    const model = viewer.ed.getModel();
    model.applyEdits([{ range: model.getFullModelRange(), text: r.lines.join('\n') }]);
    viewer.base = model.getAlternativeVersionId();
    viewer.dirty = false;
    const pretty = { Utf8: 'UTF-8', ShiftJis: 'Shift_JIS', Utf16Le: 'UTF-16LE', Utf16Be: 'UTF-16BE' };
    el.vAbout.textContent = `${pretty[r.encoding] || r.encoding}  ·  ${r.eol.toUpperCase()}`
        + `  ·  ${r.lines.length} 行`;
    say(`文字コード: ${pretty[r.encoding] || r.encoding}`);
}

/// The Markdown preview — the rendered document instead of the source.
///
/// **The one thing this build can do that the terminal cannot.** A terminal
/// draws Markdown in one face at one size; a window can set it, and a document
/// that is set properly is read faster than the same words in a grid.
///
/// The HTML comes from the engine, from the same parse cian-tui draws — so the
/// two never disagree about the program's own README — and it arrives already
/// escaped. A preview that runs what it finds is a preview that runs whatever
/// was in the repository somebody cloned.
let reading = false;

/// mermaid, loaded the first time a diagram appears — most Markdown has none,
/// and 3.4 MB is not a toll every preview should pay.
let mermaidLoading = null;

function loadMermaid() {
    if (mermaidLoading) return mermaidLoading;
    mermaidLoading = new Promise((ok, no) => {
        // The same trap monaco-vim fell into: a UMD bundle sees Monaco's AMD
        // `define` and takes the AMD branch, which cannot work here. With
        // `define` out of sight for the length of the load it lands on the
        // plain global instead.
        const savedDefine = window.define;
        window.define = undefined;
        const sc = document.createElement('script');
        sc.src = 'vendor/mermaid.js';
        sc.onload = () => {
            window.define = savedDefine;
            // strict: a README is a file from somewhere, and a diagram that
            // can run script is not a diagram.
            window.mermaid.initialize({ startOnLoad: false, securityLevel: 'strict' });
            ok(window.mermaid);
        };
        sc.onerror = () => {
            window.define = savedDefine;
            no(new Error('vendor/mermaid.js がありません — node gui/vendor.js'));
        };
        document.head.append(sc);
    });
    return mermaidLoading;
}

/// Draw every ```mermaid fence in the preview.
///
/// The terminal build folds these into an arrow list, because a terminal
/// cannot draw; this is the drawing. A diagram that fails to parse keeps its
/// source on screen with the reason — a blank where a diagram should be says
/// nothing, and the source at least says what was meant.
let mermaidSeq = 0;

async function drawDiagrams() {
    const fences = [...el.vRead.querySelectorAll('code.language-mermaid')];
    if (!fences.length) return;
    let mermaid;
    try {
        mermaid = await loadMermaid();
    } catch (e) { say(e.message, true); return; }
    const dark = isDark();
    // mermaid ships trebuchet, which has no Japanese and falls back silently
    // to whatever the system picks. Hand it the page's own body face so a
    // diagram's labels are set in the same type as the prose around them.
    const body = getComputedStyle(el.vRead).fontFamily;
    mermaid.initialize({
        startOnLoad: false,
        securityLevel: 'strict',
        theme: dark ? 'dark' : 'default',
        fontFamily: body,
        themeVariables: { fontFamily: body },
    });
    for (const code of fences) {
        const src = code.textContent;
        const pre = code.parentElement;
        try {
            mermaidSeq += 1;
            const { svg } = await mermaid.render(`cian-mermaid-${mermaidSeq}`, src);
            const box = document.createElement('div');
            box.className = 'diagram';
            box.innerHTML = svg;
            pre.replaceWith(box);
        } catch (e) {
            const why = document.createElement('div');
            why.className = 'diagram-error';
            why.textContent = `図として読めませんでした: ${String(e.message || e).split('\n')[0]}`;
            pre.before(why);
        }
    }
}

async function togglePreview2() {
    if (!viewer.on || !viewer.ed) { say('先にファイルを開いてください', true); return; }
    if (reading) {
        reading = false;
        el.vRead.hidden = true;
        el.vBody.hidden = false;
        viewer.ed.focus();
        say('ソースに戻りました');
        return;
    }
    const r = await ask('markdown', { lines: viewer.ed.getValue().split(/\r?\n/) });
    if (!r) return;
    // `innerHTML` on purpose, and only here: the engine escaped every piece of
    // text on the way out, and the markup is its own — not the file's.
    el.vRead.innerHTML = r.html;
    // Links go to the desktop's browser rather than replacing the preview.
    // A file manager that navigates away from itself is a file manager you
    // have to restart.
    for (const a2 of el.vRead.querySelectorAll('a[href]')) {
        a2.addEventListener('click', (e) => {
            e.preventDefault();
            const href = a2.getAttribute('href');
            if (/^https?:|^mailto:/i.test(href)) ask('openurl', { url: href });
            else say(`${href} — 相対リンクはまだ開けません`);
        });
    }
    reading = true;
    el.vBody.hidden = true;
    el.vRead.hidden = false;
    el.vRead.scrollTop = 0;
    drawDiagrams();
    say('プレビュー — Ctrl+E でソースに戻ります');
}

/// `:ws` — the characters you cannot see but a compiler can.
///
/// A trailing space, a tab where spaces were meant, an ideographic space that
/// arrived from a Japanese editor and looks exactly like a normal one. All
/// three break things silently, which is why showing them is a mode rather
/// than a hunt.
let wsOn = false;
function toggleWs() {
    if (!viewer.ed) { say('先にファイルを開いてください', true); return; }
    wsOn = !wsOn;
    viewer.ed.updateOptions({
        renderWhitespace: wsOn ? 'all' : 'selection',
        renderControlCharacters: wsOn,
        // The ideographic space is not whitespace to Monaco, so it needs the
        // unicode highlighter to be pointed at it — and it is the one of the
        // three that a person cannot spot by eye at all.
        unicodeHighlight: { ambiguousCharacters: wsOn, invisibleCharacters: wsOn },
    });
    say(wsOn ? '見えない文字を表示' : '見えない文字を隠しました');
}

let rulerOn = false;
function toggleRuler() {
    if (!viewer.ed) { say('先にファイルを開いてください', true); return; }
    rulerOn = !rulerOn;
    viewer.ed.updateOptions({ rulers: rulerOn ? [80, 100, 120] : [] });
    say(rulerOn ? '桁の目盛り: 80 / 100 / 120' : '目盛りを消しました');
}

/// `:s/old/new/g` — the same substitution language as the grep-wide replace,
/// because it is the same question asked of one file instead of many.
async function cmdSubstitute(spec) {
    await rewriteBuffer('substitute', { spec }, (r) => `${r.changed} 箇所を置換しました`);
}

/// The replace plan, with each line kept or dropped one at a time.
///
/// **Everything starts checked.** The common case is "yes, all of them", and
/// unchecking the exceptions is less work than checking the rest — which is
/// the terminal build's reasoning and it is right. Space unchecks; the count
/// on the header says how many are still going.
function showReplacePlan(spec, plan) {
    const picked = plan.changes.map(() => true);
    const draw = () => {
        const on = picked.filter(Boolean).length;
        show(`置換 ${spec}`,
            `${on} / ${plan.changes.length} 行   `
            + `${new Set(plan.changes.map((c) => c.path)).size} ファイル`
            + (plan.skipped.length ? `   飛ばした ${plan.skipped.length} 件` : ''),
            plan.changes.map((c, i) => ({
                n: (picked[i] ? '✓ ' : '  ') + (c.line + 1),
                label: c.path.split(/[\\/]/).pop() + '  ' + c.before,
                sub: c.after,
                at: i,
            })),
            {
                foot: 'Space 外す／戻す   a 全部   n 全部外す   Enter 実行   Esc やめる',
                act: {
                    ' ': () => { picked[report.at] = !picked[report.at]; keepPlace(draw); },
                    a: () => { picked.fill(true); keepPlace(draw); },
                    n: () => { picked.fill(false); keepPlace(draw); },
                },
                pick: async () => {
                    const going = plan.changes.filter((_, i) => picked[i]);
                    if (!going.length) { say('選ばれている行がありません', true); return; }
                    closeReport();
                    if (!await confirm(`${going.length} 行を置換します`,
                        `${new Set(going.map((c) => c.path)).size} ファイル — u では戻せません`)) {
                        say('やめました');
                        return;
                    }
                    const done = await ask('replaceapply', { changes: going });
                    if (!done) return;
                    await reread();
                    const bits = [`${done.files} ファイル ${done.lines} 行を置換`];
                    if (done.stale) bits.push(`${done.stale} 行は変わっていたので触らず`);
                    say(bits.join('   '), done.errors.length > 0);
                },
            });
    };
    draw();
}

/// Redraw a report without losing where the cursor was. `show` resets it, and
/// a list that jumps to the top every time you tick a box is a list you cannot
/// work down.
function keepPlace(redraw) {
    const at = report.at;
    redraw();
    report.at = Math.min(at, report.rows.length - 1);
    drawReport();
}

/// `:g/re/d` and `:v/re/d` — drop or keep every matching line.
///
/// The one line operation that filters rather than transforms, and the one
/// people reach for on a log: "everything except the heartbeats", once.
async function cmdLineFilter(pattern, keep) {
    if (!viewer.on || !viewer.ed) { say('先にファイルを開いてください', true); return; }
    const lines = viewer.ed.getValue().split(/\r?\n/);
    const r = await ask('grepdel', { lines, pattern, keep });
    if (!r) return;
    replaceAll(r.lines);
    say(keep ? `${r.removed} 行を落として、一致した行だけ残しました` : `${r.removed} 行を削除しました`);
}

/// `:combine` — join the next line up, with a space or without.
async function cmdCombine(spec) {
    if (!viewer.on || !viewer.ed) { say('先にファイルを開いてください', true); return; }
    const bang = /!$/.test(spec || '');
    const count = Math.max(2, Number((spec || '').replace('!', '').trim()) || 2);
    const lines = viewer.ed.getValue().split(/\r?\n/);
    const at = viewer.ed.getPosition().lineNumber - 1;
    const r = await ask('combine', { lines, at, count, space: !bang });
    if (!r) return;
    replaceAll(r.lines);
    viewer.ed.setPosition({ lineNumber: at + 1, column: 1 });
    say(`${r.joined} 行を連結しました`);
}

/// Put a whole new set of lines in, through the edit stack so `u` takes it
/// back. Every line operation ends here, which is why it is one function.
function replaceAll(lines) {
    const model = viewer.ed.getModel();
    viewer.ed.executeEdits('cian', [{
        range: model.getFullModelRange(),
        text: lines.join('\n'),
    }]);
    viewer.ed.pushUndoStop();
}

/// `Ctrl+Q` / `Alt+V` — a rectangle, and what can be done to one.
///
/// Monaco has rectangular *selection*; what it does not have is vim's verbs
/// for it — `I` and `A` put text down the left or right edge of every line at
/// once, which is the whole reason anybody selects a rectangle. Columns are
/// display columns, so a line with a tab in it lines up the way it looks.
async function blockEdit(what) {
    if (!viewer.on || !viewer.ed) { say('先にファイルを開いてください', true); return; }
    const sels = viewer.ed.getSelections() || [];
    if (!sels.length) { say('矩形選択がありません', true); return; }
    const top = Math.min(...sels.map((s) => s.startLineNumber)) - 1;
    const bottom = Math.max(...sels.map((s) => s.endLineNumber)) - 1;
    const left = Math.min(...sels.map((s) => Math.min(s.startColumn, s.endColumn))) - 1;
    const right = Math.max(...sels.map((s) => Math.max(s.startColumn, s.endColumn))) - 1;
    let text = '';
    if (what !== 'delete') {
        text = await askFor(
            { insert: '左端に入れる文字', append: '右端に足す文字', replace: '置き換える文字' }[what],
            '',
        );
        if (text === null) return;
    }
    const lines = viewer.ed.getValue().split(/\r?\n/);
    const r = await ask('block', { lines, what, top, bottom, left, right, text });
    if (!r) return;
    replaceAll(r.lines);
    say({ delete: '矩形を削除', insert: '左端に挿入', append: '右端に追加', replace: '矩形を置換' }[what]);
}

async function cmdDf() {
    const r = await ask('df', { pane: state.focus });
    if (!r) return;
    const pct = r.total ? Math.round((r.used / r.total) * 100) : 0;
    show('ディスクの空き', r.where, [
        { n: human(r.total), label: '全体' },
        { n: human(r.used), label: '使用中', sub: `${pct}%` },
        { n: human(r.available), label: '空き' },
    ], { foot: 'Esc 閉じる' });
}

/// `:head` / `:tail` — the ends of the file, without opening it. What a
/// log asks for first: the tail says what is happening, the head says when
/// it started.
async function cmdPeek(args, tail) {
    const n = Number(((args || '').match(/-n\s*(\d+)/) || [, 10])[1]) || 10;
    const r = await ask('peek', { pane: state.focus, n, tail });
    if (!r) return;
    show(`${tail ? 'tail' : 'head'} -n ${n}  ${r.name}`, `${r.rows.length} 行`,
        r.rows.map((t, i2) => ({ n: String(tail ? '' : i2 + 1), label: t })),
        { foot: 'Esc 閉じる' });
}

/// `:recent` — the files this session has opened, newest first.
const recentFiles = [];

function noteRecent(path, name) {
    const at = recentFiles.findIndex((x) => x.path === path);
    if (at >= 0) recentFiles.splice(at, 1);
    recentFiles.unshift({ path, name });
    if (recentFiles.length > 40) recentFiles.pop();
}

async function cmdRecent() {
    if (!recentFiles.length) { say('まだ何も開いていません'); return; }
    show('最近開いたファイル', `${recentFiles.length} 件`,
        recentFiles.map((x) => ({ label: x.name, sub: x.path, path: x.path })),
        {
            foot: 'Enter そこへ   Esc 閉じる',
            pick: (row) => { closeReport(); revealPath(row.path, false); },
        });
}

async function cmdVersion() {
    const w = await ask('where', {});
    show('cian', 'GUI 1.1.0 — cian-core の上の窓', [
        { label: '設定', sub: (w && w.config) || '(なし)' },
        { label: '書き込み先', sub: (w && w.writes) || '(なし)' },
        { label: 'エンジン', sub: 'cian-server（JSON lines / stdio）' },
    ], { foot: 'Esc 閉じる' });
}

async function cmdWc() {
    const r = await ask('wc', { pane: state.focus });
    if (!r) return;
    if (!r.rows.length) { say('数えられるファイルがありません'); return; }
    const sum = r.rows.reduce((a, x) => ({
        lines: a.lines + x.lines, words: a.words + x.words, bytes: a.bytes + x.bytes,
    }), { lines: 0, words: 0, bytes: 0 });
    show('行・単語・バイト',
        `${r.rows.length} ファイル   ${sum.lines.toLocaleString()} 行   `
        + `${sum.words.toLocaleString()} 語   ${human(sum.bytes)}`,
        r.rows.map((x) => ({
            n: x.lines.toLocaleString(),
            label: x.name,
            sub: `${x.words.toLocaleString()} 語   ${human(x.bytes)}`,
        })),
        { foot: 'Esc 閉じる' });
}

/// `:where` — which of the config files cian actually found.
///
/// The question exists because a copy beside the executable wins over the one
/// in the home directory, and that is not where anybody looks first. Editing
/// the wrong file and wondering why nothing changed is the failure this
/// answers.
async function cmdWhere() {
    const r = await ask('where', {});
    if (!r) return;
    show('設定の場所', '書き込み先: ' + (r.writes || '(不明)'), [
        { label: 'init.lua', sub: r.config || '(なし)' },
        { label: 'state.toml', sub: r.state || '(なし)' },
        { label: 'shortcuts.lua', sub: r.shortcuts || '(なし)' },
        { label: 'macro.lua', sub: r.macros || '(なし)' },
    ], { foot: 'Esc 閉じる' });
}

async function cmdMarkGlob(glob, on) {
    const r = await ask('markglob', { pane: state.focus, glob, on });
    if (!r) return;
    state[state.focus] = r;
    draw(state.focus);
    say(`${glob}: ${r.matched} 件を${on ? 'マーク' : '解除'}`);
}

/// `:copyto` / `:moveto` — somewhere that is not the other pane.
async function cmdTo(what, dest) {
    const r = await ask(what, { pane: state.focus, dest });
    if (!r) return;
    beginOp(r, r.kind, r.kind === 'move' ? '移動' : 'コピー');
    if (!r.queued) say(`${r.count} 件を ${r.dest} へ`);
}

/// `:vi` / `:vim` / `:nvim` — the file, in that editor, in a new shell tab.
/// The tab is the terminal build's arrangement: the editor gets a real
/// terminal, and closing it brings you back to the files rather than to a
/// desktop window somewhere.
async function cmdEditorTab(_arg, invokedAs) {
    const pane = state[state.focus];
    const row = pane.entries[pane.cursor];
    if (!row || row.parent || row.is_dir) { say('ファイルを選んでください', true); return; }
    const editor = invokedAs && invokedAs !== 'vi' ? invokedAs : 'vi';
    if (!term.on) await openShell();
    const t = await ask('shelltab', { pane: state.focus, ...shellSize() });
    if (!t) return;
    takeShell(t);
    setShellFocus(true);
    await ask('run', { pane: state.focus, line: `${editor} %f` });
    say(`${editor} で開きました（F10 でタブごと閉じる）`);
}

async function cmdEditStyle(arg, invokedAs) {
    const want = (arg || (invokedAs === 'notepad' ? 'notepad' : '')).trim();
    const at = STYLES.findIndex(([v]) => v === want);
    if (at < 0) { say(':editstyle vim か :editstyle notepad', true); return; }
    setStyle(at);
    say(`エディタ: ${STYLES[at][1]}`);
}

/// `:scratch` — an empty buffer to think in. `:w` (or Ctrl+S) asks for a
/// name and it becomes a real file where you stand; closing it unsaved
/// costs nothing, which is the point of a scratchpad.
async function cmdScratch() {
    let monaco;
    try {
        monaco = await loadMonaco();
    } catch (e) { say(e.message, true); return; }
    if (viewer.on) await closeView(false);
    viewer.on = true;
    scratch.on = true;
    viewer.name = '下書き';
    el.view.hidden = false;
    el.vBody.hidden = false;
    el.vPic.hidden = true;
    makeEditor(monaco, '', 'plaintext');
    viewer.base = viewer.ed.getModel().getAlternativeVersionId();
    viewer.dirty = false;
    setStyle(style);
    el.vName.textContent = '下書き';
    el.vAbout.textContent = '保存されるまでどこにもありません';
    el.vFoot.textContent = 'Ctrl+S 名前を付けて保存   ·   Esc ×3 捨てる';
    viewer.ed.focus();
}

const scratch = { on: false };

async function saveScratch() {
    const name = await askFor('保存する名前', 'scratch.txt');
    if (name === null || !name) return false;
    const r = await ask('writefile', {
        pane: state.focus, name,
        text: viewer.ed.getValue() + '\n',
    });
    if (!r) return false;
    scratch.on = false;
    viewer.dirty = false;
    await reread();
    say(`${r.wrote} に保存しました`);
    await closeView(false);
    return true;
}

async function cmdLimit(spec) {
    const r = await ask('limit', { spec });
    if (!r) return;
    say(r.bps ? `転送の上限: ${human(r.bps)}/s` : '転送の上限: なし');
}

/// `:aicommit` — the staged diff in, a Conventional Commits message out,
/// **shown, not committed**. Enter commits with it; Esc walks away. The
/// model drafts; the person signs.
async function cmdAiCommit() {
    const r = await ask('aicommit', { pane: state.focus });
    if (!r) return;
    say('コミットメッセージを作っています…');
    aiWaiting = (answer) => {
        const msg = answer.trim();
        show('コミットメッセージ（案）', 'Enter でこのままコミット   Esc やめる',
            msg.split('\n').map((t) => ({ label: t })), {
                foot: 'Enter コミット   Esc やめる',
                pick: async () => {
                    closeReport();
                    if (!await confirm('この文でコミットします', msg)) { say('やめました'); return; }
                    const done = await ask('commit', { pane: state.focus, message: msg });
                    if (!done) return;
                    state[state.focus] = done.pane;
                    draw(state.focus);
                    say('コミットしました');
                },
            });
    };
}

/// The AI extension family. Everything here is metadata in, a *plan* out,
/// and nothing happens until the person says so on a list they can read —
/// which is the terminal build's arrangement and the only sane one for a
/// model with opinions about other people's files.

async function cmdAiScan(what) {
    const r = await ask(what, { pane: state.focus });
    if (!r) return;
    say(what === 'aijunk' ? '不要そうなものを探しています…' : '畳み方を考えています…');
    aiWaiting = async (payload) => {
        const rows = payload.rows || [];
        if (!rows.length) {
            say(what === 'aijunk' ? '明らかな不要ファイルは見つかりませんでした' : 'もう整っています、と言っています');
            return;
        }
        if (what === 'aijunk') {
            show('不要かもしれないもの', `${rows.length} 件 — AI の見立てです。確かめてから`,
                rows.map((x) => ({ label: x.name, sub: x.reason || '', path: x.path })),
                {
                    foot: 'Enter 全部をマーク（消すのは d で自分で）   Esc 閉じる',
                    pick: async () => {
                        closeReport();
                        const p = await ask('setmarks', {
                            pane: state.focus, paths: rows.map((x) => x.path),
                        });
                        if (!p) return;
                        state[state.focus] = p;
                        draw(state.focus);
                        say(`${rows.length} 件をマークしました — d で削除（ゴミ箱へ）`);
                    },
                });
            return;
        }
        show('畳み方の案', `${rows.length} 件 — 移すだけ。消しも改名もしません`,
            rows.map((x) => ({ n: '→ ' + x.folder, label: x.name, sub: x.reason || '', path: x.path, folder: x.folder })),
            {
                foot: 'Enter 実行（u で戻せます）   Esc やめる',
                pick: async () => {
                    closeReport();
                    if (!await confirm(`${rows.length} 件をサブフォルダへ移します`,
                        rows.map((x) => `${x.name} → ${x.folder}/`).join('\n'))) { say('やめました'); return; }
                    const done = await ask('organizeapply', {
                        pane: state.focus,
                        rows: rows.map((x) => ({ path: x.path, folder: x.folder })),
                    });
                    if (!done) return;
                    state[state.focus] = done.pane;
                    draw(state.focus);
                    if (done.errors.length) say(done.errors.join('  /  '), true);
                    else say(`${done.moved} 件を移しました（u で戻せます）`);
                },
            });
    };
}

async function cmdAiRename(instruction) {
    const r = await ask('airename', { pane: state.focus, instruction });
    if (!r) return;
    say('改名案を考えています…');
    aiWaiting = (payload) => {
        const rows = (payload.rows || []).filter((x) => x.new_name && !/[\\/]/.test(x.new_name));
        if (!rows.length) { say('変える案がありませんでした'); return; }
        // Through the same plan screen every bulk rename uses: clashes marked,
        // nothing moves until Enter.
        showRenamePlanRows(rows.map((x) => ({
            from: x.name, to: x.new_name, path: x.path,
            same: x.name === x.new_name, clash: false,
        })), `AI 改名案`);
    };
}

/// The bulk-rename confirmation, callable with rows from anywhere — the
/// pattern rename builds them from a pattern, the AI from an instruction.
function showRenamePlanRows(rows, title) {
    const changing = rows.filter((x) => !x.same);
    if (!changing.length) { say('変わる名前がありません'); return; }
    show(title, `${changing.length} 件が変わります`,
        rows.map((x) => ({ n: x.same ? '=' : '→', label: x.from, sub: x.to })),
        {
            foot: 'Enter 実行   Esc やめる',
            pick: async () => {
                closeReport();
                if (!await confirm(`${changing.length} 件の名前を変えます`,
                    changing.map((x) => `${x.from}  →  ${x.to}`).join('\n'))) { say('やめました'); return; }
                const done = await ask('renameapply', { rows: changing });
                if (!done) return;
                await reread();
                if (done.errors.length) say(done.errors.join('  /  '), true);
                else say(`${done.renamed} 件の名前を変えました`);
            },
        });
}

async function cmdAiSearch(query) {
    const r = await ask('aisearch', { pane: state.focus, query });
    if (!r) return;
    say('意味で探しています…');
    aiWaiting = (payload) => {
        const rows = payload.rows || [];
        if (!rows.length) { say('それらしいものは見つかりませんでした'); return; }
        show(`「${query}」らしいもの`, `${rows.length} 件 — AI の見立てです`,
            rows.map((x) => ({ label: x.path, sub: x.reason || '', full: x.full })),
            {
                foot: 'Enter そこへ   Esc 閉じる',
                pick: (row) => { closeReport(); revealPath(row.full, false); },
            });
    };
}

/// `:ime` — the input method, put where this moment wants it.
///
/// The one thing a keyboard program cannot survive is an IME that stays on
/// where the keys are commands: `j` becomes か and nothing moves. When
/// cian.ime{…} names a helper, cian switches to the no-IME source wherever it
/// is being driven and puts back whatever was on wherever it is being typed
/// into. `:ime` toggles the herding and says what is configured.
const ime = { on: false, want: null, broken: false };

async function cmdIme() {
    const r = await ask('ime', {});
    if (!r) return;
    ime.on = !ime.on;
    ime.want = null;
    ime.broken = false;
    say(ime.on
        ? `IME 連携: オン（いま ${r.current || '?'}）— 文字を打つ所だけ IME が戻ります`
        : 'IME 連携: オフ');
    if (ime.on) syncIme();
}

/// Is cian taking text right now, rather than being driven by commands?
///
/// The terminal build's whole rule, in this window's terms (ime.rs
/// `wants_text_input`): everything that reads a typed string says yes; the
/// file panes and a viewer being *read* say no. It used to be wired to
/// monaco-vim's mode changes alone, so the `:` line, the filter, every
/// askFor prompt, the finder and the shell were never herded at all — and in
/// notepad style, which is the default, nothing was.
function wantsTextInput() {
    // The viewer first, because Monaco holds the focus in a hidden textarea
    // whether or not it is taking text — the generic test below would say
    // "typing" in vim's normal mode, which is precisely the case this exists
    // to switch the IME *off* for.
    if (viewer.on && viewer.ed) return STYLES[style][0] === 'vim' ? vimTyping() : true;
    if (term.on && term.focused) return true;
    const at = document.activeElement;
    return !!at && (at.tagName === 'INPUT' || at.tagName === 'TEXTAREA' || at.isContentEditable);
}

/// Put the input method where this moment wants it.
///
/// Cheap to call — it compares one boolean and does nothing until the answer
/// changes — so it runs after every keystroke and every focus change rather
/// than being remembered at each of the dozen places that open a prompt.
function syncIme() {
    if (!ime.on || ime.broken) return;
    const want = wantsTextInput();
    if (ime.want === want) return;
    ime.want = want;
    window.cian.call('ime', { do: want ? 'restore' : 'off' }).catch((e) => {
        // Said once, and then left alone. A helper that is not there fails
        // on every switch, and a message per keystroke would bury the one
        // that matters.
        ime.broken = true;
        say(`IME 連携を止めました — ${e.message}`, true);
    });
}

// Once per turn round the event loop, which is where the terminal build calls
// it. A capture listener always fires (an overlay's stopPropagation cannot
// reach it), and the microtask runs after the handlers have moved the state
// the answer depends on.
document.addEventListener('keydown', () => queueMicrotask(syncIme), true);
document.addEventListener('focusin', syncIme);
document.addEventListener('focusout', () => queueMicrotask(syncIme));

async function cmdAiError() {
    const r = await ask('aierror', {});
    if (!r) return;
    say('シェルの画面を読んでいます…');
    aiWaiting = (answer) => {
        show('直近のエラーの説明', 'AI の答え — 確かめてから使ってください',
            String(answer).split('\n').map((t) => ({ label: t })), { foot: 'Esc 閉じる' });
    };
}

/// `:revealos` — hand the file to Finder, with it selected.
async function cmdRevealOs() {
    const r = await ask('revealos', { pane: state.focus });
    if (r) say(`${r.revealed} を Finder で表示しました`);
}

async function cmdEditExternal() {
    const r = await ask('editexternal', { pane: state.focus });
    if (!r) return;
    say(`${r.name} を ${r.editor} で開きました`);
}

/// F12 — give the shell the window, or give it back.
///
/// Two thirds of the height is not enough to read a build's output and too
/// much to keep a listing usable; the answer everywhere else is a key that
/// swaps between them rather than a compromise that suits neither.
/// F12 — the surface the keys are in fills the window.
///
/// The terminal build's `toggle_zoom` (keys.rs:363) zooms *the focused
/// surface*: standing in a file pane, that pane; standing in the shell, the
/// shell. This only ever grew the shell, whichever pane you were in — so F12
/// from a listing made the thing you were not looking at bigger.
function zoomFocused() {
    const now = el.work.dataset.zoom;
    if (now) {
        el.work.dataset.zoom = '';
        say('戻しました');
    } else if (term.on && term.focused) {
        el.work.dataset.zoom = 'shell';
        say('シェルを広げました（F12 で戻る）');
    } else {
        el.work.dataset.zoom = 'files';
        say(`${state.focus === 'left' ? '左' : '右'}ペインを広げました（F12 で戻る）`);
    }
    // Whatever just changed shape, the shell's idea of its own size is stale.
    if (term.on) ask('shellresize', shellSize());
    measureFoot();
}

/// The two dividers, moved by Ctrl+Shift+arrow.
///
/// `main` is the share given to the *files* and `panes` the share given to
/// the left pane, which is how the terminal build holds them (`main_pct`,
/// `panes_pct`). The help has listed this key since the beginning and the
/// listing had no handler for it at all — only the shell's inner splits did.
const layout = { main: 75, panes: 50 };
const MIN_PCT = 15;
const STEP_PCT = 4;

function applyLayout(remember = true) {
    const clamp = (v) => Math.max(MIN_PCT, Math.min(100 - MIN_PCT, v));
    layout.main = clamp(layout.main);
    layout.panes = clamp(layout.panes);
    const r = document.documentElement.style;
    r.setProperty('--main-pct', `${layout.main}%`);
    r.setProperty('--panes-pct', `${layout.panes}%`);
    if (term.on) ask('shellresize', shellSize());
    if (remember) {
        ask('remember', { key: 'gui_main_pct', value: String(layout.main) });
        ask('remember', { key: 'gui_panes_pct', value: String(layout.panes) });
    }
}

function resizeSplit(key) {
    // In the shell, the arrow first tries the nearest inner split along that
    // axis; only when there is none does it move the files|shell divider.
    // That is the terminal build's order, and it is what makes the key mean
    // "make the thing I am looking at bigger" in both places.
    if (term.on && term.focused) {
        const wider = key === 'ArrowRight' || key === 'ArrowDown';
        const down = key === 'ArrowUp' || key === 'ArrowDown';
        ask('shellresizepane', { wider, down }).then((r) => {
            if (r && r.moved) { takeShell(r); return; }
            // No split that way: grow or shrink the whole panel instead.
            if (key === 'ArrowUp') { layout.main -= STEP_PCT; applyLayout(); }
            else if (key === 'ArrowDown') { layout.main += STEP_PCT; applyLayout(); }
            if (r) takeShell(r);
        });
        return;
    }
    if (key === 'ArrowRight') layout.panes += STEP_PCT;
    else if (key === 'ArrowLeft') layout.panes -= STEP_PCT;
    // Down gives the files more room, which is to say the shell less.
    else if (key === 'ArrowDown') layout.main += STEP_PCT;
    else if (key === 'ArrowUp') layout.main -= STEP_PCT;
    else return;
    applyLayout();
    say(`ファイル ${layout.main}%   左ペイン ${layout.panes}%`);
}

/// `:preview` — follow the cursor.
///
/// Off by default and deliberately: reading every file the cursor passes over
/// is a lot of disk for a feature you want on the ten seconds you are looking
/// for something. On, it is the fastest way to find "the one with the error
/// in it".
const preview = { on: false };

function togglePreview() {
    preview.on = !preview.on;
    say(preview.on ? 'プレビュー: カーソルを追います' : 'プレビューを止めました');
    if (preview.on) showPreview();
    else if (viewer.on) closeView(false);
}

let previewSoon = null;
function showPreview() {
    if (!preview.on) return;
    // A beat behind the cursor. Held down, `j` would otherwise open every file
    // it passes, and the one you stop on is the only one that matters.
    clearTimeout(previewSoon);
    previewSoon = setTimeout(async () => {
        const pane = state[state.focus];
        const row = pane && pane.entries[pane.cursor];
        if (!row || row.parent || row.is_dir) return;
        if (viewer.on) await closeView(false);
        await lookInside();
        // The keys stay with the listing: this is a preview, not an opening.
        viewer.on = false;
    }, 250);
}

/// `Ctrl+Shift+Enter` — the commands init.lua keeps, sent to the shell.
///
/// `enter: false` types the line and stops, which is for the commands worth
/// reading before running — the terminal build's distinction, kept.
/// `:shellname` — what this shell tab is for.
///
/// The terminal build wants this too: a tab strip is only worth having if the
/// labels tell the tabs apart, and `shell 2` never does. Empty puts the
/// number back.
async function cmdShellName(name) {
    if (!term.on) { say('シェルが開いていません', true); return; }
    const now = (term.names || [])[term.tab] || '';
    // An optional argument arrives as `''`, not `undefined` — so testing for
    // undefined renamed the tab to nothing the moment `:shellname` was typed
    // on its own, which is the one spelling that should ask.
    const want = name ? name : await askFor('このシェルの名前', now);
    if (want === null) return;
    const r = await ask('shellrename', { name: want.trim() });
    if (!r) return;
    // takeShell lays the panes out again, and the strip is drawn from there.
    // Calling drawShell() by hand meant calling it with no screen at all.
    takeShell(r);
    say(want.trim() ? `シェル ${term.tab + 1}: ${want.trim()}` : '名前を外しました');
}

/// `:sessionlog` — everything this pane shows, teed to a file. On again to
/// stop. The frame turns carmine while it runs, which is the terminal
/// build's signal: a recorded shell should not look like an unrecorded one.
async function cmdShellLog() {
    if (!term.on) { say('シェルが開いていません', true); return; }
    const d = new Date();
    const p = (n) => String(n).padStart(2, '0');
    const name = `cian-shell-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}.log`;
    const r = await ask('shelllog', { pane: state.focus, name });
    if (!r) return;
    el.shell.classList.toggle('logging', !!r.logging);
    if (r.logging) say(`記録中: ${r.logging}`);
    else say(`記録を止めました: ${r.stopped || ''}`);
}

async function cmdSnippets() {
    const r = await ask('snippets', {});
    if (!r) return;
    if (!r.rows.length) { say('スニペットがありません（init.lua の cian.snippets）'); return; }
    show('スニペット', `${r.rows.length} 件`, r.rows.map((x) => ({
        n: x.enter ? '⏎' : '',
        label: x.name,
        sub: x.cmd,
        cmd: x.cmd,
        enter: x.enter,
    })), {
        foot: 'Enter シェルへ送る   Esc 閉じる',
        pick: async (row) => {
            closeReport();
            if (!term.on) await openShell();
            await ask('shellinput', { text: row.cmd + (row.enter ? '\n' : '') });
            setShellFocus(true);
            say(row.enter ? `${row.label} を実行` : `${row.label} を置きました — Enter で実行`);
        },
    });
}

async function cmdSync() {
    if (!term.on) { say('シェルが開いていません', true); return; }
    const r = await ask('shellsync', {});
    if (!r) return;
    takeShell(r);
    say(r.sync ? '同期入力: 全ペインに送ります' : '同期入力を止めました');
}

async function splitShell(down) {
    const r = await ask('shellsplit', { pane: state.focus, down, ...shellSize() });
    if (!r) return;
    takeShell(r);
    say(down ? '上下に分割' : '左右に分割');
}

/// Close one shell pane — the focused one, or the named one when a shell
/// has ended on its own.
///
/// Asked for first when it is a deliberate close, as the terminal build asks:
/// a split pane may be holding a program that is still running, and Shift+F10
/// is one key away from Shift+F9.
async function closePane(id) {
    const byHand = id === undefined;
    if (byHand && !await confirm('この分割パネルを閉じます',
        '動いているプログラムがあれば終わります')) { say('やめました'); return; }
    const r = await ask('shellpaneclose', id === undefined ? {} : { id });
    if (!r) return;
    if (r.gone) { closeShell(); say('シェルを閉じました'); return; }
    takeShell(r);
    if (byHand) say('分割パネルを閉じました');
}

async function shellTab() {
    if (!term.on) { await openShell(); return; }
    const r = await ask('shelltab', { pane: state.focus, ...shellSize() });
    if (!r) return;
    takeShell(r);
    say(`シェル ${term.tab + 1} / ${term.tabs}`);
}

async function goTabOfShell(at) {
    const r = await ask('shellgo', { at });
    if (!r) return;
    takeShell(r);
    say(`シェル ${term.tab + 1} / ${term.tabs}`);
}

async function shellGo(how) {
    const r = await ask('shellgo', how);
    if (!r) return;
    takeShell(r);
    say(`シェル ${term.tab + 1} / ${term.tabs}`);
}

/// Close the whole shell tab — every split pane in it.
///
/// Asked for, as the terminal build asks: F10 sits one key from F9, and the
/// difference between them is a tab appearing and a tab with four panes in it
/// disappearing.
async function shellCloseTab() {
    if (!await confirm('このシェルタブを閉じます（分割ごと）',
        '動いているプログラムがあれば終わります')) { say('やめました'); return; }
    const r = await ask('shellclose', {});
    if (!r) return;
    if (r.gone) { closeShell(); say('シェルを閉じました'); return; }
    takeShell(r);
    say(`シェル ${term.tab + 1} / ${term.tabs}`);
}

function drawShell(screen, into) {
    // A pane that is not on screen keeps running — that is the point of tabs —
    // and keeps sending screens. Without its own box to go in, drawing it
    // would let a build scrolling in another tab stamp itself over this one.
    const node = into || el.sPanes.querySelector(`.sgrid[data-id="${screen.id}"]`);
    if (!node) return;
    if (screen.id === term.showing) {
        term.rows = screen.rows;
        term.cols = screen.cols;
        // The tab strip, spelled the way the terminal build spells it, and
        // drawn even for one tab — the strip is where you learn that F9 makes
        // another, which a heading reading "シェル" never told anybody.
        el.sTabs.replaceChildren(...Array.from({ length: Math.max(1, term.tabs) }, (_, i) => {
            const t = document.createElement('span');
            // Its name where it has one. Four tabs called `shell 1..4` are
            // four tabs you have to open to tell apart, and the reason for
            // the second one is always that the first is busy with something
            // in particular.
            const name = (term.names || [])[i];
            t.textContent = name || `shell ${i + 1}`;
            t.title = name ? `${name}（shell ${i + 1}）` : `shell ${i + 1}`;
            if (i === term.tab) t.className = 'on';
            t.addEventListener('mousedown', () => goTabOfShell(i));
            // Double-click to rename, the way every tab strip renames.
            t.addEventListener('dblclick', () => { goTabOfShell(i); cmdShellName(); });
            return t;
        }));
        // What the shell itself says it is: `user@host: cwd`, which is the
        // only part of this bar carrying information.
        el.sTitle.textContent = screen.title || '';
        el.sAbout.textContent = `${screen.cols}×${screen.rows}`
            + (screen.scrollback ? `   ↑ ${screen.scrollback} 行戻っています` : '');
    }
    const frag = document.createDocumentFragment();
    screen.lines.forEach((runs, row) => {
        const div = document.createElement('div');
        // The cursor is drawn by splitting the run it lands in, because a cell
        // is not an element here — runs are, and a run is however many cells
        // looked the same.
        let col = 0;
        for (const run of runs) {
            const text = run.t;
            const onThisRun = !screen.hidden && screen.cursor.row === row
                && screen.cursor.col >= col && screen.cursor.col < col + text.length;
            if (!onThisRun) {
                div.append(styled(run, text));
                col += text.length;
                continue;
            }
            const at = screen.cursor.col - col;
            if (at > 0) div.append(styled(run, text.slice(0, at)));
            const cur = styled(run, text.slice(at, at + 1) || ' ');
            cur.classList.add('cur');
            div.append(cur);
            if (at + 1 < text.length) div.append(styled(run, text.slice(at + 1)));
            col += text.length;
        }
        if (!div.childNodes.length) div.append(document.createTextNode(' '));
        frag.append(div);
    });
    node.replaceChildren(frag);
}

function styled(run, text) {
    const span = document.createElement('span');
    span.textContent = text;
    if (run.f) span.style.color = run.f.startsWith('c') ? `var(--${run.f})` : run.f;
    if (run.b) span.style.background = run.b.startsWith('c') ? `var(--${run.b})` : run.b;
    if (run.bold) span.style.fontWeight = '600';
    if (run.it) span.style.fontStyle = 'italic';
    if (run.ul) span.style.textDecoration = 'underline';
    if (run.inv) span.classList.add('inv');
    return span;
}

/// What a key means to a shell.
///
/// Not a lookup table of everything — the printable characters are themselves,
/// and only the ones that are not a character need naming. Ctrl+letter is the
/// arithmetic it has always been: the letter's position in the alphabet.
function shellBytes(e) {
    const k = e.key;
    if (e.ctrlKey && k.length === 1) {
        const up = k.toUpperCase();
        if (up >= 'A' && up <= 'Z') return String.fromCharCode(up.charCodeAt(0) - 64);
    }
    const named = {
        Enter: '\r', Tab: '\t', Backspace: '\x7f', Escape: '\x1b',
        ArrowUp: '\x1b[A', ArrowDown: '\x1b[B', ArrowRight: '\x1b[C', ArrowLeft: '\x1b[D',
        Home: '\x1b[H', End: '\x1b[F', Delete: '\x1b[3~',
        PageUp: '\x1b[5~', PageDown: '\x1b[6~',
    };
    if (named[k]) return named[k];
    if (k.length === 1) return e.altKey ? `\x1b${k}` : k;
    return null;
}

/// Select text in the shell with the mouse; it is on the clipboard the
/// moment the button comes up. The terminal build's gesture — a terminal
/// where selecting is copying is a terminal you never reach for Cmd+C in.
document.addEventListener('mouseup', () => {
    if (!term.on) return;
    const sel = window.getSelection();
    if (!sel || sel.isCollapsed) return;
    if (!el.sPanes.contains(sel.anchorNode)) return;
    const text = sel.toString();
    if (!text.trim()) return;
    navigator.clipboard.writeText(text);
    say(`${text.length} 文字をコピー`);
});

document.addEventListener('keydown', (e) => {
    if (!term.on || !term.focused) return;
    // Esc hands the keys back to the files. A shell wants Esc too — vi lives
    // in one — so it is the one key that has to be pressed twice to reach it,
    // the same bargain the terminal build makes.
    if (e.key === 'Escape' && !escTwice()) {
        e.stopPropagation();
        e.preventDefault();
        blurShell();
        return;
    }
    // The panel's own keys, before the shell's. F-keys are cian's here for
    // the same reason they are in the terminal build: a shell almost never
    // wants them, and a panel with no way to open a second tab is a panel you
    // leave to run one thing.
    if (e.key === 'F9' && !e.shiftKey) { e.stopPropagation(); e.preventDefault(); shellTab(); return; }
    if (e.key === 'F12' && e.shiftKey) {
        e.stopPropagation();
        e.preventDefault();
        ask('shellpanezoom', {}).then((r) => {
            if (!r) return;
            takeShell(r);
            say(r.zoom ? 'このペインだけを表示（Shift+F12 で戻る）' : '分割に戻しました');
        });
        return;
    }
    if (e.key === 'F12') {
        e.stopPropagation();
        e.preventDefault();
        zoomFocused();
        return;
    }
    if (e.key === 'F10' && !e.shiftKey) { e.stopPropagation(); e.preventDefault(); shellCloseTab(); return; }
    // The terminal build's three: split, split the other way, close the pane.
    if (e.shiftKey && (e.key === 'F8' || e.key === 'F9' || e.key === 'F10')) {
        e.stopPropagation();
        e.preventDefault();
        if (e.key === 'F10') closePane();
        else splitShell(e.key === 'F9');
        return;
    }
    // Ctrl+S here is not save — there is nothing to save in a shell — it is
    // "say this to all of them", which is what splits are for.
    if (e.key === 's' && (e.ctrlKey || e.metaKey)) {
        e.stopPropagation();
        e.preventDefault();
        ask('shellsync', {}).then((r) => {
            if (!r) return;
            takeShell(r);
            say(r.sync ? '同期入力: 全ペインに送ります' : '同期入力を止めました');
        });
        return;
    }
    // Ctrl+Shift+arrow drags the border the focused pane sits against.
    if (e.ctrlKey && e.shiftKey && e.key.startsWith('Arrow')) {
        e.stopPropagation();
        e.preventDefault();
        const down = e.key === 'ArrowUp' || e.key === 'ArrowDown';
        const wider = e.key === 'ArrowRight' || e.key === 'ArrowDown';
        ask('shellresizepane', { wider, down }).then((r) => r && takeShell(r));
        return;
    }
    // F1-F8 go straight to a tab; the pane keys are the Shift ones.
    if (/^F[1-8]$/.test(e.key) && !e.shiftKey && !e.ctrlKey) {
        e.stopPropagation();
        e.preventDefault();
        goTabOfShell(Number(e.key.slice(1)) - 1);
        return;
    }
    if (e.shiftKey && (e.key === 'F1' || e.key === 'F2')) {
        e.stopPropagation();
        e.preventDefault();
        ask('shellfocus', { step: e.key === 'F1' ? -1 : 1 }).then((r) => r && takeShell(r));
        return;
    }
    if (e.key === 'F1' || e.key === 'F2') {
        e.stopPropagation();
        e.preventDefault();
        shellGo({ step: e.key === 'F1' ? -1 : 1 });
        return;
    }
    // Scrolling back through what has gone past, rather than into the shell.
    if (e.shiftKey && (e.key === 'PageUp' || e.key === 'PageDown')) {
        e.stopPropagation();
        e.preventDefault();
        scrollShell(e.key === 'PageUp' ? -term.rows : term.rows);
        return;
    }
    const bytes = shellBytes(e);
    if (bytes === null) {
        // A key the shell cannot encode still must not fall through to the
        // listing — F3 used to open the viewer over a shell being typed in.
        // Propagation stops; the browser default is left alone.
        e.stopPropagation();
        return;
    }
    e.stopPropagation();
    e.preventDefault();
    ask('shellinput', { text: bytes });
}, true);

/// Two Escs in quick succession go through to the shell; one comes back to the
/// files. Anything else in between resets it.
let lastEsc = 0;
function escTwice() {
    const now = performance.now();
    const twice = now - lastEsc < 500;
    lastEsc = twice ? 0 : now;
    return twice;
}

async function scrollShell(lines) {
    const r = await ask('shellscroll', { lines });
    if (r) takeShell(r);
}

// ─────────────────────────────────────────────────────────────────────────
// Dropping files in from the desktop.
//
// The one thing a window can do that a terminal can only imitate. It **moves**
// them, which is what the terminal build's drop does and what dragging between
// two folders means everywhere else — and it asks first, by name, because a
// drop is the easiest gesture in the whole program to make by accident.
// ─────────────────────────────────────────────────────────────────────────
for (const which of ['left', 'right']) {
    const pane = el[which];
    // Clicking a pane puts the keys in it — anywhere in it, and whether or
    // not there is a row under the pointer.
    //
    // The row handlers moved the *cursor* and the current pane, which is not
    // the same question: with the shell focused, clicking a listing left the
    // keyboard in the shell, so neither surface looked right. Registered on
    // the pane once rather than on every row every repaint, so an empty pane
    // and the path line take focus too.
    pane.addEventListener('mousedown', () => {
        setShellFocus(false);
        state.focus = which;
        draw('left');
        draw('right');
    });
    pane.addEventListener('dragover', (e) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        pane.classList.add('dropping');
    });
    pane.addEventListener('dragleave', () => pane.classList.remove('dropping'));
    pane.addEventListener('drop', async (e) => {
        e.preventDefault();
        pane.classList.remove('dropping');
        const paths = [...e.dataTransfer.files]
            .map((f) => window.cian.pathOf(f))
            .filter(Boolean);
        if (!paths.length) { say('落とされたものの場所が分かりません', true); return; }
        const dest = state[which];
        if (!dest) return;
        // A drop lands in `pane.cwd`, and on a remote pane that is still the
        // *local* directory from before the connection — the files would move
        // somewhere real and invisible, which is the worst combination.
        const names = paths.map((p) => p.split(/[\\/]/).pop());
        if (dest.remote) {
            if (!await confirm(`${paths.length} 件を ${dest.remote} へアップロードします`, names.join('\n'))) {
                say('やめました');
                return;
            }
            const up = await ask('uploadpaths', { pane: which, paths });
            if (!up) return;
            state[which] = up;
            draw(which);
            if (up.errors.length) say(up.errors.join('  /  '), true);
            else say(`${up.ok} 件をアップロードしました`);
            return;
        }
        if (!await confirm(`${paths.length} 件を ${dest.cwd} へ移動します`, names.join('\n'))) {
            say('やめました');
            return;
        }
        const r = await ask('drop', { pane: which, paths });
        if (!r) return;
        beginOp(r, 'move', '移動');
    });
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
            running.done = msg.done;
            running.total = Math.max(msg.total, running.total);
            running.bytes = msg.bytes ?? 0;
            running.bytesTotal = msg.bytes_total ?? 0;
            running.ms = msg.ms ?? 0;
            running.path = msg.path || '';
            prog.stalledAt = performance.now();
            drawProg();
            say(`${running.verb}中… ${msg.done} / ${running.total}  ${base(msg.path)}`);
            return;

        case 'done': {
            if (!running || msg.op !== running.op) {
                // A job cancelled while it was still in the queue: it never
                // ran, so nothing here is tracking it, and it still has to be
                // said — silence would read as "the cancel did not take".
                if (msg.cancelled) say(`#${msg.op} を取り消しました`);
                return;
            }
            const verb = running.verb;
            running = null;
            el.prog.hidden = true;
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

        case 'ai': {
            const hand = aiWaiting;
            aiWaiting = null;
            if (msg.error) { say(msg.error, true); return; }
            // A plain answer hands over its text; a structured one (rows and
            // a `what`) hands over the whole payload.
            if (hand) await hand(msg.rows ? msg : msg.answer);
            return;
        }

        case 'shell':
            if (term.on) drawShell(msg);
            return;

        case 'shellnote':
            say(msg.note, true);
            return;

        case 'shellexit':
            // A shell that ended by itself — `exit`, Ctrl+D, a crash. Its
            // pane goes; the others keep running. Nothing is asked, because
            // the person already said so by typing exit.
            if (term.on) await closePane(msg.id);
            return;

        case 'finding':
            if (finder.open) el.findFoot.textContent = `${msg.found} 件を見ています…`;
            return;

        case 'found':
            if (!finder.open) return;
            // The walk is over. Left true, the very next drawHits() painted
            // "（まだ探しています）" over this line forever.
            finder.walking = false;
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

/// `Ctrl+=` / `Ctrl+-` / `Ctrl+0` — the window's own type size.
///
/// The terminal build cannot do this: the font belongs to the emulator, so it
/// asks the emulator to change it and remembers a point size in `font_level`.
/// This build owns its window, so it just changes it — and keeps the number
/// under `gui_font`, because pixels here and points there are not the same
/// number even though they are the same idea.
const FONT = { min: 10, max: 28, at: 15 };

/// What size this look starts from — 端末譲り is deliberately tighter. Ctrl+0
/// returns here, and an inline override is only written when the choice
/// differs from it: a permanent inline style silently beat the look's own
/// 14px/19px forever after the first Ctrl+=.
function baseFont() {
    return document.documentElement.dataset.look === 'terminal' ? 14 : 15;
}

function setFont(px, remember = true) {
    FONT.at = Math.max(FONT.min, Math.min(FONT.max, px));
    const r = document.documentElement.style;
    if (FONT.at === baseFont()) {
        r.removeProperty('--size');
        r.removeProperty('--cell-h');
    } else {
        r.setProperty('--size', `${FONT.at}px`);
        // The rows have to grow with the type or the listing keeps its old
        // spacing and the text collides with it.
        r.setProperty('--cell-h', `${Math.round(FONT.at * 1.7)}px`);
    }
    if (viewer.ed) viewer.ed.updateOptions({ fontSize: FONT.at });
    if (term.on) ask('shellresize', shellSize());
    // The foot bars grew or shrank with everything else; the panes have to be
    // told, or the last row hides under them.
    measureFoot();
    if (remember) ask('remember', { key: 'gui_font', value: String(FONT.at) });
}

/// What the last session chose. Applied before the first draw, so the window
/// never flashes the default look on its way to the chosen one.
async function recall() {
    const s = await ask('settings', {});
    if (!s) return;
    if (s.look) {
        const at = LOOKS.findIndex(([v]) => (v || 'hakuji') === s.look);
        if (at >= 0) setLook(at, false);
    }
    if (s.style) {
        const at = STYLES.findIndex(([v]) => v === s.style);
        if (at >= 0) setStyle(at, false);
    }
    if (s.font) {
        const px = Number(s.font);
        if (px >= FONT.min && px <= FONT.max) setFont(px, false);
    }
    if (s.view && VIEWS.includes(s.view)) setView(s.view, false);
    if (s.hints === '0') { hintsOn = false; drawHints(); }
    // Where the dividers were left. Applied without saving them straight back.
    const pct = (v, fallback) => (Number.isFinite(Number(v)) && Number(v) > 0 ? Number(v) : fallback);
    layout.main = pct(s.main_pct, layout.main);
    layout.panes = pct(s.panes_pct, layout.panes);
    applyLayout(false);
    applyKeymaps(s.keymaps);
    // The eighteen, and whichever of them the terminal build was last set to
    // — because they are one program and `theme` is one setting.
    const t = await ask('themes', {});
    if (t) {
        for (const p of t.list) palettes.set(p.name, p);
        // `theme` wins unless one of the window's own looks (陰翳・端末譲り)
        // was chosen *after* it — setPalette resets gui_look to 白磁, so a
        // surviving non-白磁 look is by definition the later choice.
        if (t.now && palettes.has(t.now) && (!s.look || s.look === 'hakuji')) {
            setPalette(t.now, false);
        }
    }
    // Lua's own complaints go in the same queue as mine — from where the
    // person stands they are one thing: "my config did not take".
    keymapErrors = [...(s.config_errors || []), ...keymapErrors];
}

/// The window changed shape, so the shell's idea of its own size is stale.
///
/// Nothing watched for this: a PTY opened at 82×7 stayed 82×7 however far the
/// window was dragged, and every full-screen program in it drew to the wrong
/// rectangle. Debounced, because a drag is a hundred of these.
let resizeTimer = null;

window.addEventListener('resize', () => {
    measureFoot();
    if (viewer.ed) viewer.ed.layout();
    // The shell is handled by the observer below, which sees the panel's box
    // change for any reason — a drag, a font change, the hint bar going away
    // — rather than only for this one.
});

recall().then(() => {
    // The third surface, from the start.
    //
    // cian-tui's normal layout is three — the two file panes and the shell —
    // and a window where the shell only exists after Shift+J is a window
    // where the shell is not part of the program. Opened without focus: the
    // keys still belong to the listing until Shift+J asks for them.
    if (!term.on) openShell({ focus: false });
});

/// Keep the PTY's size equal to the box it is drawn in.
///
/// Watched rather than timed. The shell is opened before the hint bar has
/// been drawn and before the panel has its final height, so any single
/// re-measure is a guess about when layout finishes — and a PTY told the
/// wrong number paints its bottom rows off the end of the panel. This fires
/// when the box actually changes, which is the condition itself.
new ResizeObserver(() => {
    if (!term.on) return;
    const size = shellSize();
    if (size.cols === term.cols && size.rows === term.rows) return;
    term.cols = size.cols;
    term.rows = size.rows;
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => ask('shellresize', size), 60);
}).observe(el.sPanes);
drawHints();

refresh().then(() => {
    if (keymapErrors.length) {
        say(`init.lua の keymap: ${keymapErrors.join('  /  ')}`, true);
        return;
    }
    // Said once, on the status line, where it costs nothing and answers the
    // only question this milestone exists to answer.
    const face = resolvedFace();
    const size = getComputedStyle(document.body).fontSize;
    // The message half only — the whole bar carries chips now, and reading
    // the element back would fold the badge and counts into the greeting.
    say(`${status.msg}   ·   ${face} ${size}`);
});
