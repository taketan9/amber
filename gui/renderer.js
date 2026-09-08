'use strict';
// amber の窓、三列。左が行き先、真ん中がノート、右が中身。
//
// **判断はここに書かない。** 題をどう決めるか、絞り込みの AND / OR をどう
// 解くか、チェックをどう切り替えるかは `amber-core` が答える ── iPhone も
// 同じ一枚に訊いている。ここに写した瞬間、**同じ操作なのに Mac と iPhone で
// 結果が違う**が、一度の編集で作れてしまう。

const el = (id) => document.getElementById(id);

/// Enter が押されたか。**鍵盤の右にもう一つある。**
///
/// フルサイズの鍵盤（会社の机にたいてい載っている）は、数字の並びの脇の
/// Enter を `NumpadEnter` として送る ── `code === 'Enter'` だけを見ていた
/// ので、そこを押した人には**升の続きが出なかった**。
///
/// 点と番号は「出ていた」ので、なおさら分からない ── あれは**画面が
/// 勝手にやっている**（`contenteditable` の既定）。こちらの手当てが要る
/// のは升だけで、手当てが飛ぶと**升だけが出ない**という形で現れる。
const isEnter = (e) => e.code === 'Enter' || e.code === 'NumpadEnter';

/// 道の最後の一片。**区切りは `/` だけではない。**
///
/// core が返す道は土台のもので、Windows では `C:\Users\…\ノート.md`。
/// `split('/')` で切ると**道まるごと**が名前になる ── 書き出すと
/// `C：Users…ノート.md` のような名前のファイルができ、ゴミ箱へ入れる
/// 確認にも道が出る。絵の在りかを求めるほうは、切り落とせずに `''` に
/// なって**絵が一枚も出なくなる**（Windows でだけ）。
const baseOf = (at) => String(at || '').split(/[/\\]/).pop();
/// その一片を落とした残り（末尾の区切りは残す ── 後ろに名前を繋ぐため）。
const dirOf = (at) => String(at || '').replace(/[^/\\]*$/, '');

/// ファイルの道を、絵に渡せる `file:` の形にする。
///
/// **Windows の道は、そのままでは URL にならない。**
/// `'file://' + encodeURI('C:\\Users\\…\\絵.png')` は
/// `file://C:%5CUsers%5C…` になる ── `C:` が**機械の名前**として読まれ、
/// 円記号は `%5C` に化ける。会社の端末で「この絵は読めません」と出たのは
/// これ（mac の道は `/` で始まるので、たまたま斜線が三本になっていた）。
///
/// 正しい形は `file:///C:/Users/…`。円記号を `/` に直し、頭に一本足す。
/// **`\\\\server\\share` は別**（機械の名前が入るので、斜線は二本のまま）
/// ── 会社の置き場所はネットワークの向こうのことがある。
const fileURL = (at) => {
    const p = String(at || '').replace(/\\/g, '/');
    if (p.startsWith('//')) return 'file:' + encodeURI(p);
    return 'file://' + encodeURI(p.startsWith('/') ? p : '/' + p);
};
/// この窓が乗っている土台。**画面に出す言葉が、ここで変わる。**
const MAC = typeof navigator !== 'undefined' && /Mac/.test(navigator.userAgent);

/// 鍵の並びを、その土台の言葉で。
///
/// **表には mac の記号で書いておく**（`⌘⇧O`）── 一つの書き方に揃えておけば、
/// 増やす人が迷わない。出すときにここで言い換える。
///
/// Windows で `⌘` が出ていた（本人が会社の端末で見た・2026-09-08）。
/// あちらに `⌘` という鍵は無いので、**押しようがない案内**が出ていたことになる。
/// 記号だけでなく**繋ぎ方も違う** ── mac は詰めて書き（`⌘⇧O`）、Windows は
/// `+` で繋ぐ（`Ctrl+Shift+O`）のが、それぞれの土地の書き方。
const keyText = (k) => {
    if (!k || MAC) return k || '';
    const map = { '⌘': 'Ctrl', '⌃': 'Ctrl', '⇧': 'Shift', '⌥': 'Alt' };
    const parts = [];
    let rest = '';
    for (const c of k) {
        if (map[c]) parts.push(map[c]);
        else rest += c;
    }
    if (!parts.length) return k;
    // `←` `→` はそのまま（矢印はどちらの土台でも矢印）。
    //
    // **記号のあとの言葉は、`+` で繋がない。** 「⌥ 押し」は鍵の並びでは
    // なく説明なので、`Alt+ 押し` になると読めない ── 空白で始まるなら
    // そのまま後ろに置く。
    const tail = rest.trim();
    if (!tail) return parts.join('+');
    return /^\s/.test(rest) ? parts.join('+') + ' ' + tail : parts.concat(tail).join('+');
};

const ask = (method, params) => window.amber.call(method, params || {});

/// **この画面を開いているアプリが、その口を持っていないとき。**
///
/// amber の画面は crmaine の中でも動く。あちらは自前で `ipcMain` を建てて
/// いて、**amber の preload が見せている口の一部しか無い**（2026-09-07 時点で
/// 23 のうち 9）。無い口を呼ぶと Electron がそのまま投げてくるので、人は
///
///     置けません: Error invoking remote method 'amber:welcome':
///     Error: No handler registered for 'amber.welcome'
///
/// という札を見る ── 自分が何を間違えたのか、どこにも書いていない。
/// 直す道も無い（直すのは同梱している側で、押した人ではない）。
///
/// **その一行だけ、人の言葉に置き換える。** ほかの失敗はそのまま通す ──
/// 「保存できません: 権限がありません」は、押した人にできることがある。
///
/// **包みだけを剥がす。** 一度、`Error invoking remote method` で判じて
/// しまい、エンジンの本物の失敗まで「対応していません」に置き換わった
/// ── Electron は**成功しなかった呼び出しを全部**その言い回しで包むので、
/// 「保存できません: 権限がありません」も同じ顔で来る。**口が無いことを
/// 名指ししているのは `No handler registered` の一語だけ。**
function why(e) {
    const m = String((e && e.message) || e);
    if (/No handler registered/.test(m)) {
        return 'この画面を開いているアプリが、まだこの操作に対応していません';
    }
    // 包みの中の一行だけを出す ── 外の言い回しは、押した人には何も言わない。
    const inner = m.match(/Error invoking remote method '[^']*':\s*(?:Error:\s*)?([\s\S]+)/);
    return inner ? inner[1] : m;
}

const state = {
    root: '',
    notes: [],
    books: [],
    stars: [],
    /// 期間の絞り込み（`{ which: 'updated'|'created', from, to }`）。
    /// `from` / `to` は `YYYY-MM-DD` か null（片方だけでもよい ── 「この日
    /// から先ぜんぶ」「この日まで」を言えないと、範囲が使いものにならない）。
    when: null,
    /// まだ落ちてきていないノート（クラウドが札だけ置いている）。
    waiting: [],
    /// あなたの名乗り。共有したノートの「誰が」に使う（ノートには書かない）。
    me: '',
    /// 家族と分けてある棚。**一つとは限らない** ── 印はフォルダごとに置く
    /// ので、家族用と仕事用が両方あっていい。`[{ at, by }]`。
    shares: [],
    /// 押して選んだ絞り込み。**タグは全部・フォルダはどれか。**
    /// ノートは一つのフォルダにしか居ないので、フォルダを「全部」にすると
    /// 二つ選んだ瞬間に必ず 0 件になる。
    picks: { tag: [], book: [] },
    /// amber の外にある一本を、単発で開いているか。
    guest: false,
    colors: {},
    /// 共有の棚へ入れたノートが、もといたフォルダ（ルートからの道 →
    /// フォルダ名）。**共有をやめたときに、そこへ戻す。**
    came: {},
    /// まとめて選んでいるノートの道。**開いている一本とは別のもの** ──
    /// 選んでいても、右にはいままで通り一本が出ている。
    picked: new Set(),
    /// 範囲選び（Shift+押し）の起点。
    anchor: null,
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

/// 焦点がいまノートの中（読む面か書く面）にあるか。
///
/// **そこから離れると保存が走る。** 保存は一覧を組み直すので、押し下げと
/// 離しのあいだに走られると `click` が消える ── 一覧や左の列を押したとき
/// だけ、焦点を移させない。ほかの場所からは既定のまま（探す欄から押した
/// ときに焦点が居座ると、今度は ↑↓ が一覧を動かさなくなる）。
function inNote(at) {
    return !!at && (el('read').contains(at) || el('ed').contains(at));
}


function tagsOf(notes) {
    const seen = new Map();
    for (const n of notes) for (const t of n.tags || []) seen.set(t, (seen.get(t) || 0) + 1);
    return [...seen.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

const starred = (n) => n.star !== null && n.star !== undefined;

function drawRail() {
    const on = (kind, what) => state.dest.kind === kind && state.dest.what === what;
    const rows = [];
    // 名前は決め打ちなので、`BRAND` をそのまま置く（人の書いた字は入らない）。
    rows.push('<div id="railtop"><span class="wm">' + BRAND + '</span></div>');
    // 「＋」は全角の空白で離していた ── 字と記号のあいだが不揃いになる。
    // 印は琥珀の丸の中に描く（同じ太さ・同じ大きさで、字と揃う）。
    rows.push('<button id="new"><span class="ring">'
        + '<svg viewBox="0 0 16 16" aria-hidden="true">'
        + '<path d="M8 3.4v9.2M3.4 8h9.2" stroke="currentColor" stroke-width="2.2"'
        + ' stroke-linecap="round"/></svg></span>'
        + '<span>新しいノート</span></button>');

    rows.push('<div class="head">ノート</div>');
    rows.push(dest('all', '', 'すべてのノート', state.notes.length, on('all', '')));

    const stars = state.notes.filter(starred);
    {
        // **一つも無くても段は出す。** 無いときに段ごと消えると、
        // 最初の一つを作る道がどこにも無くなる ── 「実装されていないのか、
        // 見えないだけなのか」が使う人には見分けられない。
        rows.push(head('ブックマーク', 'star'));
        rows.push(dest('star', '', 'すべて', stars.length, on('star', '')));
        for (const sh of state.stars) {
            const n = stars.filter((x) => x.star === sh || (x.star || '').startsWith(sh + '/')).length;
            rows.push(dest('star', sh, sh.split('/').pop(), n, on('star', sh), sh.split('/').length - 1));
        }
    }

    {
        rows.push(head('フォルダ', 'book'));
        for (const b of state.books) {
            // **共有のフォルダは、こちらには出さない。** 下の「共有」の段に
            // 同じものが並ぶ ── 二つの場所に同じものが出ると、人はそれを
            // 二度消そうとする（ブックマークを別枠にしたのと同じ理由）。
            if (state.shares.some((sh) => sh.at && (b === sh.at || b.startsWith(sh.at + '/')))) continue;
            const n = state.notes.filter((x) => x.book === b || x.book.startsWith(b + '/')).length;
            rows.push(dest('book', b, b.split('/').pop(), n, on('book', b),
                           b.split('/').length - 1, state.colors[b]));
        }
    }

    // **共有は、行き先の一つ。** 分けるのはクラウドの仕事で、amber が持つ
    // のは「どれが分けてあるか」だけ ── だからここは新しい仕組みではなく、
    // フォルダの一つを別の名前で呼んでいるだけになる。
    //
    // **決めていないうちは出さない。** 空の「共有」が並んでいると、共有が
    // 壊れているのか、まだ何も分けていないのかが見分けられない ── 作る道は
    // フォルダの右押しにある。
    if (state.shares.length) {
        rows.push(head('共有'));
        for (const sh of state.shares) {
            const n = state.notes.filter((x) => inShare(sh.at, x)).length;
            rows.push(dest('share', sh.at, sh.at.split('/').pop() || 'ぜんぶ', n,
                           on('share', sh.at)));
        }
    }

    const tags = tagsOf(state.notes);
    {
        rows.push(head('タグ', 'tag'));
        // 30 で切る。**タグは増える一方**で、全部並べると行き先の列が
        // 「タグの一覧」になり、フォルダもブックマークも押し出される。
        for (const [t, n] of tags.slice(0, 30)) rows.push(dest('tag', t, t, n, on('tag', t)));
    }
    el('rail').innerHTML = rows.join('');
    el('new').onclick = newNote;
    for (const b of el('rail').querySelectorAll('.plus')) {
        b.onclick = (e) => { e.stopPropagation(); railPlus(b.dataset.plus); };
    }
    for (const d of el('rail').querySelectorAll('.dest')) {
        // 一覧の行と同じ理由で押し下げ（`.row` の註）。
        d.onmousedown = (e) => {
            if (e.button !== 0) return;
            if (inNote(document.activeElement)) e.preventDefault();
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

/// 行き先の左の印。**フォルダと、しおりと、タグを、形で分ける。**
///
/// 前は色を付けたフォルダにだけ小さな四角が出て、それ以外は名前だけ ──
/// 色を付けていないフォルダとブックマークの棚は、字の形しか違いが無かった。
/// 列は上から下へ読むものなので、段の見出しを覚えていないと**いま何の
/// 一覧を見ているのか分からない**。
///
/// 色は形の上に載せる（色だけで分けない）── 色の見分けにくい人にも、
/// フォルダはフォルダの形をしている。
const RAIL_MARKS = {
    all: '<path d="M1.5 9.5h3.2l1 1.8h4.6l1-1.8h3.2M1.5 9.5 3.4 3.6h9.2l1.9 5.9'
        + 'v3.4a1 1 0 0 1-1 1H2.5a1 1 0 0 1-1-1z"/>',
    star: '<path d="M8 1.9 10 6l4.5.6-3.3 3.1.8 4.4L8 12l-4 2.1.8-4.4L1.5 6.6 6 6z"/>',
    book: '<path d="M1.6 12.6V4.2a1 1 0 0 1 1-1h3.3l1.5 1.8h6a1 1 0 0 1 1 1v6.6'
        + 'a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z"/>',
    // **札の形。** ここは長らく「＃」を線で描いていたが、16px の枠に
    // 四本線を渡すと線が詰まって、字の「＃」が太って見えるだけになる ──
    // 隣のフォルダも星も形で分かるのに、タグだけ記号を読ませていた。
    // 共有は**二人**。フォルダでも星でもない形にする ── 「ここに置いたものは
    // 自分だけのものではない」が、名前を読まなくても分かるように。
    share: '<circle cx="5.6" cy="5.4" r="2.5"/><circle cx="11.2" cy="6.6" r="1.9"/>'
        + '<path d="M1.6 13.4c0-2.2 1.8-3.6 4-3.6s4 1.4 4 3.6"/>'
        + '<path d="M11 9.9c1.9.1 3.4 1.4 3.4 3.5"/>',
    tag: '<path d="M9.1 1.9h3.9a1.1 1.1 0 0 1 1.1 1.1v3.9a1.1 1.1 0 0 1-.32.78'
        + 'l-6.1 6.1a1.1 1.1 0 0 1-1.56 0L1.9 9.58a1.1 1.1 0 0 1 0-1.56l6.1-6.1'
        + 'a1.1 1.1 0 0 1 .78-.32z"/><path d="M11.2 4.8h.01"/>',
};

function dest(kind, what, name, n, isOn, depth, color) {
    const d = RAIL_MARKS[kind] || RAIL_MARKS.book;
    // **色を付けたフォルダは、塗る。** 線にだけ色を載せていた頃は、
    // 15px の輪郭の色を見分けることになって「色を付けた」と気づけなかった
    // （電話は前から塗っている）。塗ると、離れて見ても色で拾える。
    const tint = color ? ' style="color:' + escapeAttr(color) + '"' : '';
    const fill = color ? escapeAttr(color) : 'none';
    const mark = '<svg class="mk" viewBox="0 0 16 16" aria-hidden="true"' + tint + '>'
        + '<g fill="' + fill + '" stroke="currentColor" stroke-width="1.35"'
        + ' stroke-linecap="round" stroke-linejoin="round">' + d + '</g></svg>';
    return '<div class="dest' + (isOn ? ' on' : '') + '" data-kind="' + escapeAttr(kind) + '"'
        + ' data-what="' + escapeAttr(what) + '" data-depth="' + (depth || 0) + '">'
        + mark + '<span class="nm">' + escapeHtml(name) + '</span><span class="n">' + n + '</span></div>';
}

/* ── 一覧（中） ── */

/// このノートは、その共有の棚の中か。
const inShare = (at, n) => !at || n.book === at || (n.book || '').startsWith(at + '/');

function inDest(n) {
    const kind = state.dest.kind;
    const what = state.dest.what;
    if (kind === 'share') return inShare(what, n);
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
    // **押して選んだものは、字にしない。** 前は `tag:仕事` を探す欄に流し
    // 込んでいた ── 押しただけなのに機械の言葉が現れ、消すには字を消す
    // ことになる。選んだものは選んだものとして持つ。
    for (const t of state.picks.tag) here = here.filter((n) => (n.tags || []).includes(t));
    if (state.picks.book.length) {
        here = here.filter((n) => state.picks.book.some(
            (b) => n.book === b || (n.book || '').startsWith(b + '/')));
    }
    // **期間は言葉ではなく、日付そのもので絞る。** `updated:` のような
    // 書き方を増やすと、覚える記法がまた一つ増える。
    if (state.when) here = here.filter(inWhen);
    if (!state.filter.trim() || !state.groups.length) return here;
    return here.filter((n) => state.groups.some((g) => g.every((t) => hitTerm(n, t))));
}

/// その日付が、選んだ範囲の中にあるか。
///
/// **日で比べる。** 秒で比べると「9月6日まで」が 9月6日の 0時0分までに
/// なり、その日に書いたものが軒並み落ちる ── 人の言う「まで」はその日を
/// 含む。`YYYY-MM-DD` の字で比べれば、この取り違えが起きようがない。
function inWhen(n) {
    const at = n[state.when.which];
    if (!at) return false;
    const day = dayOf(at);
    if (state.when.from && day < state.when.from) return false;
    if (state.when.to && day > state.when.to) return false;
    return true;
}

/// 秒を `YYYY-MM-DD` に（**その土地の日付で**）。
function dayOf(sec) {
    const d = new Date(sec * 1000);
    return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0')
        + '-' + String(d.getDate()).padStart(2, '0');
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

/// 名前順に並べる物差し。**一度だけ作って、使い回す。**
///
/// `localeCompare(b, 'ja', {…})` は呼ぶたびに物差しを作り直す ── 1002 本を
/// 並べると一万回作ることになり、それだけで 119ms かかっていた（同じ並びが
/// `Intl.Collator` の使い回しでは 7ms）。並べ替えは一覧を組むたびに走り、
/// 一覧は保存のたびに組み直される。
const BY_NAME = new Intl.Collator('ja', { numeric: true });

function sortNotes(list) {
    const out = [...list];
    if (order === 'title') {
        out.sort((a, b) => BY_NAME.compare(a.title || '', b.title || ''));
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

el('findbtn').innerHTML = '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none"'
    + ' stroke="currentColor" stroke-width="1.5" stroke-linecap="round">'
    + '<circle cx="7.2" cy="7.2" r="4.6"/><path d="m10.6 10.6 3 3"/></svg>'
    + '<span>ノートを探す</span>';
el('findbtn').onclick = () => (el('findbox').hidden ? openFind() : closeFind());
el('findoff').onclick = () => { closeFind(); el('find').blur(); };
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
        share: '共有 ── ' + what.split('/').pop(),
        tag: '#' + what,
        star: what ? '★ ' + what.split('/').pop() : '★ ブックマーク',
    }[state.dest.kind] || 'すべてのノート';
    el('where').textContent = name;
    const all = state.notes.filter(inDest).length;
    el('count').textContent = rows.length + ' 件' + (rows.length !== all ? '（' + all + ' 件中）' : '');
    if (!rows.length) {
        el('rows').innerHTML = '<div id="empty">'
            // **絞ったから空なのか、もともと空なのかを分けて言う。**
            // 同じ「ありません」だと、外せば出てくることに気づけない。
            + (filtering() ? '絞り込みに当たるノートがありません'
                : 'ここにはまだノートがありません')
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
        // **押し下げで開く。** `click` は押し下げと離しが同じ節に当たって
        // 初めて出る ── 打っていたノートから一覧を押すと、焦点が外れた
        // ことで保存が走り、一覧が組み直され、離す頃には押した行がもう
        // 別の節になっていた。**一度目が効かず、二度目で開く**のはこれ。
        // 選ぶのは押し下げ、が机の上の一覧のふつうでもある。
        r.onmousedown = (e) => {
            if (e.button !== 0) return;
            if (inNote(document.activeElement)) e.preventDefault();
            const at = r.dataset.path;
            // **`Ctrl`／`⌘` 押しは、出し入れ。** 開かない ── 二十本目を
            // 選ぶたびに二十本目が開いていては、選んでいる意味が無い。
            if (e.metaKey || e.ctrlKey) { pickToggle(at); return; }
            // `Shift` 押しは、起点からここまで。**一覧に出ている順**で
            // 数える（並び替えたら、見えている通りに繋がる）。
            if (e.shiftKey && state.anchor) { pickTo(at); return; }
            // **`⌥` 押しで、新しいタブ。** `⌘` ではない ── あちらは
            // 「まとめて選ぶ」が先に取っている。同梱先（crmaine）が
            // 欲しがる鍵（`⌘W`・`⌘＋数字`・`⌃Tab`）は、こちらは取らない。
            if (e.altKey) { if (state.picked.size) unpickAll(); openNote(at, { tab: true }); return; }
            // ふつうの押し下げは、いままで通り開く。**選びは畳む** ──
            // 選んだままにすると、次に押した「ゴミ箱へ」が二十本に効く。
            if (state.picked.size) unpickAll();
            state.anchor = at;
            openNote(at);
        };
        // 右押しでも、⋯ と同じ献立。**開いてから出す** ── 開いていない
        // ノートに「削除」を出すと、どれが消えるのか画面が言っていない。
        r.oncontextmenu = async (e) => {
            e.preventDefault();
            const at = r.dataset.path;
            // **選んでいる行を右押ししたら、選んだぶんの献立。** 開かない
            // ── 開くと選びが畳まれて、出したかった献立が消える。
            if (state.picked.has(at)) { pickedMenu({ x: e.clientX, y: e.clientY }); return; }
            if (state.picked.size) unpickAll();
            // **「新しいタブで開く」は、開く前に。** 開いてしまうと、
            // いまのタブが差し替わったあとで「新しいタブ」を押すことになる。
            if (!state.open || state.open.path !== at) {
                popMenu([
                    { name: '開く', run: () => openNote(at) },
                    { name: '新しいタブで開く', key: '⌥ 押し', run: () => openNote(at, { tab: true }) },
                    { name: 'このノートにすること…', sep: true, run: async () => {
                        await openNote(at);
                        openMenu({ right: e.clientX + 190, bottom: e.clientY });
                    } },
                ], { x: e.clientX, y: e.clientY });
                return;
            }
            openMenu({ right: e.clientX + 190, bottom: e.clientY });
        };
    }
    drawPicked();
}

/// 一覧の空きどころの右押し。**行の上ではない**ので、ノートのことではなく
/// 「この一覧」のこと。
el('list').addEventListener('contextmenu', (e) => {
    if (e.target.closest('.row')) return;          // 行は行の献立が受ける
    if (e.target.closest('input, textarea')) return;
    e.preventDefault();
    const at = { x: e.clientX, y: e.clientY };
    popMenu([
        { name: '新しいノート', key: '⌘N', run: newNote },
        { name: 'ここに貼り付けて新しいノート', run: cmdPasteNote },
        { name: '並び順 ── ' + ORDERS.find(([k]) => k === order)[1], sep: true, run: () => {
            const n = ORDERS.findIndex(([k]) => k === order);
            order = ORDERS[(n + 1) % ORDERS.length][0];
            window.amber.remember({ order });
            drawOrder();
            drawList();
        } },
        { name: 'すべて選ぶ', key: '⌘A', sep: true, run: pickAll },
        { name: '一覧を畳む', key: '⌘⇧/', run: toggleList },
    ], at);
});

/// 左の列の空きどころの右押し。行き先の上ではないので、「この列」のこと。
el('rail').addEventListener('contextmenu', (e) => {
    if (e.target.closest('.dest, .plus')) return;  // 行き先は `railMenu` が受ける
    e.preventDefault();
    popMenu([
        { name: '新しいフォルダ', run: () => cmdMkBook() },
        { name: '新しいブックマークの置き場所', run: () => newShelf('') },
        { name: '左の列を畳む', key: '⌘/', sep: true, run: toggleRail },
    ], { x: e.clientX, y: e.clientY });
});

/// 帯の題の右押し。
el('title').addEventListener('contextmenu', (e) => {
    if (!state.open) return;
    e.preventDefault();
    const path = state.open.path;
    popMenu([
        { name: '題を直す', run: renameTitle },
        { name: 'ファイル名を写す', run: () => copyText(baseOf(path), 'ファイル名') },
        { name: '道を写す', run: () => copyText(path, '置き場所') },
        { name: 'Finder で表示', sep: true, run: () => window.amber.reveal(path) },
    ], { x: e.clientX, y: e.clientY });
});

/// 貼り付けた字から、新しいノートを一本。
/// **型を置くフォルダの名前。**
///
/// 決め打ちの一語 ── 設定にしない。設定にすると「どこに置けば型になるか」
/// が人によって違い、見本ノートにも書けない（読んだ人の amber では違う
/// 名前かもしれない）。**ただのフォルダ**なので、中のノートは一覧にも
/// 普通に出るし、開いて直せる ── 型のための新しい入れ物は作らない。
const TEMPLATES = 'テンプレート';

/// 型から新しいノートを作る（依頼 417）。
///
/// **写す仕組みは「複製」と同じ**（core の `duplicate`）── 行き先だけが
/// 違う。別の道にすると、`created` を今日にするのを片方だけ直した日に、
/// 二つの作り方が食い違う。
///
/// できたノートは**いま見ているフォルダ**へ（「新しいノート」と同じ）──
/// どこに出来たか分からない、がいちばん困る。
async function cmdTemplate() {
    const rows = state.notes.filter(
        (n) => n.book === TEMPLATES || n.book.startsWith(TEMPLATES + '/'));
    if (!rows.length) {
        // **どうすれば使えるかを言う。** 「ありません」だけだと、
        // 作れないのか置き場所が違うのかが分からない。
        say('「' + TEMPLATES + '」フォルダを作って、中にノートを置くと型になります');
        return;
    }
    const path = await askPick('どの型から',
        sortNotes(rows).map((n) => ({
            name: n.title || '（タイトルなし）',
            sub: n.book === TEMPLATES ? '' : n.book.slice(TEMPLATES.length + 1),
            value: n.path,
        })), '選ぶと、その中身で新しいノートを作ります');
    if (path === null) return;
    // **型のフォルダを見ているときは、いちばん上に作る。** そこに作ると
    // 型が増えていくだけで、書いたものがどこにも出てこない。
    const here = state.dest.kind === 'book' && state.dest.what !== TEMPLATES
        && !state.dest.what.startsWith(TEMPLATES + '/')
        ? state.root + '/' + state.dest.what
        : state.root;
    try {
        const r = await ask('copy', { path, dir: here });
        await reload({ quiet: true });
        await openNote(r.path);
        if (editor) editor.focus();
    } catch (e) {
        say('作れません: ' + why(e));
    }
}

/// 同じ中身のノートをもう一つ（依頼 412）。
///
/// **先に保存する。** 打った字がまだファイルに無いうちに写すと、写しは
/// 画面に見えているものと違うものになる ── 「複製したのに古い」は、
/// 原因が画面のどこにも出ない。
///
/// 写したほうを開く ── 押した人がこれから触るのは写したほうで、元の
/// ノートに残されると「効いたのか」が分からない（一覧の同じ題が二つに
/// 増えただけに見える）。
async function cmdDup() {
    if (!state.open) return;
    try {
        await save();
        const r = await ask('copy', { path: state.open.path });
        await reload({ quiet: true });
        await openNote(r.path);
        say('複製しました');
    } catch (e) {
        say('複製できません: ' + why(e));
    }
}

async function cmdPasteNote() {
    let text = '';
    try {
        text = await navigator.clipboard.readText();
    } catch (e) {
        say('貼り付けられません: ' + why(e));
        return;
    }
    if (!text.trim()) { say('貼り付けるものがありません'); return; }
    const made = await newNote();
    if (!made || !editor || !state.open || state.open.path !== made) return;
    loading = true;
    editor.setValue(text);
    loading = false;
    state.dirty = true;
    await save();
}

/* ── まとめて選ぶ ── */

/// いま一覧に出ている順のノート。**選びの範囲は、見えている通り。**
/// `drawList` と同じ並べ方を通す（ブックマークが上に別枠で出るところまで）。
function shownNotes() {
    const rows = sortNotes(narrowed());
    if (state.dest.kind === 'star') return rows;
    return [...rows.filter(starred), ...rows.filter((n) => !starred(n))];
}

function pickToggle(at) {
    if (state.picked.has(at)) state.picked.delete(at);
    else { state.picked.add(at); state.anchor = at; }
    drawList();
}

/// 起点からここまで、まとめて選ぶ。**足すだけで、外さない** ── 続けて
/// 二回 Shift を押した人が、一回目に選んだぶんを失わないように。
function pickTo(at) {
    const rows = shownNotes().map((n) => n.path);
    const a = rows.indexOf(state.anchor);
    const b = rows.indexOf(at);
    if (a < 0 || b < 0) { pickToggle(at); return; }
    for (const p of rows.slice(Math.min(a, b), Math.max(a, b) + 1)) state.picked.add(p);
    drawList();
}

function pickAll() {
    // **絞り込んでいるなら、絞り込んだぶんだけ。** 見えていないものまで
    // 選ぶと、次に押した「ゴミ箱へ」が見えていない本に効く。
    for (const n of shownNotes()) state.picked.add(n.path);
    drawList();
}

function unpickAll() {
    if (!state.picked.size) return;
    state.picked.clear();
    state.anchor = null;
    drawList();
}

/// 選んでいる間だけ、一覧の頭に帯を出す。
function drawPicked() {
    const bar = el('picked');
    const n = state.picked.size;
    bar.hidden = !n;
    if (!n) return;
    bar.innerHTML = '<span class="n">' + n + ' 本を選んでいます</span><span class="sp"></span>'
        + '<button id="pickdo">まとめて ▾</button><button id="pickoff">やめる</button>';
    el('pickdo').onclick = (e) =>
        pickedMenu(e.currentTarget.getBoundingClientRect());
    el('pickoff').onclick = unpickAll;
}

/// 選んだノートの行（一覧に無いものは落とす ── 外で消えていることがある）。
function pickedNotes() {
    return state.notes.filter((n) => state.picked.has(n.path));
}

/// 選んだノートにすること。**「まとめて ▾」と、選んだ行の右押しで同じもの。**
function pickedMenu(at) {
    const n = state.picked.size;
    popMenu([
        { name: n + ' 本にタグを付ける', run: () => manyTagOn() },
        { name: n + ' 本からタグを外す', run: () => manyTagOff() },
        { name: n + ' 本をフォルダへ移動', run: () => manyMove() },
        { name: n + ' 本をブックマークに登録', run: () => manyStar(true) },
        { name: n + ' 本のブックマークを外す', run: () => manyStar(false) },
        { name: '選ぶのをやめる', key: 'Esc', sep: true, run: unpickAll },
        { name: n + ' 本をゴミ箱へ入れる', sep: true, run: () => manyDelete() },
    ], at);
}

/// 選んだノートを一本ずつ書き換える。**一本転んでも、残りは進む** ──
/// 二十本のうち三本目で止まると、どこまで済んだのかが誰にも分からない。
///
/// `change(note, text)` が新しい字を返す。**同じ字を返したら書かない** ──
/// 同期しているフォルダで、中身の変わらないファイルの時刻だけ動くのが
/// いちばん困る（「向こうが書き換えた」に見える）。
async function eachPicked(change) {
    const done = [];
    const skipped = [];
    const failed = [];
    for (const note of pickedNotes()) {
        try {
            const got = await ask('read', { path: note.path });
            const was = typeof got.text === 'string' ? got.text : null;
            if (was === null) { failed.push(note); continue; }
            const now = await change(note, was);
            if (now === null || now === was) { skipped.push(note); continue; }
            await ask('write', { path: note.path, text: now, force: true });
            done.push(note);
        } catch {
            failed.push(note);
        }
    }
    await reload({ quiet: true });
    // 開いていた一本も書き換わっているかもしれない ── 読み直す。
    if (state.open && state.picked.has(state.open.path)) await openNote(state.open.path, { quiet: true });
    return { done, skipped, failed };
}

/// 数で言う。**「済みました」だけにしない** ── 何本に効いて、何本は
/// もともとそうだったのかは、押した人が知りたいことそのもの。
function sayMany(verb, r, why2) {
    let m = r.done.length + ' 本' + verb;
    if (r.skipped.length) m += '（' + r.skipped.length + ' 本は' + why2 + '）';
    if (r.failed.length) m += '／' + r.failed.length + ' 本は書けませんでした';
    say(m);
}

async function manyTagOn() {
    const notes = pickedNotes();
    const all = tagsOf(state.notes).map(([t, c]) => ({ name: t, sub: c + ' 件', value: t }));
    const pick = await askPick(notes.length + ' 本に付けるタグ',
        [...all, { name: '＋ 新しいタグを作る', value: ' new' }]);
    if (pick === null) return;
    let tag = pick;
    if (pick === ' new') {
        const v = await askText('新しいタグの名前', '', '空白は使えません（`買い物` のように）');
        if (!v || !v.trim()) return;
        tag = v.trim().replace(/^#/, '').replace(/\s+/g, '');
        if (!tag) return;
    }
    const r = await eachPicked(async (note, text) => {
        const now = note.tags || [];
        // **もう付いている本は触らない。** 書き換えないので時刻も動かない。
        if (now.includes(tag)) return null;
        const got = await ask('settags', { text, tags: [...now, tag] });
        return typeof got.text === 'string' ? got.text : null;
    });
    sayMany('に #' + tag + ' を付けました', r, 'もとから付いています');
}

async function manyTagOff() {
    const notes = pickedNotes();
    // **選んだ中に実際にあるタグだけを、件数つきで出す。**
    //
    // 打ち込ませると、選んだ二十本のうち十二本にしか無いタグを外したとき
    // 「残り八本で何が起きたのか」が誰にも分からない。並べてしまえば、
    // 無いタグは選びようがない。
    const here = tagsOf(notes);
    if (!here.length) { say('選んだノートにタグは付いていません'); return; }
    const tag = await askPick(notes.length + ' 本から外すタグ',
        here.map(([t, c]) => ({ name: t, sub: notes.length + ' 本中 ' + c + ' 本', value: t })));
    if (tag === null) return;
    const r = await eachPicked(async (note, text) => {
        const now = note.tags || [];
        if (!now.includes(tag)) return null;
        const got = await ask('settags', { text, tags: now.filter((t) => t !== tag) });
        return typeof got.text === 'string' ? got.text : null;
    });
    sayMany('から #' + tag + ' を外しました', r, 'もとから付いていません');
}

async function manyStar(on) {
    const r = await eachPicked(async (note, text) => {
        if (starred(note) === on) return null;
        const got = await ask('star', { text, shelf: on ? '' : null });
        return typeof got.text === 'string' ? got.text : null;
    });
    sayMany(on ? 'をブックマークに入れました' : 'のブックマークを外しました',
        r, on ? 'もとから入っています' : 'もとから入っていません');
}

async function manyMove() {
    const notes = pickedNotes();
    const here = [{ name: '（いちばん上）', value: '' },
        ...state.books.map((b) => ({ name: b, value: b })),
        { name: '＋ 新しいフォルダを作る', value: ' new' }];
    let to = await askPick(notes.length + ' 本をどのフォルダへ', here);
    if (to === null) return;
    if (to === ' new') {
        const made = await cmdMkBook();
        if (!made) return;
        to = made;
    }
    const dir = to ? state.root + '/' + to : state.root;
    if (state.dirty) await save();
    let moved = 0;
    let failed = 0;
    const now = new Set();
    for (const note of notes) {
        // もう居るところへは動かさない。
        if ((note.book || '') === to) { now.add(note.path); continue; }
        try {
            const r = await ask('move', { path: note.path, dir });
            moved++;
            if (r && r.path) now.add(r.path);
        } catch { failed++; now.add(note.path); }
    }
    // **選びは道で憶えている。** 移すと道が変わるので、繋ぎ直す ──
    // 繋がないと、移した直後に選びが空になる。
    state.picked = now;
    await reload({ quiet: true });
    if (state.open) await openNote(state.open.path, { quiet: true });
    drawList();
    say(moved + ' 本を' + (to ? '「' + to + '」へ' : 'いちばん上へ') + '移しました'
        + (failed ? '／' + failed + ' 本は移せませんでした' : ''));
}

/// まとめてゴミ箱へ。**訊くのは一度だけ。**
///
/// 一本ずつの `cmdDelete` を二十回まわすと、二十回訊かれる ── ゴミ箱の
/// 無い置き場所（会社の OneDrive）では、断られてからもう一度、で四十回に
/// なる。数を言って一度承知をもらい、そのあとは黙って進める。
async function manyDelete() {
    const notes = pickedNotes();
    if (!notes.length) return;
    const knew = noBin();
    const head = notes.length + ' 本を';
    if (!await askYes(knew
        ? head + '消しますか（ここにはゴミ箱が無いので、戻せません）'
        : head + 'ゴミ箱へ入れますか')) return;
    let binned = 0;
    let erased = 0;
    let failed = 0;
    let asked = knew;
    for (const note of notes) {
        const got = await window.amber.trash(note.path);
        if (got === true) { binned++; if (noBin()) markNoBin(false); continue; }
        // 断られた ── **ここで初めて分かったときだけ、もう一度訊く。**
        markNoBin(true);
        if (!asked) {
            asked = true;
            const go = await askYes('ゴミ箱へ入れられませんでした'
                + (got && got.why ? '（' + got.why + '）' : '')
                + '。残りをこのまま消しますか。もう戻せません');
            if (!go) break;
        }
        try { await ask('delete', { path: note.path }); erased++; } catch { failed++; }
    }
    // 消えた一本を開いたままにしない。
    if (state.open && state.picked.has(state.open.path)) { state.open = null; state.dirty = false; applyView(); }
    unpickAll();
    await reload({ quiet: true });
    say((binned ? binned + ' 本をゴミ箱へ入れました' : '')
        + (binned && erased ? '／' : '') + (erased ? erased + ' 本を消しました' : '')
        + (failed ? '／' + failed + ' 本は消せませんでした' : '')
        || '何も消しませんでした');
}

function row(n) {
    const open = state.open && state.open.path === n.path;
    // **控えは、消さずに札を貼る。** 一覧から外すと、中身を助け出す道が
    // どこにも無くなる ── 並べたうえで、そういうものだと言う。
    const clash = n.clash
        ? '<span class="clashmark" title="クラウドが作った控えです">競合</span>'
        : '';
    // **共有中は、書く前に分かるように。** 家族が読むノートに、そうと
    // 知らずに書くことがないように ── 印は題の隣（開いてからでは遅い）。
    const shared = n.shared && state.dest.kind !== 'share'
        ? '<span class="sharemark" title="家族と分けているフォルダの中です">共有</span>'
        : '';
    const tags = (n.tags || []).slice(0, 3)
        .map((t) => '<span class="tag">' + escapeHtml(t) + '</span>').join('');
    // チェックのあるノートは、いくつ済んだかを出す。**数えるだけで、
    // 新しい欄は作らない** ── 進み具合は既にノートの中に書いてある。
    const done = (n.excerpt || '').match(/\[x\]/gi)?.length || 0;
    const todo = (n.excerpt || '').match(/\[ \]/g)?.length || 0;
    const bar = done + todo ? '<span class="done">' + done + '/' + (done + todo) + '</span>' : '';
    return '<div class="row' + (open ? ' on' : '') + (state.picked.has(n.path) ? ' pick' : '')
        + '" data-path="' + escapeAttr(n.path) + '">'
        + '<div class="t">' + (starred(n) ? '<span class="star">★</span> ' : '')
        + escapeHtml(n.title || '（タイトルなし）') + shared + clash + '</div>'
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

/// いま変換の途中か。**確定するまで、面を触らない。**
///
/// `input` は変換の一字ごとに来るので、`readChanged` の 700ms が**変換の
/// 途中で切れうる** ── そこで書き戻すと、未確定の字が保存される。同期先に
/// 「あいう」が届き、次に「愛」が届く ── 履歴が確定前の姿を積み、混ぜる側
/// には「向こうが二度書いた」に見える。
///
/// 組み直しも同じ。`syncRead` は箱そのものに `contentEditable` を掛け直す
/// （`armPaper`）── 変換中に箱の属性を触ると、変換が落ちるおそれがある。
let composing = false;
/// 変換中に来た「組み直して」を、確定まで預かる。
let drawAfter = false;

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

            // **変換の途中かどうかを、エディタからも受ける。** 読む面は
            // `compositionstart` を持っているが、Monaco は自分の中で
            // 変換を扱うので、こちらから聞かないと分からない。
            editor.onDidCompositionStart?.(() => { composing = true; });
            editor.onDidCompositionEnd?.(() => { composing = false; });

            editor.onDidChangeModelContent(() => {
                // **読み込みの `setValue` も変更として届く。** 守らないと、
                // 開いただけで自動保存が走り、触っていないノートの更新時刻が
                // 動く（実際に一本動かした）。同期しているフォルダでは、それが
                // 相手側に「向こうが編集した」と見える ── 何もしていないのに。
                if (loading || !state.open) return;
                // 打ったので、読む面はもう今の字ではない（組み直すまで）。
                readStale();
                state.dirty = true;
                el('state').textContent = '書きかけ';
                drawStrip();
                clearTimeout(saveTimer);
                // **変換中に切れたら、待つ。** Monaco も変換の一字ごとに
                // ここへ来るので、間合いが変換の途中で切れうる ── そこで
                // 保存すると、未確定の字がファイルに入る（読む面と同じ話）。
                saveTimer = setTimeout(function again() {
                    if (composing) { saveTimer = setTimeout(again, 900); return; }
                    save();
                }, 900);
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

/* ── 机（タブ） ── */

/// 開いているノートの列。**電話と同じ形**（`Desk.Tab`）── 一本ぶんの
/// 持ちものを、まとめてしまっておく箱。
///
/// **エディタは一台のまま。** 電話は面をタブごとに持てるが、Monaco を
/// ノートの数だけ建てるのは高い ── 替えるときに字と caret と巻き位置を
/// 出し入れすれば、同じことになる。
///
/// **単発で開く一本（`state.guest`）は机に載せない。** あれは amber の棚の
/// 外にある一本で、閉じれば元の机へ戻るもの ── 列に並べると、閉じたあとに
/// 棚の外のノートがタブに残る。
let tabs = [];
let showing = null;

/// タブ一本ぶんの持ちもの。**ここに挙げたものが、タブごとに別々**。
/// 面（表示／コード）は窓ぜんぶのことなので、入れない ── タブごとに
/// 変わると、替えるたびに面が飛ぶ。
function stashTab() {
    const t = tabs.find((x) => x.path === showing);
    if (!t || !state.open) return;
    t.keep = {
        open: state.open,
        stamp: state.stamp,
        head: state.head,
        base: state.base,
        was: state.was,
        dirty: state.dirty,
        backs: backs.slice(),
        forwards: forwards.slice(),
        lastSaved,
        incoming,
        body: editor ? editor.getValue() : '',
        at: editor ? editor.getPosition() : null,
        top: editor ? editor.getScrollTop() : 0,
    };
}

/// しまってあったものを出す。**読み直さない** ── タブに戻ったときに
/// ファイルから読み直すと、打ちかけの字が消える（タブがある意味が無い）。
function restoreTab(t) {
    const k = t.keep;
    if (!k) return false;
    state.open = k.open;
    state.stamp = k.stamp;
    state.head = k.head;
    state.base = k.base;
    state.was = k.was;
    state.dirty = k.dirty;
    backs = k.backs.slice();
    forwards = k.forwards.slice();
    lastSaved = k.lastSaved;
    incoming = k.incoming;
    if (editor) {
        loading = true;
        editor.setValue(k.body);
        loading = false;
        if (k.at) editor.setPosition(k.at);
        editor.setScrollTop(k.top || 0);
    }
    return true;
}

/// 机の帯を描く。**一本のときは出さない** ── ふだんの画面を、タブのために
/// 一段ぶん狭くしない（電話も同じ）。
/// 最後に描いた机の姿。**同じなら描き直さない** ── 打つたびに帯を組み
/// 直すと、掴んでいる巻き位置が毎回先頭へ戻る。
let stripWas = null;

function drawStrip() {
    const box = el('strip');
    const was = box.hidden;
    box.hidden = state.guest || tabs.length < 2;
    // 出たり引っ込んだりすると、下の面の高さが変わる ── Monaco は自分で
    // 気づかないので、測り直させる（畳む鍵と同じ扱い）。
    if (was !== box.hidden && editor) setTimeout(() => editor.layout(), 0);
    if (box.hidden) { box.innerHTML = ''; stripWas = null; return; }
    // **いま出しているタブは、しまってあるものを見ない。** `keep` が書かれる
    // のは離れるときなので、出している間ずっと古い ── 書きかけの点が
    // 点かないし、題を直しても帯が変わらない。生のほうを見る。
    const shape = tabs.map((t) => {
        const here = t.path === showing;
        const row = state.notes.find((x) => x.path === t.path);
        const name = (here && state.open && state.open.title)
            || (t.keep && t.keep.open && t.keep.open.title)
            || (row && row.title) || baseOf(t.path);
        return { path: t.path, here, name: name || '（タイトルなし）',
                 dirty: here ? state.dirty : !!(t.keep && t.keep.dirty) };
    });
    const key = JSON.stringify(shape);
    if (key === stripWas) return;
    stripWas = key;
    box.innerHTML = shape.map((t, n) =>
        '<div class="tab' + (t.here ? ' on' : '') + '" data-n="' + n + '"'
        + ' title="' + escapeAttr(t.path) + '">'
        + (t.dirty ? '<span class="d"></span>' : '')
        + '<span class="t">' + escapeHtml(t.name) + '</span>'
        + '<button class="x" title="閉じる">✕</button></div>').join('');
    for (const d of box.querySelectorAll('.tab')) {
        const t = tabs[Number(d.dataset.n)];
        d.onmousedown = (e) => {
            // まん中押しで閉じる（机の上のふつう）。
            if (e.button === 1) { e.preventDefault(); closeTab(t.path); return; }
            if (e.button !== 0) return;
            if (e.target.closest('.x')) return;      // ✕ は下の `onclick` が受ける
            if (inNote(document.activeElement)) e.preventDefault();
            openNote(t.path, { keep: true });
        };
        d.querySelector('.x').onclick = (e) => { e.stopPropagation(); closeTab(t.path); };
        d.oncontextmenu = (e) => {
            e.preventDefault();
            const at = tabs.indexOf(t);
            popMenu([
                { name: '閉じる', run: () => closeTab(t.path) },
                { name: 'ほかを閉じる', dim: tabs.length < 2,
                  run: () => { for (const o of tabs.slice()) if (o.path !== t.path) closeTab(o.path); } },
                { name: '右のぜんぶを閉じる', dim: at >= tabs.length - 1,
                  run: () => { for (const o of tabs.slice(at + 1)) closeTab(o.path); } },
                { name: '一覧でこの本を選ぶ', sep: true, run: () => openNote(t.path, { keep: true }) },
                { name: 'Finder で表示', run: () => window.amber.reveal(t.path) },
            ], { x: e.clientX, y: e.clientY });
        };
    }
    const on = box.querySelector('.tab.on');
    if (on) on.scrollIntoView({ block: 'nearest', inline: 'nearest' });
}

/// タブを閉じる。**書きかけは、黙って捨てない。**
///
/// この窓は打てば勝手に保存されるので、閉じる前に一度書いてから閉じる
/// ── 訊かない（訊くほうが、この窓の作りに合っていない）。
async function closeTab(path) {
    const at = tabs.findIndex((t) => t.path === path);
    if (at < 0) return;
    if (path === showing) {
        clearTimeout(readTimer);
        await syncRead();
        if (state.dirty) await save();
        stashTab();
    } else {
        const t = tabs[at];
        // 出していないタブの書きかけも、置いていかない。
        if (t.keep && t.keep.dirty) await saveTab(t);
    }
    tabs.splice(at, 1);
    rememberTabs();
    if (path !== showing) { drawStrip(); return; }
    if (!tabs.length) {
        showing = null;
        state.open = null;
        state.dirty = false;
        applyView();
        drawStrip();
        drawList();
        return;
    }
    // **左の隣へ。** そこから来たので ── 端まで飛ぶと、居た場所を見失う。
    const next = tabs[Math.min(Math.max(0, at - 1), tabs.length - 1)];
    await openNote(next.path, { keep: true });
}

/// 出していないタブの書きかけを、ファイルへ。**開き直さずに書く** ──
/// 閉じるためだけに面を組み直すのは高いし、caret が飛ぶ。
async function saveTab(t) {
    const k = t.keep;
    if (!k) return;
    try {
        await ask('write', { path: t.path, text: k.head + k.body, stamp: k.stamp });
    } catch { /* 書けなくても、閉じるのは止めない ── 字はファイルに残っている */ }
}

/// 開いていたタブを憶える。**次に開いたとき、同じ机に戻る**（電話と同じ）。
function rememberTabs() {
    if (state.guest) return;
    window.amber.remember({ tabs: tabs.map((t) => t.path) });
}

async function openNote(path, opts) {
    // 一覧に無い一本（外から来たもの）は、`opts.guest` が持ってくる。
    const note = (opts && opts.guest) || state.notes.find((n) => n.path === path);
    if (!note) return;
    // **机の上を整えるのが先。** どのタブに出すかを決めてから読む。
    if (!opts || !opts.guest) {
        const at = tabs.findIndex((t) => t.path === path);
        if (at >= 0) {
            // もう机の上にある ── そのタブへ。
            if (showing !== path) {
                clearTimeout(readTimer);
                await syncRead();
                if (state.dirty) await save();
                stashTab();
                showing = path;
                // **しまってあるなら、読み直さない。** 打ちかけの字を
                // 失わないための机なので、戻るだけで捨てては元も子もない。
                if (restoreTab(tabs[at])) {
                    if (!opts || !opts.walking) trailPush(path);
                    afterTab();
                    return;
                }
            } else if (opts && opts.keep && tabs[at].keep) {
                // 同じタブを押しただけ ── 何もしない。
                return;
            }
        } else if (opts && opts.tab && showing) {
            // **新しいタブは、いまのすぐ右へ。** 端に足すと、たどっていた
            // 順と並びが合わなくなる。
            clearTimeout(readTimer);
            await syncRead();
            if (state.dirty) await save();
            stashTab();
            tabs.splice(tabs.findIndex((t) => t.path === showing) + 1, 0, { path, keep: null });
            showing = path;
        } else if (showing) {
            // いまのタブを差し替える（ふつうに一覧を押したとき）。
            const now = tabs.findIndex((t) => t.path === showing);
            if (now >= 0) tabs[now] = { path, keep: null };
            showing = path;
        } else {
            tabs = [{ path, keep: null }];
            showing = path;
        }
        rememberTabs();
    }
    // たどっている最中は積まない ── 積むと前へ戻れなくなる。
    if (!opts || !opts.walking) trailPush(path);
    // 開く前に、書きかけを置いていかない。**読む面はまだ字になっていない**
    // ── DOM に打った跡が `syncRead` を通るまで、エディタは前の字のまま。
    // 先に戻さないと、最後の数百ミリ秒ぶんが黙って消える。
    clearTimeout(readTimer);
    await syncRead();
    if (state.dirty) await save();
    if (!editor) await makeEditor();
    let r;
    try {
        r = await ask('read', { path });
    } catch (e) {
        say('開けません: ' + why(e));
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
    // 別のノートを開いたら、戻り道は捨てる ── 別のノートの姿をここへ
    // 戻せると、一度の押し間違いで二本まとめて壊れる。
    if (!state.open || state.open.path !== path) {
        forgetSteps();
        // 前のノートの字を面に残さない ── 「コード」の面では組み直さない
        // ので、残すと次の書き戻しがそれを今のノートへ書く（`readDrawn`）。
        el('read').replaceChildren();
    }
    // 同じノートを開き直すときも、面はもう今の字ではない（エディタは
    // このあとファイルの字に置き換わる。組み直せば札は付け直される）。
    readStale();
    state.open = note;
    state.stamp = r.stamp || null;
    state.head = head;
    state.dirty = false;
    loading = true;
    editor.setValue(body);
    loading = false;
    lastSaved = body;
    // 履歴に渡すのは「保存する前の姿」── 開いた時点の中身。
    state.was = head + body;
    // **混ぜるときの土台**（分かれる前）は、別に持つ。
    // `state.was` は履歴のための「保存する前の姿」で、保存のたびに
    // 動く ── それを土台に使うと、自動保存が一度でも通ったあとは
    // 「こちらは何も書いていない」ことになり、**向こうで丸ごと上書き**
    // される（実際にそうなって、足した行が消えた）。
    // ここが動くのは、**ファイルと確かに一致した瞬間**だけ。
    state.base = head + body;
    loadIncoming();
    el('state').textContent = when(note.updated)
        + ((note.tags || []).length ? '  ' + note.tags.map((t) => '#' + t).join(' ') : '');
    afterTab();
}

/// 一本を出したあとに、画面を揃える。**読んだときも、タブに戻ったときも
/// 同じ一組**を通す ── 二か所に並べると、片方にだけ増えた描き直しができる。
function afterTab() {
    drawBand();
    drawTitle();
    drawCount();
    drawSteps();
    applyView();
    drawZones();
    drawStrip();
    if (!state.guest) drawList();
    if (state.open) window.amber.remember({ open: state.open.path });
}

/// 帯の題。**開いたときだけでなく、保存のたびに書き直す。**
///
/// 題は一行目から決まる（`note::title`）ので、新しいノートは一行目を
/// 打った瞬間に題を持つ ── 一覧の二列目はすぐそう出ていたのに、帯だけが
/// 「（タイトルなし）」のまま残っていた。同じノートの名前が、画面の二か所
/// で食い違って見えていたことになる。
function drawTitle() {
    // 直している最中は、下から書き換えない ── 打っている字が消える。
    if (document.activeElement === el('title')) return;
    el('title').textContent = (state.open && state.open.title) || '（タイトルなし）';
}

/* ── 帯の題を直す ── */

/// 帯の題は押せば直せる。**直すのは前書きの `title:` だけ。**
///
/// 題の出どころは core が決めている（`title:` → 最初の見出し → 書き出しの
/// 一行 → ファイル名）── そのうち**書き換えるのは `title:` の欄だけ**に
/// する。見出しからきていた人の見出しは、そのまま残って題が付く。
///
/// **本文には触らない。** 「題を直す」と押した人が期待しているのは題が
/// 変わることで、一行目の見出しが書き換わることではない。ただしそのぶん
/// 題と見出しが食い違いうるので、**一覧も帯も同じ core の答え**を出す
/// （どちらかが自前で「一行目が題」と決めない ── `freshenRow` と同じ話）。
///
/// 空にしたら `title:` を**外す** ── 元の決め方（見出し・書き出し）に戻る。
/// 「題を消す」ではなく「付けるのをやめる」。
async function renameTitle() {
    const box = el('title');
    box.contentEditable = 'plaintext-only';
    box.spellcheck = false;
    box.focus();
    // 全部選んでおく ── 直したい人は、たいてい丸ごと書き換える。
    const r = document.createRange();
    r.selectNodeContents(box);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
}

/// 直した題を書き込む。**空白だけなら「付けない」。**
async function titleDone(keep) {
    const box = el('title');
    box.contentEditable = 'false';
    if (!state.open) return;
    const want = keep ? box.textContent.trim().replace(/\s+/g, ' ') : null;
    drawTitle();
    if (!keep) return;
    // 「（タイトルなし）」はこちらが出している言葉で、人が書いた題ではない。
    const to = want === '（タイトルなし）' ? '' : want;
    if (to === (state.open.title || '')) return;
    try {
        // **前書きの組み立ては core。** ここで `---` を書き足すと、
        // 前書きの形を決めるところが二つになる。
        const got = await ask('setfield', {
            text: state.head + editor.getValue(),
            key: 'title',
            value: to || null,
        });
        if (typeof got.text !== 'string') return;
        const cut = await ask('split', { text: got.text });
        state.head = cut.head || '';
        loading = true;
        editor.setValue(cut.body || '');
        loading = false;
        state.dirty = true;
        await save();
        say(to ? '題を「' + to + '」にしました' : '題を外しました（見出しから決まります）');
    } catch (e) {
        say('題を直せません: ' + why(e));
        drawTitle();
    }
}

el('title').onclick = () => { if (state.open && el('title').contentEditable !== 'plaintext-only') renameTitle(); };
el('title').onkeydown = (e) => {
    if (isEnter(e)) { e.preventDefault(); el('title').blur(); return; }
    // **やめたら、何も変えない。** 打ちかけの字を捨てて元の題に戻す。
    if (e.code === 'Escape') { e.preventDefault(); titleDone(false); el('title').blur(); }
};
el('title').onblur = () => {
    if (el('title').contentEditable === 'plaintext-only') titleDone(true);
};

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
        say('vim を読めません: ' + why(e));
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
    const dir = dirOf(state.open.path);
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
            img.alt = w.alt;
            // **読めない絵は、黙って空けない。** 貼り間違いに気づけるように、
            // 何が読めなかったのかを出す ── ただし、**手を尽くしてから**。
            // 前はここだけ助け船（`fileBytes`）を持っておらず、読む面では
            // 出る絵が、コードの面でだけ「読めません」と言っていた。
            if (/^file:/i.test(w.src)) img.src = w.src;
            else showPicture(img, absPath(w.src, dir), () => {
                box.classList.add('bad');
                box.textContent = 'この絵は読めません: ' + w.src;
            });
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

/// **読む面に組んであるのは、どのノートの字か。**
///
/// `drawRead` が組んだときに札（`data-of`）を置き、**それ以外で字が動いたら
/// 剥がす** ── 別のノートを開いたとき、「コード」の面で打ったとき。
/// 書き戻し（`syncRead`）は、札がいま開いているノートを指しているときだけ
/// 通す。
///
/// これが無いと、読む面は「いま出ているのは、いま開いているノートの字」と
/// 思い込んだまま書き戻す。「コード」の面ではノートを替えても組み直さない
/// ので、面は**前のノートの字のまま** ── 次に別のノートへ替えた瞬間、
/// その字が今のノートへ書き込まれる（実際に二本のノートが、前書きだけ
/// 自分のまま**本文が別のノート**になった）。同じ道で、「コード」で打った
/// 行が、ノートを替えた瞬間に**組んだ時の字へ戻される**。
function readDrawn(path) { el('read').dataset.of = path; }
function readStale() { delete el('read').dataset.of; }
function readCurrent() { return !!state.open && el('read').dataset.of === state.open.path; }

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
    if (node.classList.contains('mermaid')) return true;
    // **中に枠や図を抱えたかたまりも、触らせない。**
    //
    // `> ``` ` のような引用は、外は引用・中は枠。外を触れるままにすると、
    // 中を字に戻すのは `blockToMd` の仕事になり、あちらは枠を知らない
    // （知らない札は中身だけ取る）── `` ` `` が行の途中のコードとして
    // 読み直され、**保存のたびに形が変わり続けた**（一周目で `> ` + 中身、
    // 二周目でさらに `` ` `` が増える）。同期していれば毎回差分になる。
    //
    // 元の字は外のかたまりが持っている（`data-md`）ので、外ごと返せば
    // 一文字も失わない。中を直したい人は「コード」の面へ ── 触れない
    // ものが一つ増えるが、**壊れるより狭いほうがよい**。
    return !!node.querySelector('pre, .mermaid');
}

/// 札を掛け替える。**元の行と元の字を、新しい節へ持たせる。**
///
/// 掛け替えるのは二か所 ── 絵を `<figure>` で包むとき（`findPictures`）と、
/// 行の記号を外して段落にするとき（`asPara`）。持たせないと、次の書き戻しで
/// **その一行が元の字を失う**（触れないかたまりなら `null` になり、保存が
/// 止まる）。
///
/// **切り出しの中に置く。** 電話も同じ掛け替えをする ── 外に置いていた
/// ときは、電話の束ねに `keepMark` が入っていなかった。
function keepMark(from, to) {
    for (const k of ['line', 'span', 'md']) {
        if (from.dataset[k] !== undefined) to.dataset[k] = from.dataset[k];
    }
}

/// 描いたあとの仕込み。
///
/// **書いてあった字を、かたまりごとに持たせておく**（`data-md`）── 戻せない
/// ものは、これをそのまま返す。行番号は `to_html` が差している。
/// **箱と字を受け取る形。** ここから下（`armPaper`・`paperToMd`・
/// `blockToMd`・`inlineToMd`・`richBlock`・`checkEnter`）は、画面のどこにも
/// 触らない ── 渡された箱と字だけを見る。
///
/// そうしてあるのは、**電話が同じものを使うため**。iPhone の「表示」も
/// `WKWebView` の `contenteditable` で、同じ組み方・同じ書き戻し方をする。
/// 書き戻しをもう一組 Swift で書けば、**同じノートが端末によって別の字に
/// 保存される** ── 失うのはたいてい表と升と図で、気づくのは何回か保存
/// したあと。`scripts/paper-test.js` が往復を見ているので、電話が使うのは
/// 試験の通ったものそのもの。
function armPaper(box, text, open) {
    box.contentEditable = open ? 'true' : 'false';
    box.spellcheck = false;
    if (!open) return;
    const src = text.split('\n');
    for (const node of [...box.children]) {
        const at = Number(node.dataset.line);
        const span = Number(node.dataset.span) || 1;
        // **一度持たせたら、二度と作り直さない。**
        //
        // 行番号は組み直した時点のもので、そのあと上の行が増えれば**ずれる**
        // ── `syncRead` は組み直さずに保存するので（caret を飛ばさないため）、
        // ずれた番号で字を切り直すと、図の「元の字」が**別の場所の字**に
        // なる。次の保存でそれが図の場所へ書き戻され、**図が丸ごと消える**。
        //
        // 実際に消した。「マーメイドの図がいつのまにか消えた」はこれ ──
        // 鍵盤で消していないのに消えるので、原因がどこにも見えない。
        //
        // 触れないかたまりの元の字は、**組み直したときにしか変わらない**
        // （図を直す工房は `readSourceEdit` を通り、あちらは必ず組み直す）
        // ので、既に持っているならそれが正しい。
        if (node.dataset.md === undefined && !Number.isNaN(at)) {
            node.dataset.md = src.slice(at, at + span).join('\n');
        }
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
function paperToMd(box, head) {
    const out = [];
    for (const node of box.children) {
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
    return head ? '\n' + body : body;
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
            // **始まりの番号は、書いた人のもの。** `3. 4.` と書いた一覧を
            // `1. 2.` に振り直さない（core は `<ol start>` で憶えている）。
            let n = (Number(node.getAttribute('start')) || 1) - 1;
            for (const li of node.children) {
                if (li.tagName !== 'LI') continue;
                n += 1;
                const mark = li.querySelector(':scope > .box');
                // **書いた印を、そのまま返す。** `* ` を `- ` に、`1) ` を
                // `1. ` に、`1. 1. 1.` を `1. 2. 3.` に丸めない ── どれも
                // 見え方は同じだが、人の書いた行を書き換えることになる
                // （同期していれば、開くたびに向こうへ差分が飛ぶ）。
                // 憶えていないもの（前の版で組んだ面・面の上で作った行）は、
                // これまで通りの形で書く。
                const was = li.dataset ? li.dataset.mark : '';
                const bullet = was || (node.tagName === 'OL' ? n + '. ' : '- ');
                const done = mark && mark.getAttribute('aria-pressed') === 'true';
                const head = mark
                    ? bullet + '[' + (done ? (li.dataset.box === 'X' ? 'X' : 'x') : ' ') + '] '
                    : bullet;
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
            // **元の字があるなら、それを返す。** `:---` と `---` は
            // core の `Align` では同じ値になる（見え方は同じでよい）が、
            // 字に戻すときに丸めると**人の書いた行が書き換わる**。
            // 憶えていないもの（前の版で組んだ面）は、見え方から作る。
            const aligns = [...rows[0].children].map((c) => {
                if (c.dataset && c.dataset.sep) return c.dataset.sep;
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
            // 書いた形をそのまま（`---` `***` `___`）。
            return (node.dataset && node.dataset.mark) || '---';
        case 'BR':
            return null;
        default: {
            const t = edges(inlineToMd(node));
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
        const t = edges(inlineToMd(node));
        if (t) out.push(t);
    }
    return out;
}

/// 前後を落とす。**全角空白（`　`）は落とさない** ── あれは字下げで、人が
/// 打った字。JavaScript の `trim()` は `　` も削るので、削るものを半角の
/// 空白と tab と改行だけに絞る（core の `mark_break` と揃えてある）。
const edges = (s) => String(s).replace(/^[ \t\n]+/, '').replace(/[ \t\n]+$/, '');

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
            // **行末の印は、そのまま戻す。** 空白二つと `\` はどちらも
            // 「ここで改行」の印で、amber は改行をそのまま描くので**見え方は
            // 同じ**だが、人が打った字なので落とさない（`markdown.rs` の
            // `mark_break` が `data-hard` に憶えさせている）。
            case 'BR': out += (c.dataset && c.dataset.hard ? c.dataset.hard : '') + '\n'; break;
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

/// 升の行で改行したら、次も升。**空の升で押したら、一覧から降りる。**
///
/// 点も番号も `contenteditable` の既定がそうしている ── 押せば次の行が
/// 同じ形で出て、何も書かずにもう一度押すと素の行に降りる。**升だけが
/// 違っていた**: 既定は升を持たない `<li>` を作るので、押した人は升を
/// 足したつもりで、出てきたのは点だった。前はそれを嫌って「何も無い行」に
/// 降ろしていたが、それだと**やることを続けて三つ書けない** ── 一つ書く
/// たびに帯の釦へ手が戻る。
///
/// 揃えるのは形ではなく**押し心地**: 三つとも「次も同じ、空なら降りる」。
function checkEnter(li) {
    if (!li) return false;
    if (!li.querySelector(':scope > .box')) return false;
    const list = li.parentElement;
    if (!list || !['UL', 'OL'].includes(list.tagName)) return false;
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return false;

    // 升だけで字が無い行 ── そこで一覧から降りる。**後ろの行は残す**
    // （降りたところで一覧を割る）。消してしまうと、真ん中で押した人が
    // 下の行ごと失う。
    if (!li.textContent.trim()) {
        const p = document.createElement('p');
        p.append(document.createElement('br'));
        const rest = [];
        for (let x = li.nextElementSibling; x; x = x.nextElementSibling) rest.push(x);
        list.after(p);
        if (rest.length) {
            const more = document.createElement(list.tagName);
            more.append(...rest);
            p.after(more);
        }
        li.remove();
        if (!list.children.length) list.remove();
        landAt(p, null);
        return true;
    }

    // 途中で押したら、後ろの字を次の升へ持っていく（点や番号と同じ）。
    const cut = sel.getRangeAt(0).cloneRange();
    cut.setEndAfter(li.lastChild);
    const tail = cut.extractContents();

    const next = document.createElement('li');
    next.className = 'task';
    const box = document.createElement('button');
    box.type = 'button';
    box.className = 'box';
    box.setAttribute('aria-pressed', 'false');
    // **升は字ではなく操作。** 中に caret が入ると、押せるものが打てる
    // ものに見える（`armPaper` が描き直しのたびに立てているのと同じ）。
    box.contentEditable = 'false';
    next.append(box, tail);
    li.after(next);
    landAt(next, box);
    return true;
}

/// 引用と注記の中で、空の行に降りたら**そこから出る**。
///
/// 中で改行すると引用が続くのは既定のとおりで、それは正しい ── けれど
/// **出る道が無かった**。点も番号も升も「空でもう一度押したら降りる」の
/// だから、引用と注記だけ抜けられないのは覚え違いに見える（Esc を押して
/// も、下へ矢印を押しても出られない）。
function quitEnter(node) {
    const box = node?.closest?.('blockquote, .alert');
    if (!box) return false;
    // 箱の直下の一行を探す ── 入れ子（引用の中の箇条書き）は既定に任せる。
    let line = node.nodeType === 3 ? node.parentElement : node;
    while (line && line.parentElement !== box) line = line.parentElement;
    if (!line || line.classList.contains('alert-h')) return false;
    if (!['P', 'DIV'].includes(line.tagName)) return false;
    // 字があるうちは、引用を続ける（既定のまま）。
    if (line.textContent.trim()) return false;

    const p = document.createElement('p');
    p.append(document.createElement('br'));
    // **後ろの行は残す。** 真ん中で押した人が下の行ごと失わないように、
    // そこで箱を割る。
    const rest = [];
    for (let x = line.nextElementSibling; x; x = x.nextElementSibling) rest.push(x);
    box.after(p);
    if (rest.length) {
        const more = box.cloneNode(false);
        more.removeAttribute('data-line');
        more.removeAttribute('data-md');
        // 注記は種類の札から始まる ── 割った先にも同じ札を付け直す。
        const label = box.querySelector(':scope > .alert-h');
        if (label) more.append(label.cloneNode(true));
        more.append(...rest);
        p.after(more);
    }
    line.remove();
    if (!box.querySelector(':scope > p, :scope > div, :scope > ul, :scope > ol')) box.remove();
    landAt(p, null);
    return true;
}

/* ── 段を深くする・浅くする（Tab / Shift+Tab） ── */

/// いま caret の居る「一行」。**箱の中の一行まで降りる** ── 引用の中の
/// 段落は、引用ではなく段落が一行。
function lineAt(box) {
    const sel = getSelection();
    let n = sel && sel.rangeCount ? sel.anchorNode : null;
    if (n && n.nodeType === 3) n = n.parentElement;
    if (!n || !box.contains(n)) return null;
    // 一行として扱うもの ── 項目・段落・見出し・表のセル。
    //
    // **箱そのものは返さない。** `div` を数えているので、行の見つからない
    // ところ（表のセルの中など）では `#read` が返ってしまい、「行が
    // 見つかった」ことになる ── セルの中の Enter が段落の Enter として
    // 通り、表の中に `<br>` が入った（試験が捕まえた）。
    const line = n.closest('li, p, div, h1, h2, h3, h4, h5, h6, td, th');
    return line && line !== box ? line : null;
}

/// caret が、その節の**行頭**に居るか。
///
/// **字が一文字も前に無いことを見る。** 節の先頭の子が升（`.box`）の
/// ことがあるので、`anchorOffset === 0` だけでは足りない。
function atHead(node) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !sel.isCollapsed) return false;
    const r = sel.getRangeAt(0).cloneRange();
    r.selectNodeContents(node);
    try {
        r.setEnd(sel.anchorNode, sel.anchorOffset);
    } catch {
        return false;                    // caret が節の外に居る
    }
    // **升の記号は、字ではない。** `☑` は操作の見た目で、人が打った字では
    // ない ── 数えると、升のある行の行頭がいつまでも「行頭ではない」に
    // なり、記号を外す一打が効かない。写しをとって、升を抜いてから測る。
    const bit = r.cloneContents();
    for (const b of bit.querySelectorAll('.box')) b.remove();
    return bit.textContent.length === 0;
}

/// 段を深くする・浅くする。受けたら `true`。
///
/// **一覧の中は子リスト、外は字下げ。** Inkdrop も一覧の中の Tab は
/// 子リストで、外は行の字下げ ── 向きは同じ（あちらは記号の面なので
/// 半角で書ける）。この面は記号を見せないので、字下げは**全角空白**
/// （`　`）を一つ置く。半角の空白と tab は、四つ揃うと Markdown が
/// コード枠にするので使えない。`　` は**ただの字**で、何段でも枠に
/// ならず、GitHub でも Obsidian でも同じ幅の空きとして出る。
///
/// **見出しでは何も起きない。焦点も動かさない** ── 見出しに字下げは
/// 無く、焦点が面から飛ぶのは事故（`PAPER.ja.md` 六章の芯の 1）。
function checkTab(box, back) {
    const line = lineAt(box);
    if (!line) return false;
    if (/^H[1-6]$/.test(line.tagName)) return true;      // 受けるが、何もしない
    if (line.closest('td, th')) return false;            // 表は表の道具が受ける
    if (line.tagName === 'LI') return back ? outdent(line) : indent(line);
    return back ? unpad(line) : pad(line);
}

/// 項目を一段深く。**上に項目が無ければ、深くしない。**
///
/// Markdown で親の無い入れ子は書けない（`  - あ` だけの一覧は段落になる）
/// ので、一覧の最初の項目は深くできない ── 何も起きないのが正しい。
function indent(li) {
    const above = li.previousElementSibling;
    if (!above || above.tagName !== 'LI') return true;
    const list = li.parentElement;
    // 上の項目が既に子を持っているなら、そこへ入る ── 新しい一覧を
    // 作ると、同じ段に一覧が二つ並ぶ。
    const there = [...above.children].find((x) => ['UL', 'OL'].includes(x.tagName));
    const into = there || document.createElement(list.tagName);
    if (!there) above.append(into);
    const at = caretIn(li);
    into.append(li);
    landBackIn(li, at);
    return true;
}

/// 項目を一段浅く。**いちばん浅い段なら、何もしない。**
function outdent(li) {
    const list = li.parentElement;
    const up = list && list.parentElement;
    if (!up || up.tagName !== 'LI') return true;
    const at = caretIn(li);
    // 下に残る兄弟は、この項目の子として連れていく ── 置いていくと
    // 順番が入れ替わる。
    const rest = [];
    for (let x = li.nextElementSibling; x; x = x.nextElementSibling) rest.push(x);
    up.after(li);
    if (rest.length) {
        const more = document.createElement(list.tagName);
        more.append(...rest);
        li.append(more);
    }
    if (!list.children.length) list.remove();
    landBackIn(li, at);
    return true;
}

/// 段落を一つ字下げる（全角空白を一つ、頭に置く）。
function pad(line) {
    const at = caretIn(line);
    line.insertBefore(document.createTextNode('　'), line.firstChild);
    line.normalize();
    landBackIn(line, at + 1);
    return true;
}

/// 字下げを一つ外す。**無ければ何もしない。**
function unpad(line) {
    const first = line.firstChild;
    if (!first || first.nodeType !== 3 || !first.data.startsWith('　')) return true;
    const at = caretIn(line);
    first.data = first.data.slice(1);
    landBackIn(line, Math.max(0, at - 1));
    return true;
}

/// その節の中で、caret が何文字目か。
function caretIn(node) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return 0;
    const r = sel.getRangeAt(0).cloneRange();
    r.selectNodeContents(node);
    r.setEnd(sel.anchorNode, sel.anchorOffset);
    return r.toString().length;
}

/// その節の中の、何文字目かへ caret を戻す。
///
/// **段を動かすと節が別の親へ移る**ので、選び目は外れている ── 文字数で
/// 数えて置き直す（`landAt` は先頭に置くだけで、居た場所には戻らない）。
function landBackIn(node, at) {
    const walk = document.createTreeWalker(node, 4 /* NodeFilter.SHOW_TEXT */);
    let seen = 0;
    let t;
    while ((t = walk.nextNode())) {
        if (seen + t.data.length >= at) {
            const r = document.createRange();
            r.setStart(t, Math.max(0, at - seen));
            r.collapse(true);
            const sel = getSelection();
            sel.removeAllRanges();
            sel.addRange(r);
            return;
        }
        seen += t.data.length;
    }
    landAt(node, null);
}

/* ── 矢印 ── */

/// 触れないかたまりを跨ぐ。受けたら `true`。
///
/// **図や枠の中に caret を置かない**（`PAPER.ja.md` 六章の芯の 3）── 中に
/// 入ると「押せるものが打てるものに見える」。二つ続いていれば、二つとも跨ぐ。
///
/// **跨いだ先が無いなら、置き場所を作る** ── 図で始まるノートの上に一行
/// 足す道が、いままで無かった（末尾には `tailStop` があるのに）。
function checkArrow(box, dir) {
    const line = lineAt(box);
    if (!line) return false;
    // 箱の直下のかたまりまで登る ── 跨ぐのは「行」ではなく「かたまり」。
    let here = line;
    while (here && here.parentElement !== box) here = here.parentElement;
    if (!here) return false;

    const back = dir === 'up' || dir === 'left';
    // 端に居るときだけ跨ぐ。真ん中なら、ふつうに一文字ずつ動く。
    if (back ? !atHead(here) : !atTail(here)) return false;

    let to = back ? here.previousElementSibling : here.nextElementSibling;
    if (!to || !richBlock(to)) return false;        // 隣が触れないものでなければ、既定のまま
    while (to && richBlock(to)) to = back ? to.previousElementSibling : to.nextElementSibling;
    if (!to) {
        // 跨いだ先が無い ── そちらの端に、降りられる一行を置く。
        to = document.createElement('p');
        to.append(document.createElement('br'));
        if (back) box.prepend(to); else box.append(to);
    }
    landBackIn(to, back ? String(to.textContent).length : 0);
    return true;
}

/// caret が、その節の**末尾**に居るか（`atHead` の裏）。
function atTail(node) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !sel.isCollapsed) return false;
    const r = sel.getRangeAt(0).cloneRange();
    r.selectNodeContents(node);
    try {
        r.setStart(sel.anchorNode, sel.anchorOffset);
    } catch {
        return false;
    }
    const bit = r.cloneContents();
    for (const b of bit.querySelectorAll('.box')) b.remove();
    return bit.textContent.length === 0;
}

/// ノートの**先頭**が触れないかたまりなら、その上に降りられる一行を置く。
///
/// `tailStop` の対。**末尾には既にあった**（表や罫線で終わるノートに caret を
/// 降ろす先が要る・依頼 203）が、先頭には無く、**図で始まるノートの上に
/// 一行足す道がどこにも無かった**。
///
/// **常に置く。** 「caret が近づいたときだけ出す」もありうるが、増えたり
/// 消えたりするものは往復の試験で数が合わなくなる ── 空のままなら字に
/// 戻すとき落ちるので、ファイルは増えも減りもしない。
function headStop(box) {
    const first = box.firstElementChild;
    if (!first || !richBlock(first)) return;
    const p = document.createElement('p');
    p.append(document.createElement('br'));
    box.prepend(p);
}

/* ── Enter ── */

/// 見出しと表の Enter。受けたら `true`。
///
/// **見出しの次は段落。** 見出しは一行のもので、見出しが二つ続くことは
/// まず無い（Word も Docs も Notion もそうしている）。**途中で押しても
/// 後ろは段落** ── 本人が決めた（2026-09-08「大きいままじゃない」）。
/// 既定は見出しを二つに割るが、「見出しの途中で Enter」はたいてい
/// 「見出しを打ち終えて本文に降りたい」のに caret が末尾に無かっただけで、
/// 見出しが二つになるより後ろが本文になるほうが直しが少ない。
///
/// **表のセルの Enter は、下のセルへ。** セルの中に改行は書けない
/// （Markdown の表は一行一行）── 既定に任せると、表を壊すか改行を黙って
/// 落とすかのどちらかになる。最後の行なら、行を一つ足す。
function checkReturn(box) {
    const line = lineAt(box);
    if (!line) return false;

    const cell = line.closest ? line.closest('td, th') : null;
    if (cell && box.contains(cell)) return nextCell(cell);

    if (!/^H[1-6]$/.test(line.tagName)) return false;
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return false;
    // 先頭で押したら、上に空の段落（既定と同じ）── 見出しは見出しのまま。
    if (atHead(line)) return false;

    // 後ろの字を、新しい段落へ連れていく（末尾で押したなら空の段落）。
    const cut = sel.getRangeAt(0).cloneRange();
    cut.setEndAfter(line.lastChild || line);
    const tail = cut.extractContents();
    const p = document.createElement('p');
    p.append(tail);
    if (!p.textContent) p.append(document.createElement('br'));
    line.after(p);
    landBackIn(p, 0);
    return true;
}

/// 表の次のセルへ ── **下**（同じ列）。最後の行なら行を一つ足す。
///
/// 表計算はどれも Enter で下へ行く。横へ行くのは Tab（`tableDo` の側）。
function nextCell(cell) {
    const row = cell.parentElement;
    const table = cell.closest('table');
    if (!row || !table) return false;
    const at = [...row.children].indexOf(cell);
    const rows = [...table.querySelectorAll('tr')];
    const n = rows.indexOf(row);
    let below = rows[n + 1];
    if (!below) {
        // 最後の行 ── 行を一つ足す。**セルの数は上に合わせる**（数が
        // 行ごとに違う表は、GFM では崩れる）。
        const body = table.querySelector('tbody') || table;
        below = document.createElement('tr');
        for (let i = 0; i < row.children.length; i++) {
            below.append(document.createElement('td'));
        }
        body.append(below);
    }
    const to = below.children[at] || below.children[below.children.length - 1];
    if (to) landBackIn(to, 0);
    return true;
}

/// 段落の中の改行（`Shift+Enter`）。
///
/// **Enter は新しい段落、`Shift+Enter` は段落の中の改行** ── 本人が決めた
/// （2026-09-08「圧倒的に案 A」）。Word・Docs・Notion の手がそのまま動く。
/// core が段落の中の一つの改行を改行として描くようになったので（依頼 384）、
/// ここで入れる `<br>` は字の側でも改行として残る。
///
/// **見出し・項目の中では、ふつうの Enter と同じ** ── 見出しと項目は
/// 一行のもの。表のセルには改行が書けないので、何も起きない。
function checkSoftReturn(box) {
    const line = lineAt(box);
    if (!line) return false;
    if (line.closest && line.closest('td, th')) return true;   // 受けて、止める
    if (line.tagName === 'LI' || /^H[1-6]$/.test(line.tagName)) return false;
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return false;
    const r = sel.getRangeAt(0);
    r.deleteContents();
    const br = document.createElement('br');
    r.insertNode(br);
    // **行末に置いたら、詰め物をもう一枚。** `<br>` が節の最後だと caret の
    // 行き先が無く、置いた場所が前の行の末尾に見える（`contenteditable` の
    // よくある形）。字に戻すときは**末尾の改行として落ちる**ので
    // （`edges`）、ファイルには出ない。
    if (!br.nextSibling) br.after(document.createElement('br'));
    const to = document.createRange();
    to.setStartAfter(br);
    to.collapse(true);
    sel.removeAllRanges();
    sel.addRange(to);
    return true;
}

/// 選んだ範囲（無ければ caret の行）が、その札の中に居るか。
function inside(box, tag) {
    for (const line of pickedLines(box)) {
        const n = line.closest ? line.closest(tag) : null;
        if (!n || !box.contains(n)) return false;
    }
    return true;
}

/// 選んだ範囲にかかっている「行」たち。選んでいなければ caret の一行だけ。
function pickedLines(box) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return [];
    const r = sel.getRangeAt(0);
    const all = [...box.querySelectorAll('li, p, h1, h2, h3, h4, h5, h6')]
        .filter((n) => r.intersectsNode(n));
    if (all.length) {
        // 入れ子は、いちばん内側だけ ── 外側の項目も範囲に当たる。
        return all.filter((n) => !all.some((m) => m !== n && n.contains(m)));
    }
    const one = lineAt(box);
    return one ? [one] : [];
}

/// 選んだ行の見出しを、段落に落とす（点を付ける前に）。
function flattenHeads(box) {
    for (const line of pickedLines(box)) {
        if (!/^H[1-6]$/.test(line.tagName)) continue;
        const p = document.createElement('p');
        p.append(...line.childNodes);
        keepMark(line, p);
        line.replaceWith(p);
    }
}

/// 引用・注記の箱から、選んだ行を出す。**範囲を選んでいれば、選んだ行ぜんぶ。**
function unwrapBlock(box, tag) {
    const lines = pickedLines(box);
    for (const line of lines) {
        const wrap = line.closest(tag);
        if (!wrap || !box.contains(wrap)) continue;
        wrap.before(line);
        if (!wrap.textContent.trim()) wrap.remove();
    }
    if (lines[0]) landBackIn(lines[0], 0);
    return true;
}

/// 一覧から、選んだ項目を出して段落にする。
function unwrapList(box) {
    const lines = pickedLines(box).filter((n) => n.tagName === 'LI');
    let last = null;
    for (const li of lines) {
        const list = li.parentElement;
        if (!list || !['UL', 'OL'].includes(list.tagName)) continue;
        const p = document.createElement('p');
        for (const x of [...li.childNodes]) {
            if (x.nodeType === 1 && x.classList?.contains('box')) continue;
            p.append(x);
        }
        if (!p.childNodes.length) p.append(document.createElement('br'));
        list.before(p);
        li.remove();
        if (!list.children.length) list.remove();
        last = p;
    }
    if (last) landBackIn(last, 0);
    return true;
}

/// 見出しは押すたびに深くなる ── 書く面と同じ（`#` → `##` → `###` → 無し）。
function readHeading() {
    const n = caretBlock();
    const now = n && /^H[1-6]$/.test(n.tagName) ? Number(n.tagName[1]) : 0;
    readBlockAs(now >= 3 ? 'p' : 'h' + (now + 1));
}

/* ── 選んで消す ── */

/// 選んだ範囲を消すとき、表を壊さないように受ける。受けたら `true`。
///
/// **セルの数が変わる操作は、表の道具だけ。** `blockToMd` は行ごとにセルを
/// 数えて書くので、**列の数が行ごとに違う表は GFM で崩れる** ── `contenteditable`
/// の既定は、セルをまたぐ選びを消すときにセルや行そのものを消したり繋げたり
/// する。表計算の Delete と同じにする ── **中身を空にして、セルは残す。**
///
/// **表の外から中へ跨ぐ選びは、何も起きない** ── 「表の途中までを消す」に
/// 正しい答えが無いので、答えないほうがよい。
function checkCut(box) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || sel.isCollapsed) return false;
    const r = sel.getRangeAt(0);
    const from = cellOf(r.startContainer, box);
    const to = cellOf(r.endContainer, box);
    if (!from && !to) return false;                 // 表に関わらない選び
    if (!from || !to || from.closest('table') !== to.closest('table')) {
        // 片方だけ表の中 ── 跨いでいる。何も起きない。
        return true;
    }
    if (from === to) return false;                  // 一つのセルの中は、既定のまま
    // セルをまたいだ ── 中身だけ空にする。
    const cells = [...from.closest('table').querySelectorAll('th, td')];
    const a = cells.indexOf(from);
    const b = cells.indexOf(to);
    for (const c of cells.slice(Math.min(a, b), Math.max(a, b) + 1)) c.textContent = '';
    landBackIn(from, 0);
    return true;
}

/// その節が入っている表のセル（箱の中のものだけ）。
function cellOf(node, box) {
    let n = node && node.nodeType === 3 ? node.parentElement : node;
    const cell = n && n.closest ? n.closest('td, th') : null;
    return cell && box.contains(cell) ? cell : null;
}

/* ── 行頭の Backspace ── */

/// **行頭の Backspace は、この行の記号を一つ外す。**
///
/// `PAPER.ja.md` 六章の芯の 1 ── 面の上の一打は、字の上の一つの記号に
/// 対応する。既定に任せると、途中の項目は**前の項目と繋がる**（升が一つ
/// 黙って消える ── 二つの「やること」が一行になる）。どの位置の項目でも
/// 同じでなければ、押すたびに結果を見る癖がつく。
///
/// 外すものが無くなったら、Backspace は字を消す鍵に戻る（`false` を返す）。
/// 受けたら `true`。
function checkBack(box) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !sel.isCollapsed) return false;   // 選びは選びの話
    const line = lineAt(box);
    if (!line || !atHead(line)) return false;

    // **触れないかたまりの隣では、何も起きない。** caret の無いものが
    // 一打で消えるのは事故（芯の 2）── 消す道は、押したときの吹き出し。
    const before = line.previousElementSibling;
    if (before && richBlock(before)) return true;

    // **字下げには、何もしない。** `　` はただの字なので、caret がその
    // 後ろに居れば既定の Backspace が一つ消す（芯の 1 がそのまま効く）。
    // ここで外しにいくと、`　` の**前**で押した人の一打が、下の行を
    // 巻き込まずに字下げだけ消す ── 押した場所と結果が合わない。

    // 見出しは段落になる。**前の段落には繋がない** ── 見出しを前の段落の
    // 字に繋ぎたい人は、まず居ない。
    if (/^H[1-6]$/.test(line.tagName)) return asPara(line);

    if (line.tagName === 'LI') {
        // 入れ子なら、一段浅く（Tab と対にする）。
        if (line.parentElement?.parentElement?.tagName === 'LI') return outdent(line);
        return unlist(line);
    }

    // 引用・注記は、**最初の行の行頭だけ**出る。途中の行は前の行と繋がる
    // （既定のまま）── 引用の中の行は自分の記号を持っていない（`>` は箱の
    // 記号で、行の記号ではない）ので、二行目に外すものは見えていない。
    const quote = line.closest('blockquote, .alert');
    if (quote && box.contains(quote)) {
        const head = [...quote.children].find((x) => !x.classList.contains('alert-h'));
        if (head === line) return unquote(quote, line);
    }
    return false;
}

/// 見出しを段落にする（字はそのまま）。
function asPara(line) {
    const p = document.createElement('p');
    p.append(...line.childNodes);
    keepMark(line, p);
    line.replaceWith(p);
    landBackIn(p, 0);
    return true;
}

/// 項目の記号を外して、段落にする。**一覧はそこで割れ、下の項目は残る。**
function unlist(li) {
    const list = li.parentElement;
    if (!list || !['UL', 'OL'].includes(list.tagName)) return false;
    const p = document.createElement('p');
    // 升も一緒に外れる ── 記号を外すとは、そういうこと。
    for (const x of [...li.childNodes]) {
        if (x.nodeType === 1 && x.classList?.contains('box')) continue;
        p.append(x);
    }
    if (!p.childNodes.length) p.append(document.createElement('br'));
    const rest = [];
    for (let x = li.nextElementSibling; x; x = x.nextElementSibling) rest.push(x);
    list.after(p);
    if (rest.length) {
        const more = document.createElement(list.tagName);
        more.append(...rest);
        p.after(more);
    }
    li.remove();
    if (!list.children.length) list.remove();
    landBackIn(p, 0);
    return true;
}

/// 引用・注記の最初の行を、箱から出す。**箱に何も残らなければ箱ごと消える**
/// （注記なら種類の札も）。
function unquote(quote, line) {
    const p = document.createElement('p');
    // **出すのは一行だけ。** 段落の中の改行は `<br>` なので、二行の引用は
    // 一つの段落になっている ── 丸ごと出すと、一打で二行ぶんの `>` が
    // 外れる（芯の 1 は「一打は一つの記号」）。最初の `<br>` で割る。
    const at = [...line.childNodes].findIndex((x) => x.nodeName === 'BR');
    if (at >= 0) {
        p.append(...[...line.childNodes].slice(0, at));
        line.childNodes[0].remove();            // 割れ目の `<br>` を落とす
    } else {
        p.append(...line.childNodes);
        line.remove();
    }
    if (!p.childNodes.length) p.append(document.createElement('br'));
    quote.before(p);
    const left = [...quote.children].filter((x) => !x.classList.contains('alert-h'));
    if (!left.length || !quote.textContent.trim()) quote.remove();
    landBackIn(p, 0);
    return true;
}

/// 飾りの終わりに caret があるなら、その外へ出す。
///
/// **選んだ字を飾ったあと、続けて打った字まで太字になっていた。**
/// `execCommand` は選び目を `<b>` の中に残すので、そこから打てば中に入る
/// ── 飾ったのは**選んだ字**であって、これから打つ字ではない。飾りの
/// 終わりに立っているときだけ外へ出す（途中に居るなら、そこは中の字）。
const DRESS = 'b, strong, i, em, s, strike, del, code';

function outOfDress() {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !sel.isCollapsed) return false;
    const node = sel.anchorNode;
    const from = node && node.nodeType === 3 ? node.parentElement : node;
    if (!from || !from.closest) return false;
    let dress = from.closest(DRESS);
    if (!dress) return false;
    // いちばん外側の飾りまで登る（`**_こう_**` は二重になる）。
    for (let up = dress.parentElement; up && up.matches && up.matches(DRESS); up = up.parentElement) {
        dress = up;
    }
    // caret から飾りの終わりまでに字が残っているなら、そこは途中。
    const tail = document.createRange();
    tail.selectNodeContents(dress);
    tail.setStart(node, sel.anchorOffset);
    if (tail.toString().length) return false;

    const out = document.createRange();
    out.setStartAfter(dress);
    out.collapse(true);
    sel.removeAllRanges();
    sel.addRange(out);
    // **選び目を動かすだけでは足りない。** ブラウザは「いま打つと何になるか」
    // を別に憶えていて（typing style）、飾りの隣で打った字をその飾りの中へ
    // 吸い込む ── 出たつもりで中に入る。憶えているほうも消す。
    // **古い呼び方なので、無い画面もある**（試験の軽い DOM がそう）──
    // 憶えているものを消せなくても、選び目は既に外に出ている。
    try {
        for (const k of ['bold', 'italic', 'strikeThrough']) {
            if (document.queryCommandState(k)) document.execCommand(k, false, null);
        }
    } catch { /* 消せなくても、出たことは変わらない */ }
    return true;
}

/// caret を置く。`after` があれば、その節の**すぐ後ろ**へ。
function landAt(node, after) {
    const r = document.createRange();
    if (after) r.setStartAfter(after);
    else { r.selectNodeContents(node); }
    r.collapse(true);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
}

/// ノートの隣に置かれた絵の、ほんとうの在りか。
function absPath(src, dir) {
    return src.startsWith('/') || /^[a-z]:[/\\]/i.test(src) ? src : dir + src;
}

/// 絵を出す。**読めなかったら、開いているアプリに読んでもらう。**
///
/// この画面は crmaine の `<webview>` の中でも動く。あちらでは `file://` の
/// 絵が届かず、**見本のノートの絵だけが出ない**ことになっていた（amber 自身
/// の窓では出るので、撮っても分からない ── 向こうで開くまで分からない）。
///
/// `fileBytes` は同梱する側も持っている口で、読んだ中身をそのまま返す ──
/// `data:` なら、どの入れ物でも出る。**先に `file:` を試すのは、そちらが
/// 安いから**（大きな絵を毎回 base64 にして持ち歩く理由は、出るなら無い）。
///
/// **一本にしてある。** 前は読む面とコードの面で別々に書いていて、助け船が
/// 片方にしか無かった ── 会社の Windows で「この絵は読めません」と出たのは
/// そこ（道の直し方も、片方だけ直せば済んでしまう形だった）。
function showPicture(img, at, onFail) {
    img.src = fileURL(at);
    img.addEventListener('error', async () => {
        // 助け船で入れ替えた絵も出なかったら、もう手が無い ── そのとき言う。
        if (onFail) img.addEventListener('error', onFail, { once: true });
        try {
            const got = await window.amber.fileBytes(at);
            if (got && got.b64) {
                img.src = 'data:image/' + (got.ext || 'png') + ';base64,' + got.b64;
                return;
            }
        } catch { /* 読めないものは読めない */ }
        if (onFail) onFail();
    }, { once: true });
}

/// この窓の「表示」の面を、上の切り出しに繋ぐ薄い包み。
///
/// **切り出しの外に置く。** 電話が持っていくのは上の切り出しだけで、ここは
/// `el('read')` も `state` も見る ── 中に混ぜると、電話の束ねに
/// 「呼べば落ちる関数」が入る。
function armRead() {
    armPaper(el('read'), whole(), !!state.open && view !== 'write');
    // 来た行の地色は、組み直すたびに敷き直す ── 札は組み直しで消えるので。
    paintIncoming();
}

function readToMd() {
    return paperToMd(el('read'), state.head);
}

/// 同じノートと見てよいか。**末尾の空行の数だけは、見ない。**
/// 読む面がそこを勝手に決めているので（`tailStop`）、そこの差は
/// 「人が書き換えた」ではない。
function sameNote(a, b) {
    return String(a).replace(/\n+$/, '') === String(b).replace(/\n+$/, '');
}

/// 打ったら、落ち着いてから書き戻す。
function readChanged() {
    if (syncing || view === 'write' || !state.open) return;
    state.dirty = true;
    el('state').textContent = '書きかけ';
    drawStrip();
    clearTimeout(readTimer);
    // **変換中に切れたら、待つ。** 数え直すだけで、書き戻しはしない ──
    // 未確定の字を保存しないため（`composing` の註）。
    readTimer = setTimeout(function again() {
        if (composing) { readTimer = setTimeout(again, 700); return; }
        syncRead();
    }, 700);
}

/// 升の行の Enter を、`checkEnter` に渡す。**判断は切り出しの側** ──
/// 電話も同じ関数を呼ぶので、押し心地が端末で分かれない。
el('read').addEventListener('keydown', (e) => {
    if (!isEnter(e) || e.isComposing || e.keyCode === 229) return;
    if (e.metaKey || e.ctrlKey) return;
    let n = getSelection()?.anchorNode;
    if (n && n.nodeType === 3) n = n.parentElement;
    if (!n || !el('read').contains(n)) return;
    // **`⇧Enter` は段落の中の改行。** Enter は新しい段落 ── Word・Docs・
    // Notion の手がそのまま動く（本人が決めた・2026-09-08）。
    if (e.shiftKey) {
        if (!checkSoftReturn(el('read'))) return;
        e.preventDefault();
        readChanged();
        return;
    }
    const li = n.closest('li');
    if (!(li ? checkEnter(li) : false) && !quitEnter(n) && !checkReturn(el('read'))) return;
    e.preventDefault();
    readChanged();
});

/// 打った字を、飾りの外へ。**打つ直前に出す** ── 飾った直後に選び目を
/// 動かすと、続けて ⌘I を押す道が消える。IME は組み始めに出す（組んで
/// いる最中に選び目を動かすと、変換そのものが壊れる）。
el('read').addEventListener('beforeinput', (e) => {
    if (e.isComposing || e.inputType !== 'insertText' || e.data == null) return;
    if (!outOfDress()) return;
    e.preventDefault();
    document.execCommand('insertText', false, e.data);
    readChanged();
});
/* ── 日本語入力（変換中） ── */

el('read').addEventListener('compositionstart', () => {
    composing = true;
    outOfDress();
});
el('read').addEventListener('compositionend', () => {
    composing = false;
    // **確定した瞬間には組み直さない。** caret が飛ぶ ──「打っている間は
    // 組み直さない」の内側。いつもの間合いで書き戻すだけ。
    readChanged();
    if (drawAfter) { drawAfter = false; readSoon(); }
});

/// 表の中の Tab は、次の升へ。
///
/// **表を打つのは「升から升へ」で、行を打つのではない。** 既定の Tab は
/// 焦点を面ごと出してしまい、打っている途中で表から追い出される ──
/// 打ち込みの表計算でも文書でも、そこは次の升に決まっている。
///
/// 最後の升で押したら**行を一つ足す** ── 足し方を探しに行かせない。
/// 行頭の Backspace ── 記号を一つ外す（`checkBack`）。
///
/// **変換中は IME に渡す。** 文節の区切りがこの鍵で動く（`PAPER.ja.md`
/// 六章の丁）。
el('read').addEventListener('keydown', (e) => {
    if (!['Backspace', 'Delete'].includes(e.code) || e.isComposing || e.keyCode === 229) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    // 選んで消すときは、表を壊さないほうが先に受ける。
    if (checkCut(el('read'))) { e.preventDefault(); readChanged(); return; }
    if (e.code !== 'Backspace') return;
    if (!checkBack(el('read'))) return;
    e.preventDefault();
    readChanged();
});

/// 矢印 ── 触れないかたまりを跨ぐ（`checkArrow`）。
///
/// **変換中は IME に渡す。** 文節の区切りがこの鍵で動く。
el('read').addEventListener('keydown', (e) => {
    const dir = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' }[e.code];
    if (!dir || e.isComposing || e.keyCode === 229) return;
    if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;   // 選びと飛びは既定のまま
    if (!checkArrow(el('read'), dir)) return;
    e.preventDefault();
});

/// Tab / Shift+Tab ── 一覧の中は段、外は字下げ（`checkTab`）。
///
/// **表の中は、表の道具が先に受ける**（次のセルへ）。
el('read').addEventListener('keydown', (e) => {
    if (e.code !== 'Tab' || e.isComposing || e.keyCode === 229) return;
    // caret の居場所は、字の節のことも升そのもののこともある ──
    // 片方だけ見ると、升の終わりに置いたときだけ効かない。
    let n = getSelection()?.anchorNode;
    if (n && n.nodeType === 3) n = n.parentElement;
    const cell = e.target.closest?.('td, th') || n?.closest?.('td, th');
    if (!cell || !el('read').contains(cell)) {
        // **表の外。** 焦点を面から飛ばさない ── 受けたなら止める。
        if (!checkTab(el('read'), e.shiftKey)) return;
        e.preventDefault();
        readChanged();
        return;
    }
    e.preventDefault();
    const table = cell.closest('table');
    const cells = [...table.querySelectorAll('th, td')];
    const at = cells.indexOf(cell);
    const to = cells[at + (e.shiftKey ? -1 : 1)];
    if (to) { landInCell(to); return; }
    if (e.shiftKey) return;
    // 最後の升 ── 行を足して、その頭へ。
    const body = table.tBodies[0] || table;
    const wide = (table.tHead?.rows[0] || body.rows[0])?.cells.length || 1;
    const row = body.insertRow();
    for (let n = 0; n < wide; n++) {
        // 空の升は、描く側によっては消える ── 全角空白で埋める（表の
        // 道具（`tableDo`）と同じ埋め方）。
        row.insertCell().textContent = '　';
    }
    landInCell(row.cells[0]);
    readChanged();
});

function landInCell(cell) {
    const r = document.createRange();
    r.selectNodeContents(cell);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
}

/// DOM を字に戻して、いつもの保存を通す。
///
/// **描き直さない。** 打っている最中に組み直すと、caret がどこかへ飛ぶ ──
/// 見た目は既に打った通りになっているので、組み直す理由も無い。
async function syncRead() {
    if (syncing || !state.open || !editor) return;
    // **変換の途中なら、書き戻さない。** 未確定の字はまだ人の字ではない
    // ── 確定してから数え直す（`composing` の註）。
    if (composing) return;
    // **面の字が、いま開いているノートのものでなければ書き戻さない。**
    // 前のノートの字を、今のノートへ書くことになる（`readDrawn`）。
    //
    // 同じ道で、**空の面**からも書き戻さない ── コードの面だけを使って
    // いる人の読む面は一度も組まれず、空の面を字に戻すと `"\n"` になり、
    // それが「一文字のノートに書き換えられた」として保存される（v2.8.3 で
    // 再現。コードの面で立ち上げて一覧で別のノートを押すと 2 バイトに
    // なった）。組んでいないなら札も無いので、ここで止まる ──
    // **見張りは一つ。** 二つ置くと、片方だけ直した日に必ずずれる。
    if (!readCurrent()) return;
    const body = readToMd();
    if (body === null) {
        // **黙って止まらない。** 打った字が消えたように見えるのがいちばん悪い。
        say('保存できません ── 図かコード枠の元の字が取れません。'
            + '「コード」の面で直してください');
        el('state').textContent = '保存できません';
        return;
    }
    // **末尾の空行の数では、変わったことにしない。**
    //
    // 読む面は末尾にいつも空の段落を一つ置く（`tailStop` ── 表や罫線で
    // 終わるノートに caret を降ろす先が要る）。だから字に戻すと、末尾の
    // 改行が元の字より一つ多くなる ── **見ただけのノートが、毎回「変わった」
    // ことになっていた**。ノートAを見てBを開くと、Aがフル保存され、履歴が
    // 一枚積まれ、棚が丸ごと数え直された（1002本で 0.7 秒）。
    //
    // 末尾の空行は、読む面では人が決められない（消しても `tailStop` が
    // 足し直す）。**決められないものの差で、保存を走らせない。**
    if (sameNote(state.head + body, whole())) return;
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
    const box = el('read');
    box.focus();
    const sel = getSelection();
    const had = sel && !sel.isCollapsed;
    // **見出しは飛ばす。** 見出しは既に太い ── 見た目が変わらないのに
    // `## **見出し**` と記号だけ増えるのは、次の組み直しで「何も変わって
    // いない」ように見えて、同期先では差分になる。
    //
    // 選んだ範囲が見出しだけなら、何もしない。混ざっているなら、見出しを
    // 外した範囲に掛け直す。
    if (had && cmd === 'bold') {
        const lines = pickedLines(box);
        const heads = lines.filter((n) => /^H[1-6]$/.test(n.tagName));
        if (heads.length) {
            const rest = lines.filter((n) => !heads.includes(n));
            if (!rest.length) return;               // 見出しだけ ── 何もしない
            const r = document.createRange();
            r.setStartBefore(rest[0]);
            r.setEndAfter(rest[rest.length - 1]);
            sel.removeAllRanges();
            sel.addRange(r);
        }
    }
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
    const box = el('read');
    box.focus();
    // **一行は、見出しか項目か、どちらか一つ。** `- ## 見出し` は書けはする
    // が、読む人にも書く人にも意味が無い ── しかも `blockToMd` は一覧の中の
    // 見出しを知らないので、**見た目は見出しのまま、保存すると黙って落ちる**。
    // 落とすなら**押した瞬間に見えて落ちる**（本人：「自分で選んだ操作だから
    // 驚かない」・2026-09-08）。
    if (what === 'ul' || what === 'ol') flattenHeads(box);

    // **同じ釦で、付けると外す。** 引用の中で「引用」を押したら外れる ──
    // 行頭の Backspace で出るのは一行ずつで、長い引用では手が疲れる
    // （本人が求めた道・2026-09-08）。Word の太字と同じ手触り。
    if (what === 'blockquote' && inside(box, 'blockquote')) {
        unwrapBlock(box, 'blockquote');
        readChanged();
        return;
    }
    if ((what === 'ul' || what === 'ol') && inside(box, what.toUpperCase())) {
        unwrapList(box);
        readChanged();
        return;
    }

    if (what === 'ul') document.execCommand('insertUnorderedList');
    else if (what === 'ol') document.execCommand('insertOrderedList');
    else document.execCommand('formatBlock', false, what);
    readChanged();
}

/// 字そのものを書き換える道具（チェック・リンク・表…）。
///
/// **いったん全部を字に戻してから直し、組み直す。** 見た目の上でやろうと
/// すると、升や表のような「札の形が決まっているもの」を DOM の上で組み立て
/// 直すことになり、そこだけ別の作り方が生える。
async function readSourceEdit(change, node, stay) {
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
        say('置けません: ' + why(e));
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
    //
    // `stay` のときは、**その行の終わりに残る** ── チェックリストや引用は
    // 「この行をそうする」道具で、押した人はそこに打ち続けようとしている。
    // 次へ降りると、改行されたように見える（箇条書きは DOM を直に触るので
    // そうならず、二つの道具が違う振る舞いをしていた）。
    landAfter(at, stay);
}

/// `n` 番目のかたまりの、次に caret を置く。
function landAfter(n, stay) {
    const box = el('read');
    const kids = [...box.children];
    const to = stay ? kids[n] : (kids[n + 1] || kids[kids.length - 1]);
    if (!to) return;
    box.focus();
    const r = document.createRange();
    r.selectNodeContents(to);
    // 残るときは行の終わりへ、次へ行くときは頭へ。
    r.collapse(!stay);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
    caretAt = to;
    to.scrollIntoView({ block: 'nearest' });
}

const readMark = (kind, withWhat, stay) => readSourceEdit((md) =>
    ask('mark', { kind, with: withWhat || '', text: md }).then((r) => r.text), null, stay);

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
    ], vimOn ? 'いま オン（vim の打ち方）' : 'いま オフ（素のメモ帳）');
    if (to === null || to === vimOn) return;
    setVim(to);
}

/// 行番号の入切。**「コード」の面だけの話。**
async function cmdLineNo() {
    const to = await askPick('行番号', [
        { name: '出す', sub: '「コード」の面の左に', value: true },
        { name: '出さない', value: false },
    ], lineNo ? 'いま オン' : 'いま オフ');
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
        ['チェックリスト', '⌘⇧9', () => onRead() ? readMark('line', '- [ ] ', true) : applyMark('line', '- [ ] ')],
        ['番号リスト', '⌘⇧7', () => onRead() ? readBlockAs('ol') : applyMark('line', '1. ')],
        ['太字', '⌘B', () => onRead() ? readDress('bold') : applyMark('wrap', '**')],
        ['画像', '', pickPicture],
        // **フローは画像の隣。** 図は「使う人は使う」もので、二列目に
        // 畳んでおくと、あることに気づかれない。
        ['フロー', '', cmdDiagram],
        // **絵文字は一列目。** 毎日の返事に使うもので、二列目に畳むと
        // あることに気づかれない（依頼 418）。
        ['絵文字', '', openEmoji, '😀'],
    ],
    [
        ['斜体', '⌘I', () => onRead() ? readDress('italic') : applyMark('wrap', '*')],
        ['取り消し線', '⌘⇧X', () => onRead() ? readDress('strikeThrough') : applyMark('wrap', '~~')],
        ['|'],
        ['リンク', '⌘K', () => onRead() ? readPut('[見せる字](https://)') : put('[](https://)', 1)],
        // **升を空で出さない。** 空の表は「これで合っているのか」が
        // 分からず、打つ前に一度立ち止まる ── 見本の字が入っていれば、
        // 上から順に置き換えるだけになる。
        ['表', '⌘⇧T', () => onRead()
            ? readPut('| 見出し | 見出し |\n| --- | --- |\n| 項目 | 項目 |')
            : put('| 見出し | 見出し |\n| --- | --- |\n| 項目 | 項目 |\n', 2)],
        ['水平線', '', () => onRead() ? readPut('---') : put('\n---\n\n')],
        ['|'],
        ['引用', "⌘'", () => onRead() ? readBlockAs('blockquote') : applyMark('line', '> ')],
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
        // **「…」は流されない。** 帯は狭い画面で横に流れるので、絵を足した
        // ぶん「ほかの記号」が右へ押し出されて**画面の外**へ行った ──
        // 畳んだり開いたりする釦が見えないと、二列目があること自体が
        // 分からない。流れるのは記号だけにして、釦は端に残す。
        const scroll = document.createElement('div');
        scroll.className = 'rs';
        r.append(scroll);
        for (const [name, key, , icon] of row) {
            if (name === '|') {
                const sep = document.createElement('div');
                sep.className = 'sep';
                scroll.append(sep);
                continue;
            }
            const b = document.createElement('button');
            // **名前だけ。絵は置かない。**
            //
            // 一度は絵だけにした（2026-09-08 の午前）── 訳の要る字を帯に
            // 並べたくない、が理由だった。**その日のうちに戻している**：
            // 「かっこよくなったが、ビギナーには何がなんだか分からない」。
            //
            // **分かることを、訳の都合で捨てない。** 外へ配る日は、字を
            // その国の言葉に置き換えればいい（絵にしても、`⌗` が「注記」だと
            // 分かる人はどの国にもいない）。絵も外したのは、名前が答えを
            // 言っているところに絵を添えても、目が二度読むだけだから。
            b.textContent = name;
            // **例外は一つだけ**（依頼 419・本人が決めた・2026-09-09）──
            // 絵文字の釦。「絵文字」と書くより 😀 のほうが速く、しかも
            // **訳が要らない**: 押すと出てくるものの見本が絵そのもので、
            // ここだけは絵が名前より多くを言っている。
            if (icon) b.textContent = icon;
            b.title = key ? `${name}（${keyText(key)}）` : name;
            // 押した瞬間に焦点を奪わない ── 奪うと、どこに入れるかを
            // 決める手がかり（選んだところ）が先に消える。
            b.onmousedown = (e) => e.preventDefault();
            b.onclick = () => {
                const found = MARKS.flat().find((m) => m[0] === name);
                if (found) found[2]();
            };
            scroll.append(b);
        }
        if (n === 0) {
            // **記号のすぐ右に置く。** 前は端に寄せていて、押した指から
            // いちばん遠かった（本人の指摘・2026-09-08）── 記号の列の続きに
            // 見えるほうが、「もっとある」も伝わる。
            //
            // **vim の釦は置かない。** 使うのはたいてい一人で、毎日見る
            // 帯に居座る値打ちは無い ── ⚙ の中にある。
            const sep = document.createElement('div');
            sep.className = 'sep';
            const more = document.createElement('button');
            // **字で書く。** 前は「…」で、押すまで何が出るか分からなかった
            // ── 「ほかの記号」なら、押す前に分かる。
            more.textContent = moreMarks ? 'たたむ' : 'ほかの記号';
            more.title = more.textContent;
            more.setAttribute('aria-expanded', String(moreMarks));
            more.classList.toggle('on', moreMarks);
            more.onclick = () => {
                moreMarks = !moreMarks;
                drawMarks();
                window.amber.remember({ moreMarks });
                if (editor) editor.layout();
            };
            scroll.append(sep, more);
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
        say('置けません: ' + why(e));
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
/* ── 絵文字 ── */

/// 表は core が持っている（依頼 418）。**一度だけ取りに行く。**
let faces = null;
/// 最近つかったもの。**憶えるのは字だけ** ── 名前は表から引ける。
let usedFaces = [];
/// いま見ている束（`0` は「最近つかったもの」）。
let faceTab = 0;

/// 絵文字の板を出す。
///
/// **押して入れるだけにする。** `:tada:` のような書き方は入れない
/// （本人：「僕でも :tada とか打たない」・2026-09-09）── 覚える記法が
/// 増えるだけで、ノートに残るのは同じ一文字。
///
/// **ノートに残るのは絵文字そのもの。** `:tada:` を書いて表示のときだけ
/// 絵にする道は採らない ── amber の外では意味の分からない字が残るし、
/// 「表示のまま書ける」との相性が悪い（絵から名前は一意に決まらない）。
async function openEmoji() {
    if (!faces) {
        try {
            faces = await ask('emoji', {});
        } catch (e) {
            say('絵文字が出せません: ' + why(e));
            return;
        }
        if (!usedFaces.length) usedFaces = [...faces.first];
    }
    const box = el('emoji');
    const find = el('emojifind');
    find.value = '';
    faceTab = 0;
    drawFaceTabs();
    drawFaces();
    box.hidden = false;
    // **帯の上に出す。** 板は下から生えるので、帯の真上に置かないと
    // 押した釦と出てきたものが繋がって見えない。
    const from = [...el('marks').querySelectorAll('button')]
        .find((b) => b.title.startsWith('絵文字'));
    const r = (from || el('marks')).getBoundingClientRect();
    const w = box.offsetWidth;
    const h = box.offsetHeight;
    box.style.left = Math.max(8, Math.min(r.left - w / 2 + r.width / 2, innerWidth - w - 8)) + 'px';
    box.style.top = Math.max(8, r.top - h - 8) + 'px';
    find.focus();
    setTimeout(() => document.addEventListener('mousedown', closeEmojiOnce), 0);
}

function closeEmoji() {
    el('emoji').hidden = true;
    document.removeEventListener('mousedown', closeEmojiOnce);
}
/// 外を押したら閉じる。**板の中と、帯の絵文字の釦は「外」ではない** ──
/// 釦を押して閉じてしまうと、開け閉めが一打ぶんずれる。
function closeEmojiOnce(e) {
    if (el('emoji').contains(e.target)) return;
    if (e.target.closest && e.target.closest('#marks button')?.title.startsWith('絵文字')) return;
    closeEmoji();
}

el('emojifind').oninput = () => drawFaces();
el('emojifind').onkeydown = (e) => {
    e.stopPropagation();
    if (e.code === 'Escape') { e.preventDefault(); closeEmoji(); return; }
    // **打って Enter で、いちばん上のものが入る。** 探せた人にもう一手
    // （目で探して押す）を足させない。
    if (isEnter(e) && !e.isComposing && e.keyCode !== 229) {
        e.preventDefault();
        const first = shownFaces()[0];
        if (first) putFace(first.ch);
    }
};

function drawFaceTabs() {
    const tabs = el('emojitabs');
    // 先頭は「最近つかったもの」── 二度目からは、ここだけで済む人が多い。
    const all = [{ icon: '🕘', name: '最近つかったもの' },
        ...faces.groups.map((g) => ({ icon: g.icon, name: g.name }))];
    tabs.innerHTML = all.map((g, n) =>
        '<button data-n="' + n + '"' + (n === faceTab ? ' class="on"' : '')
        + ' title="' + escapeAttr(g.name) + '">' + g.icon + '</button>').join('');
    for (const b of tabs.querySelectorAll('button')) {
        b.onmousedown = (e) => e.preventDefault();
        b.onclick = () => {
            faceTab = Number(b.dataset.n);
            el('emojifind').value = '';
            drawFaceTabs();
            drawFaces();
        };
    }
}

/// いま出す顔ぶれ。**探しているときは束を無視する** ── 打った人が
/// 探しているのは字であって、どの束に居るかではない。
function shownFaces() {
    const q = el('emojifind').value.trim().toLowerCase();
    const all = faces.groups.flatMap((g) => g.faces);
    if (q) return all.filter((x) => x.ch === q || x.words.toLowerCase().includes(q));
    if (faceTab === 0) {
        return usedFaces.map((ch) => all.find((x) => x.ch === ch) || { ch, words: '' });
    }
    return faces.groups[faceTab - 1].faces;
}

function drawFaces() {
    const grid = el('emojigrid');
    const rows = shownFaces();
    if (!rows.length) {
        grid.innerHTML = '<div class="none">見つかりません</div>';
        el('emojiname').textContent = '';
        return;
    }
    grid.innerHTML = rows.map((x, n) =>
        '<button data-n="' + n + '" title="' + escapeAttr(firstWord(x.words)) + '">'
        + escapeHtml(x.ch) + '</button>').join('');
    el('emojiname').textContent = firstWord(rows[0].words);
    for (const b of grid.querySelectorAll('button')) {
        const x = rows[Number(b.dataset.n)];
        // 押し下げで焦点を奪わない ── 奪うと、入れる先（caret）が消える。
        b.onmousedown = (e) => e.preventDefault();
        b.onmouseenter = () => { el('emojiname').textContent = firstWord(x.words); };
        b.onclick = () => putFace(x.ch);
    }
}

/// 探し言葉の最初の一語 ── 板の下に出す名前。
const firstWord = (words) => String(words || '').split(' ')[0] || '';

/// 選ばれた絵文字を、いま打っているところへ。
///
/// **板は閉じない。** 顔文字は続けて置くもので（「👍✨」）、一つ入れる
/// たびに開き直させない。閉じるのは Esc か、外を押したとき。
function putFace(ch) {
    if (onRead()) {
        el('read').focus();
        document.execCommand('insertText', false, ch);
    } else {
        put(ch);
    }
    // 最近つかったものへ。**同じものは前へ出す**（二つに増やさない）。
    usedFaces = [ch, ...usedFaces.filter((x) => x !== ch)].slice(0, 24);
    window.amber.remember({ faces: usedFaces });
    if (faceTab === 0 && !el('emojifind').value.trim()) drawFaces();
}

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
        // **どちらの面でも打つ。** `put` は「表示」の面では黙って帰るので、
        // ここだけ面を見ずに呼んでいると、**絵はフォルダに置かれたのに
        // リンクがどこにも入らない** ── 誰も指していない絵が
        // `attachments/` に溜まる（ほかの記号は道具帯の側で面を見ている）。
        if (onRead()) await readPut(`![](${r.link})`);
        else put(`![](${r.link})\n`);
        zonesSoon();
    } catch (e) {
        say('絵を置けません: ' + why(e));
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
    // 幅が変わったので測り直す。**畳みが効いた後で**（今のフレームでは
    // まだ古い幅しか見えない）。
    if (editor && view !== 'read') setTimeout(() => editor.layout(), 0);
    // **組み直しの終わりを返す。** 面を替えたあとに「さっき居た場所」へ
    // 立つには、読む面が組み直って触れるようになってからでないと、
    // 置いた選び目が古い DOM のものになる（実際に、立てずに落ちていた）。
    const drawn = open && view !== 'write' ? drawRead() : Promise.resolve();
    if (open && tocOn) drawToc();
    return drawn;
}

/// いま見ている（打っている）のは、ファイルの何行目か（`headLines` は下にある）。
///
/// 見ているものが同じなら、面を替えても同じ場所に居てほしい ── 替えた
/// 先で毎回いちばん上に飛ばされると、**替えるたびに探し直す**ことになる。
function whereAmI() {
    if (view === 'write' && editor) {
        return editor.getPosition().lineNumber - 1 + headLines();
    }
    // 読む面 ── caret のあるかたまり。無ければ、いま上に見えているもの。
    const rd = el('read');
    let n = getSelection()?.anchorNode;
    if (n && n.nodeType === 3) n = n.parentElement;
    let at = n && rd.contains(n) ? n.closest('#read > *') : null;
    if (!at) {
        const top = rd.getBoundingClientRect().top;
        for (const b of rd.children) {
            if (b.getBoundingClientRect().bottom > top + 4) { at = b; break; }
        }
    }
    const line = at && Number(at.dataset.line);
    return Number.isNaN(line) || line === null || at === null ? -1 : line;
}

/// その行のところへ、替えた先で立つ。
///
/// **図の中には立たせない。** 図は触れないかたまりで、caret を置くと
/// 押せるものが打てるものに見える ── **図の一つ上**に置く（本人の言葉で
/// 「マーメイドのちょっと上にカーソルがあればいい」）。
function goToLine(line) {
    if (line < 0) return;
    if (view !== 'read' && editor) {
        const at = Math.max(line - headLines(), 0) + 1;
        const last = editor.getModel().getLineCount();
        const n = Math.min(at, last);
        editor.revealLineNearTop(n);
        editor.setPosition({ lineNumber: n, column: 1 });
        return;
    }
    const rd = el('read');
    let hit = null;
    for (const b of rd.children) {
        const from = Number(b.dataset.line);
        if (Number.isNaN(from)) continue;
        const span = Number(b.dataset.span) || 1;
        if (from <= line && line < from + span) { hit = b; break; }
        if (from > line) break;
        hit = b;
    }
    if (!hit) return;
    hit.scrollIntoView({ block: 'center' });
    // 触れないかたまり（図・枠・絵）なら、その一つ上の打てるところへ。
    let land = hit;
    while (land && richBlock(land)) land = land.previousElementSibling;
    // **焦点を渡してから置く。** 置くだけだと、面に焦点が無いあいだの
    // 選び目は誰のものでもなく、そのまま打っても入らない
    // （`setView` は「表示」に移るとき焦点を落とすので、そのあとに要る）。
    if (land && rd.isContentEditable) {
        rd.focus();
        landAt(land, null);
    }
}

async function setView(v) {
    // **替える前に、どこに居たかを控える。** 替えたあとでは、もう
    // 前の面の caret も巻き位置も残っていない。
    const at = whereAmI();
    view = v;
    const drawn = applyView();
    window.amber.remember({ view: v });
    if (v === 'read') document.activeElement?.blur();
    else if (editor) editor.focus();
    await drawn;
    goToLine(at);
}

/// 上の帯の三つ。**押せる形と、キーと、同じ一本の道を通す。**
for (const b of document.querySelectorAll('#views button[data-view]')) {
    b.onclick = () => setView(b.dataset.view);
}

el('gear').onclick = (e) => {
    if (el('more').hidden) openMenu(e.currentTarget.getBoundingClientRect(), 'app');
    else closeMenu();
};
// 目次の開け閉め。**押せる場所は一つでいい** ── 献立の「目次」と同じ
// ものを呼ぶ（`toggleToc`）。一度この釦を外して献立だけにしたら、
// 見つからず「消えた」と言われた（依頼 297 の幅の都合だったが、
// 幅は `#meta` を固定してから空いている）。
el('tocbtn').onclick = () => toggleToc();

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
    // **変換中は組み直さない。** 組み直すと未確定の字が消える ── 用事は
    // 預かって、確定してから通す（向こうから来た字の混ぜ込み・テーマ替え・
    // 升の押し下げ）。混ぜ込みは待てる（ファイルは既に向こうの字で、
    // こちらが書き戻すときに混ざる）。
    if (composing) { drawAfter = true; return; }
    const seq = ++readSeq;
    // 組みはじめたときのノート。帰ってきたときに別のノートが開いていたら
    // 捨てる ── 「コード」の面へ替えてから別のノートを開くと組み直しが
    // 走らないので、遅れて着いた前のノートの字に今のノートの札が付く。
    const of = state.open.path;
    let html;
    try {
        html = (await ask('html', { text: whole() })).html || '';
    } catch (e) {
        say('組めません: ' + why(e));
        return;
    }
    // 追い越されていたら捨てる。速く打つと、古い答えが後から着く。
    if (seq !== readSeq) return;
    if (!state.open || state.open.path !== of) return;
    // 空のノートでも打ちはじめられるように、空の段落を一つ置く ──
    // `contenteditable` は中身が無いと caret を置く先が無い。
    el('read').innerHTML = html.trim() ? html : '<p data-line="' + headLines() + '" data-span="1"><br></p>';
    readDrawn(of);
    // **末尾には、いつも降りられる一行を置く。**
    //
    // 表や水平線でノートが終わっていると、その下に caret を置く手が
    // 無い（表の外側は表の一部ではないので、矢印でも出られない）。
    // 空のままなら字に戻すときに落ちるので、増えも減りもしない。
    tailStop();
    // 先頭が図や枠なら、その上にも降りられる一行を（`tailStop` の対）。
    headStop(el('read'));
    // **札を配るのが先。** 絵や図はこのあと札を掛け替える（`<pre>` →
    // `<div class="mermaid">`、`<img>` → `<figure>`）ので、掛け替える前に
    // 元の字を持たせておかないと、引き継ぐものが無い ── 図を入れたノートで
    // 保存が黙って止まった。
    armRead();
    findPictures();
    // **枠の色付けが先、図はそのあと。** 図の読み込みは `define` を伏せる
    // 一瞬を持っていて、Monaco はそのとき言語を後から読みに行くことがある
    // （`rust.js` は `define` を呼ぶ）── 重なると、枠のある図つきノートで
    // どちらかが落ちる。並べて速くなる場面でもないので、順に行う。
    paintCode().then(drawDiagrams);
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

/// mermaid が測るために建てた仮の箱を、片付ける。
///
/// `render(id, …)` は `#<id>` と `#d<id>` を `document.body` に建てて
/// 測る。うまくいけば自分で片付けるが、**転ぶと置いていく** ── 積もると
/// 画面の上に並び、幅を持つのでノートが潰れる。**呼んだほうが片付ける。**
function sweepMermaid(id) {
    for (const at of [id, 'd' + id]) {
        const n = document.getElementById(at);
        // **`document.body` の直下だけ消す。** mermaid は返す SVG にも
        // 渡した id をそのまま付けるので、id だけで消すと**いま面に挿した
        // 図そのもの**が消える（実際に消えて、図が一枚も出なくなった）。
        // 片付けたいのは、測るために建てられた仮の箱だけ。
        if (n && n.parentElement === document.body) n.remove();
    }
    // 前に置いていかれたものも、ついでに。
    for (const x of document.body.querySelectorAll(':scope > [id^="dmmd"], :scope > [id^="dstudio"]')) {
        x.remove();
    }
}

/// **図のあるノートを開くまで、読み込まない。** 3.4MB あって、ほとんどの
/// ノートには図が無い ── 起動のたびに払う値段ではない。
let Mermaid = null;
let mermaidSeq = 0;

/// 図に使う色。**円グラフも年表もマインドマップも、この十一色**。図ごとに
/// 別の並びを持つと、同じノートの中で色の意味が変わる。
///
/// 前は `PALETTE`（フォルダの色）をそのまま流用していたが、**大きく塗った
/// ときに芋臭かった** ── どれも明るさも鮮やかさも同じくらいで、並べると
/// 平らな帯になる（九〇年代の業務資料の色）。7px の点と、円の三割を占める
/// 面は、同じ色でうまくいくものではない。
///
/// 明るさを上げ、鮮やかさを落とした**淡彩**にする。濃い色を並べると、
/// 円の面積の大半が暗い塊になって図全体が沈む ── ノートは白い紙で、その上に
/// 置く図が紙より重くなる理由が無い。
///
/// 淡いぶん**字は白ではなく濃い墨**を載せる（`pieSectionTextColor`）。
/// 明るい地に白は乗らない。
const FAMILY = [
    '#F7BD5C', '#8FC8E8', '#A8D9A8', '#C9AEE0', '#F7A99C', '#8ED9CE',
    '#EFDA8A', '#AEBBEE', '#F4B4CE', '#C6DE8E', '#D3D3D9',
];

/// 淡彩の上に置く字の色。
const FAMILY_INK = '#3a2408';

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
        // **書き損じの絵を、mermaid に描かせない。**
        //
        // 既定では、字が通らないと mermaid は**自分で赤い絵を描いて
        // 置いていく** ── その置き場所は `document.body` で、こちらの
        // `catch` は絵を消せない。打つたびに描き直すので、一文字ごとに
        // 一枚ずつ積み上がり、画面の上に「Syntax error in text」が
        // 並んだ。積まれた絵は幅を持つので**ノートが左へ潰れる**。
        // 戻す（⌘Z）は字を戻すだけで、置いていかれた絵には届かない ──
        // だから閉じて開くまで直らなかった。
        suppressErrorRendering: true,
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
            // 年表は `cScale` を見る。渡さないと**灰色の帯が並ぶだけ**に
            // なって、「いつ何があったか」が全部同じ重さに見える。
            // 字の色は塗りに載るので、濃い色には白、明るい色には濃い茶。
            ...Object.fromEntries(FAMILY.flatMap((c, n) => [
                ['cScale' + n, c],
                ['cScaleLabel' + n, light(c) ? '#2a2011' : '#ffffff'],
                ['cScaleInv' + n, c],
            ])),
            pieStrokeColor: v('--paper', '#fffdf8'),
            pieOuterStrokeColor: v('--line', '#e4d9c4'),
            pieTitleTextColor: v('--ink', '#2a2011'),
            pieSectionTextColor: FAMILY_INK,
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
            // 年表の札の字。**縮むぶんを見越して大きめに** ── 出来事が
            // 増えるほど図は横に伸び、伸びたぶんだけ全体が縮んで字も縮む。
            + '.timeline text,.timeline tspan{font-size:15px}'
            + '.timeline .sectionTitle,.timeline .sectionTitle tspan{font-weight:700}'
            // マインドマップだけは `themeVariables` を見ない ── 灰と藤色で
            // 描かれて、琥珀のノートの上で**そこだけ別のアプリ**に見える。
            // 枝は円グラフと同じ十一色にして、まん中は琥珀そのものに。
            // **まん中は、ただの丸い橙ではない。** 濃い琥珀で塗って、
            // 一段明るい琥珀の輪を掛け、影を一枚敷く ── 枝より手前にある
            // ものとして見えないと、ここが中心だと形が言っていない。
            + '.mindmap-node.section--1 circle.basic{fill:#C97F16;stroke:'
                + v('--amber', '#f0a52b') + ';stroke-width:3px;'
                + 'filter:drop-shadow(0 2px 5px rgba(0,0,0,.28))}'
            + '.mindmap-node.section--1 .nodeLabel{color:#fff;font-weight:700;font-size:15px}'
            + FAMILY.map((c, n) =>
                // 淡彩になったので、枝の地は 14% では紙と見分けが付かない。
                // 塗りを濃くし、枠は一段沈めて形が立つようにする。
                '.mindmap-node.section-' + n + ' .node-bkg{fill:color-mix(in srgb,' + c
                    + ' 46%,' + v('--paper', '#fffdf8') + ');stroke:color-mix(in srgb,'
                    + c + ' 78%,#6b5a41)}'
                // **枝の字の色を、こちらで決める。** 渡さないと mermaid が
                // 塗りの色から作り、明るい琥珀の枝では薄い字が薄い地に
                // 載って読めなかった（「仕事」が消えていた）。地はどの枝も
                // 14% の淡い色なので、字はノートの地の色でいい。
                + '.mindmap-node.section-' + n + ' .nodeLabel{color:'
                    + v('--ink', '#2a2011') + ';font-weight:600}'
                + '.mindmap-node.section-' + n + ' line{stroke:color-mix(in srgb,'
                    + c + ' 78%,#6b5a41);stroke-width:2px}'
                // 線は淡彩のままだと紙に消える ── 一段沈めた色で引く。
                + '.edge.section-edge-' + n + '{stroke:color-mix(in srgb,'
                    + c + ' 72%,#6b5a41);stroke-width:2.5px}').join(''),
        flowchart: { curve: 'basis', padding: 14, nodeSpacing: 44, rankSpacing: 46, htmlLabels: true },
        pie: { textPosition: 0.62, useMaxWidth: true },
        sequence: { actorMargin: 44, mirrorActors: false },
        // 年表は、既定だと札が小さくて中の字が読めない ── 「いつ何があった
        // か」を見る図なのに、その「なに」が潰れている。札を広げて字に余白を。
        timeline: {
            useMaxWidth: true, width: 168, height: 66,
            padding: 10, boxMargin: 12, boxTextMargin: 8,
            diagramMarginX: 22, diagramMarginY: 18, leftMargin: 70,
        },
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
        { name: '四象限', sub: '影響と期限で、やることの順を決める', value: 'quad' },
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
        const q = await ask3('分かれ道の問い', '', '左利きですか？');
        if (q === null) return;
        const yes = await ask3('「はい」のとき', '', '左利きのハサミを使う');
        if (yes === null) return;
        const no = await ask3('「いいえ」のとき', '', '右利きのハサミを使う');
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
        const v = await ask3('置くもの', '「なに: 影響, 期限の近さ」を 0〜1 で。読点で区切ってください',
            '週報: 0.8, 0.9、片付け: 0.3, 0.2、勉強: 0.9, 0.2');
        if (v === null) return;
        const rows = v.split(/\s*[、]\s*/).filter(Boolean).map((x) => {
            const m = /^(.*?)\s*[:：]\s*([\d.]+)\s*,\s*([\d.]+)/.exec(x.trim());
            return m ? '  "' + m[1] + '": [' + m[3] + ', ' + m[2] + ']' : null;
        }).filter(Boolean);
        md = '```mermaid\nquadrantChart\n  title やることの優先順\n'
            + '  x-axis 期限遠 --> 期限近\n  y-axis 影響小 --> 影響大\n'
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
        //
        // **旗（`define.amd`）を下ろすだけでは足りない。** 中の部品は
        // `typeof define === 'function'` しか見ておらず、旗が無くても
        // 名乗り出る（「無名の define は一つまで」で落ちた）。
        //
        // 伏せているあいだ Monaco が言語を読みに行くと、今度はあちらが
        // `define is not a function` で落ちる ── 図と枠の両方があるノートで
        // 実際に落ちた。**だから枠の色付けを先に終わらせてから呼ぶ**
        // （`drawRead` を見よ）。伏せるのは初回の一度きり。
        const keep = window.define;
        window.define = undefined;
        const back = () => { window.define = keep; };
        const tag = document.createElement('script');
        tag.src = 'vendor/mermaid/mermaid.min.js';
        tag.onload = () => {
            back();
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
            back();
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
        say('図を読めません: ' + (e && why(e) ? why(e) : e));
        return;
    }
    if (seq !== readSeq) return;
    for (const code of blocks) {
        const src = code.textContent;
        const id = 'mmd' + (++mermaidSeq);
        try {
            const { svg } = await lib.render(id, src);
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
            code.parentElement.title = '図にできません: ' + (e && why(e) ? why(e) : e);
        } finally {
            sweepMermaid(id);
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
        // 凝った形（`枝[四角]`）は表にできない ── 平らに直すと形が変わる。
        if (kids.some((l) => /[[({]/.test(l.trim()))) return null;
        const deep = (l) => /^\s*/.exec(l)[0].length;
        // **字下げの段を、深さの番号に直す。** 空白が二つでも四つでも
        // 「一段下」は一段下なので、出てきた字下げを浅い順に並べて
        // 何番目かを取る ── 人が書いた図の空白の数を当てにしない。
        const steps = [...new Set(kids.map(deep))].sort((x, y) => x - y);
        const rows = kids.map((l) => ({ a: l.trim(), at: steps.indexOf(deep(l)) }));
        // 一段飛ばし（親の無い孫）は、そのまま持つと書き戻したときに
        // 形が変わる ── 詰めて、親のある形にしておく。
        let top = 0;
        for (const r of rows) {
            r.at = Math.min(r.at, top);
            top = r.at + 1;
        }
        return {
            kind, first: 'mindmap', head: [], edges: [],
            title: (/root\(\((.*)\)\)/.exec(lines[rootAt]) || ['', ''])[1],
            rows,
        };
    }

    const first = live[0].trim();
    const head = [];
    const rows = [];
    const edges = [];
    /// 箱ごとの色（流れ図だけ）。`{ 合言葉: '#RRGGBB' }`
    const paint = {};
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
            // 箱の色。**こちらが書いた形だけ読む** ── 手で書いた凝った
            // `style`（線の太さや破線）を色だけの表に押し込むと、書き戻した
            // ときに残りが消える。読めない形なら表にせず、字の面で直す。
            const c = NODE_STYLE.exec(l);
            if (c) { paint[c[1]] = c[3]; continue; }
        }
        return null;    // 読めない行が一つでもあれば、表にしない
    }
    if (!rows.length) return null;
    // 色は箱に付けて持つ ── 表の行と色の行が別々にあると、箱を消したときに
    // 色の行だけが残る（消したはずの箱が図に戻る、と同じ形の間違い）。
    for (const r of rows) if (paint[r.id]) r.color = paint[r.id];
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
        // 深さ一段につき空白二つ。まん中が二つなので、一段目は四つ。
        return 'mindmap\n  root((' + (plain(d.title) || 'まん中') + '))\n'
            + live.map((r) => ' '.repeat(4 + (r.at || 0) * 2) + plain(r.a)).join('\n');
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
        // 色は最後にまとめて ── 箱と線を読んでから見るほうが、字のままでも
        // 図の形が先に読める。
        ...live.filter((r) => r.color).map((r) => nodeStyle(r.id, r.color)),
    ]);
}

/* ── 箱の色 ── */

/// 図の箱の色。**名前は amber のどこでも同じ十一色**（フォルダと同じ）──
/// 覚える色の名前を、画面ごとに増やさない。
///
/// 紙の上では**淡く塗って、濃い線で囲む**。濃いまま塗ると箱の中の字が沈み、
/// 読ませるには白い字に替えることになる ── そうすると「どの色なら白か」を
/// 人が考える羽目になる。淡い地なら、字はいつも同じ濃い墨でいい。
const NODE_INK = '#3a2408';

/// こちらが書く `style` の形。**この形だけ読み戻す**（`mmdParse`）。
const NODE_STYLE =
    /^style\s+([A-Za-z_]\w*)\s+fill:(#[0-9A-Fa-f]{6}),stroke:(#[0-9A-Fa-f]{6}),color:(#[0-9A-Fa-f]{6})$/;

/// 白と混ぜて淡くする。`keep` はもとの色の割合。
function soften(hex, keep) {
    const n = parseInt(String(hex).slice(1), 16);
    if (!Number.isFinite(n)) return hex;
    const mix = (v) => Math.round(v * keep + 255 * (1 - keep));
    return '#' + [(n >> 16) & 255, (n >> 8) & 255, n & 255]
        .map((v) => mix(v).toString(16).padStart(2, '0')).join('').toUpperCase();
}

/// 一つの箱の `style` の行。
function nodeStyle(id, hex) {
    return '  style ' + id + ' fill:' + soften(hex, 0.22)
        + ',stroke:' + String(hex).toUpperCase() + ',color:' + NODE_INK;
}

/// 0〜1 に収める。**打ち間違いで図が壊れないように** ── 四象限は枠の外に
/// 置かれると、点が消えたようにしか見えない。
function num(v) {
    const n = Number(String(v ?? '').trim());
    return Number.isFinite(n) ? Math.min(Math.max(n, 0), 1) : 0.5;
}

/// この色の上に濃い字を置けるか。**塗りの明るさで決める** ── 白い字を
/// 明るい黄の上に置くと読めず、濃い字を深い青の上に置いても読めない。
/// 目が明るさを感じる重みは色ごとに違うので、そのまま重みを掛ける。
function light(hex) {
    const n = parseInt(hex.slice(1), 16);
    const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    return (r * 0.299 + g * 0.587 + b * 0.114) > 150;
}

/// 種類ごとの、表の形。**列の名前がそのまま説明になる** ── 「なに」「いつ」
/// と書いてあれば、何を打てばいいかを別に書かなくていい。
const DIAGRAM_FORM = {
    // 流れ図だけは表が二つ（箱と線）なので、窓は `studioFlow` が別に描く。
    // **`cols` はそれでも書いておく** ── 電話はここを読んで欄を作るので、
    // 無いと「形は選べるのに名前を書く欄が無い」画面ができる（実際にできた）。
    flow: {
        name: '流れ図', add: '箱を足す',
        cols: [{ k: 'a', label: '箱の中の言葉', w: 3, ph: '書く' }],
    },
    pie: {
        name: '円グラフ', title: '題', add: '割合を足す',
        cols: [{ k: 'a', label: '名前', w: 3, ph: '仕事' },
               { k: 'b', label: '数', w: 1, ph: '5' }],
    },
    quad: {
        name: '四象限', title: '題', add: 'やることを足す',
        cols: [{ k: 'a', label: 'やること', w: 3, ph: '週報' },
               { k: 'b', label: '影響', w: 1, ph: '0.8', slide: true },
               { k: 'c', label: '期限の近さ', w: 1, ph: '0.9', slide: true }],
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
        name: 'マインドマップ', title: 'まん中', add: '枝を足す', deep: true,
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
        Object.fromEntries(spec.cols.map((c) => [c.k, c.check ? false : ''])), spec.deep));
    if (spec.deep) {
        form.append(tag('div', 'note',
            '「→」で一段深く、「←」で一段浅く。枝の下に枝を、'
            + 'そのまた下にも書けます（mermaid の枝と枝のあいだには字を置けないので、'
            + '間に入れたい言葉は一段の枝として足してください）。'));
    }
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
function studioRows(cols, rows, addName, blank, deep) {
    const box = tag('div', 'rows');
    const head = tag('div', 'row hd');
    for (const c of cols) {
        const h = tag('span', '', c.label);
        h.style.flex = c.w ? c.w + ' 1 0' : '0 0 auto';
        head.append(h);
    }
    head.append(tag('span', 'sp' + (deep ? ' wide' : '')));
    box.append(head);

    // **親のいない孫を作らせない。** 一段目の次にいきなり三段目を置くと
    // mermaid はその枝を捨てる ── 画面では足したのに図に出ない、という
    // いちばん分かりにくい壊れ方になる。深くできるのは「一つ上の行より
    // 一段だけ」まで。
    const roof = (n) => (n === 0 ? 0 : (rows[n - 1].at || 0) + 1);

    rows.forEach((r, n) => {
        const line = tag('div', 'row');
        if (deep) {
            // 深さは、字下げそのもので見せる ── 数字で「2」と書くより、
            // ずれている形のほうが枝に見える。
            const pad = tag('span', 'deep');
            pad.style.flex = '0 0 ' + ((r.at || 0) * 17) + 'px';
            if (r.at) pad.textContent = '└';
            line.append(pad);
        }
        for (const c of cols) {
            const cell = studioCell(c, r);
            cell.style.flex = c.w ? c.w + ' 1 0' : '0 0 auto';
            line.append(cell);
        }
        const move = (to) => {
            if (to < 0 || to >= rows.length) return;
            rows.splice(to, 0, rows.splice(n, 1)[0]);
            if (deep) settle(rows);
            studioDraw();
            studioShow();
        };
        if (deep) {
            const shift = (by) => {
                r.at = Math.min(Math.max((r.at || 0) + by, 0), roof(n));
                settle(rows);
                studioDraw();
                studioShow();
            };
            const out = studioBtn('←', '一段浅く（親の隣へ）', () => shift(-1));
            const into = studioBtn('→', '一段深く（上の枝の下へ）', () => shift(1));
            out.disabled = (r.at || 0) === 0;
            into.disabled = (r.at || 0) >= roof(n);
            line.append(out, into);
        }
        line.append(studioBtn('↑', '一つ上へ', () => move(n - 1)));
        line.append(studioBtn('↓', '一つ下へ', () => move(n + 1)));
        line.append(studioBtn('✕', 'この行を消す', () => {
            rows.splice(n, 1);
            if (deep) settle(rows);
            studioDraw();
            studioShow();
        }));
        box.append(line);
    });

    const add = tag('button', 'add', '＋ ' + addName);
    add.onclick = () => {
        const fresh = blank();
        // **足した枝は、直前の枝と同じ深さに。** 一段目に戻すと、枝の下に
        // 続きを書いている途中で毎回まん中まで戻される。
        if (deep && rows.length) fresh.at = rows[rows.length - 1].at || 0;
        rows.push(fresh);
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

/// 段の飛びを詰める ── 動かしたり消したりしたあと、親のいない孫が残る。
function settle(rows) {
    let roof = 0;
    for (const r of rows) {
        r.at = Math.min(Math.max(r.at || 0, 0), roof);
        roof = r.at + 1;
    }
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
    const ch = tag('span', '', '色');
    ch.style.flex = '0 0 104px';
    head.append(sh, ch, tag('span', 'sp'));
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
        // **色は、形の隣。** 箱ごとに決められないと、流れ図は「どれが
        // 通る道でどれが行き止まりか」を形だけで言うことになる。名前は
        // フォルダと同じ十一色 ── 覚える色の名前を画面ごとに増やさない。
        const paint = studioPick('', [['', '色なし'], ...PALETTE.map(([h, name]) => [h, name])],
                                 r.color || '', (v) => {
                                     r.color = v || undefined;
                                     studioPaint(paint, v);
                                     studioShow();
                                 });
        paint.classList.add('bare', 'paint');
        paint.style.flex = '0 0 104px';
        studioPaint(paint, r.color || '');
        line.append(paint);
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
        label.placeholder = '（なし）';
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

/// 色の選び口そのものを、選んだ色で塗る。**名前だけでは色が分からない**
/// ── 「ベルガモット」がどれかを覚えている人はいない。
function studioPaint(pick, hex) {
    const s = pick.querySelector('select');
    if (!s) return;
    s.style.color = hex || '';
    s.style.fontWeight = hex ? '700' : '';
    pick.style.background = hex ? soften(hex, 0.18) : '';
    pick.style.borderRadius = hex ? '7px' : '';
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
            'この図は表にできない形（手で書いたか、ambər の知らない書き方）です。'
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
        err.textContent = '図を読めません: ' + (e && why(e) ? why(e) : e);
        return;
    }
    if (!studio) return;
    // **工房はいちばん積もる場所。** 打つたびに描き直すので、通らない字を
    // 打っているあいだ、一文字ごとに転ぶ ── 片付けないと、そのぶん溜まる。
    const id = 'studio' + (++mermaidSeq);
    try {
        const { svg } = await lib.render(id, src);
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
        err.textContent = 'いまの字では図になりません: ' + (e && why(e) ? why(e) : e);
    } finally {
        sweepMermaid(id);
    }
}

/* ── 工房の受け口 ── */

el('studio').addEventListener('keydown', (e) => {
    // 中で打った字を、外の近道に取られない（`b` で太字、`e` で面替え…）。
    e.stopPropagation();
    if (e.isComposing || e.keyCode === 229) return;
    if (e.code === 'Escape') { e.preventDefault(); studioClose(); return; }
    if (isEnter(e) && (e.metaKey || e.ctrlKey)) { e.preventDefault(); studioOk(); }
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


function findPictures() {
    const dir = state.open ? dirOf(state.open.path) : '';
    for (const img of el('read').querySelectorAll('img')) {
        const src = img.getAttribute('src') || '';
        if (src && !/^[a-z][a-z0-9+.-]*:/i.test(src) && !src.startsWith('//')) {
            showPicture(img, absPath(src, dir));
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
        // **押した瞬間に裏返す。** 字を待たない ── 100ms 後に変わるのは
        // 「効いたか分からない」の入口（`PAPER.ja.md` 六章の己）。升の
        // 状態は DOM が既に持っている（`aria-pressed`）ので、そこを先に。
        const line = Number(box.dataset.line);
        const done = box.textContent.trim() === '☐';
        box.textContent = done ? '☑' : '☐';
        box.setAttribute('aria-pressed', String(done));
        // **判断は core。** `check` は字を返すだけで、保存はいつもの
        // `save()` ── だから衝突の検査も同じものが効く。
        try {
            const r = await ask('check', { text: whole(), line, done });
            const cut = await ask('split', { text: r.text });
            loading = true;
            editor.setValue(cut.body || '');
            loading = false;
            state.head = cut.head || '';
            state.dirty = true;
            await save();
            // **組み直さない**（`PAPER.ja.md` 六章の己）── 見た目は既に
            // 裏返っていて、組み直す理由が無い。組み直せば caret が飛び、
            // 長いノートでは一瞬止まる。行番号は動いていない（升の一文字が
            // 変わっただけ）ので、札を持たせ直すだけでよい。
            armRead();
            drawCount();
            if (tocOn) drawToc();
        } catch (err) {
            // 効かなかった ── 見た目を戻す（裏返したままにしない）。
            box.textContent = done ? '☐' : '☑';
            box.setAttribute('aria-pressed', String(!done));
            say('直せません: ' + err.message);
        }
        return;
    }
    // **触れないものは、押すと吹き出し。** 図・枠・絵で同じ形に揃える
    // （`PAPER.ja.md` 六章の甲・本人が決めた）── 覚えることを一つにする。
    //
    // 前は図と枠で違うことが起きていた（図は工房、枠は並べて表示のその行へ）
    // ── どちらも「押した」だけなのに、行き先が違った。**「消す」を置くのは、
    // 図や枠を消すのに「コード」へ行かせないため**（表示のまま消せること）。
    const art = diagramAt(e.target);
    if (art) {
        e.preventDefault();
        popMenu([
            { name: '図を直す', sub: '工房が開きます', run: () => studioOpen(art) },
            { name: '消す', sep: true, run: () => dropBlock(art) },
        ], { x: e.clientX, y: e.clientY });
        return;
    }
    const a = e.target.closest('a');
    if (!a) {
        // **触れないかたまりは、書く面のその行へ送る。**
        // 打てるのに保存されない、を作らないための逃げ道。
        // **`richBlock()` と同じ顔ぶれにする。** ここだけ古いままだと、
        // 触れるようにしたはずの表を押した瞬間に書く面へ飛ぶ（実際に飛んだ）。
        const rich = e.target.closest('pre, figure, .mermaid');
        if (rich && el('read').contains(rich)) {
            const pic = rich.tagName === 'FIGURE';
            popMenu([
                // **絵の大きさは、押して選べる**（依頼 420）── 記法を
                // 覚えていない人が、いちばん変えたがるのがこれ。
                pic ? { name: '大きさ…', sub: sizeNow(rich), run: () => askSize(rich) } : null,
                pic ? null : { name: 'コードで直す', sub: '書く面のその行へ', run: () => toSource(rich) },
                { name: '消す', sep: true, run: () => dropBlock(rich) },
            ], { x: e.clientX, y: e.clientY });
        }
        return;
    }
    // **窓の中では開かせない。** 踏んだ先に窓ごと持っていかれると、
    // 題字を窓の中に描いている以上、戻る道が無い。
    e.preventDefault();
    const href = a.getAttribute('href') || '';
    // ノートの中の見出しへ飛ぶリンクは、そのまま飛ぶ ── 外へ出ないので、
    // 「開く／直す」を訊く意味が無い。
    if (href.startsWith('#')) {
        let id = href.slice(1);
        try { id = decodeURIComponent(id); } catch { /* そのまま使う */ }
        el('read').querySelector(`[id="${CSS.escape(id)}"]`)
            ?.scrollIntoView({ behavior: 'smooth', block: 'start' });
        return;
    }
    // **押すと吹き出し。** 打てる面で押した瞬間に外へ飛ぶと、字を直したい
    // 人に道が無い（右押しを知らない人が多い）── Notion・Google Docs の型。
    // 触れないかたまりと同じ「押すと吹き出し」に揃える。
    popMenu([
        { name: '開く', sub: href, run: () => openLink(href) },
        { name: '字を直す', sub: 'ここに caret を置きます', run: () => landAt(a, null) },
        { name: 'リンク先を写す', sep: true, run: () => copyText(href, 'リンク先') },
    ], { x: e.clientX, y: e.clientY });
});

/// 外の行き先を開く。
async function openLink(href) {
    if (!(await window.amber.openLink(href))) say('この行き先は開けません: ' + href);
}

/// **絵の大きさの選び肢。**
///
/// 数を訊かない ── 「200px」と打てる人は記法で書ける（`![w:200px]`）。
/// ここに来るのは打てない人なので、**言葉で選ばせる**。
/// 幅だけを指す: 縦横のどちらも訊くと、釣り合いを自分で守る仕事になる。
const SIZES = [
    { name: 'はばいっぱい', sub: '指示なし（もとの出かた）', px: null },
    { name: '大きめ', sub: '横 640px', px: '640px' },
    { name: '中くらい', sub: '横 400px', px: '400px' },
    { name: '小さめ', sub: '横 200px', px: '200px' },
];

/// いまの大きさを、献立の右に添える一言。
function sizeNow(fig) {
    const w = fig.querySelector('img')?.style.width || '';
    return SIZES.find((s) => s.px === w)?.name || (w ? '横 ' + w : 'はばいっぱい');
}

/// 絵の大きさを選んで、**ノートの字に書く**。
///
/// 押して選んだ結果が `![猫 w:200px](…)` という**打てる字**として残る ──
/// あとから記法で直せるし、amber の外でも読める（芯の 1）。
async function askSize(fig) {
    const now = fig.querySelector('img')?.style.width || '';
    const px = await askPick('絵の大きさ',
        SIZES.map((s) => ({ name: s.name, sub: s.sub, value: s.px === null ? '' : s.px })),
        SIZES.find((s) => s.px === now)
            ? 'いま ' + SIZES.find((s) => s.px === now).name
            : (now ? 'いま 横 ' + now : 'いま はばいっぱい'));
    if (px === null) return;
    await readSourceEdit(async (md) => {
        const r = await ask('imgsize', { line: md, width: px || null });
        return r.line;
    }, fig);
}

/// 触れないかたまりを、書く面のその行へ。
function toSource(rich) {
    const at = Number(rich.dataset.line);
    setView('split');
    if (!Number.isNaN(at) && editor) {
        const line = Math.max(at - headLines(), 0) + 1;
        editor.revealLineNearTop(line);
        editor.setPosition({ lineNumber: line, column: 1 });
        editor.focus();
    }
}

/// 触れないかたまりを消す。**訊かない** ── 一つ戻すで戻せるし、押した人が
/// 「消す」を選んでいる（選びは意思・`PAPER.ja.md` 六章の芯の 2）。
function dropBlock(rich) {
    if (!rich || !el('read').contains(rich)) return;
    rich.remove();
    // 何も残らないなら、打てる一行を置く ── 空の面には caret を置けない。
    if (!el('read').children.length) tailStop();
    readChanged();
}

// 右押しでも同じ扉。**押しても右押しでも開く** ── どちらだったかを
// 覚えている人はいないので、両方に置く。
//
// ただし**箱そのものを右押ししたときは、色の献立**（`paintNode`）。
// 図の余白を右押しすれば、これまでどおり工房が開く。
el('read').addEventListener('contextmenu', (e) => {
    const art = diagramAt(e.target);
    if (art) {
        e.preventDefault();
        if (paintNode(art, e)) return;
        studioOpen(art);
        return;
    }
    readMenu(e);
});

/* ── 右押し ── */

/// 字を写す。**転んでも黙らない** ── 写せたつもりで貼れないのが最悪。
async function copyText(text, what) {
    try {
        await navigator.clipboard.writeText(text);
        say(what + 'を写しました');
    } catch (e) {
        say('写せません: ' + why(e));
    }
}

/// 「表示」の面の右押し。**指しているものにすることだけを出す。**
///
/// 表の上なら表のこと、リンクの上ならリンクのこと、升の上なら升のこと。
/// どこでもない字の上なら、切り貼りと飾り ── 飾りの中身は道具帯
/// （`MARKS`）から引く。**二か所に書かない** ── 書くと、記号を一つ足した
/// 日に道具帯にだけ増えて、右押しには出てこない。
function readMenu(e) {
    if (view === 'write') return;
    e.preventDefault();
    const at = { x: e.clientX, y: e.clientY };
    const t = e.target;
    const sel = String(getSelection() || '');

    // 升の上 ── 済み／未済と、下に一つ。
    const box = t.closest('.box');
    if (box) { popMenu([
        { name: '済み／未済を入れ替える', run: () => box.click() },
        { name: 'この下に一つ足す', run: () => { landAfter(box); readMark('line', '- [ ] ', true); } },
    ], at); return; }

    // リンクの上。
    const a = t.closest('a[href]');
    if (a) { popMenu([
        { name: '開く', sub: a.getAttribute('href'), run: () => window.amber.openLink(a.href) },
        { name: 'リンク先を写す', run: () => copyText(a.getAttribute('href') || '', 'リンク先') },
        { name: '字だけ残す', run: () => { landAt(a); readDress('unlink'); } },
    ], at); return; }

    // 表の中 ── 道具帯と同じもの。**同じ命令を二度書かない。**
    const cell = t.closest('td, th');
    if (cell && el('read').contains(cell)) {
        landAt(cell);
        popMenu([
            { name: '行を足す', run: () => tableDo('row+') },
            { name: '行を消す', run: () => tableDo('row-') },
            { name: '列を足す', sep: true, run: () => tableDo('col+') },
            { name: '列を消す', run: () => tableDo('col-') },
            { name: '左に寄せる', sep: true, run: () => tableDo('align:left') },
            { name: '真ん中に寄せる', run: () => tableDo('align:center') },
            { name: '右に寄せる', run: () => tableDo('align:right') },
        ], at);
        return;
    }

    // どこでもない字の上 ── 切り貼りと飾り。
    popMenu([
        { name: '切り取り', key: '⌘X', dim: !sel, run: () => document.execCommand('cut') },
        { name: '写す', key: '⌘C', dim: !sel, run: () => document.execCommand('copy') },
        { name: '貼り付け', key: '⌘V', run: () => document.execCommand('paste') },
        ...MARKS.flat().filter(([n]) => n !== '|' && n !== '画像' && n !== 'フロー')
            .map(([name, key, run], i) => ({ name, key, sep: i === 0, run })),
    ], at);
}

/// 升のある行へ caret を置く（`landAt` は切り出しの側にある一本を使う ──
/// 同じことをする関数を二つ持たない）。
function landAfter(box) {
    const li = box.closest('li') || box.parentElement;
    if (li) landAt(li);
}

/// 押されたところの図。描けた図（`.mermaid`）と、描けなかった枠のどちらも。
/// 描かれた図の、箱そのものを右押しして色を変える。受けたら `true`。
///
/// **工房を開かずに、一つだけ直せる道。** 色を一つ変えるためだけに工房を
/// 開いて、表を見つけて、閉じるのは遠い ── フォルダの色を右押しで変える
/// のと同じ手ぶりにする（この窓で「色を変える」は右押し、と一つに決まる）。
///
/// mermaid は箱に `id="<図の番号>-flowchart-<合言葉>-<番号>"` を差すので、
/// そこから合言葉を取り戻す（**頭に図ごとの番号が付く** ── `^` で当てると
/// 一つも当たらない）。取れなければ `false` ── 箱でないところの右押しは、
/// これまでどおり工房。
function paintNode(art, e) {
    if (!art.classList.contains('mermaid')) return false;
    const g = e.target.closest('g.node');
    const id = g && /flowchart-(\w+)-\d+$/.exec(g.id || '');
    if (!id) return false;
    const src = fenceBody(art.dataset.md);
    const data = src === null ? null : mmdParse(src);
    if (!data || data.kind !== 'flow') return false;
    const row = data.rows.find((r) => r.id === id[1]);
    if (!row) return false;
    paintMenu({ x: e.clientX, y: e.clientY }, row.color, (hex) => {
        row.color = hex || undefined;
        readSourceEdit(() => '```mermaid\n' + mmdBuild(data).trim() + '\n```', art);
    });
    return true;
}

/// 色の献立。**名前と、その色そのものを並べる** ── 「ベルガモット」が
/// どれかを覚えている人はいない。
function paintMenu(at, now, set) {
    popMenu([['', '色なし'], ...PALETTE].map(([hex, name]) => ({
        html: '<span class="dot" style="background:'
            + (hex ? escapeAttr(soften(hex, 0.3)) : 'transparent')
            + ';border-color:' + (hex ? escapeAttr(hex) : 'var(--line)') + '"></span>'
            + escapeHtml(name),
        key: hex === (now || '') ? 'いま' : '',
        run: () => set(hex),
    })), at);
}

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
/* ── 取り消しと、やり直し ── */

/// **ノートの字そのものを積む。** Monaco の取り消しには任せられない ──
/// 表を入れる・図を入れる・升を押す、はどれも `editor.setValue()` で組み
/// 直しており、`setValue` は Monaco の積み木を**まるごと捨てる**。押した
/// 直後に ⌘Z を押しても、そこには何も積まれていない。
///
/// 積むのは**保存のたび**（打鍵の 0.9 秒後）。一続きに打っているあいだは
/// 一つにまとまり、手が止まるごとに一段になる ── 「さっきの姿」の単位が
/// 人の感覚と揃う。一文字ずつ積むと、消した一段落を戻すのに何十回押す
/// ことになる。
///
/// **ノートを替えたら捨てる。** 別のノートの姿をここへ戻す道があると、
/// 一度の押し間違いで二本まとめて壊れる。
/// 一世代の区切り。**最後の打鍵から五分空いたら、その前の姿を一つ。**
/// 保存のたびに残すと、十分書けば数十世代になり、五十世代が一回の執筆で
/// 埋まる ── 「昨日の夕方の姿」を訊いたときには、もう無い。
const KEEP_GAP = 300;

let backs = [];
let forwards = [];
let lastSaved = '';
let steppingBack = false;
const BACKS = 120;

/// いまの姿を積む。`save()` の中から、書き込む直前に呼ばれる。
function keepStep(now) {
    if (steppingBack || now === lastSaved) return;
    if (lastSaved !== '') {
        backs.push(lastSaved);
        if (backs.length > BACKS) backs.shift();
        // 新しく打ったら、先の道は消える ── 分かれた先を持っておくと
        // 「やり直し」が何を指すのか誰にも言えなくなる。
        forwards = [];
    }
    lastSaved = now;
    drawSteps();
}

function forgetSteps() {
    backs = [];
    forwards = [];
    lastSaved = '';
    drawSteps();
}

/// 一段もどす／すすめる。
/// 二つの字の、**初めて食い違う行**（0 起点）。同じなら `-1`。
///
/// 戻したあと、そこへ連れていくために要る ── どこが変わったのかは、
/// 変わった場所を見せる以外に言いようがない。
function firstDiff(a, b) {
    const x = String(a).split('\n');
    const y = String(b).split('\n');
    const n = Math.min(x.length, y.length);
    for (let i = 0; i < n; i++) if (x[i] !== y[i]) return i;
    return x.length === y.length ? -1 : n;
}

async function stepBack(forward) {
    const from = forward ? forwards : backs;
    const to = forward ? backs : forwards;
    if (!from.length || !editor) return;
    to.push(lastSaved);
    const text = from.pop();
    // **戻したら、戻った場所へ連れていく。**
    //
    // `editor.setValue` は Monaco の caret を 1 行目へ戻し、画面も先頭へ
    // 飛ばす ── 十行目を直して戻すと、**関係のない冒頭へぐいーんと動く**。
    // Undo が効いているのは分かるのに、いまどこを見ているのか分からない。
    //
    // どこへ連れていくかは決まっている ── **変わった行**。
    const at = firstDiff(lastSaved, text);
    steppingBack = true;
    loading = true;
    editor.setValue(text);
    loading = false;
    lastSaved = text;
    state.dirty = true;
    await save();
    steppingBack = false;
    await drawRead();
    drawCount();
    drawSteps();
    landBack(at);
    say(forward ? 'やり直しました' : '一つ戻しました');
}

/// 戻したあとの行き先。書く面ならその行、読む面ならその行のかたまり。
///
/// 変わった行が無い（`-1`）ときは動かさない ── 動く理由が無いのに
/// 動くのが、そもそもの不具合だった。
function landBack(at) {
    if (at < 0) return;
    // 書く面が出ているときは Monaco（`gotoHead` と同じ切り分け ── 一度
    // ここを `!== 'write'` と書き、書く面でだけ動かなかった）。
    if (view !== 'read' && editor) {
        const line = at + 1;
        editor.revealLineNearTop(line);
        editor.setPosition({ lineNumber: line, column: 1 });
    }
    if (view !== 'write') {
        // 読む面のかたまりは、元の字の何行目からかを持っている
        // （`data-line`）── そこを跨ぐものが、変わった行のかたまり。
        const cut = state.head ? state.head.split('\n').length - 1 : 0;
        const want = at + cut;
        let hit = null;
        for (const b of el('read').children) {
            const from2 = Number(b.dataset.line);
            if (Number.isNaN(from2)) continue;
            const span = Number(b.dataset.span) || 1;
            if (from2 <= want && want < from2 + span) { hit = b; break; }
            if (from2 > want) break;
            hit = b;
        }
        if (hit) hit.scrollIntoView({ block: 'center' });
    }
}

/// 矢印は字ではなく線で描く ── 「↩」は書体によって太さも向きも変わる。
const STEP_ICON = (back) => '<svg viewBox="0 0 16 16" aria-hidden="true">'
    + '<path d="' + (back
        ? 'M6 3.6 2.4 7.2 6 10.8M2.4 7.2h6.9a3.4 3.4 0 0 1 0 6.8H7.4'
        : 'M10 3.6 13.6 7.2 10 10.8M13.6 7.2H6.7a3.4 3.4 0 0 0 0 6.8h1.9')
    + '" fill="none" stroke="currentColor" stroke-width="1.6"'
    + ' stroke-linecap="round" stroke-linejoin="round"/></svg>';

/// 鐘。**画面の上から仕掛けたい** ── 通知は「このノートに」するもので、
/// 献立の奥にあると、仕掛けたことも仕掛かっていることも見えない。
const BELL_ICON = '<svg viewBox="0 0 16 16" aria-hidden="true">'
    + '<path d="M8 1.6a3.9 3.9 0 0 0-3.9 3.9c0 3.4-1.3 4.4-1.3 4.4h10.4'
    + 's-1.3-1-1.3-4.4A3.9 3.9 0 0 0 8 1.6zM6.6 12.4a1.6 1.6 0 0 0 2.8 0"'
    + ' fill="none" stroke="currentColor" stroke-width="1.35"'
    + ' stroke-linecap="round" stroke-linejoin="round"/></svg>';

function drawSteps() {
    const b = el('back');
    const f = el('fwd');
    if (!b || !f) return;
    if (!b.innerHTML) {
        b.innerHTML = STEP_ICON(true);
        f.innerHTML = STEP_ICON(false);
        b.onclick = () => stepBack(false);
        f.onclick = () => stepBack(true);
        const bell = el('bell');
        bell.innerHTML = BELL_ICON;
        bell.onclick = () => cmdRemind();
    }
    const bell = el('bell');
    bell.hidden = !state.open;
    // 仕掛かっているかは、いま開いているノートの前書きが言う。
    bell.classList.toggle('on', !!state.open && /(^|\n)remind:/.test(state.head || ''));
    const on = !!state.open;
    b.disabled = !on || !backs.length;
    f.disabled = !on || !forwards.length;
    b.hidden = !on;
    f.hidden = !on;
}

/* ── 入ってきたもの ── */

/// いま開いているノートに入ってきたもの（`null` なら何も無い）。
///
/// **憶えるのはこの機械の引き出し**（`amber.json`）── 「自分が確認したか」は
/// 人ごと・機械ごとのことで、フォルダに置くと家族の誰かが読んだ時点で
/// 全員のぶんが消える。ノートにも書かない（ただの Markdown のまま）。
let incoming = null;
let incomings = {};

function keepIncoming() {
    if (incoming) incomings[incoming.path] = incoming;
    else if (state.open) delete incomings[state.open.path];
    window.amber.remember({ incomings });
}

/// このノートに、まだ確認していないものがあるか。**開き直しても出る**
/// （消え方① ── 押すまで残す。閉じただけで消えると、見逃した日に
/// 気づく道がどこにも無くなる）。
function loadIncoming() {
    incoming = (state.open && incomings[state.open.path]) || null;
}

/// 報せの帯。**誰が・何行**。
///
/// 「直し」でも「更新」でもなく**「書きました」** ── 相手は間違いを
/// 正したのではなく、書いたのだから（本人と決めた言葉。amber は前から
/// 「同時に書いた控え」「あちらでも書き換えられています」と言っている）。
function drawBand() {
    const b = el('band');
    if (!incoming || !incoming.came.length) { b.hidden = true; b.innerHTML = ''; return; }
    const n = incoming.came.length;
    const what = incoming.eyes
        ? '同じところを二人が更新しました。どちらにするか決めてください'
        : 'ほかの人が ' + n + ' 行更新しました';
    b.className = incoming.eyes ? 'eyes' : '';
    b.hidden = false;
    b.innerHTML = '<span class="dot"></span><span>' + what + '</span>'
        + '<button class="act">ほかの人が更新したところを確認した</button>';
    b.querySelector('.act').onclick = () => {
        incoming = null;
        keepIncoming();
        drawBand();
        paintIncoming();
    };
}

/// 来た行に、地色を敷く。
///
/// **できるだけ細かい単位で。** 箇条書きは `<ul>` ひとつで一かたまりなので、
/// 上の段だけを見て塗ると**一行来ただけで三行とも光る** ── 買い物リストは
/// まさにその形で、それでは何が来たのか分からない（実物で見て気づいた）。
/// 行の札（`data-line`）は `<li>` も持っているので、そこまで降りる。
///
/// **最初の `<li>` は札を持たない**（親の `<ul>` が同じ行を指している）ので、
/// 親から継ぐ。降りきれない形（段落の途中の一行など）は、そのかたまり
/// ぜんぶを塗る ── 塗り過ぎるほうが、塗り落とすよりまし。
function paintIncoming() {
    const rd = el('read');
    for (const b of rd.querySelectorAll('.came, .both')) b.classList.remove('came', 'both');
    if (!incoming) return;

    // 行番号 → いちばん深い持ち主。
    const owner = new Map();
    const walk = (node, inherited) => {
        for (const b of node.children) {
            let line = Number(b.dataset.line);
            // 一覧の最初の一つは、親の行をそのまま指している。
            if (Number.isNaN(line) && inherited !== null && b === node.children[0]) line = inherited;
            if (!Number.isNaN(line)) owner.set(line, b);
            walk(b, Number.isNaN(line) ? inherited : line);
        }
    };
    walk(rd, null);

    const paint = (rows, cls) => {
        for (const n of rows) {
            const at = owner.get(n);
            if (at) { at.classList.add(cls); continue; }
            // 持ち主が居ない行は、跨いでいるかたまりごと。
            for (const b of rd.children) {
                const from = Number(b.dataset.line);
                if (Number.isNaN(from)) continue;
                const span = Number(b.dataset.span) || 1;
                if (from <= n && n < from + span) { b.classList.add(cls); break; }
            }
        }
    };
    paint(incoming.came, 'came');
    paint(incoming.both, 'both');
}

/// 向こうと混ぜて、書き戻す。返すのは混ざった字（駄目なら `null`）。
///
/// **判断は core、運ぶのはここ**（`sync` と同じ切り分け）── どの行が
/// 向こうから来たかも core が言うので、画面はそれを受け取って印を差す。
///
/// **印はノートに書かない。** 憶えるのはこの機械の引き出し（`amber.json`）
/// ── 「自分が確認したか」は**人ごと・機械ごと**のことで、フォルダに
/// 置くと家族の誰かが読んだ時点で全員のぶんが消える。
async function mergeIn(path, ours, was) {
    // 向こうの、いまの中身（`read` ── 開くときと同じ口）。
    let theirs;
    try {
        const got2 = await ask('read', { path });
        theirs = got2 && typeof got2.text === 'string' ? got2.text : null;
    } catch (e) {
        say('向こうの中身を読めません: ' + why(e));
        return null;
    }
    // **空が返ってきたら、混ぜない。** 読めなかったのか本当に空なのかを
    // 見分けられないまま混ぜると、**混ざった結果も空**になり、それを
    // そのまま書き戻す ── 一度それでノートを消した。
    if (theirs === null) {
        say('向こうの中身を読めません');
        return null;
    }
    let got;
    try {
        got = await ask('merge', { was, ours, theirs });
    } catch (e) {
        say('混ぜられません: ' + why(e));
        return null;
    }
    // **空を書き戻さない。** 混ぜた結果が空になるのは、どちらかが空だった
    // ときだけ ── 両方に字があったのに空が出たなら、それは混ぜ損ねている。
    if (!got.text.trim() && (was.trim() || ours.trim())) {
        say('混ぜた結果が空になりました。書き戻していません');
        return null;
    }
    try {
        const w = await ask('write', { path, text: got.text, force: true });
        if (w && w.stamp) state.stamp = w.stamp;
    } catch (e) {
        say('保存できません: ' + why(e));
        return null;
    }
    // 面に出す ── 誰が・何行・どこ。名前は向こうが書いた履歴が持っている
    // ものではないので、分かるときだけ言う。
    // **誰が書いたかは、言えない。** ノートはただの Markdown で、名前は
    // どこにも書いていない（書かないと決めた ── 依頼 320）。分からない
    // ことを分かったように言わない。
    incoming = {
        path,
        came: got.came || [],
        both: got.both || [],
        eyes: !!got.eyes,
    };
    keepIncoming();
    // 混ざった字を面へ。**caret は飛ばさない**ので、組み直しはこのあと。
    loading = true;
    const body = got.text.startsWith(state.head) ? got.text.slice(state.head.length) : got.text;
    editor.setValue(body);
    loading = false;
    lastSaved = body;
    state.was = got.text;
    state.base = got.text;
    drawBand();
    return got.text;
}

async function save() {
    if (!state.open || !editor) return;
    const path = state.open.path;
    // 頭を戻してから書く。**ここを忘れると、保存のたびに front matter が
    // 一枚ずつ消える** ── 題もタグも作った日も。
    // 混ぜたときに差し替わるので `let`。
    let text = state.head + editor.getValue();
    // 書き込む直前の姿を積む ── 書いたあとだと、戻る先が「いまの姿」になる。
    keepStep(editor.getValue());
    // **一世代にするかは core が決める。** 同じフォルダを二つの端末で
    // 触るので、片方の決まりで消したものをもう片方が残っていると思う、が
    // 起きてはいけない。ここは「保存する前の姿はこれです」と言うだけ。
    if (!state.guest) {
        try {
            await ask('keep', { root: state.root, path, text: state.was ?? text, gap: KEEP_GAP });
        } catch { /* 履歴が置けないことで、保存が止まる理由はない */ }
    }
    // **分かれる前の姿**（開いた時点、または前に保存できた時点の中身）。
    // `state.was` はこのあと上書きされるので、先に控える。
    const ancestor = state.base ?? state.was ?? text;
    state.was = text;
    try {
        const r = await ask('write', { path, text, stamp: state.stamp });
        if (r && r.conflict) {
            // **どちらかを捨てない。混ぜる。**
            //
            // 前はここで「こちらで上書きしますか／向こうを読み直しますか」と
            // 訊いていた ── どちらを押しても、**片方の書いたものが消える**。
            // 家族で同じ棚を触るのが前提のアプリで、それは強すぎる。
            //
            // 混ぜ方は core（`merge`）── 分かれる前（開いた時点の中身）と、
            // こちらと、向こうの三つを渡す。同じ場所を二人が書いていたら
            // 両方残る（迷ったら残す）。
            const merged = await mergeIn(path, text, ancestor);
            if (merged === null) {
                // 混ぜられなかった（向こうが読めないなど）── 前の姿に戻す。
                state.dirty = false;
                await openNote(path);
                return;
            }
            text = merged;
        }
        if (r && r.stamp) state.stamp = r.stamp;
        // 書けた ── ここでファイルと一致したので、土台を進める。
        state.base = text;
        state.dirty = false;
        el('state').textContent = '保存しました';
        await freshenRow(path);
        drawStrip();
        setTimeout(() => {
            if (!state.dirty && state.open && state.open.path === path) {
                el('state').textContent = when(state.open.updated);
            }
        }, 1400);
    } catch (e) {
        el('state').textContent = '保存できません';
        say('保存できません: ' + why(e));
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
        // **作れた道を返す。** 貼り付けて作る道（`cmdPasteNote`）が、
        // 作れたかどうかを見るのに要る。
        return r.path;
    } catch (e) {
        say('作れません: ' + why(e));
        return null;
    }
}

/* ── 読み直し ── */

/// いま書いた一本の行だけ、新しくする。
///
/// **書いたあとに棚を丸ごと数え直さない。** 1002 本で毎回 370ms かかって
/// いた（エンジンが 220ms、一覧を組み直すのに 150ms）── しかも打ち終えて
/// 0.7 秒後に走るので、**打ち終わるたびに画面が固まる**。自分が書いた一本の
/// ことは自分が知っていて、外で何かが変わったなら見張り（`onChanged`）が
/// 別に教えてくれる。
///
/// **重ねる欄は、名指しで選ぶ。** 返ってきたものを丸ごと重ねてはいけない
/// ── `note` は「amber の外にある一本」も読める口なので、`rel` はファイル名
/// だけ、`book` は空で返る。丸ごと重ねると、**保存するたびにノートが
/// いちばん上のフォルダへ移ったように見える**（共有の印も消える）。
/// 字を書いて変わるのは、下に並べた欄だけ。
///
/// 訊けなかったら、前のように丸ごと数え直す ── 一覧が古いまま残るよりよい。
const FRESH = ['title', 'excerpt', 'tags', 'updated', 'created', 'bytes', 'search'];

/// 最後に自分で書いたノート。見張りが自分の書き込みで起きたかを見分ける。
let lastWrote = null;

async function freshenRow(path) {
    lastWrote = path;
    const at = state.notes.findIndex((n) => n.path === path);
    if (at < 0) return reload({ quiet: true });
    let one;
    try {
        one = await ask('note', { path });
    } catch {
        return reload({ quiet: true });
    }
    const row = { ...state.notes[at] };
    for (const k of FRESH) if (k in one) row[k] = one[k];
    state.notes[at] = row;
    if (state.open && state.open.path === path) state.open = row;
    drawTitle();
    drawList();
}

async function reload(opts) {
    try {
        const r = await ask('notes', { path: state.root });
        state.notes = r.notes || [];
        state.books = r.books || [];
        state.stars = r.stars || [];
        state.colors = r.colors || {};
        // 共有へ入れたノートが、もといたフォルダ。**献立に「どこへ戻すか」
        // を出すのに要る** ── そのつど訊きに行くと、押す前に消費してしまう。
        state.came = r.came || {};
        state.waiting = r.waiting || [];
        state.shares = r.shares || [];
        // 開いていた行を新しいほうに繋ぎ直す（更新時刻が動くので）。
        if (state.open) {
            state.open = state.notes.find((n) => n.path === state.open.path) || state.open;
            drawTitle();
        }
        // **消えたタグやフォルダで絞ったままにしない。** 外から消えた
        // ものを選んだままだと、一覧がずっと空で、理由が帯にしか出ない。
        const tags = new Set(tagsOf(state.notes).map(([t]) => t));
        state.picks.tag = state.picks.tag.filter((t) => tags.has(t));
        state.picks.book = state.picks.book.filter((b) => state.books.includes(b));
        drawRail();
        drawDrawers();
        drawDrawer();
        drawCloud();
        drawList();
    } catch (e) {
        if (!opts || !opts.quiet) say('読めません: ' + why(e));
    }
}

/* ── キー ── */

function moveCursor(delta) {
    const rows = [...el('rows').querySelectorAll('.row')];
    if (!rows.length) return;
    const at = rows.findIndex((r) => r.classList.contains('on'));
    const next = rows[Math.min(rows.length - 1, Math.max(0, (at < 0 ? -1 : at) + delta))];
    if (next) {
        openNote(next.dataset.path);
        next.scrollIntoView({ block: 'nearest' });
    }
}

// **`e.code` で当てる。`e.key` ではない。** JIS 配列では `key` が
// `Zenkaku` にも `Process` にも `Unidentified` にもなり、IME が拾っている
// 間は `?` すら `Process` になる。cian で「Mac では直ったのに JIS で効かない」
// を二件出している。
/// 戻す・やり直す。**捕捉の段で受ける。**
///
/// Monaco にも読む面にも自前の取り消しがあるが、どちらも
/// `editor.setValue()` で組み直したところ（表・図・升）で積み木ごと消える
/// ── 押した直後に ⌘Z を押しても何も起きない。ノートの字を積んでいる
/// こちらに一本化する。
///
/// **泡の段では届かないことがある** ── Monaco は自分の textarea で ⌘Z を
/// 受けて、そこで止めることがある。捕捉の段なら、どこを打っていても先に
/// 通る。小窓と工房の中だけは、あちらの受け口に譲る。
/// ⌘S は「保存」ではなく「ここを残す」── 押した反射に、意味のある返事を。
document.addEventListener('keydown', (e) => {
    if (!(e.metaKey || e.ctrlKey) || e.code !== 'KeyS' || e.shiftKey) return;
    if (!el('veil').hidden || !el('studio').hidden || !state.open) return;
    e.preventDefault();
    e.stopPropagation();
    cmdKeepNow();
}, true);

document.addEventListener('keydown', (e) => {
    if (!(e.metaKey || e.ctrlKey) || e.code !== 'KeyZ') return;
    if (!el('veil').hidden || !el('studio').hidden) return;
    if (!state.open) return;
    e.preventDefault();
    e.stopPropagation();
    stepBack(e.shiftKey);
}, true);

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
        if (!el('emoji').hidden) { e.preventDefault(); closeEmoji(); return; }
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
    // 鍵の一覧（⌘? ── mac で「ショートカットを見る」はここ）。
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.code === 'Slash') {
        e.preventDefault(); cmdKeys(); return;
    }
    // 左の列を畳む（Inkdrop の ⌘/）。狭い画面では二列ぶんが効く。
    if ((e.metaKey || e.ctrlKey) && e.code === 'Slash') {
        e.preventDefault();
        // `⇧` を足すと二枚目。**同じ鍵の並びに揃える** ── 畳むことは
        // 一つの動きで、畳む相手が違うだけ。
        if (e.shiftKey) toggleList(); else toggleRail();
        return;
    }
    // **まとめて選ぶ。** 打っている最中は取らない ── エディタと探す欄の
    // `⌘A` は「字を全部選ぶ」で、そちらのほうが強い。
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyA' && !inField && !inEditor && !inNote(document.activeElement)) {
        e.preventDefault();
        pickAll();
        return;
    }
    // 見たノートの前後（Inkdrop の ⌘← / ⌘→）。
    if ((e.metaKey || e.ctrlKey) && e.code === 'ArrowLeft') { e.preventDefault(); walk(-1); return; }
    if ((e.metaKey || e.ctrlKey) && e.code === 'ArrowRight') { e.preventDefault(); walk(1); return; }
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyN') { e.preventDefault(); newNote(); return; }
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyF') { e.preventDefault(); openFind(); return; }
    if (e.metaKey || e.ctrlKey || e.altKey) return;

    if (e.code === 'Escape') {
        // 探す欄の Esc は**畳んで空にする** ── 見えない絞り込みを残さない。
        if (inField) { el('find').blur(); closeFind(); return; }
        // 選んでいるなら、まず選びを畳む ── いちばん手前のものから。
        if (state.picked.size) { unpickAll(); return; }
        if (inEditor) { document.activeElement.blur(); return; }
    }
    // 文字を打っている場所では、素の一文字は文字であって命令ではない。
    if (inField || inEditor || inRead) return;

    if (e.code === 'ArrowDown' || e.code === 'KeyJ') { e.preventDefault(); moveCursor(1); }
    else if (e.code === 'ArrowUp' || e.code === 'KeyK') { e.preventDefault(); moveCursor(-1); }
    else if (isEnter(e)) { e.preventDefault(); if (editor) editor.focus(); }
    else if (e.code === 'KeyN') { e.preventDefault(); newNote(); }
    // **消すのは、必ず訊いてから。** 打っている場所では上で戻しているので、
    // ここに来るのは一覧を見ているときだけ。
    else if ((e.code === 'Backspace' || e.code === 'Delete') && state.open) {
        e.preventDefault(); cmdDelete();
    }
    else if (e.code === 'Slash') { e.preventDefault(); openFind(); }
});

/// 修飾キー付きの一打を、道具の一押しに。
///
/// **表は一つ（`MARKS`）。** 前はここに書く面用の写しがもう一組あって、
/// 鍵盤から押したときだけ**読む面で効かなかった** ── ⌘B が帯からは効いて
/// 一打からは効かない、という見分けの付かない差になっていた。帯に書いた
/// 鍵をそのまま引く。
///
/// **`e.code` で当てる。** JIS では `e.key` が `Process` になり、数字の
/// 段は配列で別の字になる ── `Digit8` は「8 の位置のキー」なので動く。
function keyName(e) {
    const c = e.code;
    let base = '';
    if (/^Key[A-Z]$/.test(c)) base = c.slice(3);
    else if (/^Digit[0-9]$/.test(c)) base = c.slice(5);
    else if (c === 'Quote') base = "'";
    else return '';
    return '⌘' + (e.shiftKey ? '⇧' : '') + base;
}

/// 見出しの深さは、鍵盤からは一打で。**帯の釦は押すたびに深くなる**まま
/// ── 一つの考えに三つの名前を付けない、は釦の話で、鍵盤には当てはまらない
/// （Inkdrop も `toggle-heading-1` … `-4` を別々に持っている）。
const HEAD_KEYS = { '⌘1': 1, '⌘2': 2, '⌘3': 3, '⌘4': 4 };

function markKey(e) {
    const name = keyName(e);
    if (!name) return null;
    const n = HEAD_KEYS[name];
    if (n !== undefined) {
        return () => (onRead()
            ? readBlockAs('h' + n)
            : applyMark('head', String(n)));
    }
    const found = MARKS.flat().find((m) => m[1] === name && m[2]);
    return found ? found[2] : null;
}

/// 貼り付けられたものが絵なら、ノートの隣に置いてリンクを打つ。
///
/// **捕まえるのは絵のときだけ。** 字の貼り付けはエディタの仕事で、
/// ここが横取りすると Monaco の取り消しが繋がらなくなる。
document.addEventListener('paste', async (e) => {
    // **「表示」の面でも受ける。** 前はここで帰っていたので、読む面に
    // 撮った画面を貼っても何も起きなかった ── 画面を撮って貼るのは、
    // いちばん「表示」の面でやりたいこと。
    if (!state.open || !editor) return;
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

/// 一覧（二枚目）を畳む。
///
/// **三枚目の「ノートだけ」は作らない。** 二枚とも畳めばそこへ行き着くし、
/// `F12`（ノートだけを大きく）が既にある ── 同じところへ着く道を三本
/// 持つと、どれで畳んだのかによって戻り方が違う画面になる。
let listOff = false;
function toggleList() {
    listOff = !listOff;
    document.body.classList.toggle('nolist', listOff);
    window.amber.remember({ listOff });
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
    { id: 'tmpl', name: 'テンプレートから新しいノート', sub: '「' + TEMPLATES + '」フォルダの中身',
      run: cmdTemplate },
    { id: 'outside', name: 'ambər フォルダ以外のノートを開く', key: '⌘O', app: true,
      run: cmdOpenOutside },
    // **`⌘S` は「現状バージョン保存」が持っている**（受け口は捕捉の段）。
    // ここにも同じ鍵を書くと、一覧に二つ並んで、どちらが走るのか
    // 画面が答えられない。ふだんは打てば勝手に保存される。
    { id: 'save', name: '保存', sub: '打てば自動でも保存されます', need: 'note', run: () => save() },
    { id: 'read', name: '表示 / コードを入れ替え', key: '⌘E', need: 'note', run: () => toggleRead() },
    { id: 'split', name: '並べて表示', key: '⌘P', need: 'note', run: () => toggleSplit() },
    // **⚙ にも出す。** 鍵（`⌘/`）を覚えていない人には、畳む道がどこにも
    // 無かった ── Inkdrop は献立に並べている。
    { id: 'rail', name: '左の列を畳む', key: '⌘/', app: true, run: () => toggleRail() },
    { id: 'list', name: '一覧を畳む', key: '⌘⇧/', app: true, run: () => toggleList() },
    { id: 'back', name: '前に見たノート', key: '⌘←', run: () => walk(-1) },
    { id: 'fwd', name: '次に見たノート', key: '⌘→', run: () => walk(1) },
    { id: 'find', name: 'ノートを探す', key: '⌘F', run: () => openFind() },
    // **絞り込みは、命令ではなくなった。** タグ・フォルダ・期間の三つは
    // 一覧の頭に引き出しとして常に出ている ── 命令の表から呼ぶものが
    // 別にあると、同じことを頼む道が二つになる。
    { id: 'when', name: '期間で絞る（こよみ）', run: () => openDrawer('when') },

    // ── このノートにすること（⋯ と、ノートの右押し）
    { id: 'star', name: 'ブックマークに登録', key: '⌘D', need: 'note', menu: true, run: cmdStar },
    { id: 'tags', name: 'タグ設定', need: 'note', menu: true, run: cmdTags },
    { id: 'move', name: 'フォルダへ移動', need: 'note', menu: true, run: cmdMove },
    { id: 'toshare', name: '家族と共有する', need: 'note', menu: true, run: cmdToShare },
    // **献立には出さない。** 上の帯にベルが居て、押せば同じ小窓が出る
    // ── 同じことを頼む道が二つあると、片方を直した日にもう片方が
    // 古いまま残る。表には残す（⌘⇧P から名前で探せる）。
    { id: 'remind', name: '通知設定', need: 'note', run: cmdRemind },
    { id: 'dup', name: '複製', sub: '同じ中身のノートをもう一つ', need: 'note', menu: true,
      run: cmdDup },
    { id: 'export', name: 'エクスポート', need: 'note', menu: true, run: cmdExport },
    // **名前で出す。** 前は帯に ☰ と ⤢ が並んでいたが、どちらが目次で
    // どちらが拡大かは記号のどこにも書いていない ── 帯の幅を食っていた
    // うえ、押してみるまで分からなかった。
    { id: 'toc', name: '目次', key: '⌘⇧O', need: 'note', menu: true, run: () => toggleToc() },
    { id: 'closetab', name: 'このタブを閉じる', need: 'note', menu: true,
      run: () => closeTab(showing) },
    { id: 'zen', name: 'ノートだけを大きく', key: 'F12', need: 'note', menu: true, run: () => setZen(!zen) },
    { id: 'delete', name: 'ゴミ箱へ入れる', need: 'note', menu: true, sep: true, run: cmdDelete },

    // ── amber のこと（⚙）
    // **鍵は「ショートカット一覧」へ渡した。** ここに `⌘⇧/` と書いて
    // ありながら、受け口はどこにも無く、押すと左の列が畳まれていた
    // ── 献立が嘘をついていた。
    { id: 'keys', name: 'ショートカット一覧', key: '⌘⇧/', app: true, run: cmdKeys },
    { id: 'syntax', name: 'マークダウンの書き方', app: true, run: cmdSyntax },
    { id: 'theme', name: 'テーマ', app: true, run: cmdTheme },
    { id: 'vim', name: 'vimモード', app: true, run: cmdVim },
    { id: 'lineno', name: '行番号', app: true, run: cmdLineNo },
    // ── ノートを入れる／出す
    // **入れる三つを、並べて置く。** 「見本のノートを入れる」は列の
    // いちばん下に一つだけ離れて座っていて、探す人は「amber について」の
    // 下まで来ない ── 同じ行い（ノートを入れる）は同じ場所に。
    { id: 'bring', name: 'ノートを取り込む', app: true, sep: true, run: cmdBring },
    { id: 'welcome', name: '見本のノートを入れる', app: true, run: cmdWelcome },
    { id: 'backup', name: 'バックアップ', app: true, run: cmdBackup },
    { id: 'restore', name: 'バックアップから戻す', app: true, run: cmdRestore },
    { id: 'root', name: 'ambər 保存ディレクトリ変更', app: true, run: cmdRoot },
    { id: 'all', name: 'コマンド一覧', key: '⌘⇧P', app: true, sep: true, run: () => palette() },
    { id: 'about', name: 'ambər について', app: true, run: cmdAbout },
    { id: 'history', name: '過去バージョン', need: 'note', menu: true, run: () => cmdHistory() },
    { id: 'keepnow', name: '現状バージョン保存', key: '⌘S', need: 'note', menu: true, run: cmdKeepNow },
    // **`back` / `fwd` は上の「前に見たノート」で使っている。** 同じ id を
    // 二つ置くと、パレットから選んだときに先に見つかったほうが走る。
    { id: 'undo', name: '一つ戻す', key: '⌘Z', need: 'note', run: () => stepBack(false) },
    { id: 'redo', name: 'やり直す', key: '⌘⇧Z', need: 'note', run: () => stepBack(true) },

    // ── 表には要るが、献立には出さないもの
    { id: 'mkbook', name: '新しいフォルダを作る', run: () => cmdMkBook() },
    { id: 'color', name: 'フォルダに色を付ける', sub: 'フォルダを右押しでも', run: () => cmdColor() },
    { id: 'bigger', name: '字を大きく', key: '⌘+', run: () => setFont(fontStep + 1) },
    { id: 'smaller', name: '字を小さく', key: '⌘−', run: () => setFont(fontStep - 1) },
    { id: 'font0', name: '字の大きさを戻す', key: '⌘0', run: () => setFont(0) },
];

/// 命令の表にも、道具の帯にも出ない一打。**ここにしか無い鍵。**
///
/// 一覧を上下する ↑↓ も、表の中の Tab も、押してみるまで分からなかった。
/// 覚えていなくてよいものにするには、まず**どこかに書いてある**必要がある。
const LOOSE_KEYS = [
    ['一覧を上下する', '↑ ↓ / J K', '一覧を見ているとき'],
    ['そのノートを開いて打つ', 'Enter', '一覧を見ているとき'],
    ['ゴミ箱へ入れる', 'Delete', '選んでいるノートを（訊いてから）'],
    ['ノートを探す', '/', '一覧を見ているとき'],
    ['閉じる・やめる', 'Esc', '小窓・工房・大きい画面から'],
    ['次の升へ', 'Tab', '表の中で（⇧Tab で前へ、最後で押すと行が増える）'],
];

/// ショートカットの一覧（⌘⇧/）。**探せれば、覚えなくていい。**
///
/// 「どうやって確かめるの」と訊かれた ── 鍵は帯の吹き出しと ⋯ の献立に
/// 散らばっていて、**全部を一度に見る場所が無かった**。ここは一枚にして、
/// 選べばその場で走る（読むだけの紙にすると、見ながら手で打ち直すことに
/// なる）。
async function cmdKeys() {
    const rows = [];
    const put = (name, key, sub, run) => rows.push({ name, key, sub, run });

    rows.push({ name: '── 書く道具（記号）', sub: '「表示」でも「コード」でも', head: true });
    for (const [name, key, run] of MARKS.flat()) {
        if (name === '|' || !run) continue;
        // 見出しだけ、鍵と釦で振る舞いが違う ── 一打はその深さに直し、
        // 釦は押すたびに深くなる。一覧では両方言う。
        if (name === '見出し') { put(name, '⌘1 ⌘2 ⌘3 ⌘4', '一打でその深さに（帯の釦は押すたび深く）', run); continue; }
        put(name, key || '', key ? '' : '帯の釦から', run);
    }
    put('マークダウンの書き方', '', '記号そのものを見る', cmdSyntax);

    rows.push({ name: '── 窓のこと', sub: 'どこを打っていても効きます', head: true });
    for (const c of CMDS) {
        if (!c.key || !canRun(c)) continue;
        put(c.name, keyText(c.key), '', () => c.run());
    }

    rows.push({ name: '── そのほか', head: true });
    for (const [name, key, sub] of LOOSE_KEYS) put(name, key, sub);

    const pick = await askPick('ショートカット一覧',
        rows.map((r, n) => ({ name: r.name, sub: r.sub || '', key: keyText(r.key), head: r.head, value: n })),
        '選ぶと、その場で実行します');
    if (pick === null) return;
    const hit = rows[pick];
    if (hit && hit.run) await hit.run();
}

const canRun = (c) => c.need !== 'note' || !!state.open;

/// 命令のパレット（⌘⇧P）。**名前で探せれば、覚えなくていい。**
///
/// **命令だけではない**（依頼 414）。ここに来る人が探しているのは
/// 「やること」ではなく「行き先」のことが多い ── ノート・フォルダ・タグ・
/// いま開いているノートの見出し。名前を打てば全部ここに出る、が
/// いちばん覚えることが少ない（Inkdrop も VS Code もそうしている）。
///
/// **一つの箱に混ぜる。** 種類ごとに別の窓を割り当てると、打つ前に
/// 「これは何を探すところか」を思い出す仕事が増える ── 打った字が
/// どれに当たるかは、こちらが数えればよい。
///
/// **並びは、近いものから。** 命令 → いま開いているノートの見出し →
/// ノート → フォルダ → タグ。見出しが上なのは、いま見ているものの中の
/// 話だから ── 手元の話が画面の下にあると、そこまで目が降りない。
async function palette() {
    const rows = [];
    const head = (name, sub) => rows.push({ name, sub, head: true });

    head('── すること');
    for (const c of CMDS.filter(canRun)) {
        rows.push({ name: c.name, key: keyText(c.key), run: () => c.run() });
    }

    // いま開いているノートの見出し ── 長いノートの中を歩く道。
    if (state.open) {
        let heads = [];
        try {
            heads = ((await ask('blocks', { text: whole() })).blocks || [])
                .filter((b) => b.kind === 'heading');
        } catch { /* 読めなければ、出さないだけ */ }
        if (heads.length) {
            head('── このノートの見出し', state.open.title || '');
            for (const h of heads) {
                rows.push({
                    // 深さを字下げで見せる ── `##` の下の `###` が同じ列に
                    // 並ぶと、目次に見えない。
                    name: '　'.repeat(Math.max(0, (h.level || 1) - 1)) + h.text,
                    run: () => gotoHead(h),
                });
            }
        }
    }

    head('── ノートを開く', state.notes.length + ' 件');
    for (const n of sortNotes(state.notes)) {
        rows.push({
            name: n.title || '（タイトルなし）',
            sub: n.book || '',
            run: () => openNote(n.path),
        });
    }

    const go = (kind, what) => () => {
        state.dest = { kind, what };
        drawRail();
        drawList();
    };
    if (state.books.length) {
        head('── フォルダへ');
        for (const b of state.books) {
            rows.push({
                name: b,
                sub: state.notes.filter((x) => x.book === b || x.book.startsWith(b + '/')).length + ' 件',
                run: go('book', b),
            });
        }
    }
    const tags = tagsOf(state.notes);
    if (tags.length) {
        head('── タグで絞る');
        for (const [t, c] of tags) rows.push({ name: t, sub: c + ' 件', run: go('tag', t) });
    }

    const pick = await askPick('何をしますか',
        rows.map((r, n) => ({ name: r.name, sub: r.sub || '', key: r.key || '', head: r.head, value: n })),
        '↑↓ で選び、Enter で実行');
    if (pick === null) return;
    const hit = rows[pick];
    if (hit && hit.run) await hit.run();
}

/// ⋯ の献立。**パレットと同じ表の、ノートに関わるところだけ。**
/// `which` が `app` なら設定の献立、既定はノートの献立。
///
/// **「見た目」をノートの右押しに出さない。** ノートを右押しした人が
/// 訊いているのは「このノートをどうするか」で、アプリの色ではない。
function openMenu(at, which) {
    const key = which === 'app' ? 'app' : 'menu';
    const items = CMDS.filter((c) => c[key] && canRun(c)).map((c) => {
        // **いまどうなっているかを、押す前に見せる。**
        // **「いま:」とは書かない。** 献立に出ている値は、これから選ぶ値では
        // なく**いまの値**しかありえない ── 一行ごとに同じ二文字を読ませる
        // 意味が無い。入切は「オン / オフ」で揃える（片方だけ「出している」
        // のような言い方をすると、同じ形の設定が二つの語彙を持つ）。
        if (c.id === 'theme') return { ...c, sub: themeName() };
        if (c.id === 'vim') return { ...c, sub: vimOn ? 'オン' : 'オフ' };
        if (c.id === 'lineno') return { ...c, sub: lineNo ? 'オン' : 'オフ' };
        if (c.id === 'rail') return { ...c, name: railOff ? '左の列を出す' : '左の列を畳む' };
        if (c.id === 'list') return { ...c, name: listOff ? '一覧を出す' : '一覧を畳む' };
        if (c.id === 'root') return { ...c, sub: shortPath(state.root) };
        if (c.id === 'toshare') {
            if (state.open && state.open.shared) {
                // **押す前に、どこへ戻るかを言う。** 「いちばん上へ」と
                // 出しておいて別のフォルダへ入るのは、黙って動かすのと同じ。
                const home = homeOf(state.open);
                return { ...c, name: '家族との共有をやめる',
                    sub: home ? '「' + home.split('/').pop() + '」へ戻します' : 'いちばん上へ戻します' };
            }
            const to = state.shares[0];
            return { ...c, sub: to
                // **無ければ作る。** 「共有する」を押した人に、その前に
                // 「フォルダを作る」を押させない。
                ? '「' + (to.at.split('/').pop() || 'ぜんぶ') + '」へ移します'
                : '「家族」という棚を作って、そこへ移します' };
        }
        return c;
    });
    // 区切りは**印の付いた命令の手前**に置く ── 「最後の一つの前」に
    // すると、命令が増えた日に区切りが勝手に動く。
    popMenu(items, at);
}

/// 献立を出す。**描くのも置くのも、ここ一つ。**
///
/// 前は三か所（⋯ の献立・左の列の右押し・色の献立）が同じことを書いて
/// いた ── 画面の外へはみ出さない直しを一か所に入れて、残り二つが古い
/// まま、が起こる形。右押しを十か所に増やすので、先に一本にする。
///
/// `items` は `{ name, sub, key, sep, dim, html, run }` の並び。`at` は
/// 押した場所（`{x, y}`）か、釦の四角（`{right, bottom}` ── 右端に揃える）。
function popMenu(items, at) {
    const box = el('more');
    const rows = items.filter(Boolean);
    if (!rows.length) return;
    box.innerHTML = rows.map((c, n) =>
        (c.sep ? '<div class="sep"></div>' : '')
        + '<button data-n="' + n + '"' + (c.dim ? ' disabled' : '') + '>'
        + (c.html || escapeHtml(c.name))
        + (c.sub ? '<span class="sub">' + escapeHtml(c.sub) + '</span>' : '')
        + (c.key ? '<span class="k">' + escapeHtml(keyText(c.key)) + '</span>' : '')
        + '</button>').join('');
    for (const b of box.querySelectorAll('button')) {
        b.onclick = async () => {
            closeMenu();
            const c = rows[Number(b.dataset.n)];
            if (c && c.run) await c.run();
        };
    }
    box.hidden = false;
    const w = box.offsetWidth;
    const h = box.offsetHeight;
    // 右端に揃えるか、押したところに置くか。どちらでも**画面の外へは
    // 出さない** ── 下に出ないときは、押したところの上へ返す。
    const x = at.right !== undefined ? at.right - w : at.x;
    const y = at.bottom !== undefined ? at.bottom + 6 : at.y + 4;
    box.style.left = Math.max(8, Math.min(x, innerWidth - w - 8)) + 'px';
    box.style.top = (y + h > innerHeight - 8 ? Math.max(8, y - h - 10) : y) + 'px';
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
        b.oncontextmenu = (e) => {
            e.preventDefault();
            const h = heads[Number(b.dataset.n)];
            popMenu([
                { name: 'ここへ飛ぶ', run: () => gotoHead(h) },
                { name: '見出しの字を写す', run: () => copyText(h.text, '見出し') },
            ], { x: e.clientX, y: e.clientY });
        };
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
        // **「＋」も字で書かない。** 全角の記号は行の高さも幅も字に引かれて、
        // 段の見出しの隣で一つだけ大きく沈む ── 印は線で描く（「新しい
        // ノート」の丸と同じ太さ・同じ形）。
        + (plus ? '<button class="plus" data-plus="' + plus + '" title="増やす">'
            + '<svg viewBox="0 0 16 16" aria-hidden="true">'
            + '<path d="M8 3.6v8.8M3.6 8h8.8" stroke="currentColor" stroke-width="1.9"'
            + ' stroke-linecap="round"/></svg></button>' : '')
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
            say('作れません: ' + why(e));
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
    const items = [];
    // **下の階層は、ここから作る。** 名前に「/」を打たせるのは、
    // 書き方を知っている人にしか通じない。
    if (kind === 'book') {
        items.push({ name: 'この中にフォルダを作る', run: () => cmdMkBook(what) });
        items.push({ name: 'フォルダに色を付ける', run: () => cmdColor(what) });
        const isShare = state.shares.some((sh) => sh.at === what);
        if (isShare) {
            items.push({
                name: '家族を招待',
                sub: 'クラウドの画面が開きます',
                run: () => window.amber.reveal(state.root + '/' + what),
            });
        }
        items.push({
            name: isShare ? '家族との共有をやめる' : '家族と共有する棚にする',
            run: () => cmdShare(what, isShare),
        });
        // フォルダの履歴は、**中のノートの姿をまとめて時系列で** ──
        // 「あのあたりで壊した」は、どのノートかを覚えていないほうが多い。
        items.push({ name: '過去バージョン', sub: 'この中のノートぜんぶ',
                     run: () => cmdHistory(state.root + '/' + what, true) });
    }
    if (kind === 'star') {
        items.push({ name: 'この中に置き場所を作る', run: () => newShelf(what) });
    }
    items.push({ name: '名前を変える', sep: items.length > 0, run: () => railRename(kind, what) });
    items.push({
        name: kind === 'book' ? 'このフォルダをゴミ箱へ'
            : (kind === 'tag' ? 'このタグを全部のノートから外す' : 'この置き場所を消す'),
        run: () => railDrop(kind, what),
    });
    popMenu(items, at);
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
            // 棚は空でも core が憶えている ── 中のノートだけ直しても、
            // 前の名前の棚が並びに残る（**空の棚は名前を変えられない**）。
            if (kind === 'star') {
                // 下の階層ごと付け替える ── `drop` は下も一緒に忘れるので、
                // 先に新しい名前で作り直しておかないと孫の棚が消える。
                for (const sh of state.stars) {
                    if (sh !== what && !sh.startsWith(what + '/')) continue;
                    await ask('shelf', { path: state.root, name: name + sh.slice(what.length) });
                }
                await ask('shelf', { path: state.root, name: what, drop: true });
            }
        }
        state.dest = { kind, what: name };
        await reload({ quiet: true });
        say('「' + name + '」に変えました（' + hit.length + ' 件）');
    } catch (e) {
        say('変えられません: ' + why(e));
    }
}

async function railDrop(kind, what) {
    const hit = underRail(kind, what);
    const what2 = kind === 'book' ? 'フォルダ' : (kind === 'tag' ? 'タグ' : 'ブックマーク');
    const ask2 = kind === 'book'
        ? '「' + what + '」を、中の ' + hit.length + ' 件ごとゴミ箱へ入れますか'
        : kind === 'star'
            ? '置き場所「' + what + '」を消しますか'
                + (hit.length ? '（中の ' + hit.length + ' 件はブックマークの直下へ）' : '')
            : '「' + what + '」の' + what2 + 'を ' + hit.length + ' 件から外しますか（ノートは残ります）';
    if (!await askYes(ask2)) return;
    try {
        if (kind === 'book') {
            const gone = await window.amber.trash(state.root + '/' + what);
            if (gone !== true) {
                say('ゴミ箱へ入れられません' + (gone && gone.why ? ': ' + gone.why : ''));
                return;
            }
        } else if (kind === 'star') {
            // **消したのは棚で、しおりではない。** 中に居たノートは
            // ブックマークの直下へ移す ── 棚を片付けたつもりで、
            // 印まで一緒に消えるのは取り返しがつかない。
            for (const n of hit) await shelveOne(n, '');
            // **棚そのものも忘れる。** 置き場所は空でも残るように core が
            // 憶えている（`notebook::add_star`）── ノートから外すだけでは、
            // 中身の無い棚が並び続けて**消せないもの**になっていた。
            await ask('shelf', { path: state.root, name: what, drop: true });
        } else {
            for (const n of hit) await retagOne(n, kind, what, null);
        }
        state.dest = { kind: 'all', what: '' };
        state.open = null;
        applyView();
        await reload({ quiet: true });
        say(kind === 'book' ? 'ゴミ箱へ入れました'
            : kind === 'star' ? '「' + what + '」を消しました'
                : '外しました（' + hit.length + ' 件）');
    } catch (e) {
        say('外せません: ' + why(e));
    }
}

/// 一本のノートのタグ（またはブックマーク）を、付け替える。`to` が `null` なら外す。
///
/// **開いているノートは、開いたまま直す。** 直接ファイルを書くと、窓が
/// 持っている字と食い違い、次の保存でどちらかが消える。
/// ノートを、指した置き場所へ移す（`''` はブックマークの直下）。
async function shelveOne(n, to) {
    const same = state.open && state.open.path === n.path;
    const text = same ? whole() : (await ask('read', { path: n.path })).text;
    const out = (await ask('star', { text, shelf: to })).text;
    if (same) { await putWhole(out); return; }
    const r = await ask('write', { path: n.path, text: out });
    if (r && r.conflict) throw new Error(n.path + ' は別のところで更新されています');
}

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
        if (r && r.conflict) throw new Error(n.path + ' は別のところで更新されています');
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
        say('開けません: ' + why(e));
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

/// フォルダの中身が外から動いたら、数え直す。
///
/// **同じフォルダを二つの端末で触るのがこのアプリの前提。** それなのに、
/// iPhone で書いた一行は窓を開き直すまで出てこなかった ── 同期はしていて、
/// 見ていなかっただけなのに「同期していない」ように見える。
///
/// 開いているノートは、**打っている途中なら触らない** ── いま書いている
/// ものを、向こうの版で黙って置き換えるのが一番悪い。打っていなければ
/// 静かに読み直す（保存のときの衝突検査は、そのまま残っている）。
let churn = null;
window.amber.onChanged((names) => {
    clearTimeout(churn);
    // **自分が書いたぶんで、棚を数え直さない。**
    //
    // 保存するとファイルが動くので、見張りが自分の書き込みで起きる ──
    // 保存の中で行を直したすぐあとに、1002 本を数え直していた（370ms）。
    // 動いたのが**いま自分で書いた一本だけ**なら、もう新しい。
    // 名前は `NFC` に揃えて比べる ── mac は濁点を分けて持つことがあり
    // （`が` = `か` + `゛`）、字の上では同じ名前が一致しなくなる。外れても
    // 数え直すだけで害は無いが、**日本語の名前のノートだけ遅い**になる。
    const nfc = (s) => String(s).normalize('NFC');
    if (names && names.length && lastWrote
        && names.every((n) => nfc(n) === nfc(lastWrote))) return;
    churn = setTimeout(async () => {
        if (state.guest) return;                 // 単発で開いている一本は索引の外
        await reload({});
        if (state.open && !state.dirty) {
            const now = state.notes.find((n) => n.path === state.open.path);
            // 消えていたら、開いたままにしない ── 無いノートを見せ続けると、
            // 次の保存で作り直してしまう。
            if (!now) { state.open = null; applyView(); return; }
            if (now.updated !== state.open.updated) await openNote(now.path, { quiet: true });
        }
    }, 250);
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
        say('直せません: ' + why(e));
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
    const f = baseOf(state.open?.path || 'note');
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
        say('作れません: ' + why(e));
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

/* ── クラウドの置き土産 ── */

/// **黙って足りない一覧を見せない。**
///
/// クラウドは二種類のものをノートの隣に置いていく。どちらも amber の側では
/// 直せないが、**言わないと「ノートが消えた」にしか見えない**。
///
/// * まだ落ちてきていない ── iCloud は中身を消して `.買い物リスト.md.icloud`
///   という札を置く。名前が違うので一覧に出ない。待てば戻ってくる。
/// * 同時に書いた控え ── クラウドが `買い物リスト (…競合コピー…).md` を作る。
///   これは**ノートとして一覧に出す**（消すと中身を助け出す道が無くなる）。
///   出したうえで、そういうものだと札を貼る。
function drawCloud() {
    const box = el('cloudsay');
    const waiting = state.waiting || [];
    const clash = state.notes.filter((n) => n.clash);
    if (!waiting.length && !clash.length) { box.hidden = true; box.innerHTML = ''; return; }

    const rows = [];
    if (waiting.length) {
        rows.push('<div class="c wait"><b>' + waiting.length
            + ' 件、まだ落ちてきていません</b>'
            + '<span>' + escapeHtml(waiting.slice(0, 3).map((w) => w.of).join('・'))
            + (waiting.length > 3 ? ' ほか' : '')
            + ' ── クラウドが中身をまだ持ってきていないだけで、消えてはいません</span></div>');
    }
    if (clash.length) {
        rows.push('<div class="c clash"><b>' + clash.length
            + ' 件、同時に更新された控えがあります</b>'
            + '<span>'
            + escapeHtml(clash.slice(0, 3).map((n) =>
                n.clash.of + (n.clash.by ? '（' + n.clash.by + '）' : '')).join('・'))
            + (clash.length > 3 ? ' ほか' : '')
            + ' ── クラウドが作ったもの。中身を見比べて、どちらにするか決めてください</span></div>');
    }
    box.innerHTML = rows.join('');
    box.hidden = false;
}

/* ── 絞り込みの帯 ── タグ・フォルダ・期間の引き出し ── */

/// **絞れることが、絞る前から見えている。**
///
/// 前は「フィルタ」という一つの釦で、押すと小窓が開き、タグかフォルダか
/// 期間の**どれか一つ**を選んで閉じる作りだった ── 重ねられないうえ、
/// 選んだ結果は `tag:仕事` という字になって探す欄に流れ込んだ。押しただけ
/// なのに機械の言葉が現れ、外すには字を消すことになる。
///
/// ここは三つの引き出しが常に並び、いくつでも重なる。
let drawer = null;

const DRAWERS = [
    ['tag', 'タグ'],
    ['book', 'フォルダ'],
    ['when', '期間'],
];

/// 何か絞っているか。
function filtering() {
    return !!(state.picks.tag.length || state.picks.book.length
        || state.when || state.filter.trim());
}

/// 帯を描く。**いくつ選んでいるかを、開かずに言う。**
function drawDrawers() {
    const box = el('drawers');
    box.innerHTML = '';
    for (const [kind, name] of DRAWERS) {
        const b = document.createElement('button');
        // **選んだものを、開かずに読ませる。** 「タグ 1」では何で絞って
        // いるか分からない ── 一つなら名前を、二つ以上なら数を出す。
        const on = kind === 'when' ? state.when : state.picks[kind];
        b.textContent = drawerName(kind, name) + (drawer === kind ? ' ▴' : ' ▾');
        b.className = (on && (kind === 'when' || on.length) ? 'on' : '')
            + (drawer === kind ? ' open' : '');
        b.onclick = () => { drawer = drawer === kind ? null : kind; drawDrawers(); drawDrawer(); };
        box.append(b);
    }
    if (filtering()) {
        const c = document.createElement('button');
        c.className = 'clear';
        c.textContent = 'ぜんぶ外す';
        c.onclick = () => clearFilter();
        box.append(c);
    }
}

/// 引き出しを一つ開く（命令の表から）。
function openDrawer(kind) {
    drawer = kind;
    drawDrawers();
    drawDrawer();
}

/// 帯の一つに出す字。
function drawerName(kind, name) {
    if (kind === 'when') {
        const w = state.when;
        if (!w) return name;
        // **「直した日」は言わない。** 既定のほうで、引き出しにも出ている
        // ── 毎回同じ四文字を読ませるぶん、日付が狭くなる。
        const head = w.which === 'created' ? '作った日 ' : '';
        if (w.from && w.to) return head + dayName(w.from) + '〜' + dayName(w.to);
        return head + (w.from ? dayName(w.from) + ' から' : dayName(w.to) + ' まで');
    }
    const on = state.picks[kind];
    if (!on.length) return name;
    if (on.length === 1) return name + ' ' + on[0].split('/').pop();
    return name + ' ' + on.length;
}

function clearFilter() {
    state.picks = { tag: [], book: [] };
    state.when = null;
    closeFind();
    drawer = null;
    drawDrawers();
    drawDrawer();
    drawList();
}

/// 開いている引き出しの中身。
function drawDrawer() {
    const box = el('drawer');
    if (!drawer) { box.hidden = true; box.innerHTML = ''; return; }
    box.hidden = false;
    box.innerHTML = '';
    if (drawer === 'when') { drawWhen(box); return; }

    const rows = drawer === 'tag'
        ? tagsOf(state.notes).map(([t, n]) => [t, t, n])
        : state.books.map((b) => [b, b, state.notes.filter(
            (n) => n.book === b || (n.book || '').startsWith(b + '/')).length]);
    if (!rows.length) {
        box.innerHTML = '<div class="none">'
            + (drawer === 'tag' ? 'タグがまだありません（ノートに付けると出ます）'
                : 'フォルダがまだありません（左の「フォルダ ＋」から作れます）')
            + '</div>';
        return;
    }
    const head = document.createElement('div');
    head.className = 'sec';
    // **どう重なるかを言う。** タグは全部・フォルダはどれか、で違う ──
    // 言わずに違えば、選んだ数と出る数が合わない理由が分からない。
    head.textContent = drawer === 'tag' ? '押して付け外し（全部付いたものだけ）'
        : '押して付け外し（どれかに入っているもの）';
    box.append(head);
    for (const [value, name, n] of rows) {
        const on = state.picks[drawer].includes(value);
        const b = document.createElement('button');
        b.className = 'it' + (on ? ' on' : '');
        b.innerHTML = '<span class="bx' + (on ? ' on' : '') + '"></span>'
            + '<span>' + escapeHtml(name) + '</span><span class="n">' + n + ' 件</span>';
        b.onclick = () => {
            const at = state.picks[drawer].indexOf(value);
            if (at < 0) state.picks[drawer].push(value);
            else state.picks[drawer].splice(at, 1);
            drawDrawers();
            drawDrawer();
            drawList();
        };
        box.append(b);
    }
}

/* ── こよみ ── */

/// いま見ている月。**開くたびに今月へ戻さない** ── 去年の秋を探している
/// 人は、引き出しを閉じて開くたびに今月へ連れ戻されると探せない。
let calAt = null;
/// 次に押した日を、どちらに入れるか。
let calEdge = 'from';

const monthDays = (y, m) => new Date(y, m + 1, 0).getDate();
const ymd = (y, m, d) => y + '-' + String(m + 1).padStart(2, '0') + '-' + String(d).padStart(2, '0');
/// 日付の呼び名。**今年なら年を言わない** ── 帯は狭いし、たいていは今年。
/// 年が違うときだけ年を出す（`12/31` が去年か今年かは、見て分からない）。
function dayName(s) {
    if (!s) return '';
    const md = s.slice(5).replace('-', '/');
    return s.slice(0, 4) === String(new Date().getFullYear()) ? md : s.slice(0, 4) + '/' + md;
}

/// こよみで、**いつからいつまでを、押して決める。**
///
/// 「7日以内」のような決め打ちは、**去年の秋**を探せない。押した日が範囲の
/// 端になり、片方だけでもよい（「この日から先ぜんぶ」が言えないと、範囲は
/// 使いものにならない）。
function drawWhen(box) {
    const now = new Date();
    if (!calAt) calAt = { y: now.getFullYear(), m: now.getMonth() };

    // どちらの日付で絞るか。
    const which = document.createElement('div');
    which.className = 'pills';
    for (const [key, name] of [['updated', '直した日'], ['created', '作った日']]) {
        const b = document.createElement('button');
        b.textContent = name;
        if ((state.when?.which || whichWhen) === key) b.className = 'on';
        b.onclick = () => {
            whichWhen = key;
            if (state.when) { state.when = { ...state.when, which: key }; drawList(); }
            drawDrawers();
            drawDrawer();
        };
        which.append(b);
    }
    box.append(which);

    // いつから・いつまで。**次に押した日がどちらに入るかを、先に見せる。**
    const span = document.createElement('div');
    span.className = 'span';
    for (const [key, name] of [['from', 'いつから'], ['to', 'いつまで']]) {
        const b = document.createElement('button');
        const at = state.when?.[key];
        b.textContent = at ? dayName(at) : name;
        if (calEdge === key) b.className = 'on';
        b.title = '次に押した日が、ここに入ります';
        b.onclick = () => { calEdge = key; drawDrawer(); };
        span.append(b);
        if (at) {
            const x = document.createElement('button');
            x.className = 'x';
            x.textContent = '✕';
            x.title = name + 'を外す';
            x.onclick = () => setWhen(key, null);
            span.append(x);
        }
        if (key === 'from') {
            span.append(Object.assign(document.createElement('span'), { textContent: '〜' }));
        }
    }
    box.append(span);

    // 月の頭。
    const head = document.createElement('div');
    head.className = 'calhead';
    const back = document.createElement('button');
    back.className = 'mv'; back.textContent = '‹'; back.title = '前の月';
    back.onclick = () => { calAt = stepMonth(calAt, -1); drawDrawer(); };
    const fwd = document.createElement('button');
    fwd.className = 'mv'; fwd.textContent = '›'; fwd.title = '次の月';
    fwd.onclick = () => { calAt = stepMonth(calAt, 1); drawDrawer(); };
    const ttl = document.createElement('span');
    ttl.textContent = calAt.y + '年 ' + (calAt.m + 1) + '月';
    const here = document.createElement('button');
    here.className = 'now'; here.textContent = '今月';
    here.onclick = () => { calAt = { y: now.getFullYear(), m: now.getMonth() }; drawDrawer(); };
    head.append(back, ttl, fwd, here);
    box.append(head);

    // 日。**前の月と次の月のはみ出しも押せる** ── 月末をまたぐ範囲は
    // よくあるのに、押せないと月を送ってから押し直すことになる。
    const cal = document.createElement('div');
    cal.className = 'cal';
    for (const w of ['日', '月', '火', '水', '木', '金', '土']) {
        cal.append(Object.assign(document.createElement('div'), { className: 'wd', textContent: w }));
    }
    const first = new Date(calAt.y, calAt.m, 1).getDay();
    const days = monthDays(calAt.y, calAt.m);
    const prev = stepMonth(calAt, -1);
    const next = stepMonth(calAt, 1);
    const cells = [];
    for (let i = first; i > 0; i--) cells.push([prev, monthDays(prev.y, prev.m) - i + 1, true]);
    for (let d = 1; d <= days; d++) cells.push([calAt, d, false]);
    for (let d = 1; cells.length % 7; d++) cells.push([next, d, true]);

    const today = dayOf(Date.now() / 1000);
    const from = state.when?.from;
    const to = state.when?.to;
    for (const [at, d, out] of cells) {
        const key = ymd(at.y, at.m, d);
        const b = document.createElement('button');
        b.textContent = String(d);
        const edge = key === from || key === to;
        const inside = from && to && key > from && key < to;
        b.className = (out ? 'out ' : '') + (edge ? 'edge ' : (inside ? 'in ' : ''))
            + (key === today ? 'today' : '');
        b.onclick = () => setWhen(calEdge, key);
        cal.append(b);
    }
    box.append(cal);
}

function stepMonth({ y, m }, step) {
    const d = new Date(y, m + step, 1);
    return { y: d.getFullYear(), m: d.getMonth() };
}

/// 日付を一つも選んでいない間の「どちらの日付で」。
let whichWhen = 'updated';

/// 範囲の端を決める。
///
/// **前後が入れ替わったら、黙って入れ替える。** 「9月10日から」を決めた
/// あとに「9月1日まで」を押すのは、たいてい始まりを言い直している ──
/// 0 件の一覧を返して考えさせる場面ではない。
function setWhen(edge, day) {
    const w = { which: state.when?.which || whichWhen,
                from: state.when?.from || null, to: state.when?.to || null };
    w[edge] = day;
    if (w.from && w.to && w.from > w.to) { const t = w.from; w.from = w.to; w.to = t; }
    state.when = (w.from || w.to) ? w : null;
    // 次はもう片方 ── 二度押しで範囲が決まる。
    if (day) calEdge = edge === 'from' ? 'to' : 'from';
    drawDrawers();
    drawDrawer();
    drawList();
}

/* ── 言葉で探す ── */

/// **探す欄は畳んでおく。** 絞り込みの帯と並べて置きっぱなしにすると、
/// 一覧の頭が毎回二段ぶん要る ── 言葉で探すのは、絞るより回数が少ない。
function openFind() {
    el('findbox').hidden = false;
    el('findbtn').classList.add('on');
    el('find').focus();
    el('find').select();
}

/// 閉じるときは**必ず空にする。** 見えない絞り込みが残るのがいちばん悪い
/// ── 一覧が減っている理由が、画面のどこにも書いていないことになる。
function closeFind() {
    el('findbox').hidden = true;
    el('findbtn').classList.remove('on');
    if (!el('find').value) return;
    el('find').value = '';
    state.filter = '';
    state.groups = [];
    drawDrawers();
    drawList();
}

/// 共有の棚にする（`off` で、やめる）。
///
/// **分けるのは amber の仕事ではない。** クラウドのフォルダ共有に任せる ──
/// amber がするのは、そのフォルダに**印を一枚置くこと**だけ。印はフォルダと
/// 一緒に旅をするので、**受け取った人は何も教えなくていい** ── 設定に
/// 書いていた頃は、相手が自分の amber に「これが共有です」と教え直す手が
/// 要り、機種を替えるたびにもう一度要った。
async function cmdShare(folder, off) {
    if (!off) {
        const ok = await askYes('「' + folder + '」を、家族と分ける棚にしますか');
        if (!ok) return;
    }
    const by = off ? '' : await myName();
    if (by === null) return;
    try {
        const r = await ask('share', {
            path: state.root, folder, off: !!off, by, today: today(),
        });
        state.shares = r.shares ? r.shares.map((at) => ({ at, by })) : [];
        await reload({ quiet: true });
        if (off) { say('共有をやめました（ノートはそのままです）'); return; }
        // **二段あることを言う。** amber が印を置いただけでは誰にも届かない
        // ── クラウド側で人に分けるのは、まだ人がやる。
        await askYes('「' + folder + '」を共有の棚にしました。\n\n'
            + 'あとは、このフォルダをクラウド側で家族に分けてください。'
            + '（いま開きますか）')
            ? window.amber.reveal(state.root + '/' + folder)
            : say('あとで、フォルダを右押し →「家族を招待」からでもできます');
    } catch (e) {
        say('できません: ' + why(e));
    }
}

/// 名乗り。**設定画面に置かない** ── 一度しか使わないものを、毎日見る画面に
/// 置く値打ちは無い。**要る瞬間に一度だけ**訊いて、憶えておく。
///
/// 名前は**履歴に付いて回る**（ノートには書かない）ので、相手の amber が
/// 「Taketan が足しました」と言える。書かなくてもいい ── そのときは
/// 「だれか」になるだけで、共有そのものは動く。
async function myName() {
    if (state.me) return state.me;
    const saved = (await window.amber.recall()).me;
    if (saved) { state.me = saved; return saved; }
    const got = await askText('あなたの名前',
        await window.amber.userName() || '',
        '共有したノートに「誰が直したか」を出すために使います（ノートには書きません）');
    if (got === null) return null;
    state.me = got.trim();
    window.amber.remember({ me: state.me });
    return state.me;
}

const today = () => {
    const d = new Date();
    return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0')
        + '-' + String(d.getDate()).padStart(2, '0');
};

/// 開いているノートを、共有の棚へ出し入れする。
///
/// **棚が無ければ作る。** 「共有する」を押した人に、その前に「フォルダを
/// 作る」と「棚にする」を押させない ── 押したいのは共有することであって、
/// フォルダを作ることではない。
async function cmdToShare() {
    if (!state.open) return;
    const back = state.open.shared;
    if (back) {
        // **もといたフォルダへ戻す。** 憶えが無ければ、いままでどおり
        // いちばん上へ ── 共有に入れたのが憶えるより前のノートもある。
        const home = homeOf(state.open);
        const rel = state.open.rel;
        const ok = await askYes('「' + (state.open.title || stem()) + '」を共有から外しますか（'
            + (home ? '「' + home.split('/').pop() + '」へ戻します' : 'いちばん上へ戻します') + '）');
        if (!ok) return;
        await moveNote(home || '', { home, forget: rel });
        return;
    }
    let to = (state.shares[0] || {}).at;
    if (to === undefined) {
        const ok = await askYes('「家族」という棚を作って、そこへ移しますか');
        if (!ok) return;
        const by = await myName();
        if (by === null) return;
        try {
            await ask('share', { path: state.root, folder: '家族', by, today: today() });
            to = '家族';
        } catch (e) { say('できません: ' + why(e)); return; }
    } else {
        const ok = await askYes('「' + (state.open.title || stem()) + '」を「'
            + (to.split('/').pop() || 'ぜんぶ') + '」へ移して共有しますか');
        if (!ok) return;
    }
    await moveNote(to, { from: state.open.book || '' });
}

/// このノートがもといたフォルダ。**憶えていなければ空**（いちばん上へ
/// 戻す、といういままでの形）。
///
/// 憶えていたフォルダが、もう無いことはある（消した・名前を変えた）──
/// **無いところへは戻さない**。移せずに止まるより、いちばん上へ。
function homeOf(note) {
    if (!note || !note.rel) return '';
    const home = (state.came || {})[note.rel];
    return home && state.books.includes(home) ? home : '';
}

/// 共有の棚へ入れる（`to`）／外して戻す（`to` が空ならいちばん上）。
///
/// `opts.from` があれば、**入れる前に居たフォルダを憶える** ── 外すときに
/// そこへ戻せるように。`opts.home` は戻した先で、言葉にするために持つ。
async function moveNote(to, opts) {
    try {
        const r = await ask('move', { path: state.open.path, dir: state.root + (to ? '/' + to : '') });
        // **憶えるのは移せてから。** 移せなかった回の憶えが残ると、次に
        // 外した人が身に覚えのないフォルダへ連れて行かれる。
        try {
            if (to && opts && opts.from !== undefined && r && r.path) {
                const rel = r.path.slice(state.root.length + 1);
                const got = await ask('came', { path: state.root, rel, from: opts.from });
                state.came = (got && got.came) || state.came;
            } else if (opts && opts.forget) {
                const got = await ask('came', { path: state.root, rel: opts.forget, forget: true });
                state.came = (got && got.came) || state.came;
            }
        } catch { /* 憶えられないことで、共有が止まる理由はない */ }
        await reload({ quiet: true });
        if (r && r.path) await openNote(r.path);
        // **行き先では、どちらか決められない。** 戻し先を憶えるようになって
        // から、外すときの `to` も空ではなくなった（もといたフォルダ）──
        // 行き先の有無で分けていたので、外したのに「共有しました」と言った。
        if (!opts || !opts.forget) { say('共有しました'); return; }
        const home = opts.home;
        say(home ? '共有から外して「' + home.split('/').pop() + '」へ戻しました' : '共有から外しました');
    } catch (e) {
        say('移せません: ' + why(e));
    }
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
        say('移せません: ' + why(e));
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
        say('作れません: ' + why(e));
        return null;
    }
}

/// 見張れなかったら、そう言う。
///
/// **黙ると、誰も気づけない。** 見張れないフォルダはある（ネットワーク
/// 越し・権限）── そのとき amber は「外で変わったら教えてもらう」を
/// しないまま動くので、二台で同じフォルダを触っているのに片方が古い
/// まま、が起きても分からない。開かない理由にはならないので、**言って
/// そのまま続ける**。同梱する側が自前の帯で言っていたのは、ここに
/// 返り値が無かったから ── 言うのは、それを知っているこちらの仕事。
function sayIfBlind(got) {
    if (got === true || got === undefined) return;
    const why2 = got && got.why;
    say('このフォルダは見張れません' + (why2 ? '（' + why2 + '）' : '')
        + '。外で変えたら、開き直すと出ます');
}

/// ゴミ箱の無い置き場所（憶えたもの）。
///
/// **一度断られたら、次からは初めからそう言う。** `Documents` が OneDrive
/// へ寄せられている机では、断られるのは一度きりではなく毎回 ── そこで
/// 二度訊き続けると、消すたびに二回答えることになる。分かっているなら、
/// 初めの一回で正直に訊けばいい。
///
/// 置き場所ごとに憶える ── 別のフォルダへ移せば、そちらにはゴミ箱がある。
let noBins = [];
const noBin = () => noBins.includes(state.root);

function markNoBin(yes) {
    const was = noBins.filter((r) => r !== state.root);
    noBins = yes ? [...was, state.root] : was;
    window.amber.remember({ noBins });
}

async function cmdDelete() {
    const name = state.open.title || stem();
    // **ゴミ箱が無いと分かっているなら、初めからそう訊く。**
    // 「ゴミ箱へ入れますか」→「入れられません」→「では消しますか」は、
    // 三度目には嘘をついているのと同じ。
    const first = noBin()
        ? '「' + name + '」を消しますか（ここにはゴミ箱が無いので、戻せません）'
        : '「' + name + '」をゴミ箱へ入れますか';
    // 訊く**前に**分かっていたか ── 断られたあとで訊き直すかどうかは、
    // これで決まる（もう承知をもらっているなら、二度は訊かない）。
    const knew = noBin();
    if (!await askYes(first)) return;
    // **消さずに、ゴミ箱へ。** core の `delete` は消してしまう（電話には
    // ゴミ箱が無いので）。机の上では、戻せないのは強すぎる。
    const path = state.open.path;
    // **真偽でも、理由つきでも受ける。** 同梱している側は `false` を返す
    // ものもあれば、`{ ok:false, why }` を返すものもある ── どちらでも
    // 人には理由を見せる。理由が無いときだけ、無いなりの一行。
    const done = await window.amber.trash(path);
    if (done !== true) {
        // **断られたら、行き止まりにしない。**
        //
        // Windows の会社端末では `Documents` が OneDrive へ寄せられている
        // ことがあり（Known Folder Move）、そこにはゴミ箱が無い ──
        // `Failed to perform delete operation` で断られる。ネットワークの
        // 置き場所も同じ。ここで黙ると、**そのノートは二度と消せない**。
        //
        // **消すのは、訊いてから。** ゴミ箱が「戻せる」ことの担保だった
        // ので、それが無い以上そう言う ── 言わずに消すほうが強すぎる。
        // **小窓は記号を解さない。** ここは `textContent` なので、
        // `**戻せません**` と書くと星が四つそのまま出る（実際に出た）。
        // 強めたいことは、言葉の側で強める。
        const why2 = done && done.why;
        markNoBin(true);
        if (!knew) {
            // 初めて断られた回だけ、ここで訊く。二度目からは、上の一回で
            // 「戻せません」と言ったうえで はい をもらっている。
            const go = await askYes('ゴミ箱へ入れられませんでした'
                + (why2 ? '（' + why2 + '）' : '')
                + '。このまま消しますか。もう戻せません');
            if (!go) return;
        }
        try {
            await ask('delete', { path });
        } catch (e) {
            say('消せません: ' + why(e));
            return;
        }
    } else {
        // 入った ── ここにはゴミ箱がある。前に断られていても、憶えを直す。
        if (noBin()) markNoBin(false);
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
        say('書き出せません: ' + why(e));
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

/// フォルダに付けられる十一色。**core に訊く。**
///
/// 前はここと `Colouring.palette`（電話）に同じ表を書いていて、両方の
/// コメントに「同じ並び」と書いてあった ── それでも**十一色のうち六色が
/// ずれていた**。電話で付けた青が、Mac では少し違う青で出ていた。
/// 写しを持てば、いつかずれる。
let PALETTE = [];

async function loadPalette() {
    try {
        const r = await ask('palette', {});
        PALETTE = (r.colors || []).map((c) => [c.hex, c.name]);
    } catch {
        // 訊けなくても色は付けられなくていい ── 窓が開かない理由にはしない。
        PALETTE = [];
    }
}

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
        say('色を付けられません: ' + why(e));
    }
}

/// バックアップ。**範囲を訊く。**
///
/// 長いあいだ「すべて」を決め打ちで渡していて、四つあることはエンジンしか
/// 知らなかった ── 電話には四つとも出ていたので、**同じアプリで電話に
/// できて窓にできない**ことが一つあった。
///
/// 四つ ── すべて／フォルダ一つ／タグの付いたもの／このノート一枚。
/// zip の名前は何が入っているかを言う（`仕事-2026-09-06.zip`）── 名前が
/// `backup.zip` ばかりのフォルダは、「どれがどれか」という一つの問いになる。
async function cmdBackup() {
    const here = state.dest.kind === 'book' ? state.dest.what : '';
    const items = [
        { name: 'すべて', sub: 'ノートも絵も、まるごと一つに', value: ['all', ''] },
    ];
    for (const b of state.books || []) {
        items.push({ name: 'フォルダ: ' + b, sub: b === here ? 'いま見ているところ' : '', value: ['book', b] });
    }
    // タグは使われている順（`tagsOf`）。多いものから並ぶので、
    // 取っておきたいまとまりはたいてい上のほうに居る。
    for (const [t, n] of tagsOf(state.notes).slice(0, 20)) {
        items.push({ name: 'タグ: #' + t, sub: n + ' 件・フォルダをまたいで集めます', value: ['tag', t] });
    }
    if (state.open) {
        items.push({ name: 'このノート一枚', sub: shortPath(state.open.path), value: ['note', state.open.path] });
    }
    const pick = await askPick('どこまで取っておきますか', items,
        '一つの zip にまとめます。いまあるノートは動きません');
    if (pick === null) return;
    const [scope, what] = pick;
    const into = await window.amber.pickFolder();
    if (!into) return;
    try {
        const r = await ask('backup', { path: state.root, scope, what, into });
        say(r.files + ' 件を保存しました: ' + shortPath(r.path || into));
    } catch (e) {
        say('保存できません: ' + why(e));
    }
}

/// よそにある .md を、ノート帳へ取り込む。
///
/// **窓には無かった。** 電話には初めからあり、窓には「バックアップから
/// 戻す」しかなかった ── zip でなければ入れる道が無く、ほかのアプリから
/// 書き出した .md を持ってきた人は、Finder でフォルダを開いて自分で写す
/// しかなかった（そのフォルダがどこかも、amber は言わない）。
///
/// **上書きしない・元は動かさない。** 同じ名前があれば `週報-2.md` に
/// して両方残す（判断は core の `notebook::bring`。窓と電話で二組書くと、
/// 同じノートが端末によって別の名前で入る）。名前を変えたぶんは数えて
/// 言う ── 言わないと、開いたノートが「さっき取り込んだやつ」なのか
/// 「前からあったやつ」なのか見分けが付かない。
async function cmdBring() {
    const files = await window.amber.pickFiles([{ name: 'ノート', extensions: ['md', 'markdown', 'txt'] }]);
    if (!files || !files.length) return;
    try {
        const r = await ask('bring', { files, to: state.root });
        await reload({});
        // **入らなかった数も言う。** 十本選んで八本入ったとき、黙って
        // いると人は八本しか選ばなかったと思う ── 気づくのは、あとで
        // 探しても出てこない日。
        const re = r.renamed ? '（' + r.renamed + ' 件は名前を変えました）' : '';
        const no = r.failed ? '。' + r.failed + ' 件は入れられませんでした' : '';
        say(r.put + ' 件を取り込みました' + re + no);
    } catch (e) {
        say('取り込めません: ' + why(e));
    }
}

/// バックアップから戻す。
///
/// **いまあるものは消さない。** 戻すのは「消えたものを取り返す」ためで、
/// いま書いているものを捨てていいという意味ではない ── 同じ名前のものが
/// あれば**いまのほうを残し**、何枚避けたかを言う。言わないと「戻した
/// つもりで戻っていない」に見える。
async function cmdRestore() {
    const zip = await window.amber.pickFile([{ name: 'バックアップ', extensions: ['zip'] }]);
    if (!zip) return;
    const go = await askYes('「' + shortPath(zip) + '」から戻しますか');
    if (!go) return;
    try {
        const r = await ask('restore', { zip, to: state.root });
        await reload({});
        const kept = r.kept ? '（' + r.kept + ' 件は、いまのを残しました）' : '';
        say(r.put + ' 件を戻しました' + kept);
    } catch (e) {
        say('戻せません: ' + why(e));
    }
}

async function cmdRoot() {
    // **いまどこかを先に見せる。** 「amber のディレクトリはどうやって
    // 決めるのか」が分からなかったのは、決める場所が無かったからではなく、
    // **いまどこを見ているのかが画面のどこにも出ていなかった**から。
    //
    // **クラウドは名前で選ばせる。** どのサービスも机の上ではただの
    // フォルダなので、amber は同期の仕組みを一つも知らなくていい ──
    // けれど `~/Library/Mobile Documents/com~apple~CloudDocs` を覚えて
    // いる人はいない。入っているものだけ並べる。
    let found = [];
    try { found = await window.amber.clouds(); } catch { /* 一つも無い機械 */ }
    const here = found.find((c) => state.root.startsWith(c.dir));
    const items = [
        ...found.map((c) => ({
            name: c.name,
            sub: c.dir === (here || {}).dir ? 'いまここ' : 'この中の「amber」に置く',
            value: c.dir,
        })),
        { name: '別の場所を選ぶ', sub: 'フォルダを一つ選びます', value: ' pick' },
    ];
    const go = await askPick('ノートの置き場所', items,
        'いま: ' + shortPath(state.root) + (here ? '（' + here.name + '）' : ''));
    if (go === null) return;

    let dir;
    if (go === ' pick') {
        dir = await window.amber.pickFolder();
        if (!dir) return;
    } else {
        // クラウドの直下には置かない ── 同期フォルダの根っこにノートを
        // ばら撒くと、ほかの物と混ざって二度と分けられない。
        dir = go + '/amber';
        try {
            await ask('place', { dir });
        } catch (e) {
            say('作れません: ' + why(e));
            return;
        }
    }
    if (dir === state.root) return;

    // **いままでのノートは、ひとりでには付いてこない。**
    // 新しいフォルダは空のフォルダで、そうと知らずに移した人は、書いた
    // ものが全部見えなくなったところに立たされる（電話は前から訊いて
    // いる ── 窓だけ訊いていなかった）。
    const had = state.notes.length;
    const was = state.root;
    if (had > 0 && await askYes('いままでの ' + had + ' 件を、新しい場所へ移しますか')) {
        try {
            // **数えるのは人が数えるもの。** `migrate` が返すのは動かした
            // ファイルの数（絵も履歴も `.amber` も入る）で、6 件のノートが
            // 「14 件を移しました」になる ── 何が 14 なのか誰も分からない。
            await ask('migrate', { from: was, to: dir });
            say('ノート ' + had + ' 件を、絵と履歴ごと移しました');
        } catch (e) {
            // **移せなくても、置き場所は変えない。** 半分だけ移った状態で
            // 向こうを見せると、残りが消えたようにしか見えない。
            say('移せません: ' + why(e));
            return;
        }
    }

    state.root = dir;
    window.amber.remember({ root: dir });
    sayIfBlind(await window.amber.watch(dir));
    state.open = null;
    applyView();
    await reload({});
    say('置き場所を変えました: ' + shortPath(dir));
}

/// いま動いている amber の身元。
///
/// **不具合が人づてに回ってくるから要る。** amber の画面は crmaine の中でも
/// 動いていて、そちらは社内の Windows 端末に配られる ── 戻ってくる報せは
/// たいてい「amber が変です」だけで、どの版のどのエンジンかは書いていない。
/// 押せば読める場所に三つ（版・エンジン・置き場所）を出しておけば、
/// 伝えるほうも訊くほうも一往復で済む。
///
/// **画面とエンジンを別々に出す。** 同梱するときは実行ファイルだけ差し替え
/// られるので、この二つはずれうる ── ずれているのが原因のときに、
/// 一つの数字しか出していないと辿れない。
async function cmdAbout() {
    let engine = '（答えません）';
    try {
        const r = await ask('version', {});
        engine = 'amber-server ' + (r.amber || '?');
    } catch (e) {
        engine = '答えません: ' + why(e);
    }
    // **名札が題を兼ねる。** 上に「ambər について」と書いて、その下に
    // もう一度 ambər と出すのは、同じことを二度言っているだけ。
    await askPick('', [
        { name: '画面', sub: 'ambər ' + (await window.amber.appVersion() || '?'), value: null },
        { name: 'エンジン', sub: engine, value: null },
        { name: 'ノートの置き場所', sub: state.root || '（まだ決めていません）', value: null },
    // **`bare` は渡さない。** `false` を渡すと `bare ?? few` の `??` が
    // それを素通しし（`??` が拾うのは null と undefined だけ）、三つしか
    // 無い一覧に「絞り込む」の欄が出る ── 三つを絞り込む人はいない。
    ], '不具合を伝えるときは、この三つを添えてください', undefined,
       'Advanced Markdown Browser & Editor for Readability');
}

/* ── 前の姿 ── */

/// 見本のノートを、いまの置き場所に置く。
///
/// **初回に置けなかった人のための道。** 自動で置くのは、まだ一本も
/// ノートが無いときの一度きり ── 既にノートがある人のフォルダに三枚
/// 落とすと、それはただの散らかし。それでも「入れてくれ」と言える場所が
/// 要る（電話の設定にも同じものがある）。
async function cmdWelcome() {
    const go = await askYes('見本のノートを、いまの置き場所に入れますか');
    if (!go) return;
    try {
        const r = await window.amber.welcome(state.root);
        await reload({});
        say(r.put ? r.put + ' 枚置きました' : 'もう入っています（同じ名前は飛ばしました）');
    } catch (e) {
        say('置けません: ' + why(e));
    }
}

/// ⌘S ── **保存ではなく「ここを残す」。**
///
/// amber は打鍵の 0.9 秒後に書いているので、保存という操作が無い。それでも
/// 人は反射で ⌘S を押す ── 何も起きないと「保存されたのか」と不安になり、
/// 「自動保存です」と出すのは正直だが役に立たない。
///
/// 押すと、いまの姿に**消えない印**が付く。五十世代・三十日の勘定から
/// 外れるので、「ここまでは効いている状態」を自分で刻める ── 自動保存では
/// 作れない、人にしか分からない区切り。
async function cmdKeepNow() {
    if (!state.open) return;
    if (state.dirty) await save();
    try {
        const r = await ask('keep', {
            root: state.root, path: state.open.path, gap: 0, force: true, kept: true,
        });
        say(r.stamp ? 'いまのバージョンを残しました（これは消えません）' : 'このバージョンはもう残してあります');
    } catch (e) {
        say('残せません: ' + why(e));
    }
}

/// 履歴を見る。ノートでもフォルダでもよい。
///
/// **戻すのは、いまを捨てることではない。** 戻す前にいまの姿を一世代
/// 残すので、戻しすぎても戻れる ── これが無いと「戻す」は取り返しの
/// つかない操作になり、押すのが怖くなる。
async function cmdHistory(at, isBook) {
    const path = at || (state.open && state.open.path);
    if (!path) { say('ノートかフォルダを選んでください'); return; }
    let r;
    try {
        r = await ask('history', { root: state.root, path });
    } catch (e) {
        say('履歴を読めません: ' + why(e));
        return;
    }
    const rows = r.versions || [];
    if (!rows.length) {
        await askPick('過去バージョン', [{ name: 'まだありません', value: null }],
            '書いて手を止めるたびに、一つずつ残ります（' + r.gens + ' 世代・'
            + r.days + ' 日ぶん）');
        return;
    }
    const items = rows.map((v) => ({
        name: v.when + (v.kept ? '  ★' : ''),
        // フォルダを訊いたときは、どのノートのものかを言う ── 言わないと
        // まとめた一覧が読めない。
        sub: (isBook ? v.note + '  ' : '') + Math.round(v.bytes / 10) / 100 + ' KB',
        value: v,
    }));
    const pick = await askPick('過去バージョン', items,
        '選ぶと中身を見られます（' + r.gens + ' 世代・' + r.days + ' 日ぶん残ります）');
    if (!pick) return;
    const note = isBook ? state.root + '/' + pick.note : path;
    let old;
    try {
        old = (await ask('oldtext', { root: state.root, path: note, stamp: pick.stamp })).text;
    } catch (e) {
        say('読めません: ' + why(e));
        return;
    }
    const go = await askPick(pick.when + ' の姿', [
        { name: 'この姿を見る', sub: '読むだけ。いまのノートは動きません', value: 'peek' },
        { name: 'この姿に戻す', sub: 'いまの姿も一世代として残ります', value: 'back' },
        { name: pick.kept ? '「残す」の印を外す' : 'この姿に「残す」の印を付ける',
          sub: '印の付いた姿は、古くなっても消えません', value: 'mark' },
    ], shortPath(note) + '  ·  ' + old.length + ' 字');
    if (go === null) return;
    if (go === 'mark') {
        await ask('keepmark', { root: state.root, path: note, stamp: pick.stamp, kept: !pick.kept });
        say(pick.kept ? '印を外しました' : 'この姿は消えなくなりました');
        return;
    }
    if (go === 'peek') {
        await openGuestText(pick.when + ' の姿', old);
        return;
    }
    // **戻す前に、いまを一世代残す。** 戻しすぎても戻れるように。
    await ask('keep', { root: state.root, path: note, gap: 0, force: true });
    await ask('write', { path: note, text: old, force: true });
    await reload({});
    if (state.open && state.open.path === note) await openNote(note);
    say(pick.when + ' の姿に戻しました（いまの姿も残してあります）');
}

/// 前の姿を、読むだけの一本として開く。
///
/// **いまのノートを書き換えない。** 「見てから決める」ができないと、
/// 戻すかどうかを名前と日付だけで決めることになる。
async function openGuestText(title, text) {
    const at = await window.amber.scratch(title + '.md', text);
    if (!at) return;
    await openGuest(at);
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

/// 名前を、名前として見せる一枚。
///
/// **ここだけ HTML を通す。** 題（`.hd`）と添え書き（`.foot`）は
/// `textContent` のまま ── あそこにはノートの名前が入る（「ゴミ箱へ
/// 入れる」など）ので、HTML にすると人の書いた字が印になる道が開く。
/// 名札はこちらが書いた決め打ちなので、そこだけ通してよい。
const BRAND = 'amb<span class="s">ə</span>r';

/// 一度に描く行の上限。**絞る数ではなく、描く数。**
const SHEET_ROWS = 200;

function sheet({ title, value, placeholder, items, foot, bare, brand }) {
    closeSheet(null);
    const veil = el('veil');
    const input = veil.querySelector('input');
    // **四つまでのときは、字を打つ欄を見せない。**
    //
    // 四つを絞り込む人はいない。それどころか、「はい／やめる」の二択に
    // 欄が出ていると「何か打つものがある」に見えて、タグを外すだけの返事
    // で人が止まる（実際に止まった ── ゴミ箱へ入れるときも同じだった）。
    //
    // 欄は残す ── 上下と Enter を受けているのはここなので、消すと鍵盤で
    // 選べなくなる。見せないだけ。
    const few = items ? items.length <= 4 : false;
    veil.querySelector('#sheet').classList.toggle('bare', bare ?? few);
    const list = veil.querySelector('.items');
    const plate = veil.querySelector('.brand');
    plate.hidden = !brand;
    plate.innerHTML = brand
        ? '<div class="nm">' + BRAND + '</div><div class="ex">' + escapeHtml(brand) + '</div>'
        : '';
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
        // **絞るのは全部から、描くのは頭だけ。**
        //
        // 「何をしますか」がノートも行き先も抱えるようになった（依頼 414）
        // ので、ここに来る数は人のノートの数になった ── 千本のノートを
        // 持っている人は、一打ごとに千行を組み直すことになる。
        // **探す範囲は狭めない**（打てば下のものも上がってくる）。
        const shown = hit.slice(0, SHEET_ROWS);
        // **見出しの行は、選び目が止まらない。** 押しても何も起きない行に
        // 選び目が乗っていると、開いた瞬間の Enter が空振りする（「何を
        // しますか」の一行目がまさにそれだった）。
        const pickable = shown.filter((i) => !i.head);
        at = Math.min(at, Math.max(pickable.length - 1, 0));
        let n = 0;
        list.innerHTML = shown.map((i) => {
            if (i.head) {
                return '<div class="gp"><span>' + escapeHtml(i.name) + '</span>'
                    + (i.sub ? '<span class="sub">' + escapeHtml(i.sub) + '</span>' : '')
                    + '</div>';
            }
            const k = n++;
            return '<div class="it' + (k === at ? ' on' : '') + '" data-n="' + k + '">'
                + '<span>' + escapeHtml(i.name) + '</span>'
                + (i.sub ? '<span class="sub">' + escapeHtml(i.sub) + '</span>' : '')
                + (i.key ? '<span class="k">' + escapeHtml(keyText(i.key)) + '</span>' : '')
                + '</div>';
        }).join('')
            // **隠したことを言う。** 黙って切ると「無い」に見える。
            + (hit.length > shown.length
                ? '<div class="more">ほか ' + (hit.length - shown.length)
                  + ' 件 ── 打つと絞れます</div>'
                : '');
        for (const row of list.querySelectorAll('.it')) {
            row.onclick = () => closeSheet(pickable[Number(row.dataset.n)].value);
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
            if (isEnter(e)) { e.preventDefault(); closeSheet(input.value); }
            return;
        }
        // **矢印と Enter が歩くのは、描いてある行。** 描いていない行へ
        // 下りると、選び目が画面の外へ消えたように見える（描く数には
        // 上限がある ── `SHEET_ROWS`）。
        const hit = items.filter((i) => {
            const q = input.value.trim().toLowerCase();
            return !q || (i.name + ' ' + (i.sub || '')).toLowerCase().includes(q);
        }).slice(0, SHEET_ROWS).filter((i) => !i.head);
        if (e.code === 'ArrowDown') { e.preventDefault(); at = Math.min(at + 1, hit.length - 1); draw(); }
        else if (e.code === 'ArrowUp') { e.preventDefault(); at = Math.max(at - 1, 0); draw(); }
        else if (isEnter(e)) {
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
/// `bare` が真なら、字を打つ欄を見せない（押して選ぶだけの一覧）。
const askPick = (title, items, foot, bare, brand) =>
    sheet({ title, items, placeholder: '絞り込む', foot, bare, brand });

/// はい／いいえ。**取り返しのつかないものだけに使う。**
function askYes(title) {
    return sheet({ title, bare: true, items: [
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
    if (MAC) document.body.classList.add('mac');
    // **釦の吹き出しに書いてある鍵も、土台の言葉に。**
    //
    // `index.html` に `title="言葉で探す（⌘F）"` と直に書いてあるものが
    // ある ── Windows には `⌘` という鍵が無いので、**押しようがない案内**が
    // 出ていた。一度だけ舐めて言い換える（`keyText` と同じ一本を通す）。
    if (!MAC) {
        for (const n of document.querySelectorAll('[title]')) {
            const t = n.getAttribute('title');
            if (!/[⌘⌃⇧⌥]/.test(t)) continue;
            n.setAttribute('title', t.replace(/[⌘⌃⇧⌥][^\s（）()]*/g, (m) => keyText(m)));
        }
    }
    el('blankmark').innerHTML = mark(54);
    const saved = await window.amber.recall();
    state.root = saved.root;
    noBins = Array.isArray(saved.noBins) ? saved.noBins : [];
    incomings = (saved.incomings && typeof saved.incomings === 'object') ? saved.incomings : {};
    // 外から動いたら教えてもらう ── 同じフォルダを二つの端末で触るのが
    // このアプリの前提なのに、開き直すまで出てこなかった。
    sayIfBlind(await window.amber.watch(saved.root));
    await loadPalette();
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
    if (saved.listOff) { listOff = true; document.body.classList.add('nolist'); }
    // エディタはまだ無い ── 開いたときに入る（`makeEditor` の末尾）。
    if (saved.vim) vimOn = true;
    if (Array.isArray(saved.faces)) usedFaces = saved.faces.slice(0, 24);
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
            drawDrawers();
            drawList();
        }, 150);
    };

    await reload();
    // **開いていた机に戻る。** 電話が憶えているのと同じ ── 閉じて開いたら
    // 一本きりに戻るのでは、机として使えない。
    //
    // 憶えた道のうち**いま棚にあるものだけ**を並べる（消えた・移した本を
    // 並べると、押せないタブが残る）。中身はまだ読まない ── 押されたぶん
    // だけ読めば足りる（`restoreTab` が空を返せば、そこで読む）。
    const back = (saved.tabs || []).filter((at) => state.notes.some((n) => n.path === at));
    if (back.length) tabs = back.map((at) => ({ path: at, keep: null }));
    const first = (saved.open && back.includes(saved.open)) ? saved.open : back[0];
    if (first) { showing = first; await openNote(first, { keep: true }); }
    else if (saved.open && state.notes.some((n) => n.path === saved.open)) await openNote(saved.open);

    // **保存しかけたまま閉じない。**
    window.addEventListener('beforeunload', () => { if (state.dirty) save(); });
})();
