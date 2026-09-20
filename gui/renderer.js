'use strict';
// amber のウィンドウ、三列。左が行き先、真ん中がノート、右が中身。
//
// **判断はここに書かない。** 題をどう決めるか、絞り込みの AND / OR をどう
// 解くか、チェックをどう切り替えるかは `amber-core` が答える ── iPhone も
// 同じコードに訊いている。ここに写した瞬間、**同じ操作なのに Mac と iPhone で
// 結果が違う**が、一度の編集で作れてしまう。

const el = (id) => document.getElementById(id);

/// Enter が押されたか。**キーボードの右にもう一つある。**
///
/// フルサイズのキーボード（会社の机にたいてい載っている）は、数字の並びの脇の
/// Enter を `NumpadEnter` として送る ── `code === 'Enter'` だけを見ていた
/// ので、そこを押した人には**セルの続きが出なかった**。
///
/// 点と番号は「出ていた」ので、なおさら分からない ── あれは**画面が
/// 勝手にやっている**（`contenteditable` の既定）。こちらの手当てが要る
/// のはセルだけで、手当てが飛ぶと**セルだけが出ない**という形で現れる。
const isEnter = (e) => e.code === 'Enter' || e.code === 'NumpadEnter';

/// **そのキーは、いま IME が食べているか**（依頼 560）。
///
/// `keyCode === 229` は「IME が処理中」の古い合図だが、**Windows では
/// 変換していなくても付いてくることがある** ── 日本語入力を載せているだけで、
/// 確定済みの行末で押した `⇧Enter` が 229 で届く。これを「変換中」と見て
/// 手当てを飛ばすと、既定の `<br>` が 1 つだけ入り、**段落の末尾の `<br>`
/// 1 つは行にならない** ── 会社の Windows で「二回押さないと改行しない」
/// として出た（本人・2026-09-14）。手元で `keyCode 229` を送って再現した。
///
/// **変換が本当に開いているかは、こちらが知っている**（`composing` は
/// `compositionstart`／`end` で立てている）── 229 はその裏付けがあるときだけ
/// 信じる。`isComposing` は仕様どおりの合図なので、こちらは常に信じる。
const imeBusy = (e) => e.isComposing || (e.keyCode === 229 && composing);

/// パスの最後の一片。**区切りは `/` だけではない。**
///
/// core が返すパスは土台のもので、Windows では `C:\Users\…\ノート.md`。
/// `split('/')` で切ると**道まるごと**が名前になる ── 書き出すと
/// `C：Users…ノート.md` のような名前のファイルができ、ゴミ箱へ入れる
/// 確認にもパスが出る。画像の在りかを求めるほうは、切り落とせずに `''` に
/// なって**画像が 1 つも出なくなる**（Windows でだけ）。
const baseOf = (at) => String(at || '').split(/[/\\]/).pop();
/// その一片を落とした残り（末尾の区切りは残す ── 後ろに名前を繋ぐため）。
const dirOf = (at) => String(at || '').replace(/[^/\\]*$/, '');

/// ファイルのパスを、画像に渡せる `file:` の形にする。
///
/// **Windows のパスは、そのままでは URL にならない。**
/// `'file://' + encodeURI('C:\\Users\\…\\画像.png')` は
/// `file://C:%5CUsers%5C…` になる ── `C:` が**環境の名前**として読まれ、
/// 円記号は `%5C` に化ける。会社の端末で「この画像は読めません」と出たのは
/// これ（mac のパスは `/` で始まるので、たまたま斜線が三本になっていた）。
///
/// 正しい形は `file:///C:/Users/…`。円記号を `/` に直し、頭に一本足す。
/// **`\\\\server\\share` は別**（環境の名前が入るので、斜線は二本のまま）
/// ── 会社の保存場所はネットワークの向こうのことがある。
const fileURL = (at) => {
    const p = String(at || '').replace(/\\/g, '/');
    if (p.startsWith('//')) return 'file:' + encodeURI(p);
    return 'file://' + encodeURI(p.startsWith('/') ? p : '/' + p);
};
/// このデスクトップ版が乗っている土台。**画面に出す言葉が、ここで変わる。**
const MAC = typeof navigator !== 'undefined' && /Mac/.test(navigator.userAgent);

/// キーの並びを、その土台の言葉で。
///
/// **表には mac の記号で書いておく**（`⌘⇧O`）── 一つの書き方に揃えておけば、
/// 増やす人が迷わない。出すときにここで言い換える。
///
/// Windows で `⌘` が出ていた（本人が会社の端末で見た・2026-09-08）。
/// あちらに `⌘` というキーは無いので、**押しようがない案内**が出ていたことになる。
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
    // **記号のあとの言葉は、`+` で繋がない。** 「⌥ 押し」はキーの並びでは
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
/// というラベルを見る ── 自分が何を間違えたのか、どこにも書いていない。
/// 直すパスも無い（直すのは同梱している側で、押した人ではない）。
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
    /// いちばん目の保存ディレクトリ（`places[0].dir`）。**一つしか無かった頃の
    /// 名残り** ── 「どこか一つ」で足りるところ（サンプルを置く・テンプレート）が使う。
    /// ノートを扱うところは `rootOf(path)` で、そのノートの保存ディレクトリを引く。
    root: '',
    /// 保存ディレクトリ（依頼 511・本人が決めた乙・2026-09-12）。**いくつでも。**
    /// `[{ name, dir, sync: 'drive'|'none', at }]` ── `at` は Drive の上での
    /// 置き場所（'' は `ambər` の直下、それ以外は `ambər/<at>/`）。名前を変えても
    /// `at` は変えない（向こうのフォルダを動かさないため）。
    places: [],
    /// 読めなかった保存ディレクトリ（`dir` → 理由）。外付けを抜いた・消した。
    placeTrouble: {},
    notes: [],
    /// フォルダ。**絶対の道**（保存ディレクトリが複数になってから ── 相対だと
    /// 二つの保存ディレクトリにある同じ名前の「仕事」が見分けられない）。
    books: [],
    /// 錠のかかったフォルダ（絶対の道・依頼 629）。中のノートとサブフォルダに効く。
    locks: [],
    stars: [],
    /// 期間の絞り込み（`{ which: 'updated'|'created', from, to }`）。
    /// `from` / `to` は `YYYY-MM-DD` か null（片方だけでもよい ── 「この日
    /// から先ぜんぶ」「この日まで」を言えないと、範囲が使いものにならない）。
    when: null,
    /// まだ落ちてきていないノート（クラウドが札だけ置いている）。
    waiting: [],
    /// あなたの名乗り。共有したノートの「誰が」に使う（ノートには書かない）。
    me: '',
    /// グループと分けてある棚。**一つとは限らない** ── マークはフォルダごとに置く
    /// ので、グループ用と仕事用が両方あっていい。`[{ at, by }]`。
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

/* ── 保存ディレクトリ（依頼 511） ──
 *
 * **パスは絶対で持つ。** フォルダ（`state.books`・`state.dest.what`・ノートの
 * `book`）はぜんぶ保存ディレクトリからの絶対の道 ── 相対にしておくと、二つの
 * 保存ディレクトリにある同じ名前の「仕事」が見分けられない。**相対が要るのは
 * core に渡す瞬間だけ**（`relOf`）。
 */

/// そのパスが入っている保存ディレクトリ（無ければ null）。長いほうが勝つ ──
/// 保存ディレクトリの中に保存ディレクトリがあっても、近いほうを引く。
function placeOf(path) {
    if (!path) return null;
    let hit = null;
    for (const p of state.places) {
        if (path !== p.dir && !path.startsWith(p.dir + '/')) continue;
        if (!hit || p.dir.length > hit.dir.length) hit = p;
    }
    return hit;
}

/// そのパスの保存ディレクトリ。分からなければいちばん目。
const rootOf = (path) => { const p = placeOf(path); return p ? p.dir : state.root; };

/// 保存ディレクトリからの相対の道（保存ディレクトリそのものなら ''）。
function relOf(path) {
    const root = rootOf(path);
    if (path === root) return '';
    return path.startsWith(root + '/') ? path.slice(root.length + 1) : path;
}

const manyPlaces = () => state.places.length > 1;

/// パスの、いちばん子の名前。**`/` と `\\` の両方で切る。**
///
/// `split('/')` だけだと、Windows の道（`C:\Users\…\OneNote`）は一つも
/// 切れず、**まるごと返る** ── 左の帯にパスがそのまま並んで読めなくなる
/// （現場で出た・依頼 596）。ここはパスを相手にするところなので、
/// 区切りは環境のものに合わせない。
function leafOf(path) {
    const s = String(path || '');
    const at = Math.max(s.lastIndexOf('/'), s.lastIndexOf('\\'));
    return at < 0 ? s : s.slice(at + 1);
}

/// フォルダの見せ名。保存ディレクトリそのものなら、その名前。
function bookName(dir) {
    const p = placeOf(dir);
    if (p && p.dir === dir) return p.name;
    return leafOf(dir);
}

/// フォルダを言葉にする（帯・ダイアログ）。二つ以上あるときは保存ディレクトリの名前を
/// 頭に付ける ── 「仕事」だけでは、どちらの仕事か分からない。
function bookLabel(dir) {
    const p = placeOf(dir);
    const rel = relOf(dir);
    if (!p) return rel || dir;
    if (!rel) return manyPlaces() ? p.name + '（トップページ）' : '（トップページ）';
    return (manyPlaces() ? p.name + ' › ' : '') + rel;
}

/// 「〜へ」の形（帯の一言）。いちばん上なら「いちばん上へ」、フォルダなら「「仕事」へ」。
function dirWords(dir) {
    const p = placeOf(dir);
    if (p && p.dir === dir) return (manyPlaces() ? '「' + p.name + '」の' : '') + 'トップページへ';
    return '「' + bookLabel(dir) + '」へ';
}

/// 新しいノートが出来る場所。**いま見ているフォルダ**（フォルダも保存
/// ディレクトリも見ていなければ、いちばん目のいちばん上）。
function hereDir() {
    const { kind, what } = state.dest;
    if ((kind === 'book' || kind === 'place') && what) return what;
    return state.root;
}

/// 移す先の一覧（`askPick` の items）。保存ディレクトリごとに、いちばん上と
/// その中のフォルダ。値はぜんぶ絶対の道。
function bookChoices() {
    const out = [];
    for (const p of state.places) {
        out.push({ name: manyPlaces() ? p.name + '（トップページ）' : '（トップページ）', value: p.dir });
        for (const b of state.books) {
            if (rootOf(b) !== p.dir) continue;
            out.push({ name: (manyPlaces() ? p.name + ' › ' : '') + relOf(b), value: b });
        }
    }
    return out;
}

/// ノートを移す。**同じ保存ディレクトリの中なら `root` を渡す**（core が画像を
/// 連れて行き、同期に「名前が変わった」と憶えさせる）。別の保存ディレクトリへ
/// 渡るときは渡さない ── 向こうの帳画面に、外のパスを書かせない（同期は
/// 片方で消え・片方で新しく上がる、として運ぶ）。
async function moveOp(path, dir) {
    const same = rootOf(path) === rootOf(dir);
    const r = await ask('move', { path, dir, root: same ? rootOf(path) : '' });
    // **移したあとの手当ては、改名と同じ**（2026-09-20 に総ざらいで見つけた）。
    //
    // パスで持っているもの（タブ・机にしまってあるノート・たどった道・
    // 開いているノート）を繋ぎ直す。`afterRename` はこれまで改名にしか
    // 繋がっていなかったので、**共有に入れる・外す・フォルダへ移す の
    // あと、机にしまってあるタブの中のノートだけが古いパスを指したまま**に
    // なっていた。そのタブへ戻ると `restoreTab` が古いノートを
    // `state.open` に戻すので、そこから先の保存は**もう無いパス**へ行き、
    // 「No such file」で落ちる ── 打った字はファイルに入らず、同期は
    // 「変わっていません」と言う。総ざらいの同期の段が 7 つ落ちていたのは、
    // 元をたどるとこれ一つだった。
    if (r && r.path && r.path !== path) afterRename(path, r.path);
    return r;
}

/// 開いているノートの保存ディレクトリ（開いていなければいちばん目）。
const openRoot = () => (state.open ? rootOf(state.open.path) : state.root);

/// 憶える（`root` も一緒に ── main.js のサンプル置きなど、一つで足りるところが読む）。
function savePlaces() {
    state.root = state.places.length ? state.places[0].dir : state.root;
    window.amber.remember({ places: state.places, root: state.root });
}

/// 見張り直す（ぜんぶの保存ディレクトリ）。
async function rewatch() {
    sayIfBlind(await window.amber.watch(state.places.map((p) => p.dir)));
}

/* ── 印 ── */

/// アプリの中の印。**アプリのアイコンそのもの**を小さくして出す。
///
/// 前はここに葉（案 S4）を SVG で写して描いていた。写しは `packaging/amber.svg`・
/// `packaging/amber.py`・`ios/Cian/Writing.swift` にもあり、四か所が同じ形かを
/// `agree()` が見張っていた ── それでも**アイコンを替えた日に、中の印だけが
/// 前の画像のまま残った**。見張れるのは「四つの写しが揃っているか」であって、
/// 「アイコンと同じか」ではなかった。同じ画像を渡せば、ずれようがない。
///
/// 128px のもので足りる ── いちばん大きい使い方（54）の2倍と、iPhone の
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

/// 焦点がいまノートの中（表示画面か編集画面）にあるか。
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
    // **カレンダーを出しているあいだは、カレンダーだけが光る**（本人・2026-09-12）──
    // 行き先（すべてのノート など）の光りは消す。閉じれば戻る。
    const on = (kind, what) => !calOn && state.dest.kind === kind && state.dest.what === what;
    const rows = [];
    // 名前は決め打ちなので、`BRAND` をそのまま置く（人の書いた文字は入らない）。
    rows.push('<div id="railtop"><span class="wm">' + BRAND + '</span></div>');
    // 「＋」は全角の空白で離していた ── 文字と記号のあいだが不揃いになる。
    // マークは琥珀の丸の中に描く（同じ太さ・同じ大きさで、文字と揃う）。
    // **カレンダーは、いちばん上に一つ**（本人・2026-09-12「ノートと切り離して最上段に」）。
    // ここに置かないと、パレットを知っている人しか辿り着けない（依頼 454・467）。
    // **開いているあいだは、ここが光る。** 行き先は変えない（一覧はそのまま）が、
    // いま何を見ているかは画面が答えるべき（依頼 477）。
    rows.push('<div class="dest' + (calOn ? ' on' : '')
        + '" data-kind="cal" data-what="" data-depth="0">'
        + '<svg class="mk" viewBox="0 0 16 16" aria-hidden="true">'
        + '<g fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"'
        + ' stroke-linejoin="round">' + RAIL_MARKS.cal + '</g></svg>'
        + '<span class="nm">カレンダー</span></div>');
    rows.push('<div class="sp"></div>');
    rows.push('<button id="new"><span class="ring">'
        + '<svg viewBox="0 0 16 16" aria-hidden="true">'
        + '<path d="M8 3.4v9.2M3.4 8h9.2" stroke="currentColor" stroke-width="2.2"'
        + ' stroke-linecap="round"/></svg></span>'
        + '<span>新しいノート</span></button>');

    // 「ノート」という見出しは置かない（本人・2026-09-12「すぐ下が『すべてのノート』で
    // 意味をなしていない」）── 見出しの無い一行として「すべてのノート」。
    rows.push(dest('all', '', 'すべてのノート', state.notes.length, on('all', '')));

    const stars = state.notes.filter(starred);
    {
        // **一つも無くても段は出す。** 無いときに段ごと消えると、
        // 最初の一つを作るパスがどこにも無くなる ── 「実装されていないのか、
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
        // **保存ディレクトリが二つ以上なら、それぞれが親の一行**（依頼 511・案ア）
        // ── その下にフォルダが一段下がって並ぶ。一つだけの日は、いままで通り
        // フォルダだけ（親が一つきりの木は、ただの余計な段）。
        const base = manyPlaces() ? 1 : 0;
        for (const p of state.places) {
            if (manyPlaces()) rows.push(placeRow(p, on('place', p.dir)));
            for (const b of state.books) {
                if (rootOf(b) !== p.dir) continue;
                // **共有のフォルダは、こちらには出さない。** 下の「共有」の段に
                // 同じものが並ぶ ── 二つの場所に同じものが出ると、人はそれを
                // 二度消そうとする（ブックマークを別枠にしたのと同じ理由）。
                if (state.shares.some((sh) => sh.at !== rootOf(sh.at) && (b === sh.at || b.startsWith(sh.at + '/')))) continue;
                const n = state.notes.filter((x) => x.book === b || x.book.startsWith(b + '/')).length;
                // 錠のかかったフォルダには 🔒 を添える（依頼 629）── 右押しの
                // メニューを開くまで分からないのでは、かけたことを忘れる。
                const shut = state.locks.some((l) => b === l || b.startsWith(l + '/'));
                rows.push(dest('book', b, b.split('/').pop() + (shut ? ' 🔒' : ''), n, on('book', b),
                               relOf(b).split('/').length - 1 + base, state.colors[b]));
            }
        }
    }

    // **共有は、行き先の一つ。** 分けるのはクラウドの仕事で、amber が持つ
    // のは「どれが分けてあるか」だけ ── だからここは新しい仕組みではなく、
    // フォルダの一つを別の名前で呼んでいるだけになる。
    //
    // **決めていないうちは出さない。** 空の「共有」が並んでいると、共有が
    // 壊れているのか、まだ何も分けていないのかが見分けられない ── 作るパスは
    // フォルダの右押しにある。
    if (state.shares.length) {
        rows.push(head('共有'));
        for (const sh of state.shares) {
            const n = state.notes.filter((x) => inShare(sh.at, x)).length;
            const top = sh.at === rootOf(sh.at);
            rows.push(dest('share', sh.at, top ? (manyPlaces() ? bookName(sh.at) : 'すべて') : sh.at.split('/').pop(), n,
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
    el('new').onclick = () => cmdNewNote();
    if (el('savenow')) el('savenow').onclick = () => save();
    if (el('zenbtn')) el('zenbtn').onclick = () => setZen(!zen);
    for (const b of el('rail').querySelectorAll('.plus')) {
        b.onclick = (e) => { e.stopPropagation(); railPlus(b.dataset.plus); };
    }
    for (const d of el('rail').querySelectorAll('.dest')) {
        // 一覧の行と同じ理由で押し下げ（`.row` の註）。
        d.onmousedown = (e) => {
            if (e.button !== 0) return;
            if (inNote(document.activeElement)) e.preventDefault();
            // **カレンダーは行き先ではない。** 一覧を絞るのではなく、
            // 月の表を開く ── 押したあとに一覧が空になったら、押した人は
            // 何が起きたか分からない（依頼 467）。
            if (d.dataset.kind === 'cal') { cmdCalendar(); return; }
            // **行き先を選んだら、カレンダーからは出る**（依頼 554・本人）。
            //
            // ノートを開くパスには前からある決まり（依頼 478）だが、**こちらの
            // パスは `calOn` に触っていなかった** ── 「すべてのノート」を押して
            // 一覧が絞られても、月の表が出たままで、絞った結果が見えない。
            // 出すには「閉じる」をもう一度押すしかなかった。
            // 閉じ方は `calShut` に一本ある ── ここで書き直さない。
            if (calOn) calShut();
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
/// 色を付けていないフォルダとブックマークのフォルダは、文字の形しか違いが無かった。
/// 列は上から下へ読むものなので、段の見出しを覚えていないと**いま何の
/// 一覧を見ているのか分からない**。
///
/// 色は形の上に載せる（色だけで分けない）── 色の見分けにくい人にも、
/// フォルダはフォルダの形をしている。
const RAIL_MARKS = {
    all: '<path d="M1.5 9.5h3.2l1 1.8h4.6l1-1.8h3.2M1.5 9.5 3.4 3.6h9.2l1.9 5.9'
        + 'v3.4a1 1 0 0 1-1 1H2.5a1 1 0 0 1-1-1z"/>',
    star: '<path d="M8 1.9 10 6l4.5.6-3.3 3.1.8 4.4L8 12l-4 2.1.8-4.4L1.5 6.6 6 6z"/>',
    // 月の表。**四角に横棒一本と、上の耳二つ** ── どの国の人も
    // カレンダーだと分かる形（絵は世界のどこでも同じ意味のものを選ぶ）。
    cal: '<path d="M2.2 4.4h11.6a1 1 0 0 1 1 1v7.2a1 1 0 0 1-1 1H2.2a1 1 0 0 1-1-1'
        + 'V5.4a1 1 0 0 1 1-1zM1.2 7.4h13.6M5 2.2v2.6M11 2.2v2.6"/>',
    book: '<path d="M1.6 12.6V4.2a1 1 0 0 1 1-1h3.3l1.5 1.8h6a1 1 0 0 1 1 1v6.6'
        + 'a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z"/>',
    // **ラベルの形。** ここは長らく「＃」を線で描いていたが、16px の枠に
    // 四本線を渡すと線が詰まって、文字の「＃」が太って見えるだけになる ──
    // 隣のフォルダも星も形で分かるのに、タグだけ記号を読ませていた。
    // 共有は**二人**。フォルダでも星でもない形にする ── 「ここに置いたものは
    // 自分だけのものではない」が、名前を読まなくても分かるように。
    share: '<circle cx="5.6" cy="5.4" r="2.5"/><circle cx="11.2" cy="6.6" r="1.9"/>'
        + '<path d="M1.6 13.4c0-2.2 1.8-3.6 4-3.6s4 1.4 4 3.6"/>'
        + '<path d="M11 9.9c1.9.1 3.4 1.4 3.4 3.5"/>',
    tag: '<path d="M9.1 1.9h3.9a1.1 1.1 0 0 1 1.1 1.1v3.9a1.1 1.1 0 0 1-.32.78'
        + 'l-6.1 6.1a1.1 1.1 0 0 1-1.56 0L1.9 9.58a1.1 1.1 0 0 1 0-1.56l6.1-6.1'
        + 'a1.1 1.1 0 0 1 .78-.32z"/><path d="M11.2 4.8h.01"/>',
    // 保存ディレクトリは**引き出し**（横長の箱に取っ手）── フォルダの形と
    // 分けておく。中にフォルダが並ぶ「入れ物の入れ物」なので、同じ形だと
    // 段の深さでしか見分けられない。
    place: '<path d="M1.8 6.2h12.4v6.3a1 1 0 0 1-1 1H2.8a1 1 0 0 1-1-1z'
        + 'M3.2 6.2 4.6 2.9h6.8l1.4 3.3M6.4 9.8h3.2"/>',
};

/// 保存ディレクトリの一行（依頼 511）。右に同期先の札（Drive のときだけ）と数。
/// 読めていなければ薄く出して「見つかりません」── 消えたように見せない。
function placeRow(p, isOn) {
    const n = state.notes.filter((x) => x.root === p.dir).length;
    const lost = state.placeTrouble[p.dir];
    const badge = (p.sync === 'drive' && !OFFICE) ? '<span class="sy">Drive</span>' : '';
    return '<div class="dest' + (isOn ? ' on' : '') + (lost ? ' lost' : '') + '" data-kind="place"'
        + ' data-what="' + escapeAttr(p.dir) + '" data-depth="0"'
        + (lost ? ' title="' + escapeAttr(lost) + '"' : '') + '>'
        + '<svg class="mk" viewBox="0 0 16 16" aria-hidden="true">'
        + '<g fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"'
        + ' stroke-linejoin="round">' + RAIL_MARKS.place + '</g></svg>'
        + '<span class="nm">' + escapeHtml(p.name) + '</span>' + badge
        + '<span class="n">' + (lost ? '見つかりません' : n) + '</span></div>';
}

function dest(kind, what, name, n, isOn, depth, color) {
    const d = RAIL_MARKS[kind] || RAIL_MARKS.book;
    // **色を付けたフォルダは、塗る。** 線にだけ色を載せていた頃は、
    // 15px の輪郭の色を見分けることになって「色を付けた」と気づけなかった
    // （iPhone は前から塗っている）。塗ると、離れて見ても色で拾える。
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

/// このノートは、その共有のフォルダの中か（`at` は絶対の道。保存ディレクトリそのもの
/// なら、その中のぜんぶ）。
const inShare = (at, n) => !at || n.book === at || (n.book || '').startsWith(at + '/');

function inDest(n) {
    const kind = state.dest.kind;
    const what = state.dest.what;
    if (kind === 'share') return inShare(what, n);
    if (kind === 'place') return n.root === what;
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
    // **押して選んだものは、文字にしない。** 前は `tag:仕事` を探す欄に流し
    // 込んでいた ── 押しただけなのに環境の言葉が現れ、消すには文字を消す
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
/// 含む。`YYYY-MM-DD` の文字で比べれば、この取り違えが起きようがない。
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
/// `-` の打ち消し。どれが見出しでどれが文字かを決めるのは `note::terms` で、
/// ここは決めない ── デスクトップ版が自分で `:` を数えはじめると、iPhone と別のものが
/// 見つかる検索デスクトップ版が二つできる。
///
/// **`body:` は無い。** 一覧が持っているのは本文の頭 100 文字だけなので、
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

/// 一覧の並び。**iPhone と同じ3 つ**（`NotesStore.sorted`）。
///
/// 名前順は `localeCompare(..., {numeric: true})` ── 素の `<` は「あ」と「い」も
/// `note-2` と `note-10` も両方まちがえる。iPhone 側は Foundation の
/// `localizedStandardCompare` で、**同じ規則をそれぞれの土地の言葉で言って
/// いる**。core に上げなかったのはそのため ── Rust には土地を知った自然順が
/// 標準に無く、上げると iPhone の並びのほうが悪くなる。
///
/// **昇順と降順は、同じボタンを押し続けて回る**（依頼 643・本人「同じボタンを
/// 押下したら昇順・降順を変更できないかな？」）。列の見出しを二度押すと逆順に
/// なるファイラの挙動が本人の手に入っているが、amber にあるのはボタン 1 つで
/// 見出しの列ではない ── なので 3 つ × 2 向き = 6 つを順ぐりにする。押し続ければ
/// 必ず元へ戻り、**行き止まりが無い**。
///
/// 矢印は「上に来るのはどちらか」を言う ── `↓` は大きいほうが上（新しい順・
/// ん→あ）、`↑` は小さいほうが上（古い順・あ→ん）。
const ORDERS = [
    // 名前は iPhone と同じ三語（本人・2026-09-12）。
    ['updated', false, '更新順 ↓', '新しい順'],
    ['updated', true, '更新順 ↑', '古い順'],
    ['created', false, '作成順 ↓', '新しい順'],
    ['created', true, '作成順 ↑', '古い順'],
    ['title', true, 'タイトル順 ↑', 'あ→ん'],
    ['title', false, 'タイトル順 ↓', 'ん→あ'],
];
let order = 'updated';
/// 昇順か。**既定は降順**（更新順と作成順は新しい順、タイトル順だけ昇順が既定）。
let asc = false;

/// いま何番目の並びか。憶えている値が古い形（向きを持たない）でも読めるように、
/// 見つからなければその物差しの**最初の向き**に落とす。
function orderAt() {
    const i = ORDERS.findIndex(([k, a]) => k === order && a === asc);
    return i >= 0 ? i : Math.max(0, ORDERS.findIndex(([k]) => k === order));
}

/// 名前順に並べる物差し。**一度だけ作って、使い回す。**
///
/// `localeCompare(b, 'ja', {…})` は呼ぶたびに物差しを作り直す ── 1002 本を
/// 並べると一万回作ることになり、それだけで 119ms かかっていた（同じ並びが
/// `Intl.Collator` の使い回しでは 7ms）。並べ替えは一覧を作るたびにラン、
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
    // **逆順は、並べ終えてからひっくり返す**（依頼 643）── 比較の向きを
    // 二通り書くと、同じ値のときの並びが向きによって変わる（`sort` は安定
    // なので、ひっくり返せば同じ値の中の順序も素直に逆になる）。
    //
    // タイトル順だけ既定が昇順なので、ひっくり返すのは `asc` が偽のとき。
    const flip = order === 'title' ? !asc : asc;
    return flip ? out.reverse() : out;
}

function drawOrder() {
    const [, , label, which] = ORDERS[orderAt()];
    const next = ORDERS[(orderAt() + 1) % ORDERS.length];
    el('order').textContent = label;
    // **次に何になるかを、札に書く。** 6 つを順ぐりにすると「あと何回押せば
    // 目当てに着くか」が見えない ── 次の一つが見えていれば、押しながら探せる。
    el('order').title = `${label}（${which}）── 押すと「${next[2]}」`;
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

el('order').onclick = nextOrder;

/// 次の並びへ。**3 つ × 2 向きを順ぐり**（依頼 643）。
function nextOrder() {
    const [k, a] = ORDERS[(orderAt() + 1) % ORDERS.length];
    order = k;
    asc = a;
    window.amber.remember({ order, orderAsc: asc });
    drawOrder();
    drawList();
}

function drawList() {
    const rows = sortNotes(narrowed());
    const what = state.dest.what;
    const name = {
        all: 'すべてのノート',
        book: bookName(what),
        place: bookName(what),
        share: '共有 ── ' + (what === rootOf(what) ? (manyPlaces() ? bookName(what) : 'すべて') : what.split('/').pop()),
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
    // あるからで、「これは大事」と言うパスを二つ持たない。
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
        // ことで保存がラン、一覧が組み直され、離す頃には押した行がもう
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
        // 右押しでも、⋯ と同じメニュー。**開いてから出す** ── 開いていない
        // ノートに「削除」を出すと、どれが消えるのか画面が言っていない。
        r.oncontextmenu = async (e) => {
            e.preventDefault();
            const at = r.dataset.path;
            // **選んでいる行を右押ししたら、選んだぶんのメニュー。** 開かない
            // ── 開くと選びが畳まれて、出したかったメニューが消える。
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
    if (e.target.closest('.row')) return;          // 行は行のメニューが受ける
    if (e.target.closest('input, textarea')) return;
    e.preventDefault();
    const at = { x: e.clientX, y: e.clientY };
    popMenu([
        { name: '新しいノート', key: '⌘N', run: newNote },
        { name: 'ここに貼り付けて新しいノート', run: cmdPasteNote },
        { name: '並び順 ── ' + ORDERS[orderAt()][2], sep: true, run: nextOrder },
        { name: 'すべて選ぶ', key: '⌘A', sep: true, run: pickAll },
        { name: '一覧を畳む', key: '⌘⌥/', run: toggleList },
    ], at);
});

/// 左の列の空きどころの右押し。行き先の上ではないので、「この列」のこと。
el('rail').addEventListener('contextmenu', (e) => {
    if (e.target.closest('.dest, .plus')) return;  // 行き先は `railMenu` が受ける
    e.preventDefault();
    popMenu([
        { name: '新しいフォルダ', run: () => cmdMkBook() },
        { name: '新しいブックマークグループ', run: () => newShelf('') },
        { name: '左の列を畳む', key: '⌘/', sep: true, run: toggleRail },
    ], { x: e.clientX, y: e.clientY });
});

/// 帯の題の右押し。**一覧の行を右押ししたときと同じメニュー**（依頼 612・本人
/// 「どちらも同じポップアップにできる？」）。
///
/// 前はここだけ手書きの四つで、⋯ とも一覧の行とも違うものが出ていた ──
/// 同じノートを右押ししているのに、押した場所で出るものが変わる。
/// 四つは命令の表へ移したので、ここは `openMenu` を呼ぶだけでよくなった。
el('title').addEventListener('contextmenu', (e) => {
    if (!state.open) return;
    e.preventDefault();
    openMenu({ x: e.clientX, y: e.clientY });
});

/// 貼り付けた文字から、新しいノートを一本。
/// **一本の Web ページを、一本のノートに**（依頼 421・乙）。
///
/// 甲（ブラウザで選んでコピー → 貼る）が「要るところだけ」なら、
/// こちらは「まるごと」── あとで読むために丸ごと置いておきたいとき。
///
/// **本文らしいところだけ採る。** ページには案内も広告も足もあるので、
/// `article` があればそれを、無ければ**いちばん文字の多いかたまり**を。
/// 完全ではないが、全部貼るよりは読める（要らない行は消せばよい）。
async function cmdClip() {
    // コピーしてあるなら、最初から入れておく ── URL は打つものではなく、
    // たいてい既に手元にある。
    let seed = '';
    try {
        const t = (await navigator.clipboard.readText()).trim();
        if (/^https?:\/\//i.test(t)) seed = t;
    } catch { /* 読めなくても、打てばよい */ }
    const url = await askText('Web からインポート', seed, 'ページの URL を貼ってください');
    if (url === null || !url.trim()) return;
    say('取りに行っています…');
    const got = await window.amber.fetchPage(url.trim());
    if (!got || got.error) { say('インポートできません: ' + (got?.error || '返事がありません')); return; }
    let md = '';
    let title = '';
    try {
        title = clipTitle(got.html);
        md = webToMd(bestPart(got.html), got.url);
    } catch (e) {
        say('読めません: ' + why(e));
        return;
    }
    if (!md.trim()) { say('本文が見つかりませんでした'); return; }
    // **出どころは本文の最後に、文字として。** 前書きに `source:` を足すパスは
    // 採らない ── amber の都合をノートに書かない（芯の 1）。この一行なら、
    // メモ帳で開いた人にもそのまま読める。
    const from = (() => {
        try { return new URL(got.url).host + new URL(got.url).pathname; } catch { return got.url; }
    })();
    // **その環境の今日。** `toISOString()` は世界標準時なので、日本の
    // 朝に取り込むと前の日の日付が入る（前書きの `created` は core が
    // 入れた今日で、そこと一日ずれた）。
    const day = new Date();
    const today = day.getFullYear() + '-'
        + String(day.getMonth() + 1).padStart(2, '0') + '-'
        + String(day.getDate()).padStart(2, '0');
    const body = md + '\n\n---\n\n出典: [' + from + '](' + got.url + ')（'
        + today + ' に取り込み）\n';
    try {
        const made = await newNote(title);
        if (!made || !editor) return;
        loading = true;
        // 前書きの後ろに一行空ける ── 新しいノートがそう作られるので、
        // ここで詰めると、取り込んだだけのノートが**同期先で差分**になる
        // （`readSourceEdit` と同じ理由）。
        editor.setValue(state.head ? '\n' + body : body);
        loading = false;
        state.dirty = true;
        await save();
        await drawRead();
    } catch (e) {
        say('作れません: ' + why(e));
    }
}

/// **型を置くフォルダの名前。**
///
/// 決め打ちの一語 ── 設定にしない。設定にすると「どこに置けば型になるか」
/// が人によって違い、サンプルノートにも書けない（読んだ人の amber では違う
/// 名前かもしれない）。**ただのフォルダ**なので、中のノートは一覧にも
/// 普通に出るし、開いて直せる ── 型のための新しい入れ物は作らない。
const TEMPLATES = 'テンプレート';

/// 型から新しいノートを作る（依頼 417）。
///
/// **写す仕組みは「複製」と同じ**（core の `duplicate`）── 行き先だけが
/// 違う。別のパスにすると、`created` を今日にするのを片方だけ直した日に、
/// 二つの作り方が食い違う。
///
/// できたノートは**いま見ているフォルダ**へ（「新しいノート」と同じ）──
/// どこに出来たか分からない、がいちばん困る。
/// そのフォルダは「テンプレート」の中か（どの保存ディレクトリのものでも）。
const inTemplates = (dir) => relOf(dir) === TEMPLATES || relOf(dir).startsWith(TEMPLATES + '/');

async function cmdTemplate() {
    const rows = state.notes.filter((n) => inTemplates(n.book));
    if (!rows.length) {
        // **無いなら、その場で作れる**（依頼 506・本人「選んでも動かない」）── 前は
        // 帯に一言出すだけで、見逃すと「押しても何も起きない」にしか見えなかった。
        const items = [{ name: 'サンプルのテンプレートを入れる', sub: '週報・議事録・買い物リスト の三枚を「' + TEMPLATES + '」フォルダに', value: 'seed' }];
        if (state.open && !state.guest) items.push({ name: '今開いているノートをテンプレートにする', sub: '「' + (state.open.title || 'このノート') + '」を「' + TEMPLATES + '」フォルダへ写します', value: 'this' });
        const pick = await askPick('テンプレートがまだありません', items,
            '「' + TEMPLATES + '」フォルダの中のノートが、テンプレートになります', true);
        if (pick === null) return;
        try {
            if (pick === 'seed') {
                const r = await window.amber.templates(state.root);
                say(r.put ? r.put + ' 件入れました' : 'もう入っています');
            } else {
                await cmdToTemplate();
            }
            await reload({ quiet: true });
        } catch (e) { say('入れられません: ' + why(e)); return; }
        return cmdTemplate();
    }
    const path = await askPick('どの型から',
        sortNotes(rows).map((n) => ({
            name: n.title || '（タイトルなし）',
            sub: (manyPlaces() ? n.place + (relOf(n.book) === TEMPLATES ? '' : ' › ') : '')
                + (relOf(n.book) === TEMPLATES ? '' : relOf(n.book).slice(TEMPLATES.length + 1)),
            value: n.path,
        })), '選ぶと、その中身で新しいノートを作ります');
    if (path === null) return;
    // **型のフォルダを見ているときは、いちばん上に作る。** そこに作ると
    // 型が増えていくだけで、書いたものがどこにも出てこない。
    const here = inTemplates(hereDir()) ? rootOf(hereDir()) : hereDir();
    try {
        const r = await ask('copy', { path, dir: here });
        await reload({ quiet: true });
        await openNote(r.path);
        if (editor) editor.focus();
    } catch (e) {
        say('作れません: ' + why(e));
    }
}

/// **このノートをテンプレートにする**（依頼 506）── 「テンプレート」フォルダへ写す。
/// 元のノートはそのまま（写しが型になる）。
async function cmdToTemplate() {
    if (!state.open || state.guest) return;
    if (state.dirty) await save();
    try {
        // 写す先は、**そのノートの保存ディレクトリ**の「テンプレート」。
        const r = await ask('copy', { path: state.open.path, dir: rootOf(state.open.path) + '/' + TEMPLATES });
        await reload({ quiet: true });
        say('「' + (state.open.title || 'このノート') + '」を「' + TEMPLATES + '」に写しました。次から「テンプレートから新しいノート」に出ます');
        return r.path;
    } catch (e) {
        say('写せません: ' + why(e));
        return null;
    }
}

/// 同じ中身のノートをもう一つ（依頼 412）。
///
/// **先に保存する。** 打った文字がまだファイルに無いうちに写すと、写しは
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
    bar.innerHTML = '<span class="n">' + n + ' 件を選んでいます</span><span class="sp"></span>'
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
        { name: n + ' 件にタグを付ける', run: () => manyTagOn() },
        { name: n + ' 件からタグを外す', run: () => manyTagOff() },
        { name: n + ' 件をフォルダへ移動', run: () => manyMove() },
        { name: n + ' 件をブックマークに登録する', run: () => manyStar(true) },
        { name: n + ' 件のブックマークを外す', run: () => manyStar(false) },
        { name: '選ぶのをやめる', key: 'Esc', sep: true, run: unpickAll },
        { name: n + ' 件をゴミ箱へ入れる', sep: true, run: () => manyDelete() },
    ], at);
}

/// 選んだノートを一本ずつ書き換える。**一本転んでも、残りは進む** ──
/// 二十本のうち三本目で止まると、どこまで済んだのかが誰にも分からない。
///
/// `change(note, text)` が新しい文字を返す。**同じ文字を返したら書かない** ──
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
    let m = r.done.length + ' 件' + verb;
    if (r.skipped.length) m += '（' + r.skipped.length + ' 件は' + why2 + '）';
    if (r.failed.length) m += '／' + r.failed.length + ' 件は書けませんでした';
    say(m);
}

async function manyTagOn() {
    const notes = pickedNotes();
    const all = tagsOf(state.notes).map(([t, c]) => ({ name: t, sub: c + ' 件', value: t }));
    const pick = await askPick(notes.length + ' 件に付けるタグ',
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
    const tag = await askPick(notes.length + ' 件から外すタグ',
        here.map(([t, c]) => ({ name: t, sub: notes.length + ' 件中 ' + c + ' 件', value: t })));
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
    const here = [...bookChoices(), { name: '＋ 新しいフォルダを作る', value: ' new' }];
    let to = await askPick(notes.length + ' 件をどのフォルダへ', here);
    if (to === null) return;
    if (to === ' new') {
        const made = await cmdMkBook();
        if (!made) return;
        to = made;
    }
    const dir = to;
    if (state.dirty) await save();
    let moved = 0;
    let failed = 0;
    const now = new Set();
    for (const note of notes) {
        // もう居るところへは動かさない。
        if (note.book === dir) { now.add(note.path); continue; }
        try {
            const r = await moveOp(note.path, dir);
            moved++;
            if (r && r.path) now.add(r.path);
        } catch { failed++; now.add(note.path); }
    }
    // **選びはパスで憶えている。** 移すとパスが変わるので、繋ぎ直す ──
    // 繋がないと、移した直後に選びが空になる。
    state.picked = now;
    await reload({ quiet: true });
    if (state.open) await openNote(state.open.path, { quiet: true });
    drawList();
    say(moved + ' 件を' + dirWords(dir) + '移しました'
        + (failed ? '／' + failed + ' 件は移せませんでした' : ''));
}

/// まとめてゴミ箱へ。**訊くのは一度だけ。**
///
/// 一本ずつの `cmdDelete` を二十回まわすと、二十回訊かれる ── ゴミ箱の
/// 無い保存場所（会社の OneDrive）では、断られてからもう一度、で四十回に
/// なる。数を言って一度承知をもらい、そのあとは黙って進める。
async function manyDelete() {
    const notes = pickedNotes();
    if (!notes.length) return;
    const knew = noBin();
    const head = notes.length + ' 件を';
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
    say((binned ? binned + ' 件をゴミ箱へ入れました' : '')
        + (binned && erased ? '／' : '') + (erased ? erased + ' 件を消しました' : '')
        + (failed ? '／' + failed + ' 件は消せませんでした' : '')
        || '何も消しませんでした');
}

function row(n) {
    const open = state.open && state.open.path === n.path;
    // **控えは、消さずにラベルを貼る。** 一覧から外すと、中身を助け出すパスが
    // どこにも無くなる ── 並べたうえで、そういうものだと言う。
    const clash = n.clash
        ? '<span class="clashmark" title="クラウドが作ったコピーです">競合</span>'
        : '';
    // **共有中は、書く前に分かるように。** グループが読むノートに、そうと
    // 知らずに書くことがないように ── マークは題の隣（開いてからでは遅い）。
    const shared = n.shared && state.dest.kind !== 'share'
        ? '<span class="sharemark" title="グループと分けているフォルダの中です">共有</span>'
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

/// いま変換の途中か。**確定するまで、画面を触らない。**
///
/// `input` は変換の一字ごとに来るので、`readChanged` の 700ms が**変換の
/// 途中で切れうる** ── そこで書き戻すと、未確定の文字が保存される。同期先に
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
        // **絶対のパスで渡す。**
        //
        // 相対のままだと、Monaco が worker のために組み立てるパスが
        // `file:///vendor/monaco/…`（ファイルシステムの根から）になり、
        // 取り込みに失敗する ── 失敗しても本体スレッドに落ちて動くので
        // 気づかないが、長いノートでデスクトップ版が固まる。`console-message` を
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

            // **変換の途中かどうかを、エディタからも受ける。** 表示画面は
            // `compositionstart` を持っているが、Monaco は自分の中で
            // 変換を扱うので、こちらから聞かないと分からない。
            // **コード画面に貼るときも、HTML は Markdown に**（依頼 421）。
            //
            // 画面によって貼れるものが違う、を作らない ── ブラウザで
            // コピーした人は、どちらの画面に貼っても同じものが入ると思う。
            // Monaco の `onDidPaste` は入ったあとなので間に合わない:
            // 素の `paste` を先に捕まえて、自分で入れる。
            el('ed').addEventListener('paste', (e) => {
                if (!e.clipboardData) return;
                const md = clipText(e.clipboardData);
                if (md === e.clipboardData.getData('text/plain')) return;   // 直すものが無い
                e.preventDefault();
                e.stopPropagation();
                put(md);
            }, true);
            editor.onDidCompositionStart?.(() => { composing = true; });
            editor.onDidCompositionEnd?.(() => { composing = false; });

            editor.onDidChangeModelContent(() => {
                // **読み込みの `setValue` も変更として届く。** 守らないと、
                // 開いただけで自動保存がラン、触っていないノートの更新時刻が
                // 動く（実際に一本動かした）。同期しているフォルダでは、それが
                // 相手側に「向こうが編集した」と見える ── 何もしていないのに。
                if (loading || !state.open) return;
                // 打ったので、表示画面はもう今の文字ではない（組み直すまで）。
                readStale();
                state.dirty = true;
                el('state').textContent = '書きかけ';
                drawSaveNow();
                drawStrip();
                clearTimeout(saveTimer);
                // **変換中に切れたら、待つ。** Monaco も変換の一字ごとに
                // ここへ来るので、間合いが変換の途中で切れうる ── そこで
                // 保存すると、未確定の文字がファイルに入る（表示画面と同じ話）。
                saveTimer = setTimeout(function again() {
                    if (composing) { saveTimer = setTimeout(again, 900); return; }
                    // 自動保存を切っているときは、「保存」を押すまで書かない（依頼 512）。
                    if (autoSave) save();
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

/// 開いているノートの列。**iPhone と同じ形**（`Desk.Tab`）── 一本ぶんの
/// 持ちものを、まとめてしまっておく箱。
///
/// **エディタは一台のまま。** iPhone は画面をタブごとに持てるが、Monaco を
/// ノートの数だけ建てるのは高い ── 替えるときに文字と caret と巻き位置を
/// 出し入れすれば、同じことになる。
///
/// **単発で開く一本（`state.guest`）は机に載せない。** あれは amber のフォルダの
/// 外にある一本で、閉じれば元の机へ戻るもの ── 列に並べると、閉じたあとに
/// フォルダの外のノートがタブに残る。
let tabs = [];
let showing = null;

/// タブ一本ぶんの持ちもの。**ここに挙げたものが、タブごとに別々**。
/// 画面（表示／コード）はウィンドウぜんぶのことなので、入れない ── タブごとに
/// 変わると、替えるたびに画面が飛ぶ。
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
/// ファイルから読み直すと、打ちかけの文字が消える（タブがある意味が無い）。
function restoreTab(t) {
    const k = t.keep;
    if (!k) return false;
    // **しまってあるノートは、しまった時の姿。** そのあいだに場所が変わって
    // いることがある（共有に入れる・外す・フォルダへ移す）ので、いまの一覧に
    // 同じパスのノートが居れば、そちらを信じる ── パスは `afterRename` が
    // 繋ぎ直すが、`rel` や `book` はここでしか新しくならない。
    // 一覧に居ないノート（外から開いた一本）は、しまってあるものをそのまま。
    state.open = state.notes.find((n) => n.path === k.open.path) || k.open;
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
/// 一段ぶん狭くしない（iPhone も同じ）。
/// 最後に描いた机の姿。**同じなら描き直さない** ── 打つたびに帯を組み
/// 直すと、掴んでいる巻き位置が毎回先頭へ戻る。
let stripWas = null;

function drawStrip() {
    const box = el('strip');
    const wrap = el('stripwrap');
    const was = wrap.hidden;
    wrap.hidden = state.guest || tabs.length < 2;
    // 出たり引っ込んだりすると、下の画面の高さが変わる ── Monaco は自分で
    // 気づかないので、測り直させる（畳むキーと同じ扱い）。
    if (was !== wrap.hidden && editor) setTimeout(() => editor.layout(), 0);
    if (wrap.hidden) { box.innerHTML = ''; stripWas = null; return; }
    // **いま出しているタブは、しまってあるものを見ない。** `keep` が書かれる
    // のは離れるときなので、出している間ずっと古い ── 書きかけの点が
    // 点かないし、題を直しても帯が変わらない。生のほうを見る。
    const shape = tabs.map((t) => {
        const here = t.path === showing;
        return { path: t.path, here, name: tabName(t),
                 dirty: here ? state.dirty : !!(t.keep && t.keep.dirty) };
    });
    el('stripall').textContent = tabs.length + ' 件 ▾';
    const key = JSON.stringify(shape);
    if (key === stripWas) return;
    stripWas = key;
    box.innerHTML = shape.map((t, n) =>
        '<div class="tab' + (t.here ? ' on' : '') + '" data-n="' + n + '"'
        + ' title="' + escapeAttr(t.path) + '">'
        + (t.dirty ? '<span class="d"></span>' : '')
        + '<span class="t">' + escapeHtml(t.name) + '</span>'
        + '<button class="x" title="このノートを閉じる">✕</button></div>').join('');
    for (const d of box.querySelectorAll('.tab')) {
        const t = tabs[Number(d.dataset.n)];
        d.onmousedown = (e) => {
            // まん中押しで閉じる（机の上のふつう）。
            if (e.button === 1) { e.preventDefault(); closeTab(t.path); return; }
            if (e.button !== 0) return;
            if (e.target.closest('.x')) return;      // ✕ は下の `onclick` が受ける
            if (inNote(document.activeElement)) e.preventDefault();
        };
        // **開くのは押し離したとき**（依頼 625）── 押した瞬間に開くと、帯を掴んで
        // 引っ張ろうとしただけでそのタブが開く。引っ張ったあとの click は帯が食べる。
        d.onclick = (e) => {
            if (e.target.closest('.x')) return;
            openNote(t.path, { keep: true });
        };
        d.querySelector('.x').onclick = (e) => { e.stopPropagation(); closeTab(t.path); };
        d.oncontextmenu = (e) => {
            e.preventDefault();
            const at = tabs.indexOf(t);
            popMenu([
                // 言い方は本人が決めた（2026-09-12）。
                { name: 'このノートを閉じる', run: () => closeTab(t.path) },
                { name: 'このノートより右のものを閉じる', dim: at >= tabs.length - 1,
                  run: () => { for (const o of tabs.slice(at + 1)) closeTab(o.path); } },
                { name: 'このノート以外をすべて閉じる', dim: tabs.length < 2,
                  run: () => { for (const o of tabs.slice()) if (o.path !== t.path) closeTab(o.path); } },
                { name: '一覧でこのノートを選ぶ', sep: true, run: () => openNote(t.path, { keep: true }) },
                { name: 'Finder で表示', run: () => window.amber.reveal(t.path) },
            ], { x: e.clientX, y: e.clientY });
        };
    }
    const on = box.querySelector('.tab.on');
    if (on) on.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    stripEnds();
}

/// タブの見せ名。**帯と一覧で同じ名前** ── 帯で「議事録」と見えているものが、
/// 一覧で別の名前になっていると、どれがどれか分からない。
function tabName(t) {
    const here = t.path === showing;
    const row = state.notes.find((x) => x.path === t.path);
    const name = (here && state.open && state.open.title)
        || (t.keep && t.keep.open && t.keep.open.title)
        || (row && row.title) || baseOf(t.path);
    return name || '（タイトルなし）';
}

/// 開いているノートの一覧（依頼 627）。選ぶとそのタブへ。**打てば絞れる**
/// （5 件を越えるとダイアログが欄を出す）── 何十枚の中から探すのは、名前を打つのが早い。
async function cmdTabList() {
    if (tabs.length < 2) { say('開いているノートは 1 件だけです'); return; }
    const items = tabs.map((t) => ({
        name: (t.path === showing ? '● ' : '　') + tabName(t),
        sub: t.path === showing ? 'いま開いているノート' : shortPath(t.path),
        value: t.path,
    }));
    const go = await askPick('開いているノート（' + tabs.length + ' 件）', items,
        '選ぶと、そのノートを開きます');
    if (go) await openNote(go, { keep: true });
}

/// 端の ‹ › を、溢れているときだけ出す。行けない向きは薄くする。
function stripEnds() {
    const box = el('strip');
    const over = box.scrollWidth > box.clientWidth + 1;
    el('stripleft').hidden = !over;
    el('stripright').hidden = !over;
    el('stripleft').disabled = box.scrollLeft <= 0;
    el('stripright').disabled = box.scrollLeft + box.clientWidth >= box.scrollWidth - 1;
}

{
    const box = el('strip');
    // **ホイールで横に。** ふつうのマウスは縦にしか回らない。トラックパッドの
    // 横なぞり（`deltaX`）はそのまま効くので、縦の回しだけを横へ回す。
    box.addEventListener('wheel', (e) => {
        if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
        if (box.scrollWidth <= box.clientWidth) return;
        e.preventDefault();
        box.scrollLeft += e.deltaY;
    }, { passive: false });
    box.addEventListener('scroll', stripEnds);
    window.addEventListener('resize', stripEnds);
    // 端の ‹ ›。**見えている幅の八割ずつ** ── 1 つずつだと 50 枚は遠い。
    const page = (dir) => box.scrollBy({ left: dir * box.clientWidth * 0.8, behavior: 'smooth' });
    el('stripleft').onclick = () => page(-1);
    el('stripall').onclick = () => cmdTabList();
    el('stripright').onclick = () => page(1);
    // **掴んで引っ張る**（スマホのフリックと同じ手つき）。押しただけならタブを開く
    // ── 5px 動いてから引っ張りに変わる。動いたあとの一押しはタブを開かない。
    let drag = null;
    box.addEventListener('mousedown', (e) => {
        if (e.button !== 0 || e.target.closest('.x')) return;
        drag = { x: e.clientX, left: box.scrollLeft, moved: false };
    }, true);
    window.addEventListener('mousemove', (e) => {
        if (!drag) return;
        const dx = e.clientX - drag.x;
        if (!drag.moved && Math.abs(dx) < 5) return;
        drag.moved = true;
        box.classList.add('grab');
        box.scrollLeft = drag.left - dx;
    });
    window.addEventListener('mouseup', () => {
        if (!drag) return;
        if (drag.moved) {
            box.classList.remove('grab');
            // 引っ張り終わりの click を、タブに届けない。
            const eat = (e) => { e.stopPropagation(); e.preventDefault(); };
            box.addEventListener('click', eat, { capture: true, once: true });
            setTimeout(() => box.removeEventListener('click', eat, { capture: true }), 0);
        }
        drag = null;
    });
}

/// タブを閉じる。**書きかけは、黙って捨てない。**
///
/// このデスクトップ版は打てば勝手に保存されるので、閉じる前に一度書いてから閉じる
/// ── 訊かない（訊くほうが、このデスクトップ版の作りに合っていない）。
/// **机の上の上限**（依頼 556・本人「50個とかかなぁ」）。
///
/// 依頼 555 で「押したぶんだけ増える」に戻したので、一覧を上から順に
/// たどると際限なく増える ── 依頼 377 が心配していたのはこれだった
/// （1002 本のフォルダを上から見ただけで 1002 タブ）。
///
/// **これはネットワークであって、道具ではない。** 50 枚も並べば帯はとうに読めないので、
/// ふだんの片付けは「このノート以外をすべて閉じる」でやる。ここが効くのは、
/// 片付けを忘れて何百本も見て回った日だけ ── だから低くしない。低くすると、
/// **まだ使っているタブが黙って消える**ほうの害が出る。
const TABS_MAX = 50;

/// 最後に見た順を憶えるための番号。**並び順は「古さ」ではない** ──
/// 新しいタブはいまのすぐ右に入るので、左にあるものが古いとは限らない。
let tabTick = 0;
function markSeen() {
    const t = tabs.find((x) => x.path === showing);
    if (t) t.seen = ++tabTick;
}

/// 上限を超えたぶんだけ、**古いものから**閉じる。
///
/// **触らないものが二つある。** いま出しているタブと、**書きかけを抱えた
/// タブ** ── 閉じる前に書き戻すパスはあるが（`closeTab`）、打っている途中の
/// ものを黙って片付けるくらいなら、上限を超えているほうがよい。
/// 全部が書きかけなら、1 つも閉じずに超えたままにする。
async function trimTabs() {
    if (tabs.length <= TABS_MAX) return;
    markSeen();
    const old = tabs
        .filter((t) => t.path !== showing && !(t.keep && t.keep.dirty))
        .sort((a, b) => (a.seen || 0) - (b.seen || 0));
    let over = tabs.length - TABS_MAX;
    let gone = 0;
    for (const t of old) {
        if (over <= 0) break;
        await closeTab(t.path);
        over -= 1;
        gone += 1;
    }
    // **黙って消さない。** 押していないのにタブが減るのは、画面の上では
    // 「勝手に閉じた」にしか見えない。
    if (gone) say('タブが ' + TABS_MAX + ' 枚を超えたので、古いものを ' + gone + ' 枚閉じました');
}

async function closeTab(path) {
    const at = tabs.findIndex((t) => t.path === path);
    if (at < 0) return;
    if (path === showing) {
        clearTimeout(readTimer);
        await syncRead(true);
        if (state.dirty) await leaveSave();
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
/// 閉じるためだけに画面を組み直すのは高いし、caret が飛ぶ。
async function saveTab(t) {
    const k = t.keep;
    if (!k) return;
    try {
        await ask('write', { path: t.path, text: k.head + k.body, stamp: k.stamp });
    } catch { /* 書けなくても、閉じるのは止めない ── 文字はファイルに残っている */ }
}

/// **消えたノートのタブを外す**（依頼 612・本人「ゴミ箱にすてたはずのノートが
/// タブの表示に残り続けてしまう」）。
///
/// **消すパスは四つある** ── ⋯ の「ゴミ箱へ入れる」、選んでまとめて、
/// フォルダごと、同期が向こうの削除を下ろしたとき。どれもタブに触って
/// いなかったので、**四か所に同じ一行を足す形**になる ── そういうものは
/// たいてい、三か所目で忘れる。数え直したあとの `reload` で一度だけ見る。
///
/// **書かずに外す。** `closeTab` は書きかけをファイルへ落とすので、
/// **消したはずのノートが書き戻って生き返る。**
///
/// **一覧に無い＝消えた、ではない。** 外付けを抜いた・ネットワークの
/// 保存ディレクトリが一度切れた回も一覧からは消える ── そこを閉じると、
/// 繋ぎ直したときに机が空になっている。**困っている保存ディレクトリの下は
/// 触らない。** 単発で開いている一本（`guest`）も、索引の外なので触らない。
function dropGoneTabs() {
    if (state.guest || !tabs.length) return;
    const here = new Set(state.notes.map((n) => n.path));
    const troubled = Object.keys(state.placeTrouble || {});
    const gone = tabs.filter((t) => !here.has(t.path)
        && !troubled.some((d) => t.path.startsWith(d + "/")));
    if (!gone.length) return;
    const lost = new Set(gone.map((t) => t.path));
    const at = tabs.findIndex((t) => t.path === showing);
    tabs = tabs.filter((t) => !lost.has(t.path));
    // **たどった道からも抜く。** `trail` は作り替えないで抜く（同じ並びを
    // 見ている `trailAt` がずれる）── 抜いたぶんだけ後ろへ詰める。
    for (let i = trail.length - 1; i >= 0; i -= 1) {
        if (!lost.has(trail[i])) continue;
        trail.splice(i, 1);
        if (i <= trailAt) trailAt -= 1;
    }
    for (const p of lost) delete incomings[p];
    rememberTabs();
    if (!lost.has(showing)) { drawStrip(); return; }
    // 出していたタブが消えた ── **左の隣へ。** そこから来たので。
    showing = null;
    if (!tabs.length) {
        state.open = null;
        state.dirty = false;
        applyView();
        drawStrip();
        return;
    }
    const next = tabs[Math.min(Math.max(0, at - 1), tabs.length - 1)];
    openNote(next.path, { keep: true });
}

/// 開いていたタブを憶える。**次に開いたとき、同じ机に戻る**（iPhone と同じ）。
/// 左の帯と一覧の幅を、掴んで動かす（依頼 596）。
///
/// **憶えるのはデスクトップ版の大きさと同じ道**（`remember`）── 次に開いたとき、
/// 前と同じ幅で出る。**狭すぎ・広すぎは止める**: 一覧が 140px を切ると
/// 題が一文字も読めず、画面の半分を越えるとノートが痩せる。
const GRAB = {
    rail: { el: 'rail', least: 120, most: 420, key: 'railWidth' },
    list: { el: 'list', least: 160, most: 640, key: 'listWidth' },
};

function setPaneWidth(which, px) {
    const g = GRAB[which];
    if (!g) return;
    // **画面の半分は越えさせない。** デスクトップ版を細くしたときに、二本で埋まって
    // ノートが見えなくなる ── 掴んで戻せない形にはしない。
    const cap = Math.min(g.most, Math.round(window.innerWidth * 0.45));
    const w = Math.max(g.least, Math.min(cap, Math.round(px)));
    const node = el(g.el);
    if (node) node.style.width = w + 'px';
    return w;
}

function grabsUp() {
    for (const [which, g] of Object.entries(GRAB)) {
        const bar = document.querySelector('.grab[data-for="' + which + '"]');
        const node = el(g.el);
        if (!bar || !node) continue;
        bar.onmousedown = (e) => {
            if (e.button !== 0) return;
            e.preventDefault();
            const from = e.clientX;
            const was = node.getBoundingClientRect().width;
            bar.classList.add('on');
            document.body.classList.add('grabbing');
            const move = (ev) => setPaneWidth(which, was + (ev.clientX - from));
            const up = () => {
                document.removeEventListener('mousemove', move);
                document.removeEventListener('mouseup', up);
                bar.classList.remove('on');
                document.body.classList.remove('grabbing');
                if (!state.guest) {
                    window.amber.remember({
                        [g.key]: Math.round(node.getBoundingClientRect().width) });
                }
            };
            document.addEventListener('mousemove', move);
            document.addEventListener('mouseup', up);
        };
        // **二度押しで元に戻す。** 動かしすぎた人が、掴み直さずに戻せる。
        bar.ondblclick = () => {
            node.style.width = '';
            if (!state.guest) window.amber.remember({ [g.key]: null });
        };
    }
}

function rememberTabs() {
    if (state.guest) return;
    window.amber.remember({ tabs: tabs.map((t) => t.path) });
}

async function openNote(path, opts) {
    // **ノートを開いたら、カレンダーからは出る**（依頼 478）── 同じ場所を
    // 使うので、開いたノートがカレンダーの裏に隠れることになる。
    calOn = false;
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
                await syncRead(true);
                if (state.dirty) await leaveSave();
                // 離れるノートの名前を、題に揃えてから（依頼 492・決めごと 2）。
                if (state.open && state.open.path !== path) await settleName(state.open.path);
                stashTab();
                showing = path;
                // **待っているあいだに、机の上が変わっていることがある。**
                // 上の `await`（書き戻し・保存・改名）の最中に `reload` が
                // 走ると、`dropGoneTabs` がタブを作り直す ── **さっき数えた
                // 番号は、もう別のタブを指している**（減っていれば何も指さない）。
                // だから番号ではなく、パスで取り直す。
                //
                // 取り直さないと `tabs[at]` が `undefined` になり、
                // `restoreTab` が「`keep` を読めません」で落ちて、**ノートが
                // 開かないまま先へ進む** ── 総ざらいで、共有をやめた直後と
                // 保存ディレクトリを移した直後に実際に落ちていた
                // （どちらもノートのパスが変わる操作・2026-09-20）。
                // 落ちるのは開く側なので、**そのあとの操作はぜんぶ前のノートに
                // 当たる**（同期の段が「上がりません」と言っていたのはこれ）。
                //
                // **しまってあるなら、読み直さない。** 打ちかけの文字を
                // 失わないための机なので、戻るだけで捨てては元も子もない。
                const back = tabs.find((t) => t.path === path);
                if (back && restoreTab(back)) {
                    if (!opts || !opts.walking) trailPush(path);
                    afterTab();
                    return;
                }
                // 机から消えていたら、置き直してからファイルを読む ──
                // `showing` だけが机に無いパスを指していると、次にタブを
                // 触ったところで同じ落ち方をする。
                if (!back) tabs.push({ path, keep: null });
            } else if (opts && opts.keep && tabs[at].keep) {
                // 同じタブを押しただけ ── 何もしない。
                return;
            }
        } else if (opts && opts.tab && showing) {
            // **新しいタブは、いまのすぐ右へ。** 端に足すと、たどっていた
            // 順と並びが合わなくなる。
            clearTimeout(readTimer);
            await syncRead(true);
            if (state.dirty) await leaveSave();
            stashTab();
            tabs.splice(tabs.findIndex((t) => t.path === showing) + 1, 0, { path, keep: null });
            showing = path;
        } else if (showing) {
            // **押したぶんだけ、タブが増える**（依頼 555・本人が依頼 522 を撤回）。
            // 前は VS Code のプレビュータブの決まりで、一覧から押しただけの
            // ノートは「仮のタブ」で開き、次を押すと入れ替わっていた ──
            // **実際に動かすと分かりにくい**（本人）。見たものは残る。
            // 置く先は `⌥` 押しと同じ「いまのすぐ右」── たどった順と並びが合う。
            clearTimeout(readTimer);
            await syncRead(true);
            if (state.dirty) await leaveSave();
            if (state.open && state.open.path !== path) await settleName(state.open.path);
            stashTab();
            const now = tabs.findIndex((t) => t.path === showing);
            tabs.splice(now + 1, 0, { path, keep: null });
            showing = path;
        } else {
            tabs = [{ path, keep: null }];
            showing = path;
        }
        rememberTabs();
        await trimTabs();
    }
    // たどっている最中は積まない ── 積むと前へ戻れなくなる。
    if (!opts || !opts.walking) trailPush(path);
    // 開く前に、書きかけを置いていかない。**表示画面はまだ文字になっていない**
    // ── DOM に打った跡が `syncRead` を通るまで、エディタは前の文字のまま。
    // 先に戻さないと、最後の数百ミリ秒ぶんが黙って消える。
    clearTimeout(readTimer);
    await syncRead(true);
    if (state.dirty) await leaveSave();
    // 離れるノートの名前を、題に揃えてから（依頼 492・決めごと 2）。
    if (state.open && state.open.path !== path && !(opts && opts.guest)) await settleName(state.open.path);
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
    // 別のノートを開いたら、戻りパスは捨てる ── 別のノートの姿をここへ
    // 戻せると、一度の押し間違いで二本まとめて壊れる。
    if (!state.open || state.open.path !== path) {
        forgetSteps();
        // 前のノートの文字を画面に残さない ── 「コード」の画面では組み直さない
        // ので、残すと次の書き戻しがそれを今のノートへ書く（`readDrawn`）。
        el('read').replaceChildren();
    }
    // 同じノートを開き直すときも、画面はもう今の文字ではない（エディタは
    // このあとファイルの文字に置き換わる。組み直せばラベルは付け直される）。
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
    // **錠はここで見る**（依頼 629）── 一覧から開くパスと、机のタブへ戻るパスの
    // 両方がここを通る。タブへ戻るパスは途中で返るので、開くところに書いたら
    // 「今だけ編集する」が別のノートへ持ち越された（本物のデスクトップ版で踏んだ）。
    for (const at of [...unlockedNow]) {
        if (!state.open || at !== state.open.path) unlockedNow.delete(at);
    }
    loadLock();
    markSeen();
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
    // 直している最中は、下から書き換えない ── 打っている文字が消える。
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
        say(to ? 'タイトルを「' + to + '」にしました' : 'タイトルを外しました（見出しから決まります）');
        // **欄から出た瞬間に、ファイル名も題に**（依頼 492・決めごと 1）。
        if (state.open) await settleName(state.open.path);
    } catch (e) {
        say('タイトルを直せません: ' + why(e));
        drawTitle();
    }
}

el('title').onclick = () => { if (state.open && el('title').contentEditable !== 'plaintext-only') renameTitle(); };
el('title').onkeydown = (e) => {
    if (isEnter(e)) { e.preventDefault(); el('title').blur(); return; }
    // **やめたら、何も変えない。** 打ちかけの文字を捨てて元の題に戻す。
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

/* ── 編集画面の画像 ── */

/// `![](attachments/…)` の行の下に、実物を小さく出す。
///
/// **文字は消さない。** ファイルは Markdown のままで、行はそこにある ──
/// 消して画像に置き換えると、消した文字を直す方法が無くなる（パスを一文字
/// 変えたいだけのときに困る）。Monaco の `view zone` は行と行のあいだに
/// 空きを作る仕掛けで、そこへ画像を置く。
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
        // 行そのものが画像のときだけ。文の途中の画像は文の中に居る。
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
            // **読めない画像は、黙って空けない。** 貼り間違いに気づけるように、
            // 何が読めなかったのかを出す ── ただし、**手を尽くしてから**。
            // 前はここだけ助け船（`fileBytes`）を持っておらず、表示画面では
            // 出る画像が、コード画面でだけ「読めません」と言っていた。
            if (/^file:/i.test(w.src)) img.src = w.src;
            else showPicture(img, absPath(w.src, dir), () => {
                box.classList.add('bad');
                box.textContent = 'この画像は読めません: ' + w.src;
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

/* ── 表示画面で書く ── */

/// **表示画面は、読むだけの画面ではない。**
///
/// 押して入力欄を開く、という一手を挟まない ── メモ帳と同じで、置いて
/// 打てる。記号は見えないまま、下の帯（太字・斜体…）がそのまま効く。
///
/// 仕掛けは3 つ:
///
///   * 画面ぜんぶを `contenteditable` にする。打った跡は DOM に付く
///   * 落ち着いたら **DOM を Markdown に戻して**、いつもの保存を通す
///   * **戻せないかたまりは、触らせない** ── 枠・表・図・注記・画像は
///     `contenteditable="false"` にして、押したら編集画面へ送る。
///     打てるのに保存されない、が**いちばん悪い**
///
/// 戻せるのは `to_html` が出す札だけ ── 語彙はこちらが決めているので、
/// 逆に読むのも数十行で済む。**外から来た HTML は入ってこない**（貼り付けは
/// 文字だけにしている）。
let readTimer = null;
/// 書き戻している間は、描き直しを止める（自分の保存で自分を消さない）。
let syncing = false;

/// **表示画面に組んであるのは、どのノートの文字か。**
///
/// `drawRead` が描いたときに札（`data-of`）を置き、**それ以外で文字が動いたら
/// 剥がす** ── 別のノートを開いたとき、「コード」の画面で打ったとき。
/// 書き戻し（`syncRead`）は、ラベルがいま開いているノートを指しているときだけ
/// 通す。
///
/// これが無いと、表示画面は「いま出ているのは、いま開いているノートの文字」と
/// 思い込んだまま書き戻す。「コード」の画面ではノートを替えても組み直さない
/// ので、画面は**前のノートの文字のまま** ── 次に別のノートへ替えた瞬間、
/// その文字が今のノートへ書き込まれる（実際に二本のノートが、前書きだけ
/// 自分のまま**本文が別のノート**になった）。同じパスで、「コード」で打った
/// 行が、ノートを替えた瞬間に**描いた時の文字へ戻される**。
function readDrawn(path) { el('read').dataset.of = path; }
function readStale() { delete el('read').dataset.of; }
function readCurrent() { return !!state.open && el('read').dataset.of === state.open.path; }

/// 打った跡を拾う。**画面ぜんぶが入力欄なので、`input` 一本で足りる。**
el('read').addEventListener('input', () => { readChanged(); tableBar(); });
document.addEventListener('selectionchange', () => {
    if (view !== 'write' && state.open) tableBar();
});
el('read').addEventListener('blur', () => { clearTimeout(readTimer); syncRead(true); }, true);

/// 貼り付けは**文字だけ**入れる ── ただし、よそから来た HTML は
/// **Markdown の文字に直してから**（依頼 421）。
///
/// HTML をそのまま画面に入れると、`inlineToMd` が知らないラベルが混ざり、
/// 文字に戻したときに消える ── 貼ったつもりのものが無い、がいちばん悪い。
/// かといって文字だけにすると、**見出しも一覧もリンクも落ちる**（ブラウザで
/// 選んでコピーした人が欲しかったのは、まさにそこ）。
///
/// だから途中に 1 つ挟む: `webToMd` が均して、`blockToMd` が文字にする。
/// 画像の貼り付けは別に拾っている。
el('read').addEventListener('paste', (e) => {
    if (!e.clipboardData) return;
    // **画像そのもののときだけ、下の受け口に渡す**（依頼 616）── Excel は
    // 表と一緒に絵も載せてくるので、「絵があれば絵」で帰ると表が写真になる。
    if ([...e.clipboardData.items].some((i) => i.kind === 'file' && i.type.startsWith('image/'))
        && justAPicture(e.clipboardData)) return;
    e.preventDefault();
    const html = e.clipboardData.getData('text/html');
    const clean = html && html.trim() ? webClean(html, clipBase(html)) : null;
    // **セルひとつは、表ではない**（依頼 616・本人「不思議なところで改行する」）。
    // Excel はセルを一つ写しても `<table>` で寄こすので、そのまま入れると
    // 文の途中に段が割り込んで**そこで行が切れる。** 文字だけ入れる。
    const one = oneCell(clean);
    if (one !== null) {
        document.execCommand('insertText', false, one);
        readChanged();
        return;
    }
    // **表のセルには文字だけ（改行は空白に）。項目には文字だけ（改行ごとに
    // 項目を増やす）。** かたまりのまま入れると、セルでは見出しの文字だけが
    // 混ざって一覧が消え、項目では一覧が入れ子になった（ネットワークが捕まえた・
    // 2026-09-10・本人が決めた）。
    {
        const plain = e.clipboardData.getData('text/plain')
            || (clean ? [...clean.children].map((n) => n.textContent).join('\n') : '');
        const line = lineAt(el('read'));
        if (line && line.closest('td, th')) {
            document.execCommand('insertText', false, plain.replace(/\s*\n\s*/g, ' ').trim());
            return;
        }
        if (line && line.tagName === 'LI') {
            pasteLines(el('read'), plain.split('\n'));
            readChanged();
            return;
        }
    }
    // **表示画面には、描いた形のまま入れる。** 文字（`## 段取り`）を入れると
    // そのまま `##` という文字が出る ── 実際にそうなった。均したあとのラベルは
    // amber が知っているものだけなので、そのまま食える。
    if (clean && edges(clean.textContent)) {
        document.execCommand('insertHTML', false, clean.innerHTML);
        // **貼られた枠に、元の文字を持たせる。** 枠は触れないかたまりで、
        // `paperToMd` は元の文字（`data-md`）が無いと**何も書き戻さない**
        // ── 貼った瞬間から保存が黙って止まる。
        for (const n of el('read').querySelectorAll('pre')) {
            if (n.dataset.md === undefined) n.dataset.md = blockToMd(n);
        }
        armRead();
        readChanged();
        return;
    }
    document.execCommand('insertText', false, e.clipboardData.getData('text/plain'));
});

/// セルひとつだけの表なら、その文字。ちがえば `null`（依頼 616）。
///
/// **Excel はセルを一つ写しても表で寄こす。** 文の途中に貼ると段が割り込んで
/// 行が切れ、貼った人には「不思議なところで改行した」としか見えない。
/// 末尾の改行も落とす ── Excel は `text/plain` にも改行を一つ足してくる。
function oneCell(clean) {
    if (!clean) return null;
    const kids = [...clean.children];
    if (kids.length !== 1 || kids[0].tagName !== 'TABLE') return null;
    const cells = kids[0].querySelectorAll('td, th');
    if (cells.length !== 1) return null;
    return cells[0].textContent.replace(/\s*\n\s*/g, ' ').trim();
}

/// 貼られたものを、**編集画面に入れる文字**にする。
///
/// **文字のほうが長ければ、文字を採る。** ブラウザによっては `text/html` に
/// 書式だけの殻を入れてくることがあり、均すと中身がほとんど残らない ──
/// そのとき HTML を採ると、貼ったものが消えたように見える。
function clipText(data) {
    const plain = data.getData('text/plain');
    const html = data.getData('text/html');
    if (!html || !html.trim()) return plain;
    // **セルひとつは、表ではない**（依頼 616）── `| 売上 |` と三行に
    // なるより、`売上` の四文字が入るほうが、写した人の思ったこと。
    const one = oneCell(webClean(html, clipBase(html)));
    if (one !== null) return one;
    let md = '';
    try {
        md = webToMd(html, clipBase(html));
    } catch { /* 読めない HTML は、文字として貼る */ }
    return md && md.length >= plain.trim().length / 2 ? md : plain;
}

/// コピー元のページ。**ブラウザが書いてくれることがある** ── Chrome と
/// Safari は `text/html` の頭に `<!--StartFragment-->` と一緒に元の URL を
/// 添える。無ければ相対のままにする（何も無いよりは、そのほうがまし）。
function clipBase(html) {
    return /<html[^>]*\ssourceurl=["']([^"']+)["']/i.exec(html)?.[1]
        || /<!--\s*sourceURL:\s*(\S+?)\s*-->/i.exec(html)?.[1]
        || '';
}

/// Enter で `<div>` ではなく `<p>` を作らせる。
///
/// **書式は `style` にさせない**（`styleWithCSS` は偽のまま）── 真にすると
/// 太字が `<b>` ではなく `<span style="font-weight:bold">` になり、文字に
/// 戻すときに書式が落ちる。**太字にしたのに保存されない**、という形で出た。
/// 色だけは `<font color>` で来るので、そちらを読む（`inlineToMd`）。
try {
    document.execCommand('defaultParagraphSeparator', false, 'p');
    document.execCommand('styleWithCSS', false, false);
} catch { /* 古い呼び方なので、断られても書けなくはならない */ }

/// 触ってはいけないかたまりか。
function richBlock(node) {
    if (!node || node.nodeType !== 1) return false;
    // **表と注記とコードブロックは触れる。** 文字に戻せる形をしているので、
    // 触らせない理由が無い ── 触れないままだと「表示画面だけで完結できる」が
    // 嘘になる。図と画像だけは、戻せないので編集画面へ送る。
    //
    // **コードブロックは 2026-09-20 に触れるようにした**（依頼 644・本人
    // 「IT素人にむけ、表示モードで操作が完結する思想なんだから、これは
    // 修正してほしい」）。戻せなかったのではなく、**戻す道を書いていなかった**
    // だけだった ── 中身に色は付けていない（`hljs` を使っていない）ので、
    // `<code>` の中の文字がそのまま元の文字で、`blockToMd` の `PRE` が
    // `` ``` `` で挟み直せば済む。写すボタン（`.cp`）は `<pre>` の直下に
    // 居て `<code>` の中ではないので、文字には混ざらない。
    if (node.tagName === 'FIGURE') return true;
    if (node.classList.contains('mermaid')) return true;
    // **図の枠だけは、触れないまま。** あれは押すと工房が開く場所で、
    // 中身は mermaid の綴り ── 直すのは工房の仕事。
    if (node.tagName === 'PRE' && node.querySelector('code.language-mermaid')) return true;
    // **折りたたみは、まるごと元の文字で返す**（依頼 619）。
    //
    // 中は畳んであるので、そこを文字に戻すのは「見えていないものを
    // 組み直す」こと ── 閉じたまま保存しただけで中身が書き換わる、が
    // いちばん怖い。押して開け閉めするのは**見え方**の話で、文字は触らない。
    if (node.tagName === 'DETAILS') return true;
    // **中に枠や図を抱えたかたまりも、触らせない。**
    //
    // `> ``` ` のような引用は、外は引用・中は枠。外を触れるままにすると、
    // 中を文字に戻すのは `blockToMd` の仕事になり、あちらは枠を知らない
    // （知らないラベルは中身だけ取る）── `` ` `` が行の途中のコードとして
    // 読み直され、**保存のたびに形が変わり続けた**（一周目で `> ` + 中身、
    // 二周目でさらに `` ` `` が増える）。同期していれば毎回差分になる。
    //
    // 元の文字は外のかたまりが持っている（`data-md`）ので、外ごと返せば
    // 一文字も失わない。中を直したい人は「コード」の画面へ ── 触れない
    // ものが一つ増えるが、**壊れるより狭いほうがよい**。
    return !!node.querySelector('pre, .mermaid');
}

/// ラベルを掛け替える。**元の行と元の文字を、新しい節へ持たせる。**
///
/// 掛け替えるのは二か所 ── 画像を `<figure>` で包むとき（`findPictures`）と、
/// 行の記号を外して段落にするとき（`asPara`）。持たせないと、次の書き戻しで
/// **その一行が元の文字を失う**（触れないかたまりなら `null` になり、保存が
/// 止まる）。
///
/// **切り出しの中に置く。** iPhone も同じ掛け替えをする ── 外に置いていた
/// ときは、iPhone のバンドルに `keepMark` が入っていなかった。
function keepMark(from, to) {
    for (const k of ['line', 'span', 'md']) {
        if (from.dataset[k] !== undefined) to.dataset[k] = from.dataset[k];
    }
}

/// 描いたあとの仕込み。
///
/// **書いてあった文字を、かたまりごとに持たせておく**（`data-md`）── 戻せない
/// ものは、これをそのまま返す。行番号は `to_html` が差している。
/// **箱と文字を受け取る形。** ここから下（`armPaper`・`paperToMd`・
/// `blockToMd`・`inlineToMd`・`richBlock`・`checkEnter`）は、画面のどこにも
/// 触らない ── 渡された箱と文字だけを見る。
///
/// そうしてあるのは、**iPhone が同じものを使うため**。iPhone の「表示」も
/// `WKWebView` の `contenteditable` で、同じ組み方・同じ書き戻し方をする。
/// 書き戻しをもう一組 Swift で書けば、**同じノートが端末によって別の文字に
/// 保存される** ── 失うのはたいてい表とセルと図で、気づくのは何回か保存
/// したあと。`scripts/paper-test.js` が往復を見ているので、iPhone が使うのは
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
        // ずれた番号で文字を切り直すと、図の「元の文字」が**別の場所の文字**に
        // なる。次の保存でそれが図の場所へ書き戻され、**図が丸ごと消える**。
        //
        // 実際に消した。「マーメイドの図がいつのまにか消えた」はこれ ──
        // キーボードで消していないのに消えるので、原因がどこにも見えない。
        //
        // 触れないかたまりの元の文字は、**組み直したときにしか変わらない**
        // （図を直す工房は `readSourceEdit` を通り、あちらは必ず組み直す）
        // ので、既に持っているならそれが正しい。
        if (node.dataset.md === undefined && !Number.isNaN(at)) {
            node.dataset.md = src.slice(at, at + span).join('\n');
        }
        if (richBlock(node)) {
            node.contentEditable = 'false';
            // **折りたたみは、押しても画面を替えない**（依頼 619）── 押すのは
            // 開け閉めのため。中を直したい人は、開いてから枠を押す。
            node.title = node.tagName === 'DETAILS'
                ? '押すと、開いたり閉じたりします'
                : (node.classList.contains('mermaid') || node.querySelector('code.language-mermaid')
                    ? '押すと、図を見ながら直せます'
                    : '押すと、「コード」のその行へ');
        }
    }
    // **枠には、押すだけで写せる札**（依頼 614・本人「押下するだけで
    // コピーできるコピーボタンがあるといい」）。
    //
    // **中に置いてよい。** 枠は触れないかたまりなので、書き戻すのは
    // `data-md`（元の文字）── ラベルの「コピー」が文字に混ざることはない。
    // **図の枠には付けない** ── あれは押すと工房が開く場所で、写したい
    // のは図であって文字ではない。
    for (const pre of box.querySelectorAll('pre')) {
        if (pre.querySelector(':scope > .cp')) continue;
        if (pre.classList.contains('mermaid') || pre.querySelector('code.language-mermaid')) continue;
        const b = document.createElement('button');
        b.type = 'button';
        b.className = 'cp';
        b.textContent = 'コピー';
        b.contentEditable = 'false';
        b.title = 'この枠の中身を写す';
        // **押しを枠に渡さない。** 枠を押すと「コード」の画面へ飛ぶので、
        // 写したいだけの人が別の画面に着く。
        b.onmousedown = (e) => e.stopPropagation();
        b.onclick = async (e) => {
            e.preventDefault();
            e.stopPropagation();
            const text = codeOf(pre);
            try {
                await navigator.clipboard.writeText(text);
                b.textContent = '写しました';
                b.classList.add('done');
                setTimeout(() => { b.textContent = 'コピー'; b.classList.remove('done'); }, 1400);
            } catch (err) {
                say('写せません: ' + why(err));
            }
        };
        pre.appendChild(b);
    }
    // セルは文字ではなく操作 ── 中に caret が入ると、押せるものが打てるものに見える。
    for (const b of box.querySelectorAll('.box')) b.contentEditable = 'false';
    // 注記の種類のラベルは、中身ではなく `> [!NOTE]` の言い換え ── 打てると
    // 「注意」を「ちゅうい」に直せてしまい、それは記法を壊す。
    for (const h of box.querySelectorAll('.alert-h')) h.contentEditable = 'false';
}

/// 枠の中の文字（依頼 614）。**画面から拾わない。**
///
/// 表示画面の枠は Monaco が色を付けたあとの姿で、**改行は `<br>`、空白は
/// `&nbsp;`** になっている ── `textContent` で拾うと、写した文字が
/// 一行に潰れて空白も別の文字になる（実際にそうなった）。
/// 元の文字は枠が `data-md` に持っているので、囲みだけ外して返す。
function codeOf(pre) {
    const md = pre.dataset ? pre.dataset.md : '';
    if (md) {
        const lines = md.split('\n');
        if (/^\s*(`{3,}|~{3,})/.test(lines[0])) {
            lines.shift();
            if (lines.length && /^\s*(`{3,}|~{3,})\s*$/.test(lines[lines.length - 1])) lines.pop();
        }
        return lines.join('\n');
    }
    // 元の文字を持たない枠（よそから貼られたもの）は、文字から拾う。
    const code = pre.querySelector('code');
    return (code || pre).textContent.replace(/\u00a0/g, ' ').replace(/\n+$/, '');
}

/// DOM を Markdown に戻す。
function paperToMd(box, head) {
    const out = [];
    for (const node of box.childNodes) {
        // **かたまりの外に、裸の文字が居ることがある。**
        //
        // まっさらなノートに打った一文字目は `<p>` に包まれず、箱の直下の
        // 文字の節として入る（WebKit がそうする）。かたまり（`children`）
        // だけを見ていたので、**打ったのに保存されなかった** ── iPhone の
        // シミュレータで、新しいノートに打った字が一つも残らなかった
        // （2026-09-21）。打ったのに保存されない、がいちばん悪い。
        //
        // かたまりとかたまりのあいだの改行や空白は、文字ではない ── 落とす。
        if (node.nodeType !== 1) {
            if (node.nodeType === 3) {
                const t = edges(node.data);
                if (t) out.push(t);
            }
            continue;
        }
        // **画面の道具（選び口など）は文字ではない。** 書き戻さない。
        if (node.classList && node.classList.contains('gadget')) continue;
        if (richBlock(node)) {
            // **書いてあった文字をそのまま返す。** 図や枠を読み解いて
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

/// 枠の中の文字を、画面から拾う（依頼 644）。
///
/// **`textContent` では足りない。** 色を付けたあとの枠は Monaco が組んだ
/// 姿で、**改行は `<br>`、空白は `&nbsp;`** になっている（`codeOf` の註と
/// 同じ話）── そのまま拾うと、直した枠が**一行に潰れて**、空白がぜんぶ
/// 別の文字（U+00A0）で保存される。本物のアプリで確かめて出てきた
/// （2026-09-20 ── const・a・= のあいだが U+00A0 でファイルに残った）。
///
/// 色を付けていない枠（言語を書いていないもの）は素の文字なので、
/// どちらの形でもここを通れば同じ答えになる。
function fenceText(box) {
    let out = '';
    const walk = (n) => {
        for (const kid of n.childNodes) {
            if (kid.nodeType === 3) out += kid.data;
            else if (kid.tagName === 'BR') out += '\n';
            else if (kid.nodeType === 1) walk(kid);
        }
    };
    walk(box);
    return out.replace(/\u00a0/g, ' ').replace(/\n+$/, '');
}

function blockToMd(node, depth = 0) {
    if (node.nodeType === 3) return node.data.trim() ? node.data : null;
    if (node.nodeType !== 1) return null;
    if (node.classList.contains('gadget')) return null;      // 画面の道具 ── 文字ではない
    const pad = '  '.repeat(depth);
    // 注記は `> [!NOTE]` に戻す。**種類のラベルは中身ではない**ので、
    // 見出しの一行（`.alert-h`）は書き出さず、class から取り直す。
    if (node.classList.contains('alert')) {
        const kind = [...node.classList].find((c) => c !== 'alert') || 'note';
        const body = [...node.children]
            .filter((c) => !c.classList.contains('alert-h'))
            .map((c) => blockToMd(c))
            .filter((x) => x !== null).join('\n\n');
        // **中身が空なら札だけ。** `>` の行を足すと、表示画面で入れた直後の
        // 注記（`> [!NOTE]`）が次の保存で `> [!NOTE]⏎>` に変わる ── 同期先に
        // 差分が一度飛ぶ（ネットワークが捕まえた・2026-09-10）。
        if (!body.trim()) return '> [!' + kind.toUpperCase() + ']';
        return ['> [!' + kind.toUpperCase() + ']',
                ...body.split('\n').map((l) => (l ? '> ' + l : '>'))].join('\n');
    }
    switch (node.tagName) {
        case 'H1': case 'H2': case 'H3': case 'H4': case 'H5': case 'H6': {
            // **文字の無い見出しは書かない。** 画面には出ている（打てる形）が、
            // 文字を打つまでファイルには出さない（本人が決めた・2026-09-11・
            // ネットワークの決めごと 7）── 空の `# ` が同期先へ飛ばない。
            const t = inlineToMd(node);
            if (!edges(t)) return null;
            return '#'.repeat(Number(node.tagName[1])) + ' ' + t;
        }
        case 'UL': case 'OL': {
            // **文字に戻すあいだ、画面には一切触らない。**
            //
            // 前はここでセル（`.box`）と入れ子の箇条書きを `remove()` して、
            // 読み終えてから付け直していた。入れ子は戻していたが**セルは
            // 戻していなかった** ── 一度書き戻すとセルが画面から消え、次の
            // 書き戻しではただの `- やること` になって、チェックが
            // ファイルから消えた。「表示画面で打っていたらチェックリストが
            // 消えた」はこれ。
            //
            // 読むだけで済むものを、動かして読む理由は無い。
            const rows = [];
            // **始まりの番号は、書いた人のもの。** `3. 4.` と書いた一覧を
            // `1. 2.` に振り直さない（core は `<ol start>` で憶えている）。
            let n = (Number(node.getAttribute('start')) || 1) - 1;
            for (const li of node.children) {
                if (li.tagName !== 'LI') continue;
                // **文字の無い項目は書かない**（入れ子も無ければ）── 項目の末尾で
                // Enter を押した瞬間の `- ` や `- [ ] ` を、文字を打つまでファイルに
                // 出さない（本人が決めた・2026-09-11・ネットワークの決めごと 11）。
                if (!edges(inlineToMd(li)) && !li.querySelector(':scope > ul, :scope > ol')) continue;
                n += 1;
                const mark = li.querySelector(':scope > .box');
                // **書いたマークを、そのまま返す。** `* ` を `- ` に、`1) ` を
                // `1. ` に、`1. 1. 1.` を `1. 2. 3.` に丸めない ── どれも
                // 見え方は同じだが、人の書いた行を書き換えることになる
                // （同期していれば、開くたびに向こうへ差分が飛ぶ）。
                // 憶えていないもの（前の版で描いた画面・画面の上で作った行）は、
                // これまで通りの形で書く。
                const was = li.dataset ? li.dataset.mark : '';
                const bullet = was || (node.tagName === 'OL' ? n + '. ' : '- ');
                const done = mark && mark.getAttribute('aria-pressed') === 'true';
                const head = mark
                    ? bullet + '[' + (done ? (li.dataset.box === 'X' ? 'X' : 'x') : ' ') + '] '
                    : bullet;
                rows.push(pad + head + inlineToMd(li).trim());
                // 入れ子は項目の中に居る。文字の上では、その項目の下に付く。
                for (const x of li.children) {
                    if (['UL', 'OL'].includes(x.tagName)) {
                        const inner = blockToMd(x, depth + 1);
                        if (inner !== null) rows.push(inner);
                    }
                }
            }
            return rows.length ? rows.join('\n') : null;
        }
        case 'TABLE': {
            const rows = [...node.querySelectorAll('tr')];
            if (!rows.length) return null;
            const cells = (tr) => [...tr.children]
                .map((c) => inlineToMd(c).trim().replace(/\|/g, '\\|') || '　');
            // **元の文字があるなら、それを返す。** `:---` と `---` は
            // core の `Align` では同じ値になる（見え方は同じでよい）が、
            // 文字に戻すときに丸めると**人の書いた行が書き換わる**。
            // 憶えていないもの（前の版で描いた画面）は、見え方から作る。
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
        case 'PRE': {
            // ここへ来るのは二とおり ── よそから貼られた HTML（依頼 421）と、
            // **表示画面で直したコードブロック**（依頼 644）。
            //
            // **中身は `<code>` から取る。** `node.textContent` だと、枠の中に
            // 置いてある「コピー」のボタン（依頼 614）の文字まで混ざる。
            const code = node.querySelector(':scope > code');
            const body = fenceText(code || node);
            // **触っていない枠は、読んだときの文字をそのまま返す**（依頼 644）。
            //
            // 囲みは `` ``` `` とは限らず `~~~` のこともあり、組み直すと
            // **触ってもいない枠が同期先で差分になる**（往復の試験が捕まえた ──
            // `~~~` で書いた枠が `` ``` `` で戻った）。
            const was = node.dataset.md;
            if (was !== undefined) {
                const lines = was.split('\n');
                const closed = lines.length > 1
                    && /^\s*(`{3,}|~{3,})\s*$/.test(lines[lines.length - 1]);
                if (lines.slice(1, closed ? -1 : undefined).join('\n') === body) return was;
            }
            // 中に ``` があるなら、囲みを長くする ── 短いと途中で閉じる。
            const fence = '`'.repeat(Math.max(3, ...(body.match(/`+/g) || []).map((x) => x.length + 1)));
            const lang = (code?.className || '').match(/language-([\w+-]+)/)?.[1] || '';
            return fence + lang + '\n' + body + '\n' + fence;
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
            // **段落の中に潜った一覧を、落とさない。** Chromium の
            // `insertOrderedList` は `<p>` の中に `<ol>` を作ることがあり
            // （`<p><ol><li>…</li></ol></p>`）、`inlineToMd` は一覧を飛ばす
            // ので、**その行が丸ごと消えていた**（ネットワークが捕まえた・2026-09-10・
            // 段落で「番号リスト」を押すと段落が消える）。中の一覧は、
            // かたまりとして続けて書く ── 失うよりは、形が少し違うほうがよい。
            const lists = [...node.children].filter((c) => ['UL', 'OL'].includes(c.tagName))
                .map((c) => blockToMd(c, depth)).filter((x) => x !== null);
            if (!lists.length) return t === '' ? null : pad + t;
            return [...(t === '' ? [] : [pad + t]), ...lists].join('\n\n');
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

/// 前後を落とす。**全角空白（`　`）は落とさない** ── あれはインデントで、人が
/// 打った文字。JavaScript の `trim()` は `　` も削るので、削るものを半角の
/// 空白と tab と改行だけに絞る（core の `mark_break` と揃えてある）。
const edges = (s) => String(s).replace(/^[ \t\n]+/, '').replace(/[ \t\n]+$/, '');

/// 一つのかたまりの中を、Markdown の文字に戻す。
///
/// **ラベルの語彙はこちらが決めている**（`to_html` が出すもの）ので、
/// 知らないラベルは中身だけ取る ── 貼り付けで紛れ込んだラベルを、記号として
/// 書き出さないため。
function inlineToMd(node) {
    let out = '';
    for (const c of node.childNodes) {
        if (c.nodeType === 3) { out += c.data; continue; }
        if (c.nodeType !== 1) continue;
        // セルは文字ではなく操作 ── 行頭の `- [ ] ` として既に書いてある。
        if (c.classList && c.classList.contains('box')) continue;
        // 入れ子の箇条書きは、かたまりとして別に書く。
        if (['UL', 'OL'].includes(c.tagName)) continue;
        const inner = inlineToMd(c);
        switch (c.tagName) {
            case 'STRONG': case 'B': out += inner.trim() ? '**' + inner + '**' : ''; break;
            case 'EM': case 'I': out += inner.trim() ? '*' + inner + '*' : ''; break;
            case 'DEL': case 'S': case 'STRIKE': out += inner.trim() ? '~~' + inner + '~~' : ''; break;
            case 'CODE': out += '`' + c.textContent + '`'; break;
            case 'A':
                // **元から裸だった URL は、裸のまま戻す**（依頼 606）。
                // `[https://x](https://x)` に書き換えると、打っていない
                // 記号がファイルに増え、ほかのアプリで開いた人には別の文字に
                // 見える ── 押せるようにしただけで、文字は変えない約束。
                out += (c.dataset && c.dataset.bare)
                    ? inner
                    : '[' + inner + '](' + (c.getAttribute('href') || '') + ')';
                break;
            // **行末のマークは、そのまま戻す。** 空白二つと `\` はどちらも
            // 「ここで改行」のマークで、amber は改行をそのまま描くので**見え方は
            // 同じ**だが、人が打った文字なので落とさない（`markdown.rs` の
            // `mark_break` が `data-hard` に憶えさせている）。
            case 'BR': out += (c.dataset && c.dataset.hard ? c.dataset.hard : '') + '\n'; break;
            case 'IMG': out += ''; break;
            case 'FONT': case 'SPAN': {
                // 色だけは記法に戻す ── ほかの書式は文字だけ取る。
                //
                // `execCommand('foreColor')` は `<font color="#rrggbb">` を
                // 置く。`styleWithCSS` を真にすれば `<span style>` になるが、
                // そうすると太字まで `<span>` になって落ちるので、こちらで
                // 両方を読む。
                const raw = (c.getAttribute('color') || '') + ' ' + (c.getAttribute('style') || '');
                const hex = /#[0-9a-f]{6}/i.exec(raw);
                // 太字・斜体を `style` で持っているラベルも、拾えるだけ拾う。
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

/// セルの行で改行したら、次もセル。**空のセルで押したら、一覧から降りる。**
///
/// 点も番号も `contenteditable` の既定がそうしている ── 押せば次の行が
/// 同じ形で出て、何も書かずにもう一度押すと素の行に降りる。**セルだけが
/// 違っていた**: 既定はセルを持たない `<li>` を作るので、押した人はセルを
/// 足したつもりで、出てきたのは点だった。前はそれを嫌って「何も無い行」に
/// 降ろしていたが、それだと**やることを続けて3 つ書けない** ── 一つ書く
/// たびに帯のボタンへ手が戻る。
///
/// 揃えるのは形ではなく**押し心地**: 3 つとも「次も同じ、空なら降りる」。
function checkEnter(li) {
    if (!li) return false;
    if (!li.querySelector(':scope > .box')) return false;
    const list = li.parentElement;
    if (!list || !['UL', 'OL'].includes(list.tagName)) return false;
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return false;

    // セルだけで文字が無い行 ── そこで一覧から降りる。**後ろの行は残す**
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

    // **行頭で押したら、上に空のセルを置く。** 後ろの文字を次へ送ると、済んだ
    // セル（`[x]`）が文字の無い行に残り、文字のほうが新しい空のセルに付く ──
    // 保存すると `- [x] やった` が `- [ ] やった` に変わる（ネットワークが捕まえた・
    // 2026-09-11）。文字は自分のセルと一緒に居る。
    if (atHead(li)) {
        const above = document.createElement('li');
        above.className = 'task';
        putBox(above);
        above.append(document.createElement('br'));
        li.before(above);
        landBackIn(li, 0);
        return true;
    }
    // 途中で押したら、後ろの文字を次のセルへ持っていく（点や番号と同じ）。
    const cut = sel.getRangeAt(0).cloneRange();
    cut.setEndAfter(li.lastChild);
    const tail = cut.extractContents();

    const next = document.createElement('li');
    next.className = 'task';
    const box = document.createElement('button');
    box.type = 'button';
    box.className = 'box';
    box.setAttribute('aria-pressed', 'false');
    // **セルは文字ではなく操作。** 中に caret が入ると、押せるものが打てる
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
/// **出るパスが無かった**。点も番号もセルも「空でもう一度押したら降りる」の
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
    // 文字があるうちは、引用を続ける（既定のまま）。
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
        // 注記は種類の札から始まる ── 割った先にも同じラベルを付け直す。
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
/// **文字が一文字も前に無いことを見る。** 節の先頭の子がセル（`.box`）の
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
    // **セルの記号は、文字ではない。** `☑` は操作の見た目で、人が打った文字では
    // ない ── 数えると、セルのある行の行頭がいつまでも「行頭ではない」に
    // なり、記号を外す一打が効かない。写しをとって、セルを抜いてから測る。
    const bit = r.cloneContents();
    for (const b of bit.querySelectorAll('.box')) b.remove();
    return bit.textContent.length === 0;
}

/// 段を深くする・浅くする。受けたら `true`。
///
/// **一覧の中は子リスト、外はインデント。** Inkdrop も一覧の中の Tab は
/// 子リストで、外は行のインデント ── 向きは同じ（あちらは記号の画面なので
/// 半角で書ける）。この画面は記号を見せないので、インデントは**全角空白**
/// （`　`）を一つ置く。半角の空白と tab は、四つ揃うと Markdown が
/// コード枠にするので使えない。`　` は**ただの文字**で、何段でも枠に
/// ならず、GitHub でも Obsidian でも同じ幅の空きとして出る。
///
/// **見出しでは何も起きない。焦点も動かさない** ── 見出しにインデントは
/// 無く、焦点が画面から飛ぶのは事故（`PAPER.ja.md` 六章の芯の 1）。
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

/// 段落を一つインデントる（全角空白を一つ、頭に置く）。
function pad(line) {
    const at = caretIn(line);
    line.insertBefore(document.createTextNode('　'), line.firstChild);
    line.normalize();
    landBackIn(line, at + 1);
    return true;
}

/// インデントを一つ外す。**無ければ何もしない。**
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
/// **跨いだ先が無いなら、保存場所を作る** ── 図で始まるノートの上に一行
/// 足すパスが、いままで無かった（末尾には `tailStop` があるのに）。
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

/// 項目の**自分の文字**の末尾に caret が居るか（入れ子の一覧は数えない）。
function atOwnTail(li) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !sel.isCollapsed) return false;
    const r = sel.getRangeAt(0).cloneRange();
    r.selectNodeContents(li);
    try {
        r.setStart(sel.anchorNode, sel.anchorOffset);
    } catch {
        return false;
    }
    const bit = r.cloneContents();
    for (const b of bit.querySelectorAll('.box, ul, ol')) b.remove();
    // 入れ子の手前の改行（組んだ HTML のインデント）は、文字ではない。
    return bit.textContent.trim().length === 0;
}

/// ノートの**先頭**が触れないかたまりなら、その上に降りられる一行を置く。
///
/// `tailStop` の対。**末尾には既にあった**（表や罫線で終わるノートに caret を
/// 降ろす先が要る・依頼 203）が、先頭には無く、**図で始まるノートの上に
/// 一行足すパスがどこにも無かった**。
///
/// **常に置く。** 「caret が近づいたときだけ出す」もありうるが、増えたり
/// 消えたりするものは往復の試験で数が合わなくなる ── 空のままなら文字に
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
    // 先頭で押したら、上に空の**段落** ── 見出しは見出しのまま。
    //
    // **既定に任せない。** Chromium の既定は、見出しの先頭の Enter で
    // **空の見出し**を上に作る（`<h1><br></h1>`）── 文字に戻すと `# ` の一行が
    // ファイルに残る（ネットワークが捕まえた・2026-09-10）。`PAPER.ja.md` 六章の乙は
    // 「上に空の段落が一つ入る」なので、こちらで置く。
    if (atHead(line)) {
        const p = document.createElement('p');
        p.append(document.createElement('br'));
        line.before(p);
        return true;
    }

    // 後ろの文字を、新しい段落へ連れていく（末尾で押したなら空の段落）。
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

/// 引用・注記の中の Enter は、**改行**（`<br>`）。
///
/// 既定は段落を割る ── 引用では文字に戻すとき二つの段落が並びの行に均される
/// のに、注記では `>` の空行が挟まり、同じ箱なのに手触りが違っていた（ネットワークが
/// 捕まえた・2026-09-10・本人が決めた「引用と同じ（改行）」・2026-09-11）。
/// 空の行での Enter は `quitEnter`（箱から出る）が先に受ける。
/// 受けたら `true`。
function quoteEnter(box) {
    const line = lineAt(box);
    if (!line) return false;
    const wrap = line.closest('blockquote, .alert');
    if (!wrap || !box.contains(wrap)) return false;
    if (line.tagName === 'LI' || /^H[1-6]$/.test(line.tagName)) return false;
    if (!line.textContent.trim()) return false;
    return checkSoftReturn(box);
}

/// 段落の中の改行（`Shift+Enter`）。
///
/// **Enter は新しい段落、`Shift+Enter` は段落の中の改行** ── 本人が決めた
/// （2026-09-08「圧倒的に案 A」）。Word・Docs・Notion の手がそのまま動く。
/// core が段落の中の一つの改行を改行として描くようになったので（依頼 384）、
/// ここで入れる `<br>` は文字の側でも改行として残る。
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
    // **行末に置いたら、詰め物をもう 1 つ。** `<br>` が節の最後だと caret の
    // 行き先が無く、置いた場所が前の行の末尾に見える（`contenteditable` の
    // よくある形）。文字に戻すときは**末尾の改行として落ちる**ので
    // （`edges`）、ファイルには出ない。
    if (!br.nextSibling) br.after(document.createElement('br'));
    const to = document.createRange();
    to.setStartAfter(br);
    to.collapse(true);
    sel.removeAllRanges();
    sel.addRange(to);
    return true;
}

/// コードブロックの中の Enter（依頼 644）。
///
/// **既定は枠を二つに割る。** `contenteditable` の中で Enter を押すと、
/// ブラウザはいまのかたまりを複製して二つにする ── 段落ではそれが正しいが、
/// コードブロックでは「二行目を書いた」つもりが**枠が二つ**になって出る
/// （本物のアプリで確かめた・2026-09-20）。
///
/// 枠の中では、改行は**ただの改行**。文字を一つ入れるだけにする。
///
/// **行末で押したときは、改行を二つ入れる。** 一つだけだと `<pre>` の
/// いちばん後ろの改行に caret の行き先が無く、押したのに何も起きていない
/// ように見える。ファイルに出るときは末尾の改行が落ちる（`blockToMd` の
/// `PRE`）ので、余分な空行は残らない。
function checkFenceReturn(box) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return false;
    let n = sel.anchorNode;
    if (n && n.nodeType === 3) n = n.parentElement;
    if (!n || !box.contains(n)) return false;
    const pre = n.closest ? n.closest('pre') : null;
    // 触れないかたまり（図）は、そもそも caret が入らない ── 入る形に
    // 変わっても、ここで書き換えない。
    if (!pre || !box.contains(pre) || richBlock(pre)) return false;
    const code = pre.querySelector(':scope > code') || pre;
    const r = sel.getRangeAt(0);
    if (!code.contains(r.startContainer)) return false;
    r.deleteContents();
    const tail = document.createRange();
    tail.setStart(r.endContainer, r.endOffset);
    tail.setEnd(code, code.childNodes.length);
    const nl = document.createTextNode(tail.toString() === '' ? '\n\n' : '\n');
    r.insertNode(nl);
    const to = document.createRange();
    to.setStart(nl, 1);
    to.collapse(true);
    sel.removeAllRanges();
    sel.addRange(to);
    return true;
}

/// 選んだかたまりを、コードブロックにする（依頼 644・iPhone の道具の帯）。
///
/// **デスクトップ版と同じ答えを出すが、通り道が違う。** あちらは文字
/// （`readSourceEdit`）を組み直して Monaco に戻すが、iPhone に Monaco は
/// 無い ── 引用や箇条書き（`blockAs`）と同じように、画面のかたまりを
/// 直に入れ替える。文字に戻すのは `blockToMd` の `PRE` なので、**出てくる
/// Markdown は同じ**。
///
/// **かたまりごと包む。** 段落の途中だけ選んでも、その段落まるごとが枠に
/// なる ── かたまりの種類を変える道具は、かたまりに効く。二つ以上に
/// またがって選べば、まとめて 1 つの枠。
///
/// 選んでいなければ、caret のかたまりの下に空の枠を置く（依頼 618 ──
/// 中身は空で出す。サンプルの文字を入れると、消すところから始まる）。
///
/// 戻せないかたまり（元の文字を持たない図）が混じっていたら、**何もしない**
/// ── 包んだ拍子に図が消えるほうが、包めないより悪い。
function fenceAs(box) {
    const sel = getSelection();
    const kids = [...box.children];
    const blockOf = (n) => {
        if (n && n.nodeType === 3) n = n.parentNode;
        if (!n || !box.contains(n)) return null;
        while (n && n.parentElement !== box) n = n.parentElement;
        return n;
    };
    const pre = document.createElement('pre');
    const code = document.createElement('code');
    pre.appendChild(code);
    const picked = sel && sel.rangeCount && !sel.isCollapsed && String(sel).trim();
    if (picked) {
        const r = sel.getRangeAt(0);
        const from = blockOf(r.startContainer);
        const to = blockOf(r.endContainer) || from;
        const a = from ? kids.indexOf(from) : -1;
        if (a >= 0) {
            const b = Math.max(a, kids.indexOf(to));
            const parts = kids.slice(a, b + 1)
                .map((n) => (richBlock(n) ? n.dataset.md : blockToMd(n)));
            if (parts.some((t) => t === undefined)) return false;
            code.textContent = parts.filter((t) => t !== null).join('\n\n');
            from.replaceWith(pre);
            for (const n of kids.slice(a + 1, b + 1)) n.remove();
            landInFence(pre);
            return true;
        }
    }
    const at = sel && sel.rangeCount ? blockOf(sel.getRangeAt(0).startContainer) : null;
    // 打てる行を 1 つ持たせる ── 空の `<pre>` には caret の行き先が無い。
    code.textContent = '\n';
    if (at) at.after(pre);
    else box.appendChild(pre);
    landInFence(pre);
    return true;
}

/// 枠の中の頭に caret を置く。
function landInFence(pre) {
    const code = pre.querySelector('code') || pre;
    const r = document.createRange();
    r.selectNodeContents(code);
    r.collapse(true);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
    pre.scrollIntoView({ block: 'nearest' });
}

/// 選んだ範囲（無ければ caret の行）が、そのラベルの中に居るか。
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

/// 一覧から、選んだ項目を出して段落にする。**行頭の Backspace と同じ割り方**
/// （`unlist`）── 一覧はそこで割れ、下の項目も入れ子も残る。
function unwrapList(box) {
    const lines = pickedLines(box).filter((n) => n.tagName === 'LI');
    let last = null;
    for (const li of lines) {
        if (unlist(li)) last = li;
    }
    // `unlist` は自分で caret を置くので、ここでは何もしない。
    return !!last || true;
}

/// 一行を、素の段落にする（項目なら記号を外す・見出しなら `#` を外す）。
///
/// **一行は、見出しか項目か、どちらか一つ。** 項目の行を見出しにするときは、
/// 先に点を外す（`PAPER.ja.md` 六章の丙・本人が決めた「点付きが見出しに
/// なっても驚かない」）。`formatBlock` を項目にそのまま掛けると、Chromium は
/// 一覧を割って**空の見出しと空の項目を作り、文字を落とす**（ネットワークが捕まえた・
/// 2026-09-10・一覧の行で「見出し」を押すと文字が消える）。
function lineToPara(box) {
    for (const line of pickedLines(box)) {
        if (line.tagName === 'LI') unlist(line);
    }
}

/// `execCommand` が段落の中に作ってしまった一覧を、外へ出す。
///
/// Chromium の `insertOrderedList` は、隣に一覧が無い段落では
/// `<p><ol><li>…</li></ol></p>` を作る（`<ul>` も同じ）── 段落の中の一覧は
/// 文字に戻す側が知らず、**その行が丸ごと消えていた**。皮を剥いで、一覧を
/// かたまりにする。元の行のラベルは一覧へ持たせる。
function tidyLists(box) {
    for (const wrap of [...box.querySelectorAll(':scope > p, :scope > div, :scope > h1, :scope > h2, :scope > h3, :scope > h4, :scope > h5, :scope > h6')]) {
        const lists = [...wrap.children].filter((c) => ['UL', 'OL'].includes(c.tagName));
        if (!lists.length) continue;
        // 一覧のほかに文字が無いなら、皮だけ剥ぐ。文字があるなら、文字を段落に
        // 残して一覧を後ろに出す。
        for (const list of lists) keepMark(wrap, list);
        const rest = [...wrap.childNodes].filter((c) => !lists.includes(c));
        const text = rest.map((c) => c.textContent).join('').trim();
        if (text) {
            wrap.after(...lists);
        } else {
            wrap.replaceWith(...lists);
        }
    }
}

/// かたまりの種類を変える（見出し・箇条書き・番号・引用）。**デスクトップ版と iPhone で一組。**
///
/// 決めごと（`PAPER.ja.md` 六章の丙）はぜんぶここに:
///   - 同じボタンで、付けると外す（中で押したら外れる）
///   - 一行は、見出しか項目か、どちらか一つ（点を付ける前に見出しを落とし、
///     見出しにする前に点を外す）
///   - **表のセルの中では、一覧にしない** ── Markdown の表のセルに一覧は
///     書けず、`insertOrderedList` はセルの文字を消す（ネットワークが捕まえた・
///     2026-09-10）。受けて、何もしない（`false` を返す）
function blockAs(box, what) {
    const sel = getSelection();
    let n = sel && sel.rangeCount ? sel.anchorNode : null;
    if (n && n.nodeType === 3) n = n.parentElement;
    const cell = n && n.closest ? n.closest('td, th') : null;
    const cmd = (name, arg) => {
        try { document.execCommand(name, false, arg); } catch { /* 軽い DOM には無い */ }
    };
    if (what === 'ul' || what === 'ol') {
        if (cell && box.contains(cell)) return false;
        if (inside(box, what.toUpperCase())) { unwrapList(box); return true; }
        // 別の種類の項目なら、**その一行だけ種類を替える**（本人が決めた・2026-09-10）。
        const items = pickedLines(box).filter((l) => l.tagName === 'LI');
        if (items.length) {
            for (const li of items) switchItem(li, what.toUpperCase());
            return true;
        }
        flattenHeads(box);
        cmd(what === 'ul' ? 'insertUnorderedList' : 'insertOrderedList');
        tidyLists(box);
        // インデントは外す ── 項目にインデントは無い（本人が決めた・2026-09-11・ネットワークの決めごと 9）。
        for (const li of pickedLines(box).filter((l) => l.tagName === 'LI')) {
            const first = li.firstChild && li.firstChild.classList && li.firstChild.classList.contains('box')
                ? li.firstChild.nextSibling : li.firstChild;
            if (first && first.nodeType === 3) first.data = first.data.replace(/^\u3000+/, '');
        }
        return true;
    }
    if (what === 'blockquote') {
        if (inside(box, 'blockquote')) { unwrapBlock(box, 'blockquote'); return true; }
        // 項目の行は、段落にしてから引用に（一覧はそこで割れる・本人が決めた・2026-09-10）。
        lineToPara(box);
        cmd('formatBlock', 'blockquote');
        return true;
    }
    if (/^h[1-6]$/.test(what) || what === 'p') {
        if (cell && box.contains(cell)) return false;
        lineToPara(box);
        // 見出しにインデントは無い ── 項目と同じく外す（ネットワークの決めごと 9 と同じ筋・2026-09-11）。
        if (what !== 'p') {
            for (const l of pickedLines(box)) {
                const first = l.firstChild;
                if (first && first.nodeType === 3) first.data = first.data.replace(/^\u3000+/, '');
            }
        }
        cmd('formatBlock', what);
        return true;
    }
    cmd('formatBlock', what);
    return true;
}

/// 見出しは押すたびに深くなる ── 編集画面と同じ（`#` → `##` → `###` → 無し）。
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
/// `PAPER.ja.md` 六章の芯の 1 ── 画面の上の一打は、文字の上の一つの記号に
/// 対応する。既定に任せると、途中の項目は**前の項目と繋がる**（セルが一つ
/// 黙って消える ── 二つの「やること」が一行になる）。どの位置の項目でも
/// 同じでなければ、押すたびに結果を見る癖がつく。
///
/// 外すものが無くなったら、Backspace は文字を消すキーに戻る（`false` を返す）。
/// 受けたら `true`。
function checkBack(box) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !sel.isCollapsed) return false;   // 選びは選びの話
    const line = lineAt(box);
    if (!line || !atHead(line)) return false;

    // **触れないかたまりの隣では、何も起きない。** caret の無いものが
    // 一打で消えるのは事故（芯の 2）── 消すパスは、押したときの吹き出し。
    const before = line.previousElementSibling;
    if (before && richBlock(before)) return true;

    // **インデントには、何もしない。** `　` はただの文字なので、caret がその
    // 後ろに居れば既定の Backspace が一つ消す（芯の 1 がそのまま効く）。
    // ここで外しにいくと、`　` の**前**で押した人の一打が、下の行を
    // 巻き込まずにインデントだけ消す ── 押した場所と結果が合わない。

    // 見出しは段落になる。**前の段落には繋がない** ── 見出しを前の段落の
    // 文字に繋ぎたい人は、まず居ない。
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

/// **行末の Delete は、次の行が素の段落のときだけ繋ぐ。**
///
/// 行頭の Backspace は「この行の記号を一つ外す」（セルが黙って消えないため）。
/// その裏で、行末の Delete は**次の行の記号を消さない** ── 既定に任せると
/// `- [ ] やること⏎- [x] やった` が `- [ ] やることやった` になり（セルが
/// 一つ消える）、表の最後のセルは下の段落を吸い込み、枠は丸ごと消えて下の
/// 段落と繋がった（ネットワークが捕まえた・2026-09-10・本人が決めた「次が記号付きの
/// 行なら、何も起きない」・2026-09-11）。段落と段落は、これまで通り繋がる。
/// 受けたら `true`（何もしない）。
function checkDel(box) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !sel.isCollapsed) return false;
    const line = lineAt(box);
    if (!line) return false;
    // **項目の「終わり」は、入れ子の手前。** 入れ子を持つ項目は `textContent`
    // に子の文字まで含むので、`atTail` では終わりにならず、既定の Delete が
    // 入れ子の一つめを親に吸い込んだ（`- ふたつ入れ子`・ネットワークが捕まえた・
    // 2026-09-11）。
    if (!(line.tagName === 'LI' ? atOwnTail(line) : atTail(line))) return false;
    // セルの終わり ── 表の外を吸い込まない。
    const cell = line.closest('td, th');
    if (cell && box.contains(cell)) return true;
    // 項目の終わり ── 次の項目（入れ子も）の記号を消さない。
    if (line.tagName === 'LI') return true;
    // 引用・注記の中の段落 ── 同じ箱の中の次の段落とは繋がる。箱の終わりでは止まる。
    let here = line;
    while (here.parentElement && here.parentElement !== box) here = here.parentElement;
    if (here !== line) {
        const next = line.nextElementSibling;
        return !(next && next.tagName === 'P');
    }
    const next = here.nextElementSibling;
    if (!next) return false;                            // ノートの末尾 ── 既定（何も起きない）
    return next.tagName !== 'P';                        // 次が素の段落でなければ、何もしない
}

/// 見出しを段落にする（文字はそのまま）。
function asPara(line) {
    landBackIn(toPara(line), 0);
    return true;
}

/// 見出しを段落に掛け替えて、その段落を返す。
function toPara(line) {
    const p = document.createElement('p');
    p.append(...line.childNodes);
    keepMark(line, p);
    line.replaceWith(p);
    return p;
}

/// 項目の記号を外して、段落にする。**一覧はそこで割れ、下の項目は残る。**
function unlist(li) {
    let list = li.parentElement;
    if (!list || !['UL', 'OL'].includes(list.tagName)) return false;
    // **入れ子の項目は、いちばん外まで出してから段落にする。** 親の項目の
    // 中に段落は置けない ── 置くと文字に戻すとき親の文字に繋がる
    // （`- ふたつ入れ子`・ネットワークが捕まえた・2026-09-10）。
    for (let n = 0; n < 8 && list.parentElement && list.parentElement.tagName === 'LI'; n += 1) {
        outdent(li);
        list = li.parentElement;
    }
    const p = document.createElement('p');
    // **入れ子は連れていかない ── 残す。** 段落の中に一覧を入れると、文字に
    // 戻す側が飛ばして**入れ子が丸ごと消える**（ネットワークが捕まえた・2026-09-10・
    // 一覧の途中の行頭で Backspace を押すと、その下の入れ子が消えた）。
    // 入れ子の項目は、下に残る項目の頭に並べる。
    const nested = [];
    // セルも一緒に外れる ── 記号を外すとは、そういうこと。
    for (const x of [...li.childNodes]) {
        if (x.nodeType === 1 && x.classList?.contains('box')) continue;
        if (x.nodeType === 1 && ['UL', 'OL'].includes(x.tagName)) { nested.push(...x.children); continue; }
        p.append(x);
    }
    if (!p.childNodes.length) p.append(document.createElement('br'));
    const rest = [...nested];
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
/// （注記なら種類のラベルも）。
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

/// 書式の終わりに caret があるなら、その外へ出す。
///
/// **選んだ文字を飾ったあと、続けて打った文字まで太字になっていた。**
/// `execCommand` は選び目を `<b>` の中に残すので、そこから打てば中に入る
/// ── 飾ったのは**選んだ文字**であって、これから打つ文字ではない。書式の
/// 終わりに立っているときだけ外へ出す（途中に居るなら、そこは中の文字）。
const DRESS = 'b, strong, i, em, s, strike, del, code';

function outOfDress() {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !sel.isCollapsed) return false;
    const node = sel.anchorNode;
    const from = node && node.nodeType === 3 ? node.parentElement : node;
    if (!from || !from.closest) return false;
    let dress = from.closest(DRESS);
    if (!dress) return false;
    // いちばん外側の書式まで登る（`**_こう_**` は二重になる）。
    for (let up = dress.parentElement; up && up.matches && up.matches(DRESS); up = up.parentElement) {
        dress = up;
    }
    // caret から書式の終わりまでに文字が残っているなら、そこは途中。
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
    // を別に憶えていて（typing style）、書式の隣で打った文字をその書式の中へ
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

/// セル（押せるボタン）を、項目の頭に置く。`checkEnter` と同じ形。
function putBox(li) {
    const box = document.createElement('button');
    box.type = 'button';
    box.className = 'box';
    box.setAttribute('aria-pressed', 'false');
    box.contentEditable = 'false';
    li.classList.add('task');
    li.prepend(box);
    return box;
}

/// 空の注記・引用に、**打てる一行**を置く。
///
/// core は中身の無い注記を札だけで組む（`<div class="alert"><p class="alert-h">`）
/// ── caret を置く先が無く、表示画面から入れた注記に**何も打てなかった**
/// （ネットワークが捕まえた・2026-09-10・本人が決めた「打てる空の行を中に置く」）。
/// 空のままなら文字に戻すとき落ちる（`blockToMd` は中身が空なら札だけ書く）。
function fillAlerts(box) {
    for (const wrap of box.querySelectorAll(':scope > .alert, :scope > blockquote')) {
        if (wrap.querySelector(':scope > p:not(.alert-h), :scope > ul, :scope > ol, :scope > table, :scope > pre, :scope > blockquote, :scope > h1, :scope > h2, :scope > h3')) continue;
        const p = document.createElement('p');
        p.append(document.createElement('br'));
        wrap.append(p);
    }
}

/// **セルは、caret の一行に。** 付いていれば外す。
///
/// 前は core の `mark` にかたまり丸ごとを渡していたので、四つの項目が
/// 一度にセルになり、引用は引用ごと外れ、見出しは `- [ ] # 見出し` になった
/// （ネットワークが捕まえた・2026-09-10・本人が決めた「caret の一行だけ。点・番号・
/// 引用・見出し・インデントは外してセルに」）。表のセルでは何もしない（`false`）。
function checkLine(box) {
    const line = lineAt(box);
    if (!line) return false;
    if (line.closest('td, th')) return false;
    if (line.tagName === 'LI') {
        const had = line.querySelector(':scope > .box');
        if (had) { had.remove(); line.classList.remove('task'); return true; }
        const at = caretIn(line);
        putBox(line);
        landBackIn(line, at);
        return true;
    }
    // 見出しは段落に。引用・注記の中なら、その一行を外へ。
    let p = /^H[1-6]$/.test(line.tagName) ? toPara(line) : line;
    const wrap = p.closest('blockquote, .alert');
    if (wrap && box.contains(wrap)) {
        const rest = [];
        for (let x = p.nextElementSibling; x; x = x.nextElementSibling) rest.push(x);
        wrap.after(p);
        if (rest.length) {
            const more = wrap.cloneNode(false);
            more.removeAttribute('data-line');
            more.removeAttribute('data-md');
            const label = wrap.querySelector(':scope > .alert-h');
            if (label) more.append(label.cloneNode(true));
            more.append(...rest);
            p.after(more);
        }
        if (!wrap.querySelector(':scope > p:not(.alert-h), :scope > ul, :scope > ol')) wrap.remove();
    }
    // インデントは外す（項目にインデントは無い）。
    const first = p.firstChild;
    if (first && first.nodeType === 3) first.data = first.data.replace(/^\u3000+/, '');
    const li = document.createElement('li');
    li.append(...p.childNodes);
    if (!li.childNodes.length) li.append(document.createElement('br'));
    const ul = document.createElement('ul');
    keepMark(p, ul);
    ul.append(li);
    p.replaceWith(ul);
    putBox(li);
    landBackIn(li, li.textContent.length);
    return true;
}

/// 項目の種類を替える（点 ⇄ 番号）。**その一行だけ ── 一覧はそこで割れる。**
///
/// 既定の `insertOrderedList` を点の項目に掛けると、空の項目が増え、
/// 押した位置で結果が変わり、セルの行ではセルだけが外に出た（ネットワークが捕まえた・
/// 2026-09-10・本人が決めた「その一行だけ種類を替える」）。セルは連れていく
/// （`1. [ ] やること` は Markdown が持っている形）。
function switchItem(li, tag) {
    const list = li.parentElement;
    if (!list || !['UL', 'OL'].includes(list.tagName) || list.tagName === tag) return false;
    const at = caretIn(li);
    const rest = [];
    for (let x = li.nextElementSibling; x; x = x.nextElementSibling) rest.push(x);
    const mine = document.createElement(tag);
    list.after(mine);
    mine.append(li);
    // 書いた印（`- `・`2. `）は元の種類のもの ── 憶えたままだと点のまま書かれる。
    if (li.dataset) delete li.dataset.mark;
    if (rest.length) {
        const more = document.createElement(list.tagName);
        more.append(...rest);
        mine.after(more);
    }
    if (!list.children.length) list.remove();
    landBackIn(li, at);
    return true;
}

/// 貼られた文字を、項目の中へ ── **改行ごとに項目を増やす。**
///
/// 項目の中に見出しや一覧を貼ると、見出しの文字が項目の文字に混ざり、一覧は
/// 入れ子になった（ネットワークが捕まえた・2026-09-10・本人が決めた「項目には文字だけ、
/// 改行ごとに項目を増やす」）。
function pasteLines(box, lines) {
    const line = lineAt(box);
    if (!line || line.tagName !== 'LI') return false;
    const rows = lines.filter((t, i) => i === 0 || t.trim());
    try { document.execCommand('insertText', false, rows[0] || ''); } catch { /* 軽い DOM */ }
    let at = line;
    const task = !!line.querySelector(':scope > .box');
    for (const t of rows.slice(1)) {
        const li = document.createElement('li');
        if (task) putBox(li);
        li.append(document.createTextNode(t));
        at.after(li);
        at = li;
    }
    if (at !== line) landBackIn(at, at.textContent.length);
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

/// ノートの隣に置かれた画像の、ほんとうの在りか。
function absPath(src, dir) {
    return src.startsWith('/') || /^[a-z]:[/\\]/i.test(src) ? src : dir + src;
}

/// 画像を出す。**読めなかったら、開いているアプリに読んでもらう。**
///
/// この画面は crmaine の `<webview>` の中でも動く。あちらでは `file://` の
/// 画像が届かず、**サンプルのノートの画像だけが出ない**ことになっていた（amber 自身
/// のデスクトップ版では出るので、撮っても分からない ── 向こうで開くまで分からない）。
///
/// `fileBytes` は同梱する側も持っている口で、読んだ中身をそのまま返す ──
/// `data:` なら、どの入れ物でも出る。**先に `file:` を試すのは、そちらが
/// 安いから**（大きな画像を毎回 base64 にして持ち歩く理由は、出るなら無い）。
///
/// **一本にしてある。** 前は表示画面とコード画面で別々に書いていて、助け船が
/// 片方にしか無かった ── 会社の Windows で「この画像は読めません」と出たのは
/// そこ（パスの直し方も、片方だけ直せば済んでしまう形だった）。
function showPicture(img, at, onFail) {
    img.src = fileURL(at);
    img.addEventListener('error', async () => {
        // 助け船で入れ替えた画像も出なかったら、もう手が無い ── そのとき言う。
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

/* ── よそから来た HTML を、ノートの文字に ── */

/// **捨てる札。** 読むためのものではないもの ── 中身ごと落とす。
const WEB_DROP = 'script,style,noscript,template,svg,canvas,iframe,object,embed,'
    + 'form,input,button,select,textarea,label,nav,header,footer,aside,'
    + 'video,audio,source,track,map,area,dialog,menu';

/// **ほどく札。** 入れ物でしかないもの ── 中身は残して、皮だけ剥ぐ。
///
/// ほどかないと `blockToMd` の既定に落ち、`<div>` 1 つの中の段落・見出し・
/// 一覧が**ぜんぶ一行の文字**になる（既定は中の文字だけ取るので）。
const WEB_PEEL = 'div,section,article,main,header,footer,figure,figcaption,'
    + 'details,summary,center,font,small,mark,ins,abbr,time,cite,q,dfn,'
    + 'picture,tbody,thead,tfoot,colgroup,col,noscript';

/// 最後に caret が居た、**そのものの場所**（かたまりではなく、文字と文字のあいだ）。
///
/// **`focus()` は caret を連れてこない。** 板やボタンを押した時点で焦点は
/// そちらへ移り、表示画面の選択は消える ── そこで `focus()` だけして文字を
/// 入れると、**ノートの頭に入る**（本人が見つけた・依頼 461）。だから
/// 動いたときに憶えておいて、入れる直前に戻す。
///
/// **デスクトップ版と iPhone で一組。** 同じ間違いを二か所で直すことになるので、ここに置く。
let caretSpot = null;

/// caret が動いた。`box` の中に居るときだけ憶える。
function markCaret(box) {
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !box) return;
    let n = sel.getRangeAt(0).startContainer;
    if (n.nodeType === 3) n = n.parentNode;
    if (!n || !box.contains(n)) return;
    caretSpot = sel.getRangeAt(0).cloneRange();
}

/// 憶えている場所へ caret を戻す。**戻せたかどうかを返す。**
///
/// 戻せないのは、そのかたまりが組み直されて消えたとき ── そのときは
/// 呼んだ側が決める（頭に入れるのか、何もしないのか）。
function caretBack(box) {
    if (!caretSpot || !box) return false;
    const n = caretSpot.startContainer;
    const holder = n.nodeType === 3 ? n.parentNode : n;
    if (!holder || !box.contains(holder)) return false;
    box.focus();
    const sel = getSelection();
    if (!sel) return false;
    sel.removeAllRanges();
    sel.addRange(caretSpot);
    return true;
}

/// ページの題。**`<title>` から、サイト名の尻尾を落とす** ──
/// 「段取りの決め方 | example」の `| example` は、ノートの題には要らない。
function clipTitle(html) {
    const raw = /<title[^>]*>([\s\S]*?)<\/title>/i.exec(html)?.[1] || '';
    const t = new DOMParser().parseFromString('<p>' + raw + '</p>', 'text/html')
        .body.textContent.trim();
    return edges(t.split(/\s+[|｜–—-]\s+/)[0] || t).slice(0, 120);
}

/// **要らないラベルを落とすだけ。** 均す前と、選ぶ前の、両方で使う。
function webDrop(body) {
    for (const n of body.querySelectorAll(WEB_DROP)) n.remove();
    // 隠してあるものは、読む人に見えていない ── 貼らない。
    for (const n of body.querySelectorAll('[hidden],[aria-hidden="true"]')) n.remove();
    return body;
}

/// 本文らしいところ。**`article` が名乗っていれば、それを信じる。**
///
/// 無ければ、いちばん文字の多いかたまり ── 案内も足も、文字の量では本文に
/// 勝てない。勝てないところまでしか当てられないので、外れたら人が消す。
///
/// **均す前の HTML を受ける。** 均すと `article` も `main` も皮を剥がれて
/// 消えるので、均したあとに名乗りを探しても、絶対に見つからない ──
/// いちばん確かな手がかりを、使う前に自分で捨てていた。
function bestPart(html) {
    const doc = new DOMParser().parseFromString(String(html || ''), 'text/html');
    const body = doc.body;
    if (!body) return '';
    // **選ぶ前に、要らないラベルは落としておく。** ページの中の長い台本や
    // 隠したラベルが、文字の量で本文に勝ってしまう。
    webDrop(body);
    const named = body.querySelector('article, [role="main"], main');
    if (named && named.textContent.trim().length > 200) return named.outerHTML;
    let best = body;
    let most = body.textContent.trim().length;
    for (const n of body.querySelectorAll('*')) {
        const len = n.textContent.trim().length;
        // **半分より少なくなるところまでは降りない。** 降りすぎると、
        // 長い一段落だけを採って前後を捨てることになる。
        if (len > most * 0.6 && len < most) { best = n; most = len; }
    }
    return best === body ? body.innerHTML : best.outerHTML;
}

/// よそから来た HTML を、**amber が知っている形へ均してから**文字にする。
///
/// **文字に直すのは `blockToMd` 一本**（依頼 421）── 画面の書き戻しと同じパスを
/// 通す。ここでやるのは「均す」ことだけ: 要らないラベルを落とし、入れ物を
/// ほどき、画像とリンクの行き先を**絶対の道**にする。二本目の変換器を
/// 書かないので、片方だけ直した日に貼り付けと書き戻しがずれない。
///
/// `base` はコピー元のページの URL（分かるとき）── 相対の行き先は
/// そのままだと、貼ったノートからは**どこも指していない文字**になる。
function webToMd(html, base) {
    const body = webClean(html, base);
    if (!body) return '';
    // 均したあとの、いちばん上の並びを文字にする。
    const out = [];
    for (const n of [...body.childNodes]) {
        const md = n.nodeType === 3
            ? (edges(n.data) || null)
            : (n.nodeType === 1 ? blockToMd(n) : null);
        if (md !== null && md !== '') out.push(md);
    }
    // 空行は詰める ── よそのページは空の入れ物が多く、そのままだと
    // 貼ったノートが隙間だらけになる。
    return out.join('\n\n').replace(/\n{3,}/g, '\n\n').trim();
}

/// よそから来た HTML を、**amber が知っている札だけの形に均す。**
///
/// 均した先は「表示」画面がそのまま食える形なので、**表示画面に貼るときは
/// これをそのまま入れる**（文字にしてから入れると `##` が文字として出る ──
/// 実際にそうなった）。編集画面に貼るときだけ、`webToMd` で文字にする。
function webClean(html, base) {
    const doc = new DOMParser().parseFromString(String(html || ''), 'text/html');
    const body = doc.body;
    if (!body) return null;
    webDrop(body);

    const abs = (u) => {
        const t = String(u || '').trim();
        if (!t || t.startsWith('#') || /^javascript:/i.test(t)) return '';
        try { return readableUrl(base ? new URL(t, base).href : t); } catch { return t; }
    };

    // **画像は、文字にしてから均す。** `inlineToMd` は `<img>` を捨てる
    // （表示画面では包み（`<figure>`）が元の文字を持っているので、それでよい）
    // ── よそから来た画像はその包みが無いので、ここで `![](…)` に直す。
    //
    // **落としてくるのではなく、リンクのまま**（本人が決めた・2026-09-09）
    // ── `attachments/` へ落とすと、一本取り込むたびにフォルダが重くなり、
    // 消すパスも無い。要る画像だけ、あとで貼り直せる。
    for (const img of [...body.querySelectorAll('img')]) {
        const src = abs(img.getAttribute('src'));
        // 一辺が 1px の画像は、たいてい数を数えるためのもの ── 読む文字ではない。
        const tiny = Number(img.getAttribute('width')) === 1
            || Number(img.getAttribute('height')) === 1;
        const alt = (img.getAttribute('alt') || '').trim();
        img.replaceWith(doc.createTextNode(
            !src || tiny || src.startsWith('data:') ? '' : `![${alt}](${src})`));
    }
    for (const a of [...body.querySelectorAll('a')]) {
        const href = abs(a.getAttribute('href'));
        if (href) a.setAttribute('href', href);
        // 行き先の無いリンクは、リンクではない ── 皮を剥いで文字だけ残す。
        else peel(a);
    }

    // 入れ物をほどく。**内側から**（外から剥ぐと、剥いだ先をもう一度
    // 見に行くことになる）。
    for (const n of [...body.querySelectorAll(WEB_PEEL)].reverse()) peel(n);
    stripAttrs(body);
    return body;
}

/// **よそから来た書式は、持ち込まない**（依頼 616・本人「やや白いハイライト
/// というかマーカーがついた文字で入力される」）。
///
/// Excel はセルに `style="background:white;color:black;mso-pattern:…"` を
/// 付けて寄こす。琥珀の紙（`#fffdf8`）の上では、その白が**マーカーを
/// 引いたように見える** ── 実物で確かめた。
///
/// **ambər が知っている書式は一つだけ**（`<span style="color:#rrggbb">`）
/// なので、それ以外は落とす。残すのは「意味のある属性」だけ ── 行き先
/// （`href`）、画像（`src` `alt`）、セルの繋がり（`colspan` `rowspan`）、
/// 一覧の始まり（`start`）、そしてセルの並び（`align`）。
///
/// **落とさないと、書き戻しでも消える。** 書式は `paperToMd` を通らない
/// ので、貼った直後だけ見えて次の保存で消える ── 「打っていないのに
/// 見え方が変わった」がいちばん分かりにくい。
const KEEP_ATTR = new Set(['href', 'src', 'alt', 'colspan', 'rowspan', 'start', 'align']);
function stripAttrs(body) {
    for (const n of body.querySelectorAll('*')) {
        for (const name of [...n.getAttributeNames()]) {
            if (KEEP_ATTR.has(name)) continue;
            // **枠の言語は書式ではない。** `class="language-rust"` は
            // 「これは Rust だ」という中身の話で、落とすと貼った枠から
            // 言語が消える（走査が捕まえた）。ほかの `class` は落とす。
            if (name === 'class' && n.tagName === 'CODE') {
                const lang = /(?:^|\s)(language-[\w+-]+)/.exec(n.getAttribute('class') || '');
                if (lang) { n.setAttribute('class', lang[1]); continue; }
            }
            if (name === 'style') {
                // 色だけは ambər の記法なので残す（`note::first_color`）。
                const color = /(?:^|;)\s*color\s*:\s*(#[0-9a-f]{6})\b/i
                    .exec(n.getAttribute('style') || '');
                // **黒は色ではない。** Excel も Word も既定の文字に
                // `color:black` を付けて寄こすので、そのまま残すと
                // ノートじゅうが `<span style="color:#000000">` で埋まる。
                if (color && color[1].toLowerCase() !== '#000000') {
                    n.setAttribute('style', 'color:' + color[1]);
                    continue;
                }
            }
            n.removeAttribute(name);
        }
    }
    return body;
}

/// **人が読める形の行き先。**
///
/// `new URL()` は日本語を `%E6%AC%A1` に直す ── 環境には正しいが、
/// ノートに残る文字としては読めない（ノートはただの Markdown なので、
/// メモ帳で開いた人もこの行を読む）。
///
/// **戻して困る文字だけは、戻さない** ── 空白と丸括弧は Markdown の
/// `[文字](道)` を途中で閉じてしまう。一つでも混ざっていたら、逃がした
/// ままの形を返す（読みにくいが、壊れているよりよい）。
function readableUrl(url) {
    let plain;
    try { plain = decodeURI(url); } catch { return url; }
    return /[\s()<>"'\\]/.test(plain) ? url : plain;
}

/// ラベルを外して、中身をその場に残す。
function peel(node) {
    const at = node.parentNode;
    if (!at) return;
    while (node.firstChild) at.insertBefore(node.firstChild, node);
    at.removeChild(node);
}

/// このデスクトップ版の「表示」画面を、上の切り出しに繋ぐ薄い包み。
///
/// **切り出しの外に置く。** iPhone が持っていくのは上の切り出しだけで、ここは
/// `el('read')` も `state` も見る ── 中に混ぜると、iPhone のバンドルに
/// 「呼べば落ちる関数」が入る。
function armRead() {
    // 錠のノートは、画面を入力欄にしない（依頼 629）── 見た目で止めるのでは
    // なく、打てる場所そのものを開かない。
    armPaper(el('read'), whole(), !!state.open && view !== 'write' && canEdit());
    // 来た行の地色は、組み直すたびに敷き直す ── ラベルは組み直しで消えるので。
    paintIncoming();
}

function readToMd() {
    return paperToMd(el('read'), state.head);
}

/// 同じノートと見てよいか。**末尾の空行の数だけは、見ない。**
/// 表示画面がそこを勝手に決めているので（`tailStop`）、そこの差は
/// 「人が書き換えた」ではない。
function sameNote(a, b) {
    return String(a).replace(/\n+$/, '') === String(b).replace(/\n+$/, '');
}

/// 打ったら、落ち着いてから書き戻す。
function readChanged() {
    if (syncing || view === 'write' || !state.open) return;
    state.dirty = true;
    el('state').textContent = '書きかけ';
    drawSaveNow();
    drawStrip();
    clearTimeout(readTimer);
    // **変換中に切れたら、待つ。** 数え直すだけで、書き戻しはしない ──
    // 未確定の文字を保存しないため（`composing` の註）。
    readTimer = setTimeout(function again() {
        if (composing) { readTimer = setTimeout(again, 700); return; }
        syncRead();
    }, 700);
}

/// セルの行の Enter を、`checkEnter` に渡す。**判断は切り出しの側** ──
/// iPhone も同じ関数を呼ぶので、押し心地が端末で分かれない。
el('read').addEventListener('keydown', (e) => {
    if (!isEnter(e) || imeBusy(e)) return;
    if (e.metaKey || e.ctrlKey) return;
    let n = getSelection()?.anchorNode;
    if (n && n.nodeType === 3) n = n.parentElement;
    if (!n || !el('read').contains(n)) return;
    // **コードブロックの中は、まずここ**（依頼 644）── `⇧` が付いていても
    // 同じ。枠の中に `<br>` を入れる意味は無い。
    if (checkFenceReturn(el('read'))) { e.preventDefault(); readChanged(); return; }
    // **`⇧Enter` は段落の中の改行。** Enter は新しい段落 ── Word・Docs・
    // Notion の手がそのまま動く（本人が決めた・2026-09-08）。
    if (e.shiftKey) {
        if (checkSoftReturn(el('read'))) {
            e.preventDefault();
            readChanged();
            return;
        }
        // **見出しと項目の中では、Enter と同じ**（`PAPER.ja.md` 六章の乙）。
        // 既定に任せると `<br>` が見出しの中に入り、`# 見⏎出し` の形で
        // ファイルに残る（ネットワークが捕まえた・2026-09-10）── 下の Enter のパスを
        // そのまま通し、どれも受けなければ段落を割る既定を自分で呼ぶ。
        const line = lineAt(el('read'));
        if (!line || !(line.tagName === 'LI' || /^H[1-6]$/.test(line.tagName))) return;
        const li = line.tagName === 'LI' ? line : null;
        e.preventDefault();
        if (!(li ? checkEnter(li) : false) && !quitEnter(n) && !checkReturn(el('read'))) {
            document.execCommand('insertParagraph');
        }
        readChanged();
        return;
    }
    const li = n.closest('li');
    if (!(li ? checkEnter(li) : false) && !quitEnter(n) && !checkReturn(el('read')) && !quoteEnter(el('read'))) return;
    e.preventDefault();
    readChanged();
});

/// 打った文字を、書式の外へ。**打つ直前に出す** ── 飾った直後に選び目を
/// 動かすと、続けて ⌘I を押すパスが消える。IME は組み始めに出す（組んで
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

/// 表の中の Tab は、次のセルへ。
///
/// **表を打つのは「セルからセルへ」で、行を打つのではない。** 既定の Tab は
/// 焦点を画面ごと出してしまい、打っている途中で表から追い出される ──
/// 打ち込みの表計算でも文書でも、そこは次のセルに決まっている。
///
/// 最後のセルで押したら**行を一つ足す** ── 足し方を探しに行かせない。
/// 行頭の Backspace ── 記号を一つ外す（`checkBack`）。
///
/// **変換中は IME に渡す。** 文節の区切りがこのキーで動く（`PAPER.ja.md`
/// 六章の丁）。
el('read').addEventListener('keydown', (e) => {
    if (!['Backspace', 'Delete'].includes(e.code) || imeBusy(e)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    // 選んで消すときは、表を壊さないほうが先に受ける。
    if (checkCut(el('read'))) { e.preventDefault(); readChanged(); return; }
    if (e.code === 'Delete') {
        // **枠のすぐ上の行末では、何も起きない**（`checkDel`）── 既定は
        // 枠を丸ごと消して下の段落と繋ぐ（ネットワークが捕まえた・2026-09-10）。
        if (checkDel(el('read'))) e.preventDefault();
        return;
    }
    if (!checkBack(el('read'))) return;
    e.preventDefault();
    readChanged();
});

/// 矢印 ── 触れないかたまりを跨ぐ（`checkArrow`）。
///
/// **変換中は IME に渡す。** 文節の区切りがこのキーで動く。
el('read').addEventListener('keydown', (e) => {
    const dir = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' }[e.code];
    if (!dir || imeBusy(e)) return;
    if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;   // 選びと飛びは既定のまま
    if ((dir === 'down' || dir === 'up') && checkCellArrow(dir)) { e.preventDefault(); return; }
    if (!checkArrow(el('read'), dir)) return;
    e.preventDefault();
});

/// **表の中の上下は、見た目どおり真下・真上のセルへ**（依頼 559・本人）。
///
/// 既定は「次の文字へ」なので、2×2 の左上で下を押すと**右上**に行く ──
/// 表は格子に見えているのに、動きだけが一列に並んだ文字のままだった。
/// Tab が「次のセルへ」なのと同じ理由で、ここも見えている形に合わせる。
///
/// **セルの中で行が折り返しているときは、まずセルの中を動く。** 一行しか
/// 無いように見えて二行あることがあるので、caret がセルの最後の行に居る
/// ときだけ隣の行へ渡す（上は最初の行のときだけ）。
///
/// 表の外へは出さない ── 最後の行より下、最初の行より上は既定に任せる
/// （`checkArrow` が表そのものを跨ぐ）。
function checkCellArrow(dir) {
    let n = getSelection()?.anchorNode;
    if (!n) return false;
    if (n.nodeType === 3) n = n.parentElement;
    const cell = n?.closest?.('td, th');
    if (!cell || !el('read').contains(cell)) return false;
    const r = getSelection().getRangeAt(0).getBoundingClientRect();
    const box = cell.getBoundingClientRect();
    const line = parseFloat(getComputedStyle(cell).lineHeight) || r.height || 16;
    // **一行しかないセルは、いつでも渡す**（ほとんどのセルがこれ）。折り返して
    // いるセルだけ、まずセルの中を動かす ── セルの高さで見分ける（詰めの厚みに
    // 寄りかからない）。
    const many = box.height > line * 1.8;
    if (many && dir === 'down' && r.bottom && r.bottom < box.bottom - line) return false;
    if (many && dir === 'up' && r.top && r.top > box.top + line) return false;
    const table = cell.closest('table');
    const rows = [...table.rows];
    const at = rows.indexOf(cell.parentElement);
    const to = rows[at + (dir === 'down' ? 1 : -1)]?.cells[cell.cellIndex];
    if (!to) return false;
    landInCell(to);
    return true;
}

/// Tab / Shift+Tab ── 一覧の中は段、外はインデント（`checkTab`）。
///
/// **表の中は、表の道具が先に受ける**（次のセルへ）。
el('read').addEventListener('keydown', (e) => {
    if (e.code !== 'Tab' || imeBusy(e)) return;
    // caret の居場所は、文字の節のこともセルそのもののこともある ──
    // 片方だけ見ると、セルの終わりに置いたときだけ効かない。
    let n = getSelection()?.anchorNode;
    if (n && n.nodeType === 3) n = n.parentElement;
    const cell = e.target.closest?.('td, th') || n?.closest?.('td, th');
    if (!cell || !el('read').contains(cell)) {
        // **表の外。** 焦点を画面から飛ばさない ── 受けたなら止める。
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
    // 最後のセル ── 行を足して、その頭へ。
    const body = table.tBodies[0] || table;
    const wide = (table.tHead?.rows[0] || body.rows[0])?.cells.length || 1;
    const row = body.insertRow();
    for (let n = 0; n < wide; n++) {
        // 空のセルは、描く側によっては消える ── 全角空白で埋める（表の
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

/// DOM を文字に戻して、いつもの保存を通す。
///
/// **描き直さない。** 打っている最中に組み直すと、caret がどこかへ飛ぶ ──
/// 見た目は既に打った通りになっているので、組み直す理由も無い。
async function syncRead(leaving) {
    if (syncing || !state.open || !editor) return;
    // **変換の途中なら、書き戻さない。** 未確定の文字はまだ人の文字ではない
    // ── 確定してから数え直す（`composing` の註）。
    //
    // **ただし、去るときは別**（依頼 558）。変換の途中で一覧の別のノートを
    // 押すと、ここで黙って戻っていたので、**打った文字がどこにも残らずに
    // 消えていた**（会社の Windows で本人が踏んだ ── 日本語を打つ人は
    // 「一区切り打って、まとめて変換」なので、一度に消える量が大きい）。
    // 押しは `mousedown` で開く（依頼 477）ので、**確定は押しのあとに来る**
    // ── 待っていると間に合わない。画面に見えている文字を、そのまま拾う。
    // 未確定のままなら読みの仮名で残るが、**消えるよりはるかにましだ。**
    if (composing && !leaving) return;
    // **画面の文字が、いま開いているノートのものでなければ書き戻さない。**
    // 前のノートの文字を、今のノートへ書くことになる（`readDrawn`）。
    //
    // 同じパスで、**空の画面**からも書き戻さない ── コード画面だけを使って
    // いる人の表示画面は一度も組まれず、空の画面を文字に戻すと `"\n"` になり、
    // それが「一文字のノートに書き換えられた」として保存される（v2.8.3 で
    // 再現。コード画面で立ち上げて一覧で別のノートを押すと 2 バイトに
    // なった）。組んでいないならラベルも無いので、ここで止まる ──
    // **見張りは一つ。** 二つ置くと、片方だけ直した日に必ずずれる。
    if (!readCurrent()) return;
    const body = readToMd();
    if (body === null) {
        // **黙って止まらない。** 打った文字が消えたように見えるのがいちばん悪い。
        say('保存できません ── 図かコード枠の元の文字が取れません。'
            + '「コード」で直してください');
        el('state').textContent = '保存できません';
        drawSaveNow();
        return;
    }
    // **末尾の空行の数では、変わったことにしない。**
    //
    // 表示画面は末尾にいつも空の段落を一つ置く（`tailStop` ── 表や罫線で
    // 終わるノートに caret を降ろす先が要る）。だから文字に戻すと、末尾の
    // 改行が元の文字より一つ多くなる ── **見ただけのノートが、毎回「変わった」
    // ことになっていた**。ノートAを見てBを開くと、Aがフル保存され、履歴が
    // 1 つ積まれ、フォルダが丸ごと数え直された（1002本で 0.7 秒）。
    //
    // 末尾の空行は、表示画面では人が決められない（消しても `tailStop` が
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
        { name: '備忘', sub: '覚えておくこと', value: 'NOTE' },
        { name: 'ヒント', sub: '知っていると楽なこと', value: 'TIP' },
        { name: '重要', sub: '見落とすと困ること', value: 'IMPORTANT' },
        { name: '注意', sub: '気をつけること', value: 'WARNING' },
        { name: '警告', sub: '取り返しがつかないこと', value: 'CAUTION' },
    // **絞り込みの欄は出さない**（依頼 615・本人「絞り込みの入力欄は不要」）
    // ── 五つを絞り込む人はいない。押して選ぶだけの一覧にする。
    ], 'GitHub でも同じ形で出ます', true);
    if (kind === null) return;
    if (onRead()) {
        // **札だけ入れて、中の打てる行に caret を降ろす**（本人が決めた・2026-09-10）。
        // `> ` の行を書くと、次の保存で `>` に変わって差分が一度飛ぶ。
        await readSourceEdit((md) => (md.trim() ? md + '\n\n' : '') + '> [!' + kind + ']', null, 'inside');
        return;
    }
    put('> [!' + kind + ']\n> ');
}

/* ── 表 ── */

/// caret が表の中にあるとき、そのすぐ上に道具を出す。
///
/// **縦棒を数えさせない。** 打ち込む表は縦棒を数え続ける表で、揃え方の行
/// （`:---` `---:`）の形は誰も覚えていない ── iPhone の表と同じ考えで、
/// 数えるのは環境の仕事にする。
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

/// 表の道具。DOM をそのまま組み替えて、あとは `readToMd` が文字に戻す。
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
        // **空のセルは全角空白で埋める。** 中身が空のセルは描く側によっては
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

/* ── 表示画面の道具 ── */

/// caret の居る、いちばん外のかたまり。
/// 最後に caret が居たかたまり。
///
/// **押した瞬間には、もう分からない。** 帯のボタンを押すと焦点はボタンへ移り、
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
    // 文字と文字のあいだ（切り出しの一組・依頼 461）と、かたまり（依頼 204）。
    // **別のことを憶えている** ── 絵文字は前者に入り、記号は後者に効く。
    markCaret(box);
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

/// 見た目をその場で変える道具（文字の書式）。
///
/// `execCommand` は古い呼び方だが、**`contenteditable` で選んだところに
/// 書式を付けるパスは、いまも実質これしかない**。付くのは `<b>` や `<i>` で、
/// 文字に戻すときに `**` や `*` になる（`inlineToMd`）。
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
    // **書式の外へ caret を出す。**
    //
    // 選んだところに `<strong>` を掛けると、caret はその中に残る ──
    // 続けて打った文字まで太字になる（「あああ だけ太字にしたいのに、
    // その後もずっと太字」）。選んでいたときだけ、掛けたラベルの**すぐ後ろ**へ
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
///
/// **判断は切り出しの側（`blockAs`）** ── 同じボタンで付けると外す・一行は
/// 見出しか項目かどちらか一つ・表のセルでは一覧にしない。iPhone も同じ関数を
/// 呼ぶので、押し心地が端末で分かれない。
function readBlockAs(what) {
    const box = el('read');
    box.focus();
    if (!blockAs(box, what)) { say('表の中では使えません'); return; }
    readChanged();
}

/// セル ── caret の一行に（`checkLine`・デスクトップ版と iPhone で一組）。
function readCheck() {
    const box = el('read');
    box.focus();
    if (!checkLine(box)) { if (inCell()) say('表の中では使えません'); return; }
    readChanged();
}

/// caret が表のセルの中に居るか（表示画面）。
function inCell() {
    const sel = getSelection();
    let n = sel && sel.rangeCount ? sel.anchorNode : null;
    if (n && n.nodeType === 3) n = n.parentElement;
    const cell = n && n.closest ? n.closest('td, th') : null;
    return !!cell && el('read').contains(cell);
}

/// 文字そのものを書き換える道具（チェック・リンク・表…）。
///
/// **いったん全部を文字に戻してから直し、組み直す。** 見た目の上でやろうと
/// すると、セルや表のような「ラベルの形が決まっているもの」を DOM の上で組み立て
/// 直すことになり、そこだけ別の作り方が生える。
async function readSourceEdit(change, node, stay, upto) {
    const box = el('read');
    // 直すところは、たいてい caret のあるかたまり。**押して開く工房だけは
    // 別** ── 右押しは caret を動かさないので、押されたものを名指しで渡す。
    const at = [...box.children].indexOf(node || caretBlock());
    if (at < 0) return;
    // **選んだ範囲が二つ以上のかたまりにまたがることがある**（依頼 644 の
    // コードブロック）。`upto` を渡すと、そこまでをひとまとめにして渡し、
    // 返ってきた文字で置き換える ── 渡さなければ、これまでどおり 1 つだけ。
    const end = upto ? Math.max(at, [...box.children].indexOf(upto)) : at;
    const blocks = [...box.children].map((n) =>
        richBlock(n) ? n.dataset.md : (blockToMd(n) ?? ''));
    if (blocks.some((b) => b === undefined)) {
        say('ここからは書き戻せません（図の元の文字が取れません）');
        return;
    }
    try {
        blocks.splice(at, end - at + 1, await change(blocks.slice(at, end + 1).join('\n\n')));
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
    let to = stay ? kids[n] : (kids[n + 1] || kids[kids.length - 1]);
    if (!to) return;
    // **中へ降りる** ── 注記を入れたら、その打てる行に立つ。
    if (stay === 'inside') {
        to = kids[n + 1] || to;
        const inner = to.querySelector(':scope > p:not(.alert-h)');
        if (inner) to = inner;
        stay = false;
    }
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

/// 表示画面の「コードブロック」。
///
/// **選んでいる文字があれば、それをコードブロックにする**（依頼 644・本人
/// 「文字を範囲選択した状態で『コードブロック』を押下すると、選択した文字じゃ
/// なく、その文字の下側にコードブロックがでてくる」）。
///
/// 前は選んでいるかどうかを一度も見ずに、caret のあるかたまりの**下に**空の枠を
/// 足していた ── 太字も引用も選んだところに効くのに、ここだけ効かなかった。
///
/// **かたまりごと包む。** 段落の途中だけを選んでも、その段落まるごとが枠に
/// なる ── 引用や箇条書き（`readBlockAs`）と同じ考えで、かたまりの種類を
/// 変える道具は、かたまりに効く。二つ以上にまたがって選べば、まとめて 1 つの枠。
///
/// 選んでいなければ、これまでどおり空の枠を下に置く（依頼 618 ── 中身は空で
/// 出す。サンプルの文字を入れると、枠の中では消すところから始まる）。
function readFence() {
    const box = el('read');
    const sel = getSelection();
    if (!sel || sel.isCollapsed || !sel.toString().trim()) return readPut('```\n\n```');
    const r = sel.getRangeAt(0);
    const blockOf = (n) => {
        if (n && n.nodeType === 3) n = n.parentNode;
        if (!n || !box.contains(n)) return null;
        while (n && n.parentElement !== box) n = n.parentElement;
        return n;
    };
    const from = blockOf(r.startContainer);
    const to = blockOf(r.endContainer) || from;
    if (!from) return readPut('```\n\n```');
    return readSourceEdit((md) => {
        // **囲みは、中身より長くする。** 中に `` ``` `` があると、そこで枠が
        // 閉じて続きが本文として出る（Markdown の決まり）。
        const longest = Math.max(0, ...md.split('\n')
            .map((l) => /^(`{3,})/.exec(l)?.[1].length || 0));
        const wall = '`'.repeat(Math.max(3, longest + 1));
        return wall + '\n' + md + '\n' + wall;
    }, from, false, to);
}

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

/// 行番号の入切。**「コード」の画面だけの話。**
async function cmdLineNo() {
    const to = await askPick('行番号', [
        { name: '出す', sub: '「コード」の左に', value: true },
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
        ['チェック', '- [ ] やること', '押してチェックできるようになります'],
        ['太字', '**ここが太字**', '前後を ** で挟む'],
        ['斜体', '*ここが斜体*', '前後を * で挟む'],
        ['取り消し線', '~~消す文字~~', '前後を ~~ で挟む'],
        ['コード', '`コード`', '前後を ` で挟む'],
        ['コードブロック', '```\nここに何行でも\n```', '``` の行で挟む（帯の「コードブロック」でも入ります）'],
        ['折りたたみ', '<details>\n<summary>見出し</summary>\n\n中身\n\n</details>',
         '押すと開いたり閉じたり。GitHub でも同じ形で畳めます'],
        ['リンク', '[リンクの文字](https://)', '角括弧が文字、丸括弧が行き先'],
        ['画像', '![説明](画像の場所)', '頭に ! を付けるとリンクではなく画像'],
        ['引用', '> 引用する文章', '行の頭に > と空白'],
        ['注記', '> [!NOTE]\n> 覚えておくこと', 'NOTE / TIP / IMPORTANT / WARNING / CAUTION'],
        ['区切り線', '---', 'ハイフン3 つだけの行'],
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
/// 何が起きるかは `amber-core` の `markdown::marks` が決める。デスクトップ版が自分で
/// `#` を数えはじめると、iPhone と押し心地が分かれる。
///
/// 並びは iPhone と同じ ── 上の一列が「ノートが実際にできているもの」で、
/// 残りは本当に時々のもの。**十四個の帯は、毎回使う五個ぶんの値段を取る。**
const MARKS = [
    [
        ['見出し', '⌘1', () => onRead() ? readHeading() : applyMark('heading')],
        ['箇条書き', '⌘⇧8', () => onRead() ? readBlockAs('ul') : applyMark('line', '- ')],
        // **表のセルでは何もしない** ── 表の文字ぜんぶに `- [ ] ` が付く
        // （ネットワークが捕まえた・2026-09-10）。
        ['チェックリスト', '⌘⇧9', () => onRead() ? readCheck() : applyMark('line', '- [ ] ')],
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
        ['リンク', '⌘K', () => onRead() ? readPut('[リンクの文字](https://)') : put('[](https://)', 1)],
        // **セルを空で出さない。** 空の表は「これで合っているのか」が
        // 分からず、打つ前に一度立ち止まる ── サンプルの文字が入っていれば、
        // 上から順に置き換えるだけになる。
        ['表', '⌘⇧T', () => onRead()
            ? readPut('| 見出し | 見出し |\n| --- | --- |\n| 項目 | 項目 |')
            : put('| 見出し | 見出し |\n| --- | --- |\n| 項目 | 項目 |\n', 2)],
        ['水平線', '', () => onRead() ? readPut('---') : put('\n---\n\n')],
        ['|'],
        ['引用', "⌘'", () => onRead() ? readBlockAs('blockquote') : applyMark('line', '> ')],
        ['注記', '', cmdAlert],
        // **名前は「コードブロック」**（依頼 618・本人が決めた）。
        //
        // 帯のすぐ上に「コード」という画面のラベルがあるので、**そこだけの
        // 「コード」は置かない** ── 同じ文字が二つあると、押す前にどちらの
        // 話か分からない。「コードブロック」なら、画面のラベルとは別のものだと
        // 文字が言っている。**帯でいちばん長い札**だが、二列目は横に流れる
        // ので押し出しはしない（狭い画面でも入ることは実機で測った）。
        //
        // **中身は空で出す。** サンプルの文字を入れると（表がそうしている）、
        // 枠の中では**消すところから始まる** ── 枠に入れたいのは自分の文字で、
        // 言い換えるためのサンプルではない。
        ['コードブロック', '', () => (onRead() ? readFence() : putFence())],
        // **折りたたみ**（依頼 619・本人「コードがめちゃくちゃ長くて見にくい」）。
        // 記法は `<details>` ── GitHub がそのまま畳む形で、メモ帳で開いた
        // 人にも「畳んであるもの」と読める。ambər だけの記号は作らない。
        ['折りたたみ', '', () => (onRead() ? readPut(FOLD.trimEnd()) : putFold())],
    ],
];

/// いま打っているのは表示画面か。
///
/// **見えている画面ではなく、焦点で決める。** 並べているときは両方見えて
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
        // 畳んだり開いたりするボタンが見えないと、二列目があること自体が
        // 分からない。流れるのは記号だけにして、ボタンは端に残す。
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
            // 一度は絵だけにした（2026-09-08 の午前）── 訳の要る文字を帯に
            // 並べたくない、が理由だった。**その日のうちに戻している**：
            // 「かっこよくなったが、ビギナーには何がなんだか分からない」。
            //
            // **分かることを、訳の都合で捨てない。** 外へ配る日は、文字を
            // その国の言葉に置き換えればいい（絵にしても、`⌗` が「注記」だと
            // 分かる人はどの国にもいない）。絵も外したのは、名前が答えを
            // 言っているところに絵を添えても、目が二度読むだけだから。
            b.textContent = name;
            // **例外は一つだけ**（依頼 419・本人が決めた・2026-09-09）──
            // 絵文字のボタン。「絵文字」と書くより 😀 のほうが速く、しかも
            // **訳が要らない**: 押すと出てくるもののサンプルが絵そのもので、
            // ここだけは絵が名前より多くを言っている。
            if (icon) b.textContent = icon;
            b.title = key ? `${name}（${keyText(key)}）` : name;
            // 押した瞬間に焦点を奪わない ── 奪うと、どこに入れるかを
            // 決める手がかり（選んだところ）が先に消える。
            b.onmousedown = (e) => e.preventDefault();
            b.onclick = () => {
                // 錠のノートでは入れない（依頼 629）── 画面は打てなくして
                // あるが、帯のラベルはそこを通らずに文字を入れる道。
                if (!canEdit()) { say('このノートはロックされています（「今だけ編集する」を押すと書けます）'); return; }
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
            // **vim のボタンは置かない。** 使うのはたいてい一人で、毎日見る
            // 帯に居座る値打ちは無い ── ⚙ の中にある。
            const sep = document.createElement('div');
            sep.className = 'sep';
            const more = document.createElement('button');
            // **文字で書く。** 前は「…」で、押すまで何が出るか分からなかった
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

/// 選んだところを core に渡し、返ってきた文字で置き換える。
///
/// 名前が `mark` でないのは、**葉のマークを描く `mark(size)` が既にいる**から ──
/// 一度そちらを覆ってしまい、デスクトップ版の左上が `[object Promise]` になった。
///
/// **位置は渡さない。** JS は UTF-16 の桁で数え、Rust は文字で数えるので、
/// 絵文字が一つ混ざれば境目がずれる ── 選んだ文字そのものを渡す。
async function applyMark(kind, withWhat) {
    if (!editor || view === 'read') return;
    const model = editor.getModel();
    let sel = editor.getSelection();
    // 行頭のマークと見出しは、選んでいなくてもその行が相手。
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
    // 選んでいなかったのに挟んだときは、**マークの中に入れる** ── そこで打ちたい。
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
/// `caret` は**置いた文字の頭から数えた文字数** ── そこに入って打てるように
/// する。省くと、置いた文字の末尾に出る。**縦棒を数えるのは cian の仕事**で、
/// 揃え方の行（`:---`）の形を人が覚えている必要は無い。
/* ── 絵文字 ── */

/// 表は core が持っている（依頼 418）。**一度だけ取りに行く。**
let faces = null;
/// 最近つかったもの。**憶えるのは文字だけ** ── 名前は表から引ける。
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
/// 絵にするパスは採らない ── amber の外では意味の分からない文字が残るし、
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
    // 押したボタンと出てきたものが繋がって見えない。
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
/// 外を押したら閉じる。**板の中と、帯の絵文字のボタンは「外」ではない** ──
/// ボタンを押して閉じてしまうと、開け閉めが一打ぶんずれる。
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
/// 探しているのは文字であって、どの束に居るかではない。
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
    // **並べて表示では、憶えている caret で決める。** 板を押した時点で焦点は
    // 板の欄に移っているので `onRead()`（いまの選び目）は表示画面を指さない
    // ── 表示画面に打っていた絵文字がコード画面へ入る（ネットワークが捕まえた・
    // 2026-09-10）。
    const spotInRead = caretSpot && el('read').contains(
        caretSpot.startContainer.nodeType === 3 ? caretSpot.startContainer.parentNode : caretSpot.startContainer);
    if (onRead() || (view === 'split' && spotInRead)) {
        // **憶えている場所へ戻してから入れる。** `focus()` だけでは caret が
        // 先頭に落ち、ノートの頭に入る（依頼 461）。
        if (!caretBack(el('read'))) el('read').focus();
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

/// かたまりを入れて、caret を中へ（依頼 618・619）。
///
/// **行の途中では、かたまりにならない。** `` ``` `` も `<details>` も行の
/// 頭に無いとただの文字で、`もとの一行。```` と繋がって入ったときは**枠が
/// 開かないまま次の行を飲み込み**、「表示」画面に空っぽの枠が出た
/// （実機で出た）。打っている行に文字があるなら、先に行を改める。
function putBlock(text, caret) {
    if (!editor) return;
    const pos = editor.getSelection().getStartPosition();
    const before = editor.getModel().getLineContent(pos.lineNumber).slice(0, pos.column - 1);
    const head = before.trim() ? '\n' : '';
    put(head + text, head.length + caret);
}

/// コードブロック（`` ``` ``）── caret は囲みの中へ。
function putFence() {
    putBlock('```\n\n```\n', 4);
}

/// 折りたたみ（`<details>`）── caret は見出しの文字の頭へ。
///
/// **サンプルの文字を入れる**（表と同じ・依頼 619）── 空で出すと「ここに何を
/// 書くのか」を思い出すところから始まる。上から順に置き換えるだけにする。
const FOLD = '<details>\n<summary>見出し</summary>\n\n中身\n\n</details>\n';
function putFold() {
    putBlock(FOLD, '<details>\n<summary>'.length);
}

/// 画像をノートの隣に置いて、リンクを打つ。
///
/// **ノートの隣に置く。** 同期しているフォルダを別の端末で開いたとき、
/// 画像だけが来ないノートは「消えたのか、元から無いのか」が分からない。
async function pickPicture() {
    if (!state.open) return;
    const file = await window.amber.pickFile(
        [{ name: '画像', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'heic'] }]);
    if (!file) return;
    const got = await window.amber.fileBytes(file);
    if (!got) { say('その画像は読めません'); return; }
    await attach(got.b64, got.ext);
}

async function attach(b64, ext) {
    try {
        const r = await ask('image', { note: state.open.path, b64, ext });
        // **どちらの画面でも打つ。** `put` は「表示」画面では黙って帰るので、
        // ここだけ画面を見ずに呼んでいると、**画像はフォルダに置かれたのに
        // リンクがどこにも入らない** ── 誰も指していない画像が
        // `attachments/` に溜まる（ほかの記号は道具帯の側で画面を見ている）。
        if (onRead()) await readPut(`![](${r.link})`);
        else put(`![](${r.link})\n`);
        zonesSoon();
    } catch (e) {
        say('画像を置けません: ' + why(e));
    }
}

/* ── 表示画面 ── */

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

/// front matter を戻した、ファイルにあるとおりの内容。
///
/// **チェックの行番号は、この数え方の行番号。** 本文だけを渡すと、
/// front matter の行数ぶんずれたセルにマークが付く。
function whole() {
    return state.head + (editor ? editor.getValue() : '');
}

function applyView() {
    // **カレンダーを出しているあいだは、ノートの画面は引っ込む**（依頼 478）。
    // 同じ場所を使うので、両方は出せない。
    el('cal').hidden = !calOn;
    const open = !!state.open && !calOn;
    // **帯はいつも出す。** 設定（⚙）はノートを開いていなくても要る ──
    // 「保存場所を変える」はノートが一本も無いときにこそ押したい。
    el('top').hidden = false;
    for (const id of ['title', 'views', 'fontbtns', 'count2', 'state', 'dots', 'tocbtn']) el(id).hidden = !open;
    el('blank').hidden = open || calOn;
    el('work').hidden = !open;
    if (calOn) el('stripwrap').hidden = true;
    el('ed').hidden = !open || view === 'read';
    el('read').hidden = !open || view === 'write';
    el('toc').hidden = !open || !tocOn;
    // **表示画面でも道具の帯は出す。** 記号を覚えていない人の道具なので、
    // 記号の見えない画面でこそ要る。
    document.body.classList.toggle('reading', false);
    document.body.classList.toggle('split', view === 'split');
    document.body.classList.toggle('zen', zen);
    for (const b of document.querySelectorAll('#views button[data-view]')) {
        b.classList.toggle('on', b.dataset.view === view);
    }
    // 幅が変わったので測り直す。**畳みが効いた後で**（今のフレームでは
    // まだ古い幅しか見えない）。
    if (editor && view !== 'read') setTimeout(() => editor.layout(), 0);
    // **組み直しの終わりを返す。** 画面を替えたあとに「さっき居た場所」へ
    // 立つには、表示画面が組み直って触れるようになってからでないと、
    // 置いた選び目が古い DOM のものになる（実際に、立てずに落ちていた）。
    const drawn = open && view !== 'write' ? drawRead() : Promise.resolve();
    if (open && tocOn) drawToc();
    return drawn;
}

/// いま見ている（打っている）のは、ファイルの何行目か（`headLines` は下にある）。
///
/// 見ているものが同じなら、画面を替えても同じ場所に居てほしい ── 替えた
/// 先で毎回いちばん上に飛ばされると、**替えるたびに探し直す**ことになる。
function whereAmI() {
    if (view === 'write' && editor) {
        return editor.getPosition().lineNumber - 1 + headLines();
    }
    // 表示画面 ── caret のあるかたまり。無ければ、いま上に見えているもの。
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
    // 触れないかたまり（図・枠・画像）なら、その一つ上の打てるところへ。
    let land = hit;
    while (land && richBlock(land)) land = land.previousElementSibling;
    // **焦点を渡してから置く。** 置くだけだと、画面に焦点が無いあいだの
    // 選び目は誰のものでもなく、そのまま打っても入らない
    // （`setView` は「表示」に移るとき焦点を落とすので、そのあとに要る）。
    if (land && rd.isContentEditable) {
        rd.focus();
        landAt(land, null);
    }
}

async function setView(v) {
    // **替える前に、どこに居たかを控える。** 替えたあとでは、もう
    // 前の画面の caret も巻き位置も残っていない。
    const at = whereAmI();
    view = v;
    const drawn = applyView();
    window.amber.remember({ view: v });
    if (v === 'read') document.activeElement?.blur();
    else if (editor) editor.focus();
    await drawn;
    goToLine(at);
}

/// 上の帯の3 つ。**押せる形と、キーと、同じ一本のパスを通す。**
for (const b of document.querySelectorAll('#views button[data-view]')) {
    b.onclick = () => setView(b.dataset.view);
}

el('gear').onclick = (e) => {
    if (el('more').hidden) openMenu(e.currentTarget.getBoundingClientRect(), 'app');
    else closeMenu();
};
// 目次の開け閉め。**押せる場所は一つでいい** ── メニューの「目次」と同じ
// ものを呼ぶ（`toggleToc`）。一度このボタンを外してメニューだけにしたら、
// 見つからず「消えた」と言われた（依頼 297 の幅の都合だったが、
// 幅は `#meta` を固定してから空いている）。
el('tocbtn').onclick = () => toggleToc();

el('dots').onclick = (e) => {
    if (el('more').hidden) openMenu(e.currentTarget.getBoundingClientRect());
    else closeMenu();
};

/// 文字の大きさ。**編集画面と表示画面を一緒に動かす** ── 片方だけ動くと、
/// 並べたときに同じノートが二つの大きさで出る。
/// 「コード」の画面に行番号を出すか。**既定は出さない** ── ノートは
/// 行で指す文書ではないので、ふだんは数字が一列ぶん余計。
let lineNo = false;

function setLineNo(on) {
    lineNo = !!on;
    window.amber.remember({ lineNo });
    if (editor) editor.updateOptions({ lineNumbers: lineNo ? 'on' : 'off' });
}

let fontStep = 0;
const FONT_BASE = 15;
const FONT_MIN = -4;
const FONT_MAX = 8;

function setFont(step, quiet) {
    fontStep = Math.max(FONT_MIN, Math.min(FONT_MAX, step));
    // 端まで来たら、その向きのボタンを薄くする ── 押しても変わらないのに
    // 押せる顔をしていると、効いていないように見える。
    el('fontdown').disabled = fontStep <= FONT_MIN;
    el('fontup').disabled = fontStep >= FONT_MAX;
    // **戻すだけのとき（`quiet`）は書かない** ── 開いただけで設定が書き換わる。
    if (!quiet) window.amber.remember({ fontStep });
    const px = FONT_BASE + fontStep;
    if (editor) editor.updateOptions({ fontSize: px });
    el('read').style.fontSize = (px - 0.5) + 'px';
    // 起動して戻すときは黙る ── 開いた瞬間にラベルが出る理由は無い。
    if (!quiet && fontStep !== 0) say('文字の大きさ ' + px + 'px（' + keyText('⌘0') + ' で戻る）');
}

/// **Ctrl＋ホイールで文字の大きさ**（依頼 634・本人「無意識に実施したんだけど
/// 有効じゃなかったので、ちょっと違和感があったんだ」）。
///
/// どのデスクトップ版でもそうなっているので、手が先に動く。`preventDefault` は要る ──
/// 入れないと Electron がウィンドウぜんぶを拡大し、ノートだけでなく帯も列も膨らむ。
///
/// **溜めてから一段。** トラックパッドの摘みも同じ形（`ctrlKey` 付きの
/// ホイール）で来るが、一回の摘みで何十も飛んでくるので、そのまま一段ずつ
/// 数えると端から端まで一瞬で行ってしまう。
let zoomRoll = 0;
const ZOOM_NOTCH = 40;
window.addEventListener('wheel', (e) => {
    if (!e.ctrlKey && !e.metaKey) return;
    e.preventDefault();
    zoomRoll += e.deltaY;
    if (Math.abs(zoomRoll) < ZOOM_NOTCH) return;
    // **一回で一段。** マウスの一目盛りは 120 ほど来るので、割り算で数えると
    // 一目盛りで二段も三段も飛ぶ（本物のデスクトップ版で測った）。溜まりは使い切る。
    const way = zoomRoll > 0 ? -1 : 1;   // 手前に回す＝小さく
    zoomRoll = 0;
    if (el('cal').hidden) setFont(fontStep + way);
    else setCalFont(calFontStep + way);
}, { passive: false });

// 帯の「A−」「A＋」（依頼 623）。キーと同じ一本の道（`setFont`）を通す。
el('fontdown').onclick = () => setFont(fontStep - 1);
el('fontup').onclick = () => setFont(fontStep + 1);
el('fontdown').title = '文字を小さく（' + keyText('⌘−') + '）';
el('fontup').title = '文字を大きく（' + keyText('⌘+') + '）';

/// カレンダーの文字の大きさ（依頼 623）。**ノートとは別の段** ── セルの中の文字は
/// ぜんぶ `em` なので、表の土台の大きさを一つ変えれば揃って動く。帯のボタンは
/// 大きくしない（大きくするたびに帯が折れて、押したボタンが逃げる）。
let calFontStep = 0;
const CAL_FONT_BASE = 14;
const CAL_FONT_MIN = -3;
const CAL_FONT_MAX = 6;
function setCalFont(step, quiet) {
    calFontStep = Math.max(CAL_FONT_MIN, Math.min(CAL_FONT_MAX, step));
    const box = el('calbox');
    const px = CAL_FONT_BASE + calFontStep;
    for (const part of box.querySelectorAll('.body, .calnone')) part.style.fontSize = px + 'px';
    box.querySelector('.fdown').disabled = calFontStep <= CAL_FONT_MIN;
    box.querySelector('.fup').disabled = calFontStep >= CAL_FONT_MAX;
    if (quiet) return;
    window.amber.remember({ calFontStep });
    say('カレンダーの文字 ' + px + 'px');
}

function toggleRead() { setView(view === 'read' ? 'write' : 'read'); }
function toggleSplit() { setView(view === 'split' ? 'write' : 'split'); }

function setZen(on) {
    zen = on;
    if (el('zenbtn')) { el('zenbtn').textContent = on ? '⤡' : '⤢'; el('zenbtn').title = on ? '元の大きさに戻す（F12 か Esc）' : 'ノートだけを大きく（F12）'; }
    applyView();
    // 戻るときは黙る ── `say('')` は空のラベルを出してしまう。
    if (on) say('ノートだけを大きく（F12 か Esc で戻る）');
}

/// いま何文字か。
///
/// **数えるのは本文だけ** ── 前書き（題・タグ・作った日）はノートが自分を
/// 説明する言葉で、書いた量ではない。語数ではなく文字数にしたのは、日本語に
/// 語の切れ目が無いから ── 空白で切って数えると、一段落が「1 語」になる。
function drawCount() {
    if (!editor || !state.open) { el('count2').textContent = ''; return; }
    const t = editor.getValue();
    const chars = [...t.replace(/\s/g, '')].length;
    const lines = t ? t.split('\n').length : 0;
    el('count2').textContent = chars.toLocaleString() + ' 文字 ・ ' + lines + ' 行';
}

/// 打っている間の組み直しは、止まってから。
function readSoon() {
    if (view === 'write') return;
    clearTimeout(drawTimer);
    drawTimer = setTimeout(drawRead, 260);
}

async function drawRead() {
    if (view === 'write' || !state.open) return;
    // **変換中は組み直さない。** 組み直すと未確定の文字が消える ── 用事は
    // 預かって、確定してから通す（向こうから来た文字の混ぜ込み・テーマ替え・
    // セルの押し下げ）。混ぜ込みは待てる（ファイルは既に向こうの文字で、
    // こちらが書き戻すときに混ざる）。
    if (composing) { drawAfter = true; return; }
    const seq = ++readSeq;
    // 組みはじめたときのノート。帰ってきたときに別のノートが開いていたら
    // 捨てる ── 「コード」の画面へ替えてから別のノートを開くと組み直しが
    // 走らないので、遅れて着いた前のノートの文字に今のノートのラベルが付く。
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
    // 空のままなら文字に戻すときに落ちるので、増えも減りもしない。
    tailStop();
    // 先頭が図や枠なら、その上にも降りられる一行を（`tailStop` の対）。
    headStop(el('read'));
    // 空の注記・引用に、打てる一行を。
    fillAlerts(el('read'));
    // **ラベルを配るのが先。** 画像や図はこのあとラベルを掛け替える（`<pre>` →
    // `<div class="mermaid">`、`<img>` → `<figure>`）ので、掛け替える前に
    // 元の文字を持たせておかないと、引き継ぐものが無い ── 図を入れたノートで
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
        // 渡した id をそのまま付けるので、id だけで消すと**いま画面に挿した
        // 図そのもの**が消える（実際に消えて、図が 1 つも出なくなった）。
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
/// 画面は、同じ色でうまくいくものではない。
///
/// 明るさを上げ、鮮やかさを落とした**淡彩**にする。濃い色を並べると、
/// 円の面積の大半が暗い塊になって図全体が沈む ── ノートは白い紙で、その上に
/// 置く図が紙より重くなる理由が無い。
///
/// 淡いぶん**文字は白ではなく濃い墨**を載せる（`pieSectionTextColor`）。
/// 明るい地に白は乗らない。
const FAMILY = [
    '#F7BD5C', '#8FC8E8', '#A8D9A8', '#C9AEE0', '#F7A99C', '#8ED9CE',
    '#EFDA8A', '#AEBBEE', '#F4B4CE', '#C6DE8E', '#D3D3D9',
];

/// 淡彩の上に置く文字の色。
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
        // 既定では、文字が通らないと mermaid は**自分で赤い絵を描いて
        // 置いていく** ── その保存場所は `document.body` で、こちらの
        // `catch` は絵を消せない。打つたびに描き直すので、一文字ごとに
        // 1 つずつ積み上がり、画面の上に「Syntax error in text」が
        // 並んだ。積まれた絵は幅を持つので**ノートが左へ潰れる**。
        // 戻す（⌘Z）は文字を戻すだけで、置いていかれた絵には届かない ──
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
            // 文字の色は塗りに載るので、濃い色には白、明るい色には濃い茶。
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
            // 節の角を丸める ── 既定の直角は、amber のどの画面にも無い形。
            nodeTextColor: v('--ink', '#2a2011'),
            darkMode: dark,
        },
        // 円グラフの色。**既定の派手な12色は、琥珀の隣で喧嘩する** ──
        // フォルダと文字色に使っている11色と同じ並びを渡して、アプリの
        // どこを見ても同じ色のグループにする。
        themeCSS: '.pieTitleText{font-size:15px;font-weight:700}'
            + '.slice{font-size:13px;font-weight:600}'
            + '.pieCircle{stroke:' + v('--paper', '#fffdf8') + ';stroke-width:2px}'
            + '.pieOuterCircle{stroke:' + v('--line', '#e4d9c4') + '}'
            + '.legend text{font-size:13px}'
            // 年表のラベルの文字。**縮むぶんを見越して大きめに** ── 出来事が
            // 増えるほど図は横に伸び、伸びたぶんだけ全体が縮んで文字も縮む。
            + '.timeline text,.timeline tspan{font-size:15px}'
            + '.timeline .sectionTitle,.timeline .sectionTitle tspan{font-weight:700}'
            // マインドマップだけは `themeVariables` を見ない ── 灰と藤色で
            // 描かれて、琥珀のノートの上で**そこだけ別のアプリ**に見える。
            // 枝は円グラフと同じ十一色にして、まん中は琥珀そのものに。
            // **まん中は、ただの丸い橙ではない。** 濃い琥珀で塗って、
            // 一段明るい琥珀の輪を掛け、影を 1 つ敷く ── 枝より手前にある
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
                // **枝の文字の色を、こちらで決める。** 渡さないと mermaid が
                // 塗りの色から作り、明るい琥珀の枝では薄い文字が薄い地に
                // 載って読めなかった（「仕事」が消えていた）。地はどの枝も
                // 14% の淡い色なので、文字はノートの地の色でいい。
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
        // 年表は、既定だとラベルが小さくて中の文字が読めない ── 「いつ何があった
        // か」を見る図なのに、その「なに」が潰れている。ラベルを広げて文字に余白を。
        timeline: {
            useMaxWidth: true, width: 168, height: 66,
            padding: 10, boxMargin: 12, boxTextMargin: 8,
            diagramMarginX: 22, diagramMarginY: 18, leftMargin: 70,
        },
        // 予定表は、既定だと細い帯に目盛りが詰まって日付が重なる（読めない）。
        // 横幅いっぱいまで伸ばし、棒と余白を広げて、目盛りの文字を離す。
        gantt: {
            // **広く描いてから、入るところまで縮める。** 既定の幅だと目盛りが
            // 重なって日付が読めない（`09/1009/12…` になる）。広い紙に描けば
            // 文字は離れ、`useMaxWidth` が枠に合わせて全体を縮めてくれる。
            useWidth: 980, useMaxWidth: true,
            barHeight: 22, barGap: 7,
            topPadding: 48, leftPadding: 88, gridLineStartPadding: 32,
            fontSize: 12, sectionFontSize: 12, numberSectionStyles: 4,
        },
        // ノートは人が書いたもの。図のラベルに書いた HTML を効かせない。
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
        const q = await ask3('分かれパスの問い', '', '左利きですか？');
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
        const title = await ask3('年表のタイトル', '', '今年');
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
        const title = await ask3('予定表のタイトル', '', '段取り');
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
        // mermaid のバンドルの中には、`define.amd` を見て自分から名乗り出る
        // 小さなパーサーが入っている。ローダはその名乗りを「mermaid だ」と
        // 受け取るので、返ってくるのは `initialize` を持たない別物になる
        // （実際にそうなった。「mermaid が名乗りません」はそれ）。
        //
        // だから **`define` を伏せてから素の `<script>` で読む。** バンドルは
        // 最後に `globalThis.mermaid` へ自分を置く。
        //
        // **旗（`define.amd`）を下ろすだけでは足りない。** 中のパーサーは
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
            // **元の文字と行番号を引き継ぐ。** 引き継がないと、文字に戻すとき
            // この図の中身がどこにも無く、**保存のたびに図が消える**
            // （実際に消えた）。組み直しでラベルを掛け替えるところは、
            // 掛け替えたぶんを必ず持っていく。
            keepMark(code.parentElement, box);
            code.parentElement.replaceWith(box);
        } catch (e) {
            // **描けない図は、書いた文字のまま残す。** 消すと、直しようがない。
            if (seq !== readSeq) return;
            code.parentElement.classList.add('bad');
            code.parentElement.title = '図にできません: ' + (e && why(e) ? why(e) : e);
        } finally {
            sweepMermaid(id);
        }
    }
    // 掛け替えたあとのラベルにも、触れないマークと元の文字を。
    if (seq === readSeq) armRead();
}

/* ── 図の工房 ── 図を見ながら、表で直す ── */

/// **書き方を覚えなくても、作った図を直せるようにする。**
///
/// 作るところ（`cmdDiagram`）は選ぶだけで済むのに、**直すところが文字だけ**
/// だった。`flowchart LR` も `A -->|はい| B` も覚えていない人にとって、一度
/// 作った図は「作り直すしか手が無いもの」で、それは作れると言えない。
///
/// ここは図を押すと工房が開く。左に表、右に図。表を直すとその場で描き直る
/// ので、**当たっているかを、保存する前に目で確かめられる**。
///
/// **読み戻せない図でも閉め出さない。** 手で書いた凝った図は表にならない
/// が、そのときは文字の画面が出る ── 文字で直しながら、右で図を見られる。工房を
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

/// 図の文字を、表に読み戻す。読めなければ `null`（そのときは文字で直す画面になる）。
///
/// **読めなかった行を、黙って落とさない。** 半分だけ読めた表を出すと、直して
/// 保存した瞬間に読めなかった行が消える ── 消えたことに気づくのは何日も後に
/// なる。だから一行でも読めなければ、表そのものを諦める。
///
/// 表にしない行（`dateFormat`、`x-axis`、注釈…）は **`head` にそのまま
/// 取っておいて、書き戻すときに戻す**。読めた行だけ組み直して、あとは元の文字。
function mmdParse(src) {
    const kind = mmdKind(src);
    if (!kind) return null;
    const live = src.split('\n').filter((l) => l.trim() !== '');
    if (!live.length) return null;

    // マインドマップだけは、**インデントが中身**（枝の深さ）なので別に読む。
    if (kind === 'mind') {
        const lines = live.slice(1).filter((l) => !l.trim().startsWith('%%'));
        const rootAt = lines.findIndex((l) => /^\s*root\(\(/.test(l));
        if (rootAt < 0) return null;
        const kids = lines.filter((_, i) => i !== rootAt);
        // 凝った形（`枝[四角]`）は表にできない ── 平らに直すと形が変わる。
        if (kids.some((l) => /[[({]/.test(l.trim()))) return null;
        const deep = (l) => /^\s*/.exec(l)[0].length;
        // **インデントの段を、深さの番号に直す。** 空白が二つでも四つでも
        // 「一段下」は一段下なので、出てきたインデントを浅い順に並べて
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
            // ときに残りが消える。読めない形なら表にせず、文字の画面で直す。
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

/// 表を、図の文字に書き戻す。
function mmdBuild(d) {
    const q = (t) => String(t ?? '').trim().replace(/"/g, '”');
    // 節の名前に括弧や縦棒が混ざると、そこで形が終わったことになって
    // 図が壊れる ── 打てる場所なので、通りパスで落としておく。
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
        // 色は最後にまとめて ── 箱と線を読んでから見るほうが、文字のままでも
        // 図の形が先に読める。
        ...live.filter((r) => r.color).map((r) => nodeStyle(r.id, r.color)),
    ]);
}

/* ── 箱の色 ── */

/// 図の箱の色。**名前は amber のどこでも同じ十一色**（フォルダと同じ）──
/// 覚える色の名前を、画面ごとに増やさない。
///
/// 紙の上では**淡く塗って、濃い線で囲む**。濃いまま塗ると箱の中の文字が沈み、
/// 読ませるには白い文字に替えることになる ── そうすると「どの色なら白か」を
/// 人が考える羽目になる。淡い地なら、文字はいつも同じ濃い墨でいい。
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

/// この色の上に濃い文字を置けるか。**塗りの明るさで決める** ── 白い文字を
/// 明るい黄の上に置くと読めず、濃い文字を深い青の上に置いても読めない。
/// 目が明るさを感じる重みは色ごとに違うので、そのまま重みを掛ける。
function light(hex) {
    const n = parseInt(hex.slice(1), 16);
    const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    return (r * 0.299 + g * 0.587 + b * 0.114) > 150;
}

/// 種類ごとの、表の形。**列の名前がそのまま説明になる** ── 「なに」「いつ」
/// と書いてあれば、何を打てばいいかを別に書かなくていい。
const DIAGRAM_FORM = {
    // 流れ図だけは表が二つ（箱と線）なので、デスクトップ版は `studioFlow` が別に描く。
    // **`cols` はそれでも書いておく** ── iPhone はここを読んで欄を作るので、
    // 無いと「形は選べるのに名前を書く欄が無い」画面ができる（実際にできた）。
    flow: {
        name: '流れ図', add: '箱を足す',
        cols: [{ k: 'a', label: '箱の中の言葉', w: 3, ph: '書く' }],
    },
    pie: {
        name: '円グラフ', title: 'タイトル', add: '割合を足す',
        cols: [{ k: 'a', label: '名前', w: 3, ph: '仕事' },
               { k: 'b', label: '数', w: 1, ph: '5' }],
    },
    quad: {
        name: '四象限', title: 'タイトル', add: 'やることを足す',
        cols: [{ k: 'a', label: 'やること', w: 3, ph: '週報' },
               { k: 'b', label: '影響', w: 1, ph: '0.8', slide: true },
               { k: 'c', label: '期限の近さ', w: 1, ph: '0.9', slide: true }],
    },
    time: {
        name: '年表', title: 'タイトル', add: 'できごとを足す',
        cols: [{ k: 'a', label: 'いつ', w: 1, ph: '4月' },
               { k: 'b', label: 'なに', w: 3, ph: '引っ越し' }],
    },
    gantt: {
        name: '予定表', title: 'タイトル', add: 'やることを足す',
        cols: [{ k: 'a', label: 'やること', w: 3, ph: '下ごしらえ' },
               { k: 'b', label: '始まり', w: 1, ph: '2026-09-10', date: true },
               { k: 'c', label: '長さ', w: 1, ph: '3d' }],
    },
    mind: {
        name: 'マインドマップ', title: 'まん中', add: '枝を足す', deep: true,
        cols: [{ k: 'a', label: '枝', w: 1, ph: '仕事' }],
    },
    seq: {
        name: 'やりとり', title: 'タイトル', add: 'やりとりを足す',
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

/// 工房を開く。`node` は表示画面の図（`.mermaid`）か、描けなかった枠。
async function studioOpen(node) {
    const md = node && node.dataset ? node.dataset.md : undefined;
    if (md === undefined) { say('この図の元の文字が取れません'); return; }
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

/// いまの表（か文字）から、図の文字を作る。
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
    // 図が主、コードが従（本人・2026-09-12）。
    swap.textContent = studio.raw ? '図で直す' : 'コードで直す';
    // 読み戻せなかった図は、表に戻れない ── 押せる顔をしておいて何も
    // 起きないより、押せないと見えているほうがいい。
    swap.disabled = studio.raw && !studio.data;
    swap.title = swap.disabled
        ? 'この図は表にできません（手で書いた形）。コードで直してください'
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
            + 'そのまた下にも書けます（mermaid の枝と枝のあいだには文字を置けないので、'
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
            // 深さは、インデントそのもので見せる ── 数字で「2」と書くより、
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

/// 一つの欄。数は 0〜1 のつまみ、日付は日付、あとは文字。
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
        // 通るパスでどれが行き止まりか」を形だけで言うことになる。名前は
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

/// 文字で直す画面。**表にできない図の逃げ道**であり、書き方を覚えた人の近道。
function studioRaw() {
    const wrap = tag('div', 'grp rawgrp');
    wrap.append(tag('label', '', 'mermaid のコード'));
    const t = document.createElement('textarea');
    t.value = studio.text;
    t.spellcheck = false;
    t.oninput = () => { studio.text = t.value; studioShow(); };
    wrap.append(t);
    if (!studio.data) {
        wrap.append(tag('div', 'note',
            'この図は表にできない形（手で書いたか、ambər の知らない書き方）です。'
            + '右の図を見ながら、ここで直してください。'));
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
/// 描くと、打っている最中の壊れた文字で「図にできません」が点滅する。
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
    // **工房はいちばん積もる場所。** 打つたびに描き直すので、通らない文字を
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
        err.textContent = 'いまの書き方では図になりません: ' + (e && why(e) ? why(e) : e);
    } finally {
        sweepMermaid(id);
    }
}

/* ── 工房の受け口 ── */

el('studio').addEventListener('keydown', (e) => {
    // 中で打った文字を、外の近道に取られない（`b` で太字、`e` で画面替え…）。
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
        // 文字 → 表。読めなければ、表にせず言う ── 読めない文字を無理に
        // 表へ入れると、読めなかったところが消える。
        const back = mmdParse(studio.text);
        if (!back) { say('いまの文字は表にできません。文字のまま直してください'); return; }
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
/// エディタは既にこの環境の中で `rust` も `js` も色分けしていて、同じ言語に
/// 二つの色分けを持つと、書いている画面と読める画面で色が違うノートができる。
/// 語彙は `vendor/monaco/vs/basic-languages` にあるものがそのまま効く。
async function paintCode() {
    const seq = readSeq;
    for (const code of el('read').querySelectorAll('pre > code[class^="language-"]')) {
        const lang = code.className.replace('language-', '').trim();
        // mermaid は図として描くので、文字に色を付けない。
        //
        // `amber` も渡さない（依頼 437）── あれは絵で、語彙ではない。
        // Monaco は知らない語彙でも文字を span で包んで返し、その色が
        // **こちらの琥珀色を上書きする**（実際に黒いまま出た）。
        if (!lang || lang === 'mermaid' || lang === 'amber') continue;
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
        // 読み上げにしか届かない文字になる。書いていなければ何も足さない
        // ── 空のラベルは、説明の無いことを説明しているように見える。
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

/// 読める形の中で押せるもの ── セルと、リンク。
el('read').addEventListener('click', async (e) => {
    const box = e.target.closest('.box');
    if (box) {
        // **押した瞬間に裏返す。** 文字を待たない ── 100ms 後に変わるのは
        // 「効いたか分からない」の入口（`PAPER.ja.md` 六章の己）。セルの
        // 状態は DOM が既に持っている（`aria-pressed`）ので、そこを先に。
        const line = Number(box.dataset.line);
        const done = box.textContent.trim() === '☐';
        box.textContent = done ? '☑' : '☐';
        box.setAttribute('aria-pressed', String(done));
        // **判断は core。** `check` は文字を返すだけで、保存はいつもの
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
            // 長いノートでは一瞬止まる。行番号は動いていない（セルの一文字が
            // 変わっただけ）ので、ラベルを持たせ直すだけでよい。
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
    // **触れないものは、押すと吹き出し。** 図・枠・画像で同じ形に揃える
    // （`PAPER.ja.md` 六章の甲・本人が決めた）── 覚えることを一つにする。
    //
    // 前は図と枠で違うことが起きていた（図は工房、枠は並べて表示のその行へ）
    // ── どちらも「押した」だけなのに、行き先が違った。**「消す」を置くのは、
    // 図や枠を消すのに「コード」へ行かせないため**（表示のまま消せること）。
    const art = diagramAt(e.target);
    if (art) {
        e.preventDefault();
        popMenu([
            { name: '図を直す', sub: 'ツールが開きます', run: () => studioOpen(art) },
            { name: '消す', sep: true, run: () => dropBlock(art) },
        ], { x: e.clientX, y: e.clientY });
        return;
    }
    const a = e.target.closest('a');
    if (!a) {
        // **触れないかたまりは、編集画面のその行へ送る。**
        // 打てるのに保存されない、を作らないための逃げ道。
        // **`richBlock()` と同じ顔ぶれにする。** ここだけ古いままだと、
        // 触れるようにしたはずの表を押した瞬間に編集画面へ飛ぶ（実際に飛んだ）。
        // **コードブロックは、ここへ来ない**（依頼 644）── 触れるように
        // なったので、押すのは「中に caret を置く」こと。図の枠だけは
        // `richBlock()` が触れないままにしてあるので、こちらへ来る。
        let rich = e.target.closest('pre, figure, .mermaid');
        if (rich && !richBlock(rich)) rich = null;
        if (rich && el('read').contains(rich)) {
            // **画像は、押したら原寸で開く**（依頼 637）── 紙の幅に
            // 合わせて描いているので、文字の入った画面写真は縮んで読めない。
            // 前はダイアログ（大きさ…／消す）を出していたが、その二つは原寸のデスクトップ版の
            // 中に置いた ── 押して真っ先にしたいのは「大きく見る」ほう。
            if (rich.tagName === 'FIGURE') { openLens(rich); return; }
            popMenu([
                { name: 'コードで直す', sub: '「コード」のその行へ', run: () => toSource(rich) },
                { name: '消す', sep: true, run: () => dropBlock(rich) },
            ], { x: e.clientX, y: e.clientY });
        }
        return;
    }
    // **デスクトップ版の中では開かせない。** 踏んだ先にウィンドウごと持っていかれると、
    // 題文字をデスクトップ版の中に描いている以上、戻るパスが無い。
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
    // **押すと吹き出し。** 打てる画面で押した瞬間に外へ飛ぶと、文字を直したい
    // 人にパスが無い（右押しを知らない人が多い）── Notion・Google Docs の型。
    // 触れないかたまりと同じ「押すと吹き出し」に揃える。
    popMenu([
        { name: '開く', sub: href, run: () => openLink(href) },
        { name: '文字を直す', sub: 'ここに caret を置きます', run: () => landAt(a, null) },
        { name: 'リンク先を写す', sep: true, run: () => copyText(href, 'リンク先') },
    ], { x: e.clientX, y: e.clientY });
});

/// 外の行き先を開く。
async function openLink(href) {
    if (!(await window.amber.openLink(href))) say('この行き先は開けません: ' + href);
}

/// **画像の大きさの選び肢。**
///
/// 数を訊かない ── 「200px」と打てる人は記法で書ける（`![w:200px]`）。
/// ここに来るのは打てない人なので、**言葉で選ばせる**。
/// 幅だけを指す: 縦横のどちらも訊くと、釣り合いを自分で守る仕事になる。
const SIZES = [
    { name: '幅いっぱい', sub: '指示なし（もとの出かた）', px: null },
    { name: '大きめ', sub: '横 640px', px: '640px' },
    { name: '中くらい', sub: '横 400px', px: '400px' },
    { name: '小さめ', sub: '横 200px', px: '200px' },
];

/// いまの大きさを、メニューの右に添える一言。
/// 原寸で見るウィンドウ（依頼 637）。**縮めない** ── 縮めるなら開く意味が無い。
/// はみ出したぶんは転がして見る。「画面に合わせる」で一度だけ縮められる。
let lensOf = null;
function openLens(fig) {
    const img = fig.querySelector('img');
    if (!img) return;
    lensOf = fig;
    const lens = el('lens');
    const shown = lens.querySelector('img');
    shown.src = img.currentSrc || img.src;
    shown.alt = img.getAttribute('alt') || '';
    lens.classList.remove('fit');
    lens.hidden = false;
    // **大きさは、画像が答える。** 読み込み前は 0 なので、載ってから書く。
    const tell = () => {
        const w = shown.naturalWidth;
        const h = shown.naturalHeight;
        lens.querySelector('.sz').textContent = w
            ? (img.getAttribute('alt') ? img.getAttribute('alt') + ' ・ ' : '') + w + ' × ' + h + ' px'
            : '';
    };
    if (shown.complete) tell(); else shown.onload = tell;
}

function closeLens() {
    el('lens').hidden = true;
    lensOf = null;
}

el('lens').onclick = (e) => {
    const what = e.target.closest('button')?.dataset.do;
    // **地を押したら閉じる。** 画像そのものを押しても閉じない ── 転がして
    // 見ている途中の一押しで消えると、探していた場所を見失う。
    if (!what) { if (!e.target.closest('#lensbox img')) closeLens(); return; }
    const fig = lensOf;
    if (what === 'fit') { el('lens').classList.toggle('fit'); return; }
    closeLens();
    if (!fig) return;
    if (what === 'size') askSize(fig);
    else if (what === 'drop') dropBlock(fig);
};

function sizeNow(fig) {
    const w = fig.querySelector('img')?.style.width || '';
    return SIZES.find((s) => s.px === w)?.name || (w ? '横 ' + w : '幅いっぱい');
}

/// 画像の大きさを選んで、**ノートの文字に書く**。
///
/// 押して選んだ結果が `![猫 w:200px](…)` という**打てる文字**として残る ──
/// あとから記法で直せるし、amber の外でも読める（芯の 1）。
async function askSize(fig) {
    const now = fig.querySelector('img')?.style.width || '';
    const px = await askPick('画像の大きさ',
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

/// 触れないかたまりを、編集画面のその行へ。
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
    // 何も残らないなら、打てる一行を置く ── 空の画面には caret を置けない。
    if (!el('read').children.length) tailStop();
    readChanged();
}

// 右押しでも同じ入口。**押しても右押しでも開く** ── どちらだったかを
// 覚えている人はいないので、両方に置く。
//
// ただし**箱そのものを右押ししたときは、色のメニュー**（`paintNode`）。
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

/// 文字を写す。**転んでも黙らない** ── 写せたつもりで貼れないのが最悪。
async function copyText(text, what) {
    try {
        await navigator.clipboard.writeText(text);
        say(what + 'を写しました');
    } catch (e) {
        say('写せません: ' + why(e));
    }
}

/// 「表示」画面の右押し。**指しているものにすることだけを出す。**
///
/// 表の上なら表のこと、リンクの上ならリンクのこと、セルの上ならセルのこと。
/// どこでもない文字の上なら、切り貼りと書式 ── 書式の中身は道具帯
/// （`MARKS`）から引く。**二か所に書かない** ── 書くと、記号を一つ足した
/// 日に道具帯にだけ増えて、右押しには出てこない。
function readMenu(e) {
    if (view === 'write') return;
    e.preventDefault();
    const at = { x: e.clientX, y: e.clientY };
    const t = e.target;
    const sel = String(getSelection() || '');

    // セルの上 ── 済み／未済と、下に一つ。
    const box = t.closest('.box');
    if (box) { popMenu([
        { name: 'チェックを入れ替える', run: () => box.click() },
        { name: 'この下に一つ足す', run: () => { landOnBox(box); readMark('line', '- [ ] ', true); } },
    ], at); return; }

    // リンクの上。
    const a = t.closest('a[href]');
    if (a) { popMenu([
        { name: '開く', sub: a.getAttribute('href'), run: () => window.amber.openLink(a.href) },
        { name: 'リンク先を写す', run: () => copyText(a.getAttribute('href') || '', 'リンク先') },
        { name: '文字だけ残す', run: () => { landAt(a); readDress('unlink'); } },
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

    // コードブロックの中（依頼 644）── **左押しは打つためのもの**になったので、
    // 「コードで直す」と「消す」はここへ移した。前は左押しでこの二つを出して
    // いたが、それだと枠の中に caret が置けず、本人の言う「表示モードで操作が
    // 完結する」に届かない。図（mermaid）は今までどおり左押しで工房が開く。
    const fence = t.closest('pre');
    if (fence && el('read').contains(fence) && !richBlock(fence)) {
        popMenu([
            { name: '枠の中身を写す', run: () => copyText(codeOf(fence), 'コード') },
            { name: 'コードで直す', sep: true, sub: '「コード」のその行へ', run: () => toSource(fence) },
            { name: 'この枠を消す', run: () => dropBlock(fence) },
        ], at);
        return;
    }

    // どこでもない文字の上 ── 切り貼りと書式。
    popMenu([
        { name: '切り取り', key: '⌘X', dim: !sel, run: () => document.execCommand('cut') },
        { name: 'コピー', key: '⌘C', dim: !sel, run: () => document.execCommand('copy') },
        { name: '貼り付け', key: '⌘V', run: () => document.execCommand('paste') },
        ...MARKS.flat().filter(([n]) => n !== '|' && n !== '画像' && n !== 'フロー')
            .map(([name, key, run], i) => ({ name, key, sep: i === 0, run })),
    ], at);
}

/// セルのある行へ caret を置く（`landAt` は切り出しの側にある一本を使う ──
/// 同じことをする関数を二つ持たない）。
///
/// **名前を分けてある**（依頼 424）。前は `landAfter` という名前で、
/// **`readSourceEdit` の末尾にある同じ名前の関数を上書きしていた** ──
/// 関数の宣言は後ろが勝つので、記号（チェックリスト・リンク・表・水平線）や
/// 画像の大きさを表示の画面から直すたびに、ここへ番号が渡って落ちていた。
/// 落ちるのは書き終えた**あと**なので、文字は入る ── caret だけがどこかへ
/// 行き、`console` にだけ跡が残る。総ざらいで見つけた。
function landOnBox(box) {
    const li = box.closest('li') || box.parentElement;
    if (li) landAt(li);
}

/// 押されたところの図。描けた図（`.mermaid`）と、描けなかった枠のどちらも。
/// 描かれた図の、箱そのものを右押しして色を変える。受けたら `true`。
///
/// **工房を開かずに、一つだけ直せる道。** 色を一つ変えるためだけに工房を
/// 開いて、表を見つけて、閉じるのは遠い ── フォルダの色を右押しで変える
/// のと同じ手ぶりにする（このデスクトップ版で「色を変える」は右押し、と一つに決まる）。
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

/// 色のメニュー。**名前と、その色そのものを並べる** ── 「ベルガモット」が
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

/// **ノートの文字そのものを積む。** Monaco の取り消しには任せられない ──
/// 表を入れる・図を入れる・セルを押す、はどれも `editor.setValue()` で組み
/// 直しており、`setValue` は Monaco の積み木を**まるごと捨てる**。押した
/// 直後に ⌘Z を押しても、そこには何も積まれていない。
///
/// 積むのは**保存のたび**（打キーの 0.9 秒後）。一続きに打っているあいだは
/// 一つにまとまり、手が止まるごとに一段になる ── 「さっきの姿」の単位が
/// 人の感覚と揃う。一文字ずつ積むと、消した一段落を戻すのに何十回押す
/// ことになる。
///
/// **ノートを替えたら捨てる。** 別のノートの姿をここへ戻すパスがあると、
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
        // 新しく打ったら、先のパスは消える ── 分かれた先を持っておくと
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
/// 二つの文字の、**初めて食い違う行**（0 起点）。同じなら `-1`。
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

/// 戻したあとの行き先。編集画面ならその行、表示画面ならその行のかたまり。
///
/// 変わった行が無い（`-1`）ときは動かさない ── 動く理由が無いのに
/// 動くのが、そもそもの不具合だった。
function landBack(at) {
    if (at < 0) return;
    // 編集画面が出ているときは Monaco（`gotoHead` と同じ切り分け ── 一度
    // ここを `!== 'write'` と書き、編集画面でだけ動かなかった）。
    if (view !== 'read' && editor) {
        const line = at + 1;
        editor.revealLineNearTop(line);
        editor.setPosition({ lineNumber: line, column: 1 });
    }
    if (view !== 'write') {
        // 表示画面のかたまりは、元の文字の何行目からかを持っている
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

/// 矢印は文字ではなく線で描く ── 「↩」は書体によって太さも向きも変わる。
const STEP_ICON = (back) => '<svg viewBox="0 0 16 16" aria-hidden="true">'
    + '<path d="' + (back
        ? 'M6 3.6 2.4 7.2 6 10.8M2.4 7.2h6.9a3.4 3.4 0 0 1 0 6.8H7.4'
        : 'M10 3.6 13.6 7.2 10 10.8M13.6 7.2H6.7a3.4 3.4 0 0 0 0 6.8h1.9')
    + '" fill="none" stroke="currentColor" stroke-width="1.6"'
    + ' stroke-linecap="round" stroke-linejoin="round"/></svg>';

/// 鐘。**画面の上から仕掛けたい** ── 通知は「このノートに」するもので、
/// メニューの奥にあると、仕掛けたことも仕掛かっていることも見えない。
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
/// **憶えるのはこの環境の引き出し**（`amber.json`）── 「自分が確認したか」は
/// 人ごと・環境ごとのことで、フォルダに置くとグループの誰かが読んだ時点で
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
/// 気づくパスがどこにも無くなる）。
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
    const spots = (incoming && incoming.spots) || [];
    const fields = (incoming && incoming.fields) || [];
    if (!incoming || (!incoming.came.length && !spots.length && !fields.length)) {
        b.hidden = true;
        b.innerHTML = '';
        return;
    }
    const who = (incoming.who || '向こう');
    b.hidden = false;
    if (spots.length || fields.length) {
        // **ぶつかった場所がある** ── 数と、飛ぶパスと、まとめて選ぶ道
        // （本人が決めた姿・2026-09-11・artifact「ぶつかったところを選ぶ」）。
        const n = spots.length + fields.length;
        b.className = 'eyes';
        b.innerHTML = '<span class="dot"></span><span>' + escapeHtml(who) + 'と同じところを直していました ── '
            + n + ' か所。選ぶまでは両方残っています</span>'
            + '<span class="nav">'
            + '<button class="k" data-go="-1">← 前</button><button class="k" data-go="1">次 →</button>'
            + '<button class="k" data-all="ours">すべてこちらの記載を反映する</button>'
            + '<button class="k" data-all="theirs">すべて' + escapeHtml(who) + 'の記載を反映する</button></span>'
            + fields.map((f, i) => '<span class="fld">'
                + '<b>' + escapeHtml(fieldName(f.key)) + '</b>を両方で変えていました ── こちら「' + escapeHtml(f.ours) + '」／'
                + escapeHtml(who) + '「' + escapeHtml(f.theirs) + '」'
                + '<button class="k go" data-f="' + i + '" data-w="ours">こちらの記載を反映する</button>'
                + '<button class="k" data-f="' + i + '" data-w="theirs">' + escapeHtml(who) + 'の記載を反映する</button>'
                + (f.key === 'tags' ? '<button class="k" data-f="' + i + '" data-w="both">両方を反映する</button>' : '')
                + '</span>').join('');
        for (const x of b.querySelectorAll('[data-go]')) x.onclick = () => stepGadget(Number(x.dataset.go));
        for (const x of b.querySelectorAll('[data-all]')) x.onclick = () => chooseAll(x.dataset.all);
        for (const x of b.querySelectorAll('[data-f]')) x.onclick = () => chooseField(Number(x.dataset.f), x.dataset.w);
        return;
    }
    const n = incoming.came.length;
    b.className = '';
    b.innerHTML = '<span class="dot"></span><span>ほかの人が ' + n + ' 行更新しました</span>'
        + '<button class="act">確認した</button>';
    b.querySelector('.act').onclick = () => {
        incoming = null;
        keepIncoming();
        drawBand();
        paintIncoming();
    };
}

/// 前書きのキーの、人の言葉。
function fieldName(key) {
    return { title: 'タイトル', tags: 'タグ', created: '作った日', remind: '通知' }[key] || key;
}

/// 改行を行に含めたまま、行に割る（core の `records` と同じ割り方）。
function rowsOf(text) {
    return text ? text.split(/(?<=\n)/) : [];
}

/// ぶつかった場所を、いまの文字の中で探す ── **行の中身で**。こちらの行の
/// 直後に向こうの行が並んでいる。見つからなければ -1（人がもう直した）。
function spotAt(rows, spot) {
    const o = spot.ours;
    const t = spot.theirs;
    const n = o.length + t.length;
    if (!n) return -1;
    // **末尾の改行は見ない。** 控えた行はファイルの文字（最後の行にも改行が
    // ある）、いまの文字はエディタの文字（前書きを切るときに末尾の改行が落ちる）
    // ── 最後の行がぶつかった場所だと、そこだけ合わなかった。
    const same = (a, b) => a !== undefined && a.replace(/\n$/, '') === b.replace(/\n$/, '');
    for (let i = 0; i + n <= rows.length; i += 1) {
        let ok = true;
        for (let k = 0; k < o.length && ok; k += 1) if (!same(rows[i + k], o[k])) ok = false;
        for (let k = 0; k < t.length && ok; k += 1) if (!same(rows[i + o.length + k], t[k])) ok = false;
        if (ok) return i;
    }
    return -1;
}

/// 表示画面の、その行を持ついちばん外のかたまり。
///
/// **中の行番号も見る。** 箇条書きの外側のラベルは最初の項目の一行ぶんしか
/// 持たない（項目ごとにラベルがあるので）── 外側だけ見ると、二つ目以降の項目が
/// どのかたまりのものでもなくなり、選び口が置けなかった（実際に置けなかった）。
function blockOfLine(rd, line) {
    for (const b of rd.children) {
        const from = Number(b.dataset.line);
        if (Number.isNaN(from)) continue;
        let end = from + (Number(b.dataset.span) || 1);
        for (const c of b.querySelectorAll('[data-line]')) {
            const at = Number(c.dataset.line);
            if (!Number.isNaN(at)) end = Math.max(end, at + (Number(c.dataset.span) || 1));
        }
        if (from <= line && line < end) return b;
    }
    return null;
}

/// ぶつかった場所の**その場の選び口**を、表示画面に置く。
///
/// 三択（こちらを残す／〇〇を残す／両方）── 消した側があっても同じ形
/// （本人が決めた・2026-09-11）。選び口は文字ではないので、書き戻さない
/// （`paperToMd` が `.gadget` を飛ばす）。
function placeGadgets() {
    const rd = el('read');
    for (const g of rd.querySelectorAll('.gadget')) g.remove();
    if (!incoming || !incoming.spots || !incoming.spots.length) return;
    const rows = rowsOf(whole());
    const who = incoming.who || '向こう';
    incoming.spots.forEach((spot, n) => {
        const at = spotAt(rows, spot);
        if (at < 0) return;
        const top = blockOfLine(rd, at);
        if (!top) return;
        const g = document.createElement('div');
        g.className = 'gadget';
        g.contentEditable = 'false';
        const what = !spot.ours.length ? 'こちらは消し、' + who + 'は直していました'
            : !spot.theirs.length ? 'こちらは直し、' + who + 'は消していました'
            : '同じ行を両方で直していました';
        const hint = !spot.ours.length ? '（こちらの記載を反映する ＝ ' + who + 'の行が消えます）'
            : !spot.theirs.length ? '（' + who + 'の記載を反映する ＝ この行が消えます）' : '';
        g.innerHTML = '<b>' + escapeHtml(what) + '</b>'
            + '<button class="go" data-w="ours">こちらの記載を反映する</button>'
            + '<button data-w="theirs">' + escapeHtml(who) + 'の記載を反映する</button>'
            + '<button data-w="both">両方を反映する</button>'
            + (hint ? '<span class="hint">' + escapeHtml(hint) + '</span>' : '');
        for (const x of g.querySelectorAll('button')) {
            x.onmousedown = (e) => e.preventDefault();
            x.onclick = () => chooseSpot(n, x.dataset.w);
        }
        top.before(g);
    });
}

/// 前後の選び口へ。
function stepGadget(dir) {
    const gs = [...el('read').querySelectorAll('.gadget')];
    if (!gs.length) return;
    const box = el('read').getBoundingClientRect();
    const mid = box.top + box.height / 2;
    // いま見えているまん中より下の最初のもの（次）／上の最後のもの（前）。
    const to = dir > 0
        ? (gs.find((g) => g.getBoundingClientRect().top > mid + 8) || gs[0])
        : ([...gs].reverse().find((g) => g.getBoundingClientRect().top < mid - 8) || gs[gs.length - 1]);
    to.scrollIntoView({ block: 'center', behavior: 'smooth' });
}

/// 文字を丸ごと差し替えて保存する（前書きも含めて）。
async function putWhole(text) {
    const cut = await ask('split', { text });
    state.head = cut.head || '';
    loading = true;
    editor.setValue(cut.body || '');
    loading = false;
    state.dirty = true;
    await save();
}

/// 一つ選ぶ。`which` は ours / theirs / both。
async function chooseSpot(n, which) {
    if (!incoming || !incoming.spots || !incoming.spots[n]) return;
    const spot = incoming.spots[n];
    const rows = rowsOf(whole());
    const at = spotAt(rows, spot);
    if (at >= 0 && which !== 'both') {
        const drop = which === 'ours'
            ? [...Array(spot.theirs.length).keys()].map((k) => at + spot.ours.length + k)
            : [...Array(spot.ours.length).keys()].map((k) => at + k);
        if (drop.length) {
            const kept = rows.filter((_, i) => !drop.includes(i));
            const shift = (k) => (drop.includes(k) ? -1 : k - drop.filter((d) => d < k).length);
            incoming.came = incoming.came.map(shift).filter((k) => k >= 0);
            incoming.both = incoming.both.map(shift).filter((k) => k >= 0);
            await putWhole(kept.join(''));
        }
    } else if (at >= 0) {
        // 両方残す ── 向こうの行の「両方残した」のマークは外す（もう決めた）。
        const from = at + spot.ours.length;
        incoming.both = incoming.both.filter((k) => k < from || k >= from + spot.theirs.length);
    }
    incoming.spots.splice(n, 1);
    if (!incoming.spots.length && !(incoming.fields || []).length && !incoming.came.length) incoming = null;
    keepIncoming();
    drawBand();
    await drawRead();
}

/// ぜんぶ、こちら（か向こう）で。
async function chooseAll(which) {
    while (incoming && incoming.spots && incoming.spots.length) {
        await chooseSpot(0, which);
    }
    while (incoming && incoming.fields && incoming.fields.length) {
        await chooseField(0, which);
    }
}

/// 前書きのキーを選ぶ。タグの「両方」は和集合。
async function chooseField(n, which) {
    if (!incoming || !incoming.fields || !incoming.fields[n]) return;
    const f = incoming.fields[n];
    let value = which === 'theirs' ? f.theirs : f.ours;
    if (which === 'both' && f.key === 'tags') {
        const list = (v) => String(v).replace(/^\[|\]$/g, '').split(',').map((x) => x.trim()).filter(Boolean);
        value = '[' + [...new Set([...list(f.ours), ...list(f.theirs)])].join(', ') + ']';
    }
    try {
        const got = await ask('setfield', { text: whole(), key: f.key, value: value || null });
        if (typeof got.text === 'string') await putWhole(got.text);
    } catch (e) {
        say('直せません: ' + why(e));
        return;
    }
    incoming.fields.splice(n, 1);
    if (!incoming.spots.length && !incoming.fields.length && !incoming.came.length) incoming = null;
    keepIncoming();
    drawBand();
    await drawRead();
}

/// 来た行に、地色を敷く。
///
/// **できるだけ細かい単位で。** 箇条書きは `<ul>` ひとつで一かたまりなので、
/// 上の段だけを見て塗ると**一行来ただけで三行とも光る** ── 買い物リストは
/// まさにその形で、それでは何が来たのか分からない（実物で見て気づいた）。
/// 行の札（`data-line`）は `<li>` も持っているので、そこまで降りる。
///
/// **最初の `<li>` はラベルを持たない**（親の `<ul>` が同じ行を指している）ので、
/// 親から継ぐ。降りきれない形（段落の途中の一行など）は、そのかたまり
/// ぜんぶを塗る ── 塗り過ぎるほうが、塗り落とすよりまし。
function paintIncoming() {
    const rd = el('read');
    for (const b of rd.querySelectorAll('.came, .both')) b.classList.remove('came', 'both');
    for (const g of rd.querySelectorAll('.gadget')) g.remove();
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
    placeGadgets();
}

/// 向こうと混ぜて、書き戻す。返すのは混ざった文字（駄目なら `null`）。
///
/// **判断は core、運ぶのはここ**（`sync` と同じ切り分け）── どの行が
/// 向こうから来たかも core が言うので、画面はそれを受け取ってマークを差す。
///
/// **マークはノートに書かない。** 憶えるのはこの環境の引き出し（`amber.json`）
/// ── 「自分が確認したか」は**人ごと・環境ごと**のことで、フォルダに
/// 置くとグループの誰かが読んだ時点で全員のぶんが消える。
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
    // ときだけ ── 両方に文字があったのに空が出たなら、それは混ぜ損ねている。
    if (!got.text.trim() && (was.trim() || ours.trim())) {
        say('混ぜた結果が空になりました。書き戻していません');
        return null;
    }
    // **混ぜる前のこちらの姿を、必ず履歴に残す**（Git の ORIG_HEAD の写し）──
    // 選び間違えても「混ぜる前に戻す」が一手でできる。
    try { await ask('keep', { root: rootOf(path), path, text: ours, gap: 0, force: true }); } catch { /* 履歴が置けなくても混ぜる */ }
    try {
        const w = await ask('write', { path, text: got.text, force: true });
        if (w && w.stamp) state.stamp = w.stamp;
    } catch (e) {
        say('保存できません: ' + why(e));
        return null;
    }
    // 画面に出す ── 誰が・何行・どこ。名前は向こうが書いた履歴が持っている
    // ものではないので、分かるときだけ言う。
    // **誰が書いたかは、言えない。** ノートはただの Markdown で、名前は
    // どこにも書いていない（書かないと決めた ── 依頼 320）。分からない
    // ことを分かったように言わない。
    // ぶつかった場所は**行の中身で**憶える（行番号は打つたびに動く）。
    const rows = rowsOf(got.text);
    incoming = {
        path,
        came: got.came || [],
        both: got.both || [],
        eyes: !!got.eyes,
        who: '向こう',
        spots: (got.spots || []).map((sp) => ({
            ours: rows.slice(sp.ours[0], sp.ours[0] + sp.ours[1]),
            theirs: rows.slice(sp.theirs[0], sp.theirs[0] + sp.theirs[1]),
        })),
        fields: got.fields || [],
    };
    keepIncoming();
    // 混ざった文字を画面へ。**caret は飛ばさない**ので、組み直しはこのあと。
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

/// 自動保存（依頼 512・本人「基本は自動保存。自発的に切れるようにしたい」）。
/// 切っているあいだは、打っても書かない ── 「保存」を押したときと、ノートから
/// 離れるときに一度だけ確認して書く（iPhone と同じ決まり）。
let autoSave = true;

/// 「保存」のボタン。**自動保存を切っていて、書きかけのときだけ出す** ──
/// 入のときに出ていると、押さないと保存されないように見える。
function drawSaveNow() {
    const b = el('savenow');
    if (!b) return;
    b.hidden = autoSave || !state.open || !state.dirty || state.guest;
}

/// ノートから離れるとき。自動保存なら黙って書く。切っているなら一度だけ確認。
async function leaveSave() {
    if (!state.dirty) return;
    if (autoSave) return save();
    const name = (state.open && state.open.title) || stem();
    if (await askYes('「' + name + '」の書きかけを保存しますか')) return save();
    // 捨てる ── 次の保存で古い文字が書かれないように、書きかけの印だけ下ろす。
    state.dirty = false;
    drawSaveNow();
}

async function cmdAutoSave() {
    autoSave = !autoSave;
    window.amber.remember({ autoSave });
    drawSaveNow();
    say(autoSave ? '自動保存を入にしました（打てば保存されます）' : '自動保存を切にしました（「保存」を押したときに書きます）');
}

/* ── 錠（依頼 629）── */

/// いま開いているノートの錠（`{ locked, why, dir }`）。core が答える。
let lockNow = { locked: false };
/// **今だけ編集する**を押したノート。**離れたら忘れる** ── 本人が決めた
/// （2026-09-18）「そのときだけ外す。閉じると自動でロックに戻る」。
const unlockedNow = new Set();

/// いま打てるか。錠が無いか、今だけ編集するを押したか。
function canEdit() {
    if (!state.open) return false;
    return !lockNow.locked || unlockedNow.has(state.open.path);
}

/// 書くときに添えるもの。**押した人のぶんだけ** ── 押していないのに
/// `unlock` を送ることはしない（送れば core の門は開いてしまう）。
function lockArg() {
    return state.open && unlockedNow.has(state.open.path) ? { unlock: true } : {};
}

/// core に訊いて、帯と画面を合わせる。
async function loadLock() {
    lockNow = { locked: false };
    if (!state.open) { drawLock(); return; }
    try {
        lockNow = await ask('locked', { path: state.open.path });
    } catch { /* 訊けないときは錠なしとして扱う（読めないより書けるほうがまし） */ }
    drawLock();
}

/// 錠の帯と、打てるかどうか。
function drawLock() {
    const bar = el('lockbar');
    const on = !!(state.open && lockNow.locked);
    bar.hidden = !on;
    if (on) {
        const free = unlockedNow.has(state.open.path);
        const where = lockNow.why === 'folder'
            ? '「' + leafOf(lockNow.dir || '') + '」はロックされたフォルダです'
            : 'このノートはロックされています';
        bar.querySelector('.t').textContent = free
            ? where + ' ── いまだけ編集しています（閉じると戻ります）'
            : where;
        el('lockedit').hidden = free;
        el('lockoff').textContent = lockNow.why === 'folder' ? 'フォルダのロックをやめる' : 'ロックをやめる';
    }
    // **打てなくする。** 見た目だけ止めても、打てば自動保存が走る。
    if (editor) editor.updateOptions({ readOnly: on && !unlockedNow.has(state.open?.path) });
    el('read').contentEditable = String(canEdit());
    document.body.classList.toggle('locked', on && !canEdit());
}

el('lockedit').onclick = () => {
    if (!state.open) return;
    unlockedNow.add(state.open.path);
    drawLock();
    if (editor && view !== 'read') editor.focus();
    say('いまだけ編集できます（閉じると、またロックに戻ります）');
};
el('lockoff').onclick = () => cmdLock(false);

/// ロックする／やめる。**ノートでもフォルダでも同じ道**（core が見分ける）。
async function cmdLock(on, what) {
    const path = what || (state.open && state.open.path);
    if (!path) { say('ロックするノートを、先に開いてください'); return; }
    const isDir = !!what && !path.endsWith('.md');
    const name = isDir ? '「' + leafOf(path) + '」' : 'このノート';
    if (!on) {
        // **やめるのは戻せる操作**だが、一度は訊く ── 錠は押し間違いを
        // 止めるためのもので、その錠自体が一押しで外れては意味が薄い。
        const ok = await askYes(name + 'のロックをやめますか');
        if (!ok) return;
    }
    try {
        await ask('lock', { path, on: !!on });
    } catch (e) {
        say('できません: ' + why(e));
        return;
    }
    if (!on) unlockedNow.delete(path);
    if (state.open) await loadLock();
    await reload({ quiet: true });
    say(on ? name + 'をロックしました' : name + 'のロックをやめました');
}

async function save() {
    if (!state.open || !editor) return;
    // **錠のノートは書かない。** 画面は読み取り専用にしてあるが、貼り付けや
    // よそからの道（同期の書き戻しなど）でここへ来ることがある。
    if (!canEdit()) { state.dirty = false; return; }
    const path = state.open.path;
    // 頭を戻してから書く。**ここを忘れると、保存のたびに front matter が
    // 1 つずつ消える** ── 題もタグも作った日も。
    // 混ぜたときに差し替わるので `let`。
    let text = state.head + editor.getValue();
    // 書き込む直前の姿を積む ── 書いたあとだと、戻る先が「いまの姿」になる。
    keepStep(editor.getValue());
    // **一世代にするかは core が決める。** 同じフォルダを二つの端末で
    // 触るので、片方の決まりで消したものをもう片方が残っていると思う、が
    // 起きてはいけない。ここは「保存する前の姿はこれです」と言うだけ。
    if (!state.guest) {
        try {
            await ask('keep', { root: rootOf(path), path, text: state.was ?? text, gap: KEEP_GAP });
        } catch { /* 履歴が置けないことで、保存が止まる理由はない */ }
    }
    // **分かれる前の姿**（開いた時点、または前に保存できた時点の中身）。
    // `state.was` はこのあと上書きされるので、先に控える。
    const ancestor = state.base ?? state.was ?? text;
    state.was = text;
    try {
        const r = await ask('write', { path, text, stamp: state.stamp, ...lockArg() });
        if (r && r.conflict) {
            // **どちらかを捨てない。混ぜる。**
            //
            // 前はここで「こちらで上書きしますか／向こうを読み直しますか」と
            // 訊いていた ── どちらを押しても、**片方の書いたものが消える**。
            // グループで同じフォルダを触るのが前提のアプリで、それは強すぎる。
            //
            // 混ぜ方は core（`merge`）── 分かれる前（開いた時点の中身）と、
            // こちらと、向こうの3 つを渡す。同じ場所を二人が書いていたら
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
        drawSaveNow();
        syncSoon();
        nameSoon();
        await freshenRow(path);
        drawStrip();
        setTimeout(() => {
            if (!state.dirty && state.open && state.open.path === path) {
                el('state').textContent = when(state.open.updated);
                drawSaveNow();
            }
        }, 1400);
    } catch (e) {
        el('state').textContent = '保存できません';
        drawSaveNow();
        say('保存できません: ' + why(e));
    }
}

/// 新しいノート。`title` を渡すと、その題で作る（Web から取り込むとき ──
/// **ページの題がそのままファイルの名前になる**ほうが、あとで探せる）。
async function newNote(title) {
    // いまフォルダを見ているなら、そこに作る ── 「どこに出来たか分からない」
    // のがいちばん困る。
    const dir = hereDir();
    try {
        const r = await ask('new', { dir, title: title || '' });
        await reload({ quiet: true });
        await openNote(r.path);
        if (editor) editor.focus();
        // **作れたパスを返す。** 貼り付けて作る道（`cmdPasteNote`）が、
        // 作れたかどうかを見るのに要る。
        return r.path;
    } catch (e) {
        say('作れません: ' + why(e));
        return null;
    }
}

/// **新しいノートのダイアログ**（依頼 513・iPhone の `Making` を採用 ── 本人が決めた・
/// 2026-09-12）。タイトル・タグ・テンプレートを先に選べる。**何も選ばなくても
/// 作れる**（空のままなら本文の一行目がタイトルになる）── 初めての人が、何も
/// 分からなくても「作成」だけ押せば先へ進めるように。
let nnDone = null;
function askNewNote() {
    const box = el('newform');
    const title = el('nntitle');
    const tagIn = el('nntag');
    const chips = el('nntags');
    const known = el('nnknown');
    const tmpl = el('nntmpl');
    const tags = [];
    title.value = '';
    tagIn.value = '';
    const drawChips = () => {
        chips.innerHTML = tags.map((t, i) => '<button type="button" class="on" data-i="' + i + '">#' + escapeHtml(t) + ' ✕</button>').join('');
        for (const b of chips.querySelectorAll('button')) b.onclick = () => { tags.splice(Number(b.dataset.i), 1); drawChips(); };
        const mine = tagsOf(state.notes).map(([t]) => t).filter((t) => !tags.includes(t)).slice(0, 12);
        known.innerHTML = mine.map((t) => '<button type="button">#' + escapeHtml(t) + '</button>').join('');
        known.querySelectorAll('button').forEach((b, i) => { b.onclick = () => { tags.push(mine[i]); drawChips(); }; });
        known.hidden = !mine.length;
    };
    const addTag = () => {
        const t = tagIn.value.trim().replace(/^#+/, '').trim();
        tagIn.value = '';
        if (t && !tags.includes(t)) tags.push(t);
        drawChips();
    };
    const drawTmpl = () => {
        const rows = sortNotes(state.notes.filter((n) => inTemplates(n.book)));
        if (!rows.length) {
            tmpl.innerHTML = '<button type="button" data-seed="1">サンプルのテンプレートを入れる（週報・議事録・買い物リスト）</button>'
                + '<span class="hint">「' + TEMPLATES + '」フォルダに置いたノートが、ここに並びます</span>';
            tmpl.querySelector('[data-seed]').onclick = async () => {
                try { await window.amber.templates(state.root); await reload({ quiet: true }); } catch (e) { say('入れられません: ' + why(e)); }
                drawTmpl();
            };
            return;
        }
        tmpl.innerHTML = rows.map((n, i) => '<button type="button" data-i="' + i + '">' + escapeHtml(n.title || '（タイトルなし）') + '</button>').join('');
        tmpl.querySelectorAll('button').forEach((b, i) => { b.onclick = () => shut({ template: rows[i].path }); });
    };
    const shut = (v) => {
        box.hidden = true;
        if (nnDone) { const f = nnDone; nnDone = null; f(v); }
    };
    const go = () => { addTag(); shut({ title: title.value.trim(), tags: tags.slice() }); };
    drawChips();
    drawTmpl();
    tagIn.onkeydown = (e) => {
        e.stopPropagation();
        if (e.isComposing || e.keyCode === 229) return;
        if (isEnter(e)) { e.preventDefault(); addTag(); }
        else if (e.code === 'Escape') { e.preventDefault(); shut(null); }
    };
    el('nnok').onclick = go;
    el('nncancel').onclick = () => shut(null);
    box.onmousedown = (e) => { if (e.target === box) shut(null); };
    box.onkeydown = (e) => {
        e.stopPropagation();
        if (e.isComposing || e.keyCode === 229) return;
        if (e.code === 'Escape') { e.preventDefault(); shut(null); }
        else if (isEnter(e) && e.target === title) { e.preventDefault(); go(); }
    };
    box.hidden = false;
    title.focus();
    return new Promise((resolve) => { nnDone = resolve; });
}

/// 「新しいノート」── ダイアログで訊いてから作る。作る場所は `newNote` と同じ（いま見ているフォルダ）。
async function cmdNewNote() {
    if (state.guest) closeGuest();
    const got = await askNewNote();
    if (!got) return null;
    if (got.template) {
        try {
            const here = inTemplates(hereDir()) ? rootOf(hereDir()) : hereDir();
            const r = await ask('copy', { path: got.template, dir: here });
            await reload({ quiet: true });
            await openNote(r.path);
            if (editor) editor.focus();
            return r.path;
        } catch (e) { say('作れません: ' + why(e)); return null; }
    }
    const at = await newNote(got.title);
    if (at && got.tags.length) {
        await editNote((t) => ask('settags', { text: t, tags: got.tags }).then((r) => r.text));
    }
    return at;
}

/* ── 読み直し ── */

/// いま書いた一本の行だけ、新しくする。
///
/// **書いたあとにフォルダを丸ごと数え直さない。** 1002 本で毎回 370ms かかって
/// いた（エンジンが 220ms、一覧を組み直すのに 150ms）── しかも打ち終えて
/// 0.7 秒後に走るので、**打ち終わるたびに画面が固まる**。自分が書いた一本の
/// ことは自分が知っていて、外で何かが変わったなら見張り（`onChanged`）が
/// 別に教えてくれる。
///
/// **重ねる欄は、名指しで選ぶ。** 返ってきたものを丸ごと重ねてはいけない
/// ── `note` は「amber の外にある一本」も読める口なので、`rel` はファイル名
/// だけ、`book` は空で返る。丸ごと重ねると、**保存するたびにノートが
/// いちばん上のフォルダへ移ったように見える**（共有のマークも消える）。
/// 文字を書いて変わるのは、下に並べた欄だけ。
///
/// 訊けなかったら、前のように丸ごと数え直す ── 一覧が古いまま残るよりよい。
const FRESH = ['title', 'excerpt', 'tags', 'updated', 'created', 'bytes', 'search'];

/// 最後に自分で書いたノート。見張りが自分の書き込みで起きたかを見分ける。
///
/// **一度きり・数秒だけ**（依頼 479）。前は書いたパスを憶えたまま消して
/// いなかったので、**そのノートへの外からの変更を、そのあとずっと
/// 無視していた** ── デスクトップ版で保存 → iPhone で直す → デスクトップ版は何も知らない、が
/// 実際に起きる（二台で同じフォルダを触るときの、まさにその形）。
let lastWrote = null;
let lastWroteAt = 0;

async function freshenRow(path) {
    lastWrote = path;
    lastWroteAt = Date.now();
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
        // **保存ディレクトリごとに数えて、一つに重ねる**（依頼 511）。core は
        // 一つの保存ディレクトリしか知らない（それでいい ── 二つを一つに見せる
        // のは画面の都合）。相対で返ってくるパスは、ここで絶対にする。
        const notes = [];
        const books = [];
        const locks = [];
        const stars = new Set();
        const colors = {};
        const came = {};
        const waiting = [];
        const shares = [];
        const trouble = {};
        let firstErr = null;
        const places = state.places.length ? state.places : (state.root ? [{ name: bookName(state.root), dir: state.root }] : []);
        for (const p of places) {
            let r;
            try {
                r = await ask('notes', { path: p.dir });
            } catch (e) {
                // **読めない保存ディレクトリは、無かったことにしない。** 外付けを
                // 抜いた・消した ── 一覧からは消えるが、列には薄く残して言う。
                trouble[p.dir] = why(e);
                firstErr = firstErr || e;
                continue;
            }
            const abs = (rel) => (rel ? p.dir + '/' + rel : p.dir);
            for (const n of r.notes || []) notes.push({ ...n, root: p.dir, place: p.name, book: abs(n.book) });
            for (const b of r.books || []) books.push(abs(b));
            for (const l of r.locks || []) locks.push(abs(l));
            for (const s of r.stars || []) stars.add(s);
            for (const [k, v] of Object.entries(r.colors || {})) colors[abs(k)] = v;
            // 共有へ入れたノートが、もといたフォルダ。**メニューに「どこへ戻すか」
            // を出すのに要る** ── そのつど訊きに行くと、押す前に消費してしまう。
            for (const [k, v] of Object.entries(r.came || {})) came[abs(k)] = abs(v);
            for (const w of r.waiting || []) waiting.push(w);
            for (const s of r.shares || []) shares.push({ at: abs(s.at), by: s.by });
        }
        state.placeTrouble = trouble;
        if (places.length && Object.keys(trouble).length === places.length) throw firstErr;
        state.notes = notes;
        state.books = books;
        state.locks = locks;
        state.stars = [...stars].sort();
        state.colors = colors;
        state.came = came;
        state.waiting = waiting;
        state.shares = shares;
        // 開いていた行を新しいほうに繋ぎ直す（更新時刻が動くので）。
        if (state.open) {
            state.open = state.notes.find((n) => n.path === state.open.path) || state.open;
            drawTitle();
        }
        // **消えたノートのタブは、残さない**（依頼 612）。
        dropGoneTabs();
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
        // 開いた直後は `state.root` がまだ無くて描けていない ── ここで描く。
        drawSyncState();
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
/// Monaco にも表示画面にも自前の取り消しがあるが、どちらも
/// `editor.setValue()` で組み直したところ（表・図・セル）で積み木ごと消える
/// ── 押した直後に ⌘Z を押しても何も起きない。ノートの文字を積んでいる
/// こちらに一本化する。
///
/// **泡の段では届かないことがある** ── Monaco は自分の textarea で ⌘Z を
/// 受けて、そこで止めることがある。捕捉の段なら、どこを打っていても先に
/// 通る。ダイアログと工房の中だけは、あちらの受け口に譲る。
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
    // **表示画面も打っている場所。** ここを数え忘れると、Enter が命令として
    // 拾われて焦点が編集画面へ飛ぶ ── 表示画面で改行できない、として出た。
    const inRead = el('read').contains(e.target);

    // **画面の形は、どこを打っていても効く。** 書いている最中に読みたく
    // なるのだから、エディタの中でこそ効かないと意味がない ── だから
    // 「打っている場所では素の一文字は文字」の線より手前に置く。
    if (e.code === 'F12') { e.preventDefault(); setZen(!zen); return; }
    // **F5 は、どこを打っていても効く。** 外でフォルダごと消えたときに
    // 押すものなので、エディタの中に居るからといって効かないと意味がない。
    if (e.code === 'F5' || ((e.metaKey || e.ctrlKey) && e.code === 'KeyR' && !e.shiftKey)) {
        e.preventDefault(); cmdRefresh(); return;
    }
    if (e.code === 'Escape') {
        // **手前にあるものから閉じる。** ダイアログが開いているのに大きい画面が
        // 戻ると、閉じたつもりのものが残る。
        if (!el('lens').hidden) { e.preventDefault(); closeLens(); return; }
        if (!el('emoji').hidden) { e.preventDefault(); closeEmoji(); return; }
        if (!el('more').hidden) { e.preventDefault(); closeMenu(); return; }
        if (!el('veil').hidden) { e.preventDefault(); closeSheet(null); return; }
        if (zen) { e.preventDefault(); setZen(false); return; }
        // 表示画面で打っている途中の Esc は、書き戻してから手を離す。
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
    // 文字の大きさ。**`e.code` は位置のキー** ── 刻印ではなく、US 配列での場所を言う。
    //
    // **JIS の「＋」は `Semicolon` の位置にある**（`; ＋ れ` のキー）。前は `Equal`
    // だけを見ていて、それは JIS では「＾ へ」のキー ── Ctrl＋「＋」と刻印どおりに
    // 押しても大きくならなかった（本人・2026-09-16）。ブラウザも JIS では
    // Ctrl＋; で拡大する。`Equal` も残す（US 配列の `=` `+`、JIS で＾を押す人）。
    // **カレンダーを見ているときは、カレンダーの文字を**（依頼 623）── 見えていない
    // ノートの文字が変わっても、押した人には何も起きていないのと同じ。
    const calUp = !el('cal').hidden;
    if ((e.metaKey || e.ctrlKey) && (e.code === 'Equal' || e.code === 'Semicolon' || e.code === 'NumpadAdd')) {
        e.preventDefault(); if (calUp) setCalFont(calFontStep + 1); else setFont(fontStep + 1); return;
    }
    if ((e.metaKey || e.ctrlKey) && (e.code === 'Minus' || e.code === 'NumpadSubtract')) {
        e.preventDefault(); if (calUp) setCalFont(calFontStep - 1); else setFont(fontStep - 1); return;
    }
    if ((e.metaKey || e.ctrlKey) && e.code === 'Digit0') {
        e.preventDefault(); if (calUp) setCalFont(0); else setFont(0); return;
    }
    // 書く道具。**帯のボタンと同じ一本のパスを通す** ── 押した形と打った形で
    // 結果が違うと、どちらかが嘘になる。
    if (e.metaKey || e.ctrlKey) {
        const hit = markKey(e);
        if (hit) { e.preventDefault(); hit(); return; }
    }
    // キーの一覧（⌘? ── mac で「ショートカットを見る」はここ）。
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.code === 'Slash') {
        e.preventDefault(); cmdKeys(); return;
    }
    // 左の列を畳む（Inkdrop の ⌘/）。狭い画面では二列ぶんが効く。
    if ((e.metaKey || e.ctrlKey) && e.code === 'Slash') {
        e.preventDefault();
        // `⌥` を足すと二枚目。**同じキーの並びに揃える** ── 畳むことは
        // 一つの動きで、畳む相手が違うだけ。
        //
        // **`⇧` ではない。** `⌘⇧/` は mac では `⌘?` ── どのアプリでも
        // 「ショートカット一覧」で、一つ上の枝がそれを先に取る。表には
        // 二つとも `⌘⇧/` と書いてあったので、**一覧を畳むは鍵から一度も
        // 押せなかった**（メニューにもパレットにも出ているのに・依頼 447）。
        if (e.altKey) toggleList(); else toggleRail();
        return;
    }
    // **まとめて選ぶ。** 打っている最中は取らない ── エディタと探す欄の
    // `⌘A` は「文字を全部選ぶ」で、そちらのほうが強い。
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyA' && !inField && !inEditor && !inNote(document.activeElement)) {
        e.preventDefault();
        pickAll();
        return;
    }
    // 見たノートの前後（Inkdrop の ⌘← / ⌘→）。
    if ((e.metaKey || e.ctrlKey) && e.code === 'KeyN') { e.preventDefault(); cmdNewNote(); return; }
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
    else if (e.code === 'KeyN') { e.preventDefault(); cmdNewNote(); }
    // **消すのは、必ず訊いてから。** 打っている場所では上で戻しているので、
    // ここに来るのは一覧を見ているときだけ。
    else if ((e.code === 'Backspace' || e.code === 'Delete') && state.open) {
        e.preventDefault(); cmdDelete();
    }
    else if (e.code === 'Slash') { e.preventDefault(); openFind(); }
});

/// 修飾キー付きの一打を、道具の一押しに。
///
/// **表は一つ（`MARKS`）。** 前はここに編集画面用の写しがもう一組あって、
/// キーボードから押したときだけ**表示画面で効かなかった** ── ⌘B が帯からは効いて
/// 一打からは効かない、という見分けの付かない差になっていた。帯に書いた
/// キーをそのまま引く。
///
/// **`e.code` で当てる。** JIS では `e.key` が `Process` になり、数字の
/// 段は配列で別の文字になる ── `Digit8` は「8 の位置のキー」なので動く。
function keyName(e) {
    const c = e.code;
    let base = '';
    if (/^Key[A-Z]$/.test(c)) base = c.slice(3);
    else if (/^Digit[0-9]$/.test(c)) base = c.slice(5);
    else if (c === 'Quote') base = "'";
    else return '';
    return '⌘' + (e.shiftKey ? '⇧' : '') + base;
}

/// 見出しの深さは、キーボードからは一打で。**帯のボタンは押すたびに深くなる**まま
/// ── 一つの考えに3 つの名前を付けない、はボタンの話で、キーボードには当てはまらない
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

/// 貼り付けられたものが画像なら、ノートの隣に置いてリンクを打つ。
///
/// **捕まえるのは画像のときだけ。** 文字の貼り付けはエディタの仕事で、
/// ここが横取りすると Monaco の取り消しが繋がらなくなる。
/// 貼られたものは、**画像そのものか**（依頼 616・本人「Excel で複数セルを
/// 範囲指定してコピーしてアンバーに貼り付けすると、絵の扱いになっている」）。
///
/// **Excel は、同じ一回のコピーで3 つ載せてくる** ── 選んだ範囲の絵
/// （PNG）、表（`text/html`）、タブ区切りの文字（`text/plain`）。前は
/// 「画像があれば画像」で採っていたので、**表を貼ったつもりが画面写真**に
/// なっていた。Word も Excel も、ブラウザの表も同じ形。
///
/// **見分けるのは文字の有無。** 画面を撮って貼る回（⌘⇧4）は、文字が一つも
/// 載っていない ── そこだけが本当に「画像そのもの」。
///
/// **ウェブの画像を右押しでコピーした回も、文字は載らない**（`text/html` は
/// `<img>` だけで、`text/plain` は空）ので、いままでどおり絵として入る。
function justAPicture(data) {
    if (!data) return true;
    if ((data.getData('text/plain') || '').trim()) return false;
    const html = data.getData('text/html') || '';
    if (!html.trim()) return true;
    // 文字の無い `<img>` だけの HTML は、画像そのもの。
    const body = new DOMParser().parseFromString(html, 'text/html').body;
    return !body || !body.textContent.trim();
}

document.addEventListener('paste', async (e) => {
    // **「表示」画面でも受ける。** 前はここで帰っていたので、表示画面に
    // 撮った画面を貼っても何も起きなかった ── 画面を撮って貼るのは、
    // いちばん「表示」画面でやりたいこと。
    if (!state.open || !editor) return;
    const items = [...(e.clipboardData?.items || [])];
    const pic = items.find((i) => i.kind === 'file' && i.type.startsWith('image/'));
    if (!pic || !justAPicture(e.clipboardData)) return;
    e.preventDefault();
    e.stopPropagation();
    const got = await window.amber.clipboardImage();
    if (!got) { say('その画像は読めません'); return; }
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
/// `F12`（ノートだけを大きく）が既にある ── 同じところへ着くパスを三本
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
/// 並び順の隣ではなく、たどったパスの隣にある。
const trail = [];
let trailAt = -1;
function trailPush(path) {
    if (trail[trailAt] === path) return;
    trail.splice(trailAt + 1);
    trail.push(path);
    trailAt = trail.length - 1;
}
// 前へ／次へ（`walk`）は 2026-09-12 に外した（本人「要らない」）── 跡（`trail`）は
// 開き直しの `walking` のマークのために残っている。


/* ── ノートから使われていない画像（依頼 449） ── */

/// いま出している一覧と、選ばれているもの。
let spareRows = [];
let sparePicked = new Set();

/// **数えるのは core、消すのは OS のゴミ箱。**
///
/// 「使われていない」はぜんぶのノートを読み切って初めて言えることなので、
/// 読めなかったノートがあれば、消す前にそう言う ── 黙って少なく数えるのが
/// いちばん危ない。
async function cmdSpare() {
    if (!state.root) { say('保存場所がありません'); return; }
    // ぜんぶの保存ディレクトリを数えて、一つの表に（依頼 511）。
    const got = { pictures: [], unsure: [] };
    for (const p of state.places) {
        if (state.placeTrouble[p.dir]) continue;
        try {
            const one = await window.amber.call('spare', { path: p.dir });
            got.pictures.push(...(one.pictures || []));
            got.unsure.push(...(one.unsure || []));
        } catch (e) {
            say('数えられません: ' + why(e));
            return;
        }
    }
    spareRows = got.pictures || [];
    sparePicked = new Set();
    const box = el('spare');
    const warn = box.querySelector('.warn');
    const unsure = got.unsure || [];
    warn.hidden = !unsure.length;
    warn.textContent = unsure.length
        ? '読めなかったノートが ' + unsure.length + ' 件あります（' + unsure.slice(0, 3).join('・')
          + (unsure.length > 3 ? ' ほか' : '') + '）。そのノートが使っている画像も、'
          + 'ここに混じります'
        : '';
    box.hidden = false;
    drawSpare();
}

function drawSpare() {
    const box = el('spare');
    const grid = box.querySelector('.grid');
    const sum = box.querySelector('.sum');
    if (!spareRows.length) {
        grid.innerHTML = '<div class="none">使われていない画像はありません。</div>';
        sum.textContent = '';
        box.querySelector('.go').disabled = true;
        box.querySelector('.all').disabled = true;
        return;
    }
    const bytes = spareRows.reduce((n, r) => n + (r.bytes || 0), 0);
    sum.textContent = spareRows.length + ' 枚・' + spareSize(bytes)
        + (sparePicked.size ? '（' + sparePicked.size + ' 枚を選んでいます）' : '');
    box.querySelector('.go').disabled = !sparePicked.size;
    box.querySelector('.all').disabled = false;
    box.querySelector('.all').textContent =
        sparePicked.size === spareRows.length ? 'すべてやめる' : 'すべて選ぶ';
    grid.innerHTML = spareRows.map((r) => {
        const on = sparePicked.has(r.path) ? ' on' : '';
        // もとのノートの名前は「らしい」だけ ── 言い切らない。
        const from = r.note ? escapeHtml(r.note) + ' のもの' : '出どころは分かりません';
        return '<div class="cell' + on + '" data-at="' + escapeHtml(r.path) + '">'
            + '<div class="shot"><img loading="lazy" src="' + fileURL(r.path) + '" alt=""></div>'
            + '<div class="cap"><b>' + (sparePicked.has(r.path) ? '選んでいます' : from) + '</b><br>'
            + spareSize(r.bytes || 0) + '・' + (when(r.when) || '日付なし') + '</div></div>';
    }).join('');
}

/// バイトを、人の読む文字に。**名前を広く取らない**（依頼 424）── `size` の
/// ような名前は、あとから誰かがもう一つ書く。
function spareSize(n) {
    if (n < 1024) return n + ' B';
    if (n < 1024 * 1024) return Math.round(n / 1024) + ' KB';
    return (n / 1024 / 1024).toFixed(1) + ' MB';
}

el('spare').addEventListener('click', async (e) => {
    const box = el('spare');
    if (e.target === box || e.target.closest('.x')) { box.hidden = true; return; }
    const cell = e.target.closest('.cell');
    if (cell) {
        const at = cell.dataset.at;
        if (sparePicked.has(at)) sparePicked.delete(at); else sparePicked.add(at);
        drawSpare();
        return;
    }
    if (e.target.closest('.all')) {
        if (sparePicked.size === spareRows.length) sparePicked = new Set();
        else sparePicked = new Set(spareRows.map((r) => r.path));
        drawSpare();
        return;
    }
    if (e.target.closest('.go')) {
        const n = sparePicked.size;
        if (!n) return;
        // **ゴミ箱へ。** 消すのではないので戻せるが、それでも一度は訊く。
        if (!await askYes(n + ' 枚をゴミ箱へ入れますか')) return;
        let done = 0;
        const left = [];
        for (const at of sparePicked) {
            const ok = await window.amber.trash(at);
            if (ok === true) done += 1; else left.push(at);
        }
        box.hidden = true;
        say(done + ' 枚をゴミ箱へ入れました'
            + (left.length ? '（' + left.length + ' 枚は入れられませんでした）' : ''));
    }
});


/* ── カレンダー（依頼 453） ── */

/// いま出している月と、選んでいる日。**開くたびに今月へ戻さない** ──
/// 先の予定を見にきた人を、閉じて開くたびに今日へ連れ戻さない。
let calMonth = null;
let calDay = null;
/// 見方（`month` / `week` / `day`）。**憶える** ── 週で暮らしている人を、
/// 開くたびに月へ連れ戻さない（依頼 468）。
let calView = 'month';
/// **並べて表示**（依頼 471・名前は依頼 527 で変えた）。日と週のときだけ
/// 効く ── 月の表を人ごとに割ると、一人ぶんのセル目が文字より小さくなる。
/// これも憶える。**「グループカレンダー」とは呼ばない** ── その名前は
/// グループと共有している予定の入れもののほうが持っている。
let calGroup = false;
/// グループカレンダー（依頼 525）── `{ id, name }` か null。**この環境が憶える**
/// （`amber.json` の `group`）。名前から探し直せないので ── `calendar.app.created`
/// に一覧を読む力は無い ── 憶えていなければ「まだ無い」と同じことになる。
let groupCal = null;
/// 何を出しているか（依頼 529）── `'me'`（自分だけ）／`'group'`／`'both'`。
/// **既定は両方**（本人）。グループカレンダーが無ければ、絞るものが無いので
/// 帯にも出さない ── 選べないものを見せない。
let calSide = 'both';
/// 見つけたグループカレンダーについて、一度訊いたか（依頼 538）。
/// **断った人に毎回訊かない。**
let groupAsked = false;
/// 作るときの名前（本人が決めた・2026-09-13）。**Google カレンダーにも、
/// 端末のカレンダーにも、グループ全員の画面にもこの名前で出る。**
const GROUP_NAME = 'ambər グループ';
/// 出さない人（段の鍵）。**全員を並べると読めない** ── 十五人の段から
/// 三人を探すのは、目でやる仕事としては重い（依頼 473）。憶える。
let calHide = [];
/// 土日を出すか（本人・2026-09-11・設定で選ぶ）。既定は出す。
let calWeekend = true;
/// この Mac の予定の色（依頼 498・本人「オレンジっぽい色が好き」）。既定は淡い緑。
let calHereColor = '';
/// 人ごとの色（みんなの表・段の鍵 → 色）。決めていない人は名前から一つ選ぶ。
let calColors = {};

/// 選べる色。**名前で呼べる八色**（依頼 540・本人が名前を決めた）── 明るい紙
/// でも暗い紙でも読める濃さ。
///
/// **十色から二色やめた**（本人）── 青は水色と、オレンジは赤と似ていて、
/// 小さな一行になると見分けが付かない。選べる数が多いほど、どれにするかで
/// 止まるので、そこは減らすほうがよい。
///
/// **青をやめたので、青はよその予定表だけの色になった** ── 人のタグに青が
/// 出てこないぶん、青い予定は「自分では直せないもの」だと読める。
const CAL_COLORS = [
    ['#e0669c', 'ローズ'], ['#d9a400', 'アンバー'], ['#2f8a52', 'リーフ'],
    ['#8e5cb3', 'バイオレット'], ['#1fa3a3', 'シアン'], ['#c0392b', 'カーマイン'],
    ['#7a5c3a', 'セピア'], ['#5a6b7f', 'スレート'],
];
const colorName = (hex) => (CAL_COLORS.find(([h]) => h === hex) || [])[1] || hex;
/// 自動で配る五色（本人・2026-09-12「見栄えのよい五色に絞る。五人以上は
/// ループで」）── 並んだ順に ローズ・アンバー・リーフ・バイオレット・シアン、
/// 六人目はまたローズ。
///
/// **青が先頭だったのを入れ替えた**（依頼 540）── 青そのものをやめたため。
/// 2026-09-12 の決め（青・ピンク・黄・緑・紫）を、本人の了解の上で更新した。
const LANE_COLORS = ['#e0669c', '#d9a400', '#2f8a52', '#8e5cb3', '#1fa3a3'];
/// その段の色。決めてあればそれ、無ければ**並んだ順**で五色を回す。
function laneColor(key, n) {
    if (calColors[key]) return calColors[key];
    return LANE_COLORS[(Number(n) || 0) % LANE_COLORS.length];
}
function paintHereColor() {
    if (calHereColor) document.documentElement.style.setProperty('--cal-here', calHereColor);
    else document.documentElement.style.removeProperty('--cal-here');
}

/// 出す予定だけ（隠した予定表のものを落とす）。**どの見方でも同じ一本**を通す
/// ── 月だけ隠せていない、が起きないように。ノートは落とさない。
/// **誰の用事かのタグ**（依頼 539）── 予定の `notes` から、まとめて読む。
///
/// タグの置き場所はメモ欄のいちばん最後の行で、**どこを読むかを決めるのは
/// core**（`caltag`）。ここで切り出すと、デスクトップ版と iPhone で読み方が二つできる。
///
/// **一度で訊く。** 月の表には予定が何十本も並ぶので、一本ずつ訊くと
/// その数だけ行き来することになる。
async function readCalTags(slots) {
    const want = slots.filter((s) => inGroup(s) && s.notes);
    if (!want.length) return;
    try {
        const got = await window.amber.call('caltag', { notes: want.map((s) => s.notes) });
        const each = (got && got.each) || [];
        want.forEach((s, i) => { s.tags = (each[i] && each[i].tags) || []; });
    } catch { /* 読めなくても、予定そのものは出す */ }
}

/// **その予定の「確かさ」を、ラベルにする**（依頼 561）。
///
/// チームの紙は「いつなら空いているか」を読むためのものなので、**仮の予定が
/// 確定の予定と同じ顔で並ぶと、読めない**（取り決めの `show_as`）。
/// 見た目を増やしすぎないよう、足すのは一つだけ ── **仮（`tentative`）は
/// 点線の枠**。休み（`oof`）や在宅（`workingElsewhere`）は題がそう言って
/// いるので、色や形は変えない。**空き（`free`）はそもそも出さない**
/// （依頼 471・core が落とす）。
///
/// 中身の見えない予定（`shut`）は縞（依頼 471）── こちらは別の話で、
/// 「見せてもらえていない」であって「決まっていない」ではない。
const evMark = (s) => (s.shut ? ' shut' : '') + (s.show === 'tentative' ? ' soft' : '');

/// **描いた札から、その予定に戻れるようにする**（依頼 574）。
///
/// 道（`data-at`）だけでは足りない ── **読むだけの予定にはパスが無い**ので、
/// 押しても何を出せばいいか分からなかった。描くたびに番号を振り直す
/// （古い番号が残っても、次に描いた時点で消える）。
let calMarks = [];
const evId = (s) => ' data-ev="' + (calMarks.push(s) - 1) + '"';
const evOf = (el) => (el && el.dataset.ev ? calMarks[Number(el.dataset.ev)] : null);

/// そのタグの色（依頼 539）。
///
/// **決めていなければ、十色を順に配る。** 設定を開かなくても色分けされた表が
/// 見られるほうがよい ── 使う前に決めさせない。決めた色は `calColors` に入り、
/// この端末だけが憶える（`みんなの表` の人ごとの色と同じ仕組み）。
const tagKey = (t) => 'tag:' + t;
let tagOrder = [];
function tagColor(t) {
    const key = tagKey(t);
    if (calColors[key]) return calColors[key];
    if (!tagOrder.includes(t)) tagOrder.push(t);
    return LANE_COLORS[tagOrder.indexOf(t) % LANE_COLORS.length];
}

/// **段の並び順**（依頼 562・本人「上下を並べ替えられるようにしたい」）。
///
/// キーを並べた一覧。ここに居るものが**この順で先に**来て、居ないものは
/// いままでの決まり（自分 → 人のタグ → この端末 → よそ → チーム、同じ
/// 種類なら名前順）で後ろに続く ── **知らない人が増えても、決めた並びは
/// 壊れない**（CSV は毎日書き換わるので、名前で固定すると消えた人の穴が空く）。
let calOrder = [];

/// **段につけた呼び名**（依頼 563・本人「僕の予定は『予定表』という名称で
/// 出力されていそう」）。
///
/// Outlook は**自分の予定表**の表示名を「予定表」と返す ── 書き出す側で
/// 直すのが筋だが（そちらは別のセッションの仕事）、**こちらでも呼べる
/// ようにしておく**。同姓の人が二人来た日にも効くし、書き出す側が何を
/// 返すかは、こちらの都合では決められない。
///
/// **キーはメール。** 名前を変えても、予定との結びつきは動かない
/// （`docs/team-csv.ja.md` の取り決め ── 名前は揺れる）。
let calNames = {};
const laneName = (key, was) => calNames[key] || was;

/// **どれが自分か**（依頼 562・本人「日・週・月は僕じゃない人の予定に
/// なってそうだ」）。
///
/// チームの紙は**人ごとの表**なので、日・週・月に全員ぶんを重ねると、
/// 自分の予定が他人の予定に埋もれる ── 本人が「自分の予定だけ出す」を
/// 選んだ（2026-09-14）。**並べて表示は別**（あれは全員を見るための画面）。
/// 空なら、いままでどおり全員出す。
let calMe = '';

/// この端末が見たことのあるタグ（依頼 544）。**選ばせるために憶えておく** ──
/// 毎回名前を打たせない。予定から読めたものと、人が足したものの両方。
let tagsKnown = [];
const tagsSeen = () => {
    const out = tagsKnown.slice();
    for (const s of calSlots) for (const t of s.tags || []) if (!out.includes(t)) out.push(t);
    return out;
};
function rememberTag(t) {
    if (!tagsKnown.includes(t)) tagsKnown.push(t);
    window.amber.remember({ tagsKnown });
}

/// タグの色を、予定の一行に差す（依頼 539）。
///
/// **一人なら、その色。二人以上なら、左の帯を分ける** ── 二人の用事は
/// 二人のものなので、どちらか片方の色にしてしまうと嘘になる。
/// タグが無ければ何も差さず、いままでの出どころ別の色のままにする。
function tagPaint(s) {
    const tags = (s.tags || []).filter(Boolean);
    if (!tags.length) return '';
    const colors = tags.map(tagColor);
    if (colors.length === 1) return ' style="--c:' + escapeAttr(colors[0]) + '"';
    const step = 100 / colors.length;
    const bands = colors
        .map((c, i) => escapeAttr(c) + ' ' + (i * step) + '% ' + ((i + 1) * step) + '%')
        .join(',');
    return ' style="--c:' + escapeAttr(colors[0])
        + ';--bands:linear-gradient(' + bands + ')"';
}

/// その予定は、グループカレンダーのものか（依頼 529）。
///
/// **予定表の名前で当てる。** EventKit が返すのは端末側のカレンダーの題で、
/// Google のカレンダー id ではない ── 端末に降りてきた時点で別の世界の
/// ものになっている。人が Google の画面で名前を変えたら、こちらの憶えも
/// 変わるまでは当たらない（`calGet` で名前を取り直せる）。
/// **当てるところは、この一か所だけ**にする。
function inGroup(s) {
    return !!groupCal && s.kind === 'here' && s.from === groupCal.name;
}

/// 出すもの。引っ込めた人を落とし、**自分だけ／グループの絞り込み**を効かせる。
///
/// ノートに書いた予定は「自分だけ」のもの ── 共有フォルダのノートでも、
/// カレンダーとしてはこの端末のものなので、グループ側には出さない。
function calShown() {
    return calSlots.filter((s) => {
        if (s.kind !== 'note' && calHide.includes(whoOf(s).key)) return false;
        // **日・週・月に、人の予定を混ぜない**（依頼 562・566）。
        //
        // チームの紙は**人ごとの表**なので、全員ぶんを重ねると自分の予定が
        // 埋もれる ── どころか、**自分じゃない誰かの予定が自分の欄に出る**
        // （本人・2026-09-14「これはダメだな」）。
        //
        // **自分を決めてあれば、その人のぶんだけ。決めていないなら、一件も
        // 出さない**（並べて表示で見る）── 「全員ぶん」を既定にすると、
        // 決めるまでずっと誰かの予定を自分のものとして読むことになる。
        // ここは日・週・月が通るパスで、並べて表示は通らない（あちらは
        // `calSlots` を直に読む）── 全員を見る画面は、全員のまま。
        if (s.kind === 'team' && whoOf(s).key !== calMe) return false;
        if (!groupCal || calSide === 'both') return true;
        return calSide === 'group' ? inGroup(s) : !inGroup(s);
    });
}
/// 出す日だけ（土日を隠すなら平日だけ）。
function calDaysOf(days) {
    return calWeekend ? days : days.filter((d) => (new Date(d + 'T00:00:00').getDay() + 6) % 7 < 5);
}
/// その月ぶんの予定（core の `month` が返したまま）。
let calSlots = [];
/// **カレンダーは、画面の一つ**（依頼 478）。
///
/// 前はダイアログとして上に重ねていたが、**重ねると左右が狭い**うえ、
/// ノートと同じ場所に出せば全画面（F12）がそのまま効く ── 予定を
/// 眺めるのは「ちょっと開く」ではなく「しばらく居る」ことなので、
/// ノートと同じ広さで扱う。
let calOn = false;

/// カレンダーを開く。
///
/// **ノートを見るところは奪わない。** 画面（表示／コード）はノートのもので、
/// カレンダーはノートではない ── 上に重ねて出し、ノートを開くときに閉じる。
async function cmdCalendar() {
    if (!state.root) { say('保存場所がありません'); return; }
    // 招待された側のために、一度だけ探す（依頼 538）。
    findGroupCal().catch(() => { /* 見つからなくても、開くのは止めない */ });
    calOn = true;
    if (!calMonth) {
        const now = new Date();
        calMonth = { y: now.getFullYear(), m: now.getMonth() + 1 };
        calDay = ymd(now.getFullYear(), now.getMonth(), now.getDate());
    }
    applyView();
    // **左の列も、いま見ているものに合わせる**（依頼 477）。
    drawRail();
    await hereAsk();
    // **開いたときに、いま置いてあるファイルを読む**（依頼 476）。ここから先は
    // 日を替えても読み直さない ── 読むのは「更新」を押したときと、毎時十分。
    if (teamFile && !teamPlans) { await teamLoad(); teamClock(); }
    await drawCal();
}

/// 出している範囲がまたぐ月。
///
/// **週は月をまたぐ。** 九月三十日から十月四日までの週で九月ぶんだけを
/// 読むと、十月の四日が白紙のまま出る ── 予定が無いのか、読んでいない
/// のかが、画面からは区別できない。
function calMonths() {
    if (calView === 'month') return [{ y: calMonth.y, m: calMonth.m }];
    const want = new Map();
    const add = (day) => {
        const y = Number(day.slice(0, 4));
        const m = Number(day.slice(5, 7));
        want.set(y * 100 + m, { y, m });
    };
    if (calView === 'day') add(calDay);
    else for (const d of weekOf(calDay)) add(d);
    return [...want.values()];
}

/// カレンダーを閉じて、ノートに戻る。**ここが、カレンダーから出る唯一の道。**
function calShut() {
    calOn = false;
    applyView();
    drawRail();
    // **帯を戻す**（依頼 554）。`applyView` はカレンダーのあいだ帯を畳むが、
    // **畳みっぱなしで誰も戻していなかった** ── タブが二枚あっても、
    // カレンダーから戻ると帯ごと消えていた（「閉じる」でも同じ）。
    // 出すかどうかを決めるのは `drawStrip` 一本なので、そちらに訊く。
    drawStrip();
}

async function drawCal() {
    const box = el('cal');
    calMarks = [];
    calSlots = [];
    awaySlots = [];
    let bad = '';
    for (const { y, m } of calMonths()) {
        // 保存ディレクトリごとに訊いて足す（依頼 511）── どの日のノートも、
        // どこに置いてあっても同じ表に出る。
        for (const p of state.places) {
            if (state.placeTrouble[p.dir]) continue;
            try {
                const got = await window.amber.call('month', { path: p.dir, year: y, month: m });
                calSlots = calSlots.concat(got.days || []);
            } catch (e) {
                bad = why(e);
            }
        }
        // この環境の予定表と、よその予定表と、チームの紙を足す ── どれが
        // 読めなくても、自分のぶんは出る。
        calSlots = calSlots.concat(await hereFor(y, m));
        const far = await awayFor(y, m);
        awaySlots = awaySlots.concat(far);
        calSlots = calSlots.concat(far, teamFor(y, m));
    }
    if (bad) say('カレンダーを読めません: ' + bad);
    // 誰の用事かのタグを、まとめて読む（依頼 539）。
    await readCalTags(calSlots);
    calSlots = calSlots.sort((a, b) =>
        a.day.localeCompare(b.day)
        || (a.at ? 0 : 1) - (b.at ? 0 : 1)
        || String(a.at).localeCompare(String(b.at))
        || a.title.localeCompare(b.title));
    // **いま見ているものが一つだけ光る**（依頼 528・案ア）。並べているなら
    // 「並べて」── 日・週・月と同じ並びに入れたので、光りも一つで済む。
    // 何を出しているか（依頼 529）── グループカレンダーが無ければ出さない。
    const side = box.querySelector('.seg.side');
    if (side) {
        side.hidden = !groupCal;
        for (const b of side.querySelectorAll('button')) {
            b.classList.toggle('on', b.dataset.side === calSide);
        }
    }
    const showing = calGroup && calView !== 'month' ? 'crowd' : calView;
    // **`.seg:not(.side)` に絞る。** ここを `.seg button` にしていたせいで、
    // 直前に付けた絞り込みの光りを、このループが全部消していた
    // （絞り込みのボタンには `data-view` が無いので、必ず「消す」側に倒れる）。
    // 撮っても気づけず、計算後の色を画面に書かせて初めて分かった。
    for (const b of box.querySelectorAll('.seg:not(.side) button')) {
        b.classList.toggle('on', b.dataset.view === showing);
    }
    // グループカレンダーのボタン ── まだ無ければ「作る」、あれば「招待」。
    // **見出しを描くところに置く** ── 一度「みんなの表を描く関数」の中に
    // 書いてしまい、押すまで文字が入らず**空のボタンが出た**（画面を撮って
    // 気づいた）。描く場所と、出す場所を取り違えると、こうなる。
    const gb = box.querySelector('.groupbtn');
    if (gb) {
        // **作ったら、帯から消える。** 作るのは一生に一度で、そのあと
        // ずっと居座らせると帯が一つぶん狭くなる（実際に折れた）。
        // 招待は**グループを見ているときだけ**出す ── 招待したくなるのは、
        // まさにそのときだから。
        // **招待は持ち主だけ。** 招待された側は Google 側の id を持たないので、
        // 共有設定の画面にも行けない（持ち主の端末からやってもらう）。
        gb.hidden = !!groupCal && (calSide !== 'group' || !groupCal.id);
        // **マークを添える**（本人・2026-09-13）── 「＋ 予定を登録する」と同じで、
        // 絵が入口、文字が答え合わせ（依頼 288）。マークは左の列の共有と同じ二人 ──
        // 同じものには同じ形を使う。
        gb.innerHTML = '<svg viewBox="0 0 16 16" aria-hidden="true">'
            + '<g fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"'
            + ' stroke-linejoin="round">' + RAIL_MARKS.share + '</g></svg>'
            + '<span>' + (groupCal ? '招待' : 'グループを作る') + '</span>';
        gb.title = groupCal
            ? '「' + groupCal.name + '」に人を招待します'
            : '一緒に使う人と予定を共有するためのカレンダーを作ります';
    }

    const grouped = calGroup && calView !== 'month';
    // 「人を選ぶ」は、並べているときだけ。
    const pick = box.querySelector('.whobtn');
    pick.hidden = !grouped;
    if (grouped) {
        const all = crowdLanes(calView === 'day' ? [calDay] : weekOf(calDay), true).length;
        const on = Math.max(0, all - calHide.length);
        pick.textContent = calHide.length ? '人を選ぶ（' + on + '/' + all + '）' : '人を選ぶ';
    }

    box.querySelector('.mo').textContent = calTitle();
    // **件数は帯から外した**（依頼 528・案ア）── 表を見れば分かる。
    // 空のときだけ、表の真ん中で言う ── 何も無いのか、読めていないのかが
    // 分からないのがいちばん困る。
    const plans = calShown().filter((s) => s.kind !== 'note').length;
    const none = box.querySelector('.calnone');
    if (none) {
        none.hidden = plans > 0 || grouped;
        // **なぜ空なのかを言う**（依頼 574）── チームの紙は読めているのに
        // 「自分」を決めていないと、日・週・月は一件も出ない。黙って空だと
        // 「読めていない」と見分けがつかない。
        none.textContent = !calMe && teamPeople.length
            ? '自分を決めると、ここに自分の予定が出ます（「並べて」で名前を右押し →「自分はこの人」）'
            : '予定はありません';
    }
    // **いつ時点の紙かを出す。** チームの予定は置き換わるファイルを読んで
    // いるだけなので、これが無いと古い紙を今の予定だと思って読む。
    //
    // **ここが、読んだあとの入口になる。** 合言葉は「まだ読んでいない人」の
    // 目に触れないためのものなので、**もう読んでいる人に毎回打たせない**
    // ── 読み直す・別の紙にする・やめるは、ここから。
    const at = box.querySelector('.teamat');
    at.hidden = !teamFile;
    at.textContent = teamBad ? 'チームは読めません'
        : teamAt ? 'チームは ' + teamAt + ' 時点' : 'チームの予定表';
    at.classList.toggle('bad', !!teamBad);
    at.title = teamBad
        ? teamBad + '（' + shortPath(teamFile) + '）'
        : shortPath(teamFile) + ' ── 押すと、別のファイルにするか、やめられます';
    box.querySelector('.teamnow').hidden = !teamFile;

    box.querySelector('.month').hidden = calView !== 'month';
    box.querySelector('.hours').hidden = calView === 'month' || grouped;
    box.querySelector('.crowd').hidden = !grouped;
    if (grouped) { drawCrowd(); return; }
    if (calView !== 'month') { drawHours(); return; }

    // **月曜はじまり。** 一覧の並びも週も、ここでは月曜から。
    const first = new Date(calMonth.y, calMonth.m - 1, 1);
    const lead = (first.getDay() + 6) % 7;
    const days = new Date(calMonth.y, calMonth.m, 0).getDate();
    const today = ymd(new Date().getFullYear(), new Date().getMonth(), new Date().getDate());
    const cells = [];
    for (let i = 0; i < lead; i += 1) cells.push(null);
    for (let d = 1; d <= days; d += 1) cells.push(ymd(calMonth.y, calMonth.m - 1, d));
    while (cells.length % 7) cells.push(null);
    // 土日を隠すなら、七つのうち五つ（月〜金）だけ並べる。
    const wide = calWeekend ? 7 : 5;
    const shownCells = calWeekend ? cells : cells.filter((_, i) => i % 7 < 5);
    box.querySelector('.dow').style.gridTemplateColumns = 'repeat(' + wide + ',1fr)';
    box.querySelector('.days').style.gridTemplateColumns = 'repeat(' + wide + ',1fr)';
    for (const i of box.querySelectorAll('.dow .sat, .dow .sun')) i.hidden = !calWeekend;
    const visible = calShown();

    box.querySelector('.days').innerHTML = shownCells.map((day) => {
        if (!day) return '<div class="d dim"></div>';
        const mine = visible.filter((s) => s.day === day);
        const plans = mine.filter((s) => s.kind !== 'note');
        const shown = new Set(plans.map((s) => s.path));
        // **予定が先、ノートは後。** 数が溢れたときに残したいのは予定。
        // 予定として出ているノートは、重ねて出さない。
        const sorted = plans.concat(
            mine.filter((s) => s.kind === 'note' && !shown.has(s.path)));
        const chips = sorted.slice(0, 3).map((s) =>
            // **塗ってあるものが、グループに見えている予定**（依頼 529）。
            // 色はタグ（誰の用事か）にだけ使うので、見えているかどうかは
            // 塗りで言う ── 10px のマークは、月の表の一行では小さすぎる。
            '<span class="ev ' + s.kind + (inGroup(s) ? ' sh' : ' lo')
            + evMark(s) + '"' + evId(s) + tagPaint(s)
            + (s.path ? ' data-at="' + escapeAttr(s.path) + '"' : '') + '>'
            + (s.at ? escapeHtml(s.at) + ' ' : '') + escapeHtml(s.title) + '</span>').join('');
        const rest = sorted.length > 3 ? '<span class="more">ほか ' + (sorted.length - 3) + '</span>' : '';
        const marks = (day === today ? ' today' : '') + (day === calDay ? ' on' : '');
        return '<div class="d' + marks + '" data-day="' + day + '">'
            + '<div class="n">' + Number(day.slice(8)) + '</div>' + chips + rest + '</div>';
    }).join('');

}


/// 上に出す名前。見方で変わる。
function calTitle() {
    if (calView === 'month') return calMonth.y + '年 ' + calMonth.m + '月';
    if (calView === 'day') return dayName(calDay) + '（' + weekName(calDay) + '）';
    const days = weekOf(calDay);
    return dayName(days[0]) + ' 〜 ' + dayName(days[6]);
}

const weekName = (d) => ['月', '火', '水', '木', '金', '土', '日'][
    (new Date(d + 'T00:00:00').getDay() + 6) % 7];

/// その日を含む週（月曜はじまり・七つ）。
function weekOf(day) {
    const d = new Date(day + 'T00:00:00');
    d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
    const out = [];
    for (let i = 0; i < 7; i += 1) {
        out.push(ymd(d.getFullYear(), d.getMonth(), d.getDate()));
        d.setDate(d.getDate() + 1);
    }
    return out;
}

/// 時刻を分に。読めなければ null。
const mins = (t) => {
    const m = /^(\d{1,2}):(\d{2})$/.exec(String(t || ''));
    return m ? Number(m[1]) * 60 + Number(m[2]) : null;
};

/// 週と日の表。**時間が縦に並ぶ。**
///
/// 高さの元は一時間 = `HOUR_PX`。終わりの時刻が無いもの（amber 自身の
/// 予定は「その時刻」しか持たない）は、三十分ぶんの高さにする ──
/// 潰れて読めないより、少し大きいほうがよい。
const HOUR_PX = 56;   // index.html の 3.5rem と合わせる
/// **出す時間帯**（依頼 562・本人が「設定で決める・既定 8〜19」を選んだ）。
///
/// 会社の予定表は朝から夕方に詰まっていて、真夜中は一日も使わない ──
/// 24 時間ぶん敷くと、**見える幅が半分以下**になり、三十分の会議の件名が
/// 読めなくなる（本人・2026-09-14）。8〜19 なら同じ画面幅で一時間あたりが
/// 約二倍になる。**家での使い方もあるので、設定で変えられる。**
let calFrom = 8;
let calTill = 19;
/// その時間帯に、時間が何つぶんあるか（目盛りと帯の高さの土台）。
const calHours = () => Array.from({ length: Math.max(1, calTill - calFrom) }, (_, i) => calFrom + i);
/// 上からの位置（分 → px）。**時間帯の頭を 0 とする。**
const calTop = (m) => ((m - calFrom * 60) / 60) * HOUR_PX;

function drawHours() {
    const box = el('cal');
    const days = calView === 'day' ? [calDay] : calDaysOf(weekOf(calDay));
    const today = ymd(new Date().getFullYear(), new Date().getMonth(), new Date().getDate());
    const calSlotsShown = calShown();

    // 終日の段（曜日の見出しも兼ねる）。
    const ad = box.querySelector('.allday');
    ad.style.gridTemplateColumns = '3rem repeat(' + days.length + ', 1fr)';
    ad.innerHTML = '<div></div>' + days.map((d) => {
        const w = (new Date(d + 'T00:00:00').getDay() + 6) % 7;
        const mark = (w === 5 ? ' sat' : w === 6 ? ' sun' : '') + (d === today ? ' today' : '');
        // **終日の段は「予定」だけ。** その日に書いたノートはここではなく、
        // 右のその日の欄に出る ── 段に混ぜると、時刻つきの予定が上と下に
        // 二度並ぶ（実際にそう見えた）。
        const whole = calSlotsShown.filter((s) => s.day === d && !s.at && s.kind !== 'note');
        return '<div><div class="hd' + mark + '">' + weekName(d) + ' '
            + Number(d.slice(8)) + '</div>'
            + whole.map((s) => '<div class="ad ' + s.kind + evMark(s) + '"' + evId(s)
                + (s.path ? ' data-at="' + escapeAttr(s.path) + '"' : '')
                + '>' + escapeHtml(s.title) + '</div>').join('') + '</div>';
    }).join('');

    // 時刻の目盛りと、日ごとの帯。
    const cols = box.querySelector('.cols');
    cols.style.gridTemplateColumns = '3rem repeat(' + days.length + ', 1fr)';
    const hours = calHours();
    cols.innerHTML = '<div class="clock">'
        + hours.map((h) => '<div>' + h + '</div>').join('') + '</div>'
        + days.map((d) => {
            const timed = calSlotsShown.filter((s) => s.day === d && s.at);
            // **重なるものは、横に分ける**（依頼 574・本人「2行、3行あるものは
            // 文字が重なって見えない」）。
            //
            // 同じ時間に二つあると、前は同じ場所に重ねて描いていて、**下の
            // 予定は一文字も読めなかった**。時間の重なりで塊にまとめ、その中で
            // 空いている列へ順に置く ── 塊の中でいちばん多い列数で割る。
            // **塊ごとに割る**（一日ぶんの最大で割らない）── 朝に3 つ重なった
            // 日は、夕方の一件まで三分の一の幅になってしまう。
            const put = timed.map((s) => {
                const from = mins(s.at);
                const till = mins(s.to);
                const to = till !== null && till > from ? till : (from === null ? null : from + 30);
                return { s, from, to };
            }).filter((x) => x.from !== null).sort((a, b) => a.from - b.from || a.to - b.to);
            let group = [];
            let edge = -1;
            const lay = (rows) => {
                const cols = [];
                for (const r of rows) {
                    let c = cols.findIndex((end) => end <= r.from);
                    if (c < 0) { c = cols.length; cols.push(0); }
                    cols[c] = r.to;
                    r.col = c;
                }
                for (const r of rows) { r.wide = cols.length; }
            };
            for (const r of put) {
                if (group.length && r.from >= edge) { lay(group); group = []; }
                group.push(r);
                edge = Math.max(edge, r.to);
            }
            if (group.length) lay(group);
            const blocks = put.map(({ s, from, col, wide }) => {
                const till = mins(s.to);
                const high = Math.max(18, ((till !== null && till > from ? till - from : 30)
                    / 60) * HOUR_PX);
                // **時間帯の外は、上下の端に寄せる** ── 落とすと「その日は
                // 空いている」と読める。位置は嘘になるが、在ることは本当。
                const top = Math.min(Math.max(calTop(from), 0), (hours.length * HOUR_PX) - high);
                const out = from < calFrom * 60 || from >= calTill * 60 ? ' out' : '';
                // 横の置き場所（重なっているぶんだけ細くなる）。
                const w = 100 / wide;
                const side = 'left:calc(' + (col * w) + '% + 2px);width:calc(' + w + '% - 4px);';
                // **狭いラベルは、時刻を省いて件名だけ**（依頼 562・本人）──
                // 時刻は置かれている位置が言っている。2 段に組むので、
                // 高さが足りるものだけ時刻を出す。
                const tall = high >= 34;
                return '<div class="blk ' + s.kind + evMark(s) + out + (tall ? ' two' : '') + '"' + evId(s)
                    + (s.path ? ' data-at="' + escapeAttr(s.path) + '"' : '')
                    + ' title="' + escapeAttr(s.at + ' ' + s.title) + '"'
                    + ' style="' + side + 'top:' + top + 'px;height:' + high + 'px">'
                    + (tall ? '<i>' + escapeHtml(s.at) + '</i>' : '')
                    + '<b>' + escapeHtml(s.title) + '</b></div>';
            }).join('');
            return '<div class="lane" data-day="' + d + '">'
                + hours.map(() => '<div class="hr"></div>').join('') + blocks + '</div>';
        }).join('');

    // **朝が見えているところから始める。** 開いた瞬間に真夜中が出ていると、
    // 毎回スクロールしてから見ることになる。いちばん早い予定か、七時。
    const early = calSlotsShown
        .filter((s) => days.includes(s.day) && s.at)
        .map((s) => mins(s.at)).filter((n) => n !== null);
    const from = early.length ? Math.min(...early) : calFrom * 60;
    cols.scrollTop = Math.max(0, calTop(from) - HOUR_PX * 0.5);
}

/* ── みんなの予定を、人ごとに並べる（依頼 471） ── */

/// その予定は**誰のものか**。段をまとめるキーと、段に出す名前。
///
/// **キーは名前ではない。** 表示名は同姓・改姓・全角半角で揺れるので、
/// 会社の紙はメールでまとめる（`docs/team-csv.ja.md` の取り決め）。
function whoOf(s) {
    if (s.kind === 'team') {
        const key = 'team:' + (s.mail || s.who);
        return { key, name: laneName(key, s.who || '（名前なし）') };
    }
    if (s.kind === 'away') return { key: 'away:' + (s.from || ''), name: s.from || 'よその予定表' };
    if (s.kind === 'here') return { key: 'here:' + (s.from || ''), name: s.from || 'この端末' };
    return { key: 'me', name: '自分のノート' };
}

/// 段の並び。
///
/// **予定の無い人も段を持つ。** 出張の週に段ごと消えると、書き出せて
/// いないのか本当に空なのかが、画面からは区別できない ── いちばん
/// 知りたいのが「空いているかどうか」なのに。
function crowdLanes(days, whole) {
    const lanes = new Map();
    const put = (key, name, kind) => {
        if (!lanes.has(key)) lanes.set(key, { key, name, kind, slots: [] });
        return lanes.get(key);
    };
    // 自分が先、つぎに端末とよそ、そのあとにチーム ── 見にきた人の段を
    // いちばん上に置く。
    put('me', '自分のノート', 'note');
    // **自分だけ／グループ／両方 を、ここでも効かせる**（依頼 549）。
    // ここだけ `calSlots` を直に読んでいたので、**絞り込みのボタンが
    // 並べて表示では死んでいた** ── 「グループ」が光っているのに会社の
    // 予定がそのまま並ぶ（依頼 477 と同じ、押しても何も起きない形）。
    //
    // **引っ込めた段（`calHide`）は、ここでは落とさない。** 落とすと
    // その段が「人を選ぶ」の一覧からも消えて、**戻すパスが無くなる**
    // （落とすのは最後の一行）。
    const mine = calSlots.filter((s) => {
        if (!groupCal || calSide === 'both') return true;
        return calSide === 'group' ? inGroup(s) : !inGroup(s);
    });
    for (const s of mine) {
        if (!days.includes(s.day)) continue;
        if (s.kind === 'note') continue;
        // **名前が付いた予定は、予定表の段から抜けて、その人の段へ**（依頼 548・
        // 本人が決めた）。カレンダーを繋いだ直後はどの予定にも名前が付いて
        // いないので、**そのうちは今までどおり予定表ごとの段**しか出ない ──
        // 使う前に段を増やして見せても、何のことか分からない。
        // **二人以上なら、両方の段に出す**（本人が決めた）── 家族旅行は
        // 太郎の用事でもあり次郎の用事でもあるので、その日その人が何を
        // しているかを段で読める。
        const tags = (s.tags || []).filter(Boolean);
        if (tags.length) {
            for (const t of tags) put('tag:' + t, t, 'tag').slots.push(s);
            continue;
        }
        const who = whoOf(s);
        put(who.key, who.name, s.kind).slots.push(s);
    }
    for (const w of teamPeople) {
        const key = 'team:' + (w.mail || w.name);
        put(key, laneName(key, w.name), 'team');
    }
    // 人の段は、予定表の段より上に ── グループを見ているときに知りたいのは
    // 「だれの用事か」のほうで、どの予定表から来たかではない。
    // **憶えた並びが先。** 居ないものは、いままでの決まりで後ろに続く。
    const fixed = (l) => { const i = calOrder.indexOf(l.key); return i < 0 ? 9999 : i; };
    const rank = (l) => (l.key === 'me' ? 0 : l.kind === 'tag' ? 1
        : l.kind === 'here' ? 2 : l.kind === 'away' ? 3 : 4);
    // **自分のノートの段は、空なら出さない**（依頼 549・本人が決めた）。
    // ノートに日付を書かない人には、ずっと空の段が一つ並ぶだけになる。
    // **チームの段は空でも残す**（依頼 471）── あちらは段ごと消えると
    // 「書き出せていない」と「本当に空」が読めなくなる。逆の決まりなので、
    // 消すのは `me` だけと名指しする。
    if (!lanes.get('me').slots.length) lanes.delete('me');
    const out = [...lanes.values()].sort((a, b) => fixed(a) - fixed(b)
        || rank(a) - rank(b)
        || a.name.localeCompare(b.name, 'ja'));
    // **選んだ人だけ並べる。** `whole` が真なら、選ぶための一覧なので全員。
    return whole ? out : out.filter((l) => !calHide.includes(l.key));
}

/// 一時間ぶんの横幅（日のとき）。文字が読める幅を確保して、足りなければ
/// 横に流す ── 一日を画面幅に押し込むと、三十分の会議が線になる。
const CROWD_HOUR = 168;

/// **横に時間（または日付）、縦に人。**
function drawCrowd() {
    const box = el('cal');
    const days = calView === 'day' ? [calDay] : calDaysOf(weekOf(calDay));
    const lanes = crowdLanes(days);
    // **一日は横に流す。週は流さない。** 一日を画面幅に押し込むと三十分の
    // 会議が線になるので幅を決め打ちにするが、週の七日は画面に収まる
    // ほうがよい ── 七日のうち五日しか見えない週の表は、週の表ではない。
    const wide = calView === 'day' ? ' style="width:' + calHours().length * CROWD_HOUR + 'px"' : '';
    const cell = calView === 'day' ? ' style="width:' + CROWD_HOUR + 'px"' : '';
    const today = ymd(new Date().getFullYear(), new Date().getMonth(), new Date().getDate());

    // 上の目盛り。日なら時刻、週なら日付。
    const ticks = calView === 'day'
        ? calHours().map((h) =>
            '<div class="tk"' + cell + '>' + h + '</div>').join('')
        : days.map((d) => {
            const w = (new Date(d + 'T00:00:00').getDay() + 6) % 7;
            const mark = (w === 5 ? ' sat' : w === 6 ? ' sun' : '') + (d === today ? ' today' : '');
            return '<div class="tk' + mark + '">'
                + weekName(d) + ' ' + Number(d.slice(8)) + '</div>';
        }).join('');

    const rows = lanes.map((lane, n) => {
        const got = calView === 'day' ? crowdDay(lane, days[0]) : { html: crowdWeek(lane, days), deep: 1 };
        const track = got.html;
        const tall = got.deep > 1 ? ';min-height:' + (got.deep * CROWD_ROW + 6) + 'px' : '';
        // 人の段は、その人の色で（依頼 548）── 予定の一行に差す色と
        // 同じ鍵（`tag:<名前>`）なので、段の色を変えると予定の色も変わる。
        const tint = lane.kind === 'tag' ? tagColor(lane.name) : laneColor(lane.key, n);
        return '<div class="ln" style="--lane:' + tint + tall + '"><div class="who ' + lane.kind + '"'
            + ' data-key="' + escapeAttr(lane.key) + '" title="押すと引っ込めます。右押しで色を変えます">'
            + '<span>' + escapeHtml(lane.name) + '</span></div>'
            + '<div class="track"' + wide + '>' + track + '</div></div>';
    }).join('');

    const crowd = box.querySelector('.crowd');
    crowd.classList.toggle('wk', calView !== 'day');
    crowd.innerHTML = '<div class="scroll">'
        + '<div class="axis"><div class="who"></div>'
        + '<div class="ticks"' + wide + '>' + ticks + '</div></div>'
        + '<div class="rows">' + rows + '</div></div>';

    // **朝が見えているところから始める**（縦の表と同じ）。
    if (calView === 'day') {
        const early = calSlots.filter((s) => s.day === days[0] && s.at)
            .map((s) => mins(s.at)).filter((n) => n !== null);
        const from = early.length ? Math.min(...early) : calFrom * 60;
        crowd.querySelector('.scroll').scrollLeft =
            Math.max(0, ((from - calFrom * 60) / 60 - 0.5) * CROWD_HOUR);
    }
}

/// 一人ぶん・一日ぶんの帯（横が時間）。
function crowdDay(lane, day) {
    const mine = lane.slots.filter((s) => s.day === day);
    // **終日は、一日ぶんの帯にする。** 二十四時間ぶん敷いて「終日」と
    // 書く ── `00:00〜23:59` に読み替えると、時刻つきの予定に混ざって
    // 並び、画面に `00:00` という嘘の時刻が出る。
    const whole = mine.filter((s) => !s.at).map((s) =>
        '<div class="span ' + s.kind + evMark(s) + '"' + evId(s)
        + (s.path ? ' data-at="' + escapeAttr(s.path) + '"' : '')
        + ' title="' + escapeAttr('終日 ' + s.title) + '">'
        // **文字は、見えているところに貼りつける。** 帯は一日ぶんの幅が
        // あるので、文字を頭に置くと横に流したとたんに画面の外へ出る
        // ── 帯だけが残って、何の帯かが読めなくなる。
        + '<b><i>終日</i> ' + escapeHtml(s.title) + '</b></div>').join('');
    // **重なるものは、縦に積む**（依頼 574・本人「重ならないようにして欲しい」）。
    //
    // こちらは横軸が時間なので、縦の表とは逆に**下へずらす**。塊ごとに数える
    // ── 朝に3 つ重なった日に、夕方の一件まで三分の一の高さにしない。
    const put = mine.filter((s) => s.at).map((s) => {
        const from = mins(s.at);
        const till = mins(s.to);
        const to = till !== null && till > from ? till : (from === null ? null : from + 30);
        return { s, from, to };
    }).filter((x) => x.from !== null).sort((a, b) => a.from - b.from || a.to - b.to);
    let group = [];
    let edge = -1;
    let deep = 1;
    const lay = (rows) => {
        const cols = [];
        for (const r of rows) {
            let c = cols.findIndex((end) => end <= r.from);
            if (c < 0) { c = cols.length; cols.push(0); }
            cols[c] = r.to;
            r.row = c;
        }
        deep = Math.max(deep, cols.length);
    };
    for (const r of put) {
        if (group.length && r.from >= edge) { lay(group); group = []; }
        group.push(r);
        edge = Math.max(edge, r.to);
    }
    if (group.length) lay(group);
    const high = deep > 1 ? CROWD_ROW : 0;
    const bars = put.map(({ s, from, row }) => {
        const till = mins(s.to);
        const wide = Math.max(24, ((till !== null && till > from ? till - from : 30) / 60) * CROWD_HOUR);
        const span = calHours().length * CROWD_HOUR;
        const left = Math.min(Math.max(((from - calFrom * 60) / 60) * CROWD_HOUR, 0), span - wide);
        // **狭いラベルは、時刻を省いて件名だけ**（依頼 562）── 時刻は置かれて
        // いる位置が言っている。三十分の会議で「13:00」に幅を取られると、
        // 件名が一文字も読めない。
        const room = wide >= CROWD_HOUR * 0.75;
        // 積むときだけ高さと位置を決める ── 一つしかない段は、いままで
        // どおり段いっぱい（そのぶん二行に回せる）。
        const stack = high ? 'top:' + (3 + row * high) + 'px;height:' + (high - 3) + 'px;bottom:auto;' : '';
        return '<div class="bar ' + s.kind + evMark(s) + '"' + evId(s)
            + ' style="' + stack + 'left:' + left + 'px;width:' + wide + 'px"'
            + (s.path ? ' data-at="' + escapeAttr(s.path) + '"' : '')
            + ' title="' + escapeAttr(s.at + (s.to ? '〜' + s.to : '') + ' ' + s.title
                + (s.place ? '（' + s.place + '）' : '')) + '">'
            + (room ? '<i>' + escapeHtml(s.at) + '</i> ' : '')
            + escapeHtml(s.title) + '</div>';
    }).join('');
    const hours = calHours().map((h, i) =>
        '<div class="vr' + (h % 3 === 0 ? ' thick' : '') + '" style="left:'
        + (i * CROWD_HOUR) + 'px"></div>').join('');
    return { html: hours + whole + bars, deep };
}

/// 積むときの一段の高さ（依頼 574）。
const CROWD_ROW = 22;

/// 一人ぶん・一週ぶん（横が日付）。
function crowdWeek(lane, days) {
    return days.map((d) => {
        const mine = lane.slots.filter((s) => s.day === d);
        // **終日が先。** その日いっぱいの用事は、時刻つきの予定より先に
        // 目に入るほうがよい（居るか居ないかの話なので）。
        const mine2 = mine.filter((s) => !s.at).concat(mine.filter((s) => s.at));
        // **2 段に組む**（依頼 562・本人）── 時刻を上、件名を下。横に
        // 並べると「13:00」に幅を取られて、件名が三文字で切れる。
        const chips = mine2.slice(0, 4).map((s) =>
            '<div class="chip two ' + s.kind + (s.at ? '' : ' all')
            + evMark(s) + '"' + evId(s)
            + (s.path ? ' data-at="' + escapeAttr(s.path) + '"' : '')
            + ' title="' + escapeAttr((s.at ? s.at + ' ' : '終日 ') + s.title
                + (s.place ? '（' + s.place + '）' : '')) + '">'
            + '<i>' + escapeHtml(s.at || '終日') + '</i>'
            + '<b>' + escapeHtml(s.title) + '</b></div>').join('');
        const rest = mine2.length > 4
            ? '<div class="more">ほか ' + (mine2.length - 4) + '</div>' : '';
        return '<div class="cell" data-day="' + d + '">' + chips + rest + '</div>';
    }).join('');
}


/// **段を上下に動かす**（依頼 562）。
///
/// 憶えるのは**いま画面に出ている並び**そのもの ── 「この段を一つ上へ」
/// だけを憶えると、人が増えた日に意味が変わる。動かしたあとの並びを丸ごと
/// 書き留めるので、**次に開いても同じ順で出る**。
async function moveLane(key, step) {
    const days = calView === 'day' ? [calDay] : weekOf(calDay);
    // **数えるのは、見えている段だけ**（依頼 574・本人）── 引っ込めた段まで
    // 数えると、その人のぶん何度も押すことになる。画面で一つ上に見えている
    // ところへ、一回で行く。
    const seen = crowdLanes(days).map((l) => l.key);
    const now = crowdLanes(days, true).map((l) => l.key);
    const at = seen.indexOf(key);
    const to = at + step;
    if (at < 0 || to < 0 || to >= seen.length) return;
    // 引っ込めた段は、いまの並びのまま置いていく ── 戻したときに、
    // 前に居た場所へ戻る。
    const from = now.indexOf(key);
    const mark = now.indexOf(seen[to]);
    now.splice(from, 1);
    now.splice(step < 0 ? mark : mark - (mark > from ? 1 : 0), 0, key);
    calOrder = now;
    window.amber.remember({ calOrder });
    await drawCal();
}

/// **予定の中身を、そのまま出すダイアログ**（依頼 574・本人「マウスを重ねたり
/// クリックするとポップアップして全部読めるようにしてほしい」）。
///
/// 重なった予定は細くなるし、三十分の会議は幅そのものが足りない ──
/// **どう詰めても切れるラベルは残る**ので、押せば全部読めるパスを用意する。
/// 出すのは**その予定が持っているものだけ**（時刻・件名・場所・だれの・
/// 出どころ）── ここで言葉を足さない。
function popEvent(s, at) {
    const rows = [];
    const when = s.at ? s.at + (s.to ? '〜' + s.to : '') : '終日';
    rows.push({ html: '<b class="evh">' + escapeHtml(s.title || '（タイトルなし）') + '</b>', dim: true });
    rows.push({ name: when, dim: true });
    if (s.place) rows.push({ name: s.place, dim: true });
    const who = s.kind === 'team' ? whoOf(s) : null;
    if (who) rows.push({ name: who.name, dim: true });
    if (s.show && s.show !== 'busy') {
        rows.push({ name: { tentative: '仮の予定', oof: '休み・外出', workingelsewhere: '別の場所で仕事',
            free: '空き' }[s.show] || s.show, dim: true });
    }
    if (s.shut) rows.push({ name: '中身は見えていません', dim: true });
    // 開ける先があるなら、そこへ行くパスも置く ── 読むだけのものには出さない。
    if (s.path && !s.noNote && s.kind === 'note') {
        rows.push({ name: 'このノートを開く', sep: true, run: () => { calShut(); openNote(s.path); } });
    }
    popMenu(rows, at);
}

/// 出す人を選ぶ。
///
/// **一人ずつ切り替えて、そのつど開き直す。** 一度に選ぶダイアログが無い
/// ので、選んだら閉じずにもう一度出す ── 三人消すのに三回開き直すのは
/// 面倒だが、「選んでいる途中」が画面に残るぶん、間違いに気づきやすい。
/// **グループカレンダーを 1 つ作る**（依頼 525・527）。
///
/// 押すのは一回。サインインも、カレンダーの許可も、主（Node）が面倒を見る
/// ── 使う人に段取りを踏ませない。
///
/// **いまの予定は一つも動かない。** 新しいカレンダーが 1 つ増えるだけで、
/// 自分のカレンダーごと共有する形は採っていない（仕事の予定まで見えるため）。
/// 押す前にそう言う ── 「共有」を押した瞬間に何が起きるか分からないのが、
/// いちばん怖い。
async function cmdMakeGroup() {
    if (!await askYes('グループカレンダーを作りますか。\n\n'
        + '「' + GROUP_NAME + '」という新しいカレンダーを作ります。'
        + 'このカレンダーに入れた予定だけが、招待した人に見えます。'
        + '今ある予定はそのままで、共有されません。')) return;
    say('カレンダーを作成しています…');
    const got = await window.amber.calMake(GROUP_NAME);
    if (!got || got.error) {
        say('作成できませんでした: ' + ((got && got.error) || 'Google から応答がありません'));
        return;
    }
    groupCal = { id: got.id, name: got.name };
    window.amber.remember({ group: groupCal });
    await drawCal();
    // **二段あることを、その場で言う。** amber がカレンダーを作っただけでは
    // 誰にも届かない ── 人を招待する口には審査の要る許可が要るので、
    // いまは Google の画面で招待してもらう（本人が決めた・2026-09-13）。
    if (await askYes('「' + got.name + '」を作成しました。\n\n'
        + '続けて、一緒に使う人を招待しますか。\n'
        + 'Google カレンダーの共有設定が開きます。'
        + '相手のメールアドレスを入力すると、招待が届きます。')) {
        await window.amber.calShare(got.id);
    } else {
        say('招待は、カレンダーの「招待」からいつでもできます');
    }
}

/// **グループカレンダーを消す**（依頼 535）。
///
/// **本当に消える。** 持ち主が消すと、グループの人の画面からも消える ──
/// 「amber から外す」ではない。だから押す前に、そう言う。二度訊くのは、
/// 戻すパスがどこにも無いから（Google のゴミ箱にも残らない）。
async function cmdDropGroup() {
    if (!groupCal) return;
    const name = groupCal.name;
    if (!await askYes('「' + name + '」を削除しますか。\n\n'
        + '招待した人のカレンダーからも消えます。'
        + '中に入っている予定もすべて削除され、元に戻すことはできません。')) return;
    const again = await askText('確認のため、カレンダーの名前を入力してください',
        '', '「' + name + '」と入力すると削除されます');
    if (again === null) return;
    if (again.trim() !== name) { say('名前が一致しないため、削除しませんでした'); return; }
    const got = await window.amber.calDrop(groupCal.id);
    if (got && got.error) { say('削除できませんでした: ' + got.error); return; }
    groupCal = null;
    calSide = 'both';
    window.amber.remember({ group: null, calSide });
    await drawCal();
    // **もう無かったときも、そう言う**（依頼 536）── 黙って「消しました」と
    // 言うと、まだあるのに消したように読める。
    say(got && got.gone
        ? '「' + name + '」は Google カレンダー側ですでに削除されていました（amber の設定からも解除しました）'
        : '「' + name + '」を削除しました');
}

/// グループに人を招待する。**いまは Google の画面で。**
async function cmdInvite() {
    if (!groupCal) return;
    await window.amber.calShare(groupCal.id);
    say('Google カレンダーの共有設定を開きました。相手のメールアドレスを入力してください');
}

async function cmdWhoPick() {
    for (;;) {
        const lanes = crowdLanes(calView === 'day' ? [calDay] : weekOf(calDay), true);
        const rows = lanes.map((l) => ({
            name: (calHide.includes(l.key) ? '　　' : '✓　') + l.name,
            sub: calHide.includes(l.key) ? '出していません' : '',
            value: l.key,
        }));
        rows.unshift({ name: '── 全員を出す', value: '*' });
        const pick = await askPick('並べて表示に出す人', rows,
            '選ぶと出し入れできます。閉じるまで続けて選べます', true);
        if (pick === null) return;
        if (pick === '*') calHide = [];
        else if (calHide.includes(pick)) calHide = calHide.filter((k) => k !== pick);
        else calHide = calHide.concat(pick);
        window.amber.remember({ calHide });
        await drawCal();
    }
}


/* ── チームの予定表（CSV）── */

/// 読む場所。**この環境の中だけに持つ**（ノートにも `.amber/` にも
/// 書かない ── あそこはフォルダと一緒に旅をする。会社のファイルの
/// 在りかが、家の環境にまで伝わることになる）。
let teamFile = '';
/// 最後に読めたファイル。**月ごとには読み直さない**（依頼 476）── この紙は
/// 「今日から何日ぶん」のファイルで、月では分かれていない。月を替えるたびに
/// 読み直すと、途中で置き換わったときに**月によって時点の違うものが
/// 並ぶ**ことになる。読むのは、開いたとき・「更新」を押したとき・毎時十分。
let teamPlans = null;
let teamPeople = [];
/// その紙が「いつ時点」か。
let teamAt = '';
/// 読めなかったときの言い分。
let teamBad = '';
/// 次に読みにいく約束。
let teamTick = null;

/// 読む紙を決める。
async function cmdTeam() {
    const pick = await window.amber.pickFile([
        { name: 'CSV', extensions: ['csv', 'txt'] },
    ]);
    if (!pick) return;
    teamFile = pick;
    teamPlans = null;
    window.amber.remember({ teamFile });
    await teamLoad();
    teamClock();
    say(teamBad ? '読めません: ' + teamBad
        : 'チームの予定表を読むようにしました' + (teamAt ? '（' + teamAt + ' 時点）' : ''));
    if (!el('cal').hidden) await drawCal();
}

/// もう読んでいる人のための入口。**合言葉は、ここでは訊かない。**
///
/// 合言葉（依頼 473）は「まだ読んでいない人の一覧に会社の話を混ぜない」
/// ためのもので、**もう読んでいる人に毎回打たせるためのものではない**。
/// 読んでいるあいだは、いつ時点かの文字がそのまま入口になる。
async function cmdTeamHere() {
    const pick = await askPick('チームの予定表', [
        { name: '別のファイルにする', sub: shortPath(teamFile) },
        { name: '読むのをやめる' },
    ].map((r, n) => ({ ...r, value: n })), '', true);
    if (pick === null) return;
    if (pick === 0) { await cmdTeam(); return; }
    await cmdTeamOff();
}

/// 読むのをやめる。
async function cmdTeamOff() {
    if (!teamFile) { say('チームの予定表は読んでいません'); return; }
    teamFile = '';
    teamPlans = null;
    teamPeople = [];
    teamAt = '';
    teamBad = '';
    teamClock();
    window.amber.remember({ teamFile });
    say('チームの予定表を読むのをやめました');
    if (!el('cal').hidden) await drawCal();
}

/// **いま読みにいく。**「更新」を押したとき。
///
/// **日を替えたときには読まない**（依頼 476）── 前の月を見に戻ったら
/// 紙が入れ替わっていた、というのが画面の上でいちばん分かりにくい。
/// 読むのは、人が読めと言ったときか、決めた時刻。
async function cmdTeamNow() {
    if (!teamFile) return;
    say('読みにいっています…');
    await teamLoad();
    if (!el('cal').hidden) await drawCal();
    say(teamBad ? '読めません: ' + teamBad
        : '読み直しました' + (teamAt ? '（' + teamAt + ' 時点）' : ''));
}

/// 1 つまるごと読む。
///
/// **読めなくても、前に読めたものは捨てない。** 会社のネットワークの中にしか無い
/// 紙なので、家では必ず読めない ── そのたびに段がまるごと消えると、
/// 「読めていない」のか「予定が無い」のかが画面から区別できなくなる。
async function teamLoad() {
    if (!teamFile) { teamPlans = null; teamPeople = []; teamAt = ''; teamBad = ''; return; }
    try {
        const got = await window.amber.call('team', { path: teamFile });
        teamPlans = got.days || [];
        teamPeople = got.people || [];
        teamAt = teamStamp(got.fetched);
        teamBad = '';
    } catch (e) {
        teamBad = why(e);
    }
}

/// 「2026-09-10T08:15:00+09:00」を「9/10 08:15」に。
function teamStamp(text) {
    const w = /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})/.exec(String(text || ''));
    return w ? Number(w[2]) + '/' + Number(w[3]) + ' ' + w[4] : '';
}

/// ひと月ぶん。**持っているデータから選ぶだけ** ── ここではファイルを開かない。
function teamFor(y, m) {
    if (!teamPlans) return [];
    const want = String(y) + '-' + String(m).padStart(2, '0');
    return teamPlans.filter((p) => p.day.startsWith(want));
}

/// **次の「毎時十分」**（依頼 476）。
///
/// 元の CSV は毎時零分に置き換わる。**零分ちょうどには読まない** ──
/// 書いている途中の紙を読むと、列の揃っていない半分だけの表になる。
/// 十分待てば書き終わっている。
function nextTenPast(now) {
    const at = new Date(now);
    at.setMinutes(10, 0, 0);
    if (at <= now) at.setHours(at.getHours() + 1);
    return at;
}

/// 約束を置き直す。**読んでいないときは置かない。**
function teamClock() {
    if (teamTick) { clearTimeout(teamTick); teamTick = null; }
    if (!teamFile) return;
    const now = new Date();
    teamTick = setTimeout(async () => {
        teamTick = null;
        await teamLoad();
        if (!el('cal').hidden) await drawCal();
        // **次の約束は、起きてから置く。** 先に置くと、寝ていた環境が
        // 起きたときに何度もまとめて鳴る。
        teamClock();
    }, nextTenPast(now) - now);
}



/// 予定を登録するダイアログ（依頼 493）── **タイトル・終日・開始・終了**を 1 つで。
/// 時刻は打たせず、十五分刻みから選ばせる。開始を選ぶと終了は一時間後に
/// 置いておく。終日なら時刻は選べない。返すのは `{ title, allDay, start, end }`、
/// やめたら null。
let evDone = null;
function askEvent(head, at0) {
    const box = el('evform');
    const title = el('evtitle');
    const all = el('evall');
    const start = el('evstart');
    const end = el('evend');
    const err = el('everr');
    if (!start.options.length) {
        const opts = ['<option value="">--:--</option>'];
        for (let h = 0; h < 24; h += 1) {
            for (const m of ['00', '15', '30', '45']) {
                const t = String(h).padStart(2, '0') + ':' + m;
                opts.push('<option value="' + t + '">' + t + '</option>');
            }
        }
        start.innerHTML = opts.join('');
        end.innerHTML = opts.join('');
    }
    const plus = (t, min) => {
        const [h, m] = t.split(':').map(Number);
        const n = Math.min(h * 60 + m + min, 23 * 60 + 45);
        return String(Math.floor(n / 60)).padStart(2, '0') + ':' + String(n % 60).padStart(2, '0');
    };
    box.querySelector('.hd').textContent = head;
    // **どこに入れるか**（依頼 544）── グループカレンダーが無ければ出さない
    // （選ぶものが無い）。あるときは、どちらも選ばれていない状態で開く。
    const where = el('evwhere');
    const who = el('evwho');
    let toGroup = null;
    let picked = [];
    where.hidden = !groupCal;
    who.hidden = true;
    const paint = () => {
        for (const b of where.querySelectorAll('button')) {
            b.classList.toggle('on', toGroup !== null && b.dataset.to === (toGroup ? 'group' : 'me'));
        }
        // **誰の用事かは、グループに出すときだけ訊く** ── 誰にも見えない
        // 予定に、誰の用事かを書く意味がない。
        who.hidden = !toGroup;
        if (toGroup) drawEvTags();
        // 選ぶまで登録できない（丙）。
        el('evok').disabled = !!groupCal && toGroup === null;
    };
    function drawEvTags() {
        const known = tagsSeen();
        el('evtags').innerHTML = known
            .map((t) => '<button class="chip' + (picked.includes(t) ? ' on' : '') + '"'
                + ' data-tag="' + escapeAttr(t) + '" style="--c:' + escapeAttr(tagColor(t)) + '">'
                + '<i class="dot"></i>' + escapeHtml(t) + '</button>').join('')
            + '<button class="chip add" data-tag=" new">＋ 名前を追加</button>';
    }
    el('evtags').onclick = async (e) => {
        const b = e.target.closest('.chip');
        if (!b) return;
        const t = b.dataset.tag;
        if (t === ' new') {
            const got = await askText('だれの用事ですか', '', '名前を入力してください（例: 太郎）');
            const name = (got || '').trim();
            if (!name || name.includes(' ')) return;
            if (!picked.includes(name)) picked.push(name);
            rememberTag(name);
        } else {
            picked = picked.includes(t) ? picked.filter((x) => x !== t) : picked.concat(t);
        }
        drawEvTags();
    };
    where.onclick = (e) => {
        const b = e.target.closest('button');
        if (!b) return;
        toGroup = b.dataset.to === 'group';
        paint();
    };
    title.value = '';
    all.checked = false;
    start.value = at0 || '';
    end.value = at0 ? plus(at0, 60) : '';
    err.hidden = true;
    const gate = () => { start.disabled = all.checked; end.disabled = all.checked; };
    gate();
    paint();
    all.onchange = () => { gate(); err.hidden = true; };
    start.onchange = () => { if (start.value) end.value = plus(start.value, 60); err.hidden = true; };
    const shut = (v) => {
        box.hidden = true;
        if (evDone) { const f = evDone; evDone = null; f(v); }
        if (editor && !box.contains(document.activeElement)) editor.focus();
    };
    const go = () => {
        const t = title.value.trim().replace(/\s+/g, ' ');
        if (!t) { err.textContent = 'タイトルを入れてください'; err.hidden = false; title.focus(); return; }
        if (!all.checked) {
            if (!start.value) { err.textContent = '開始の時刻を選んでください（終日なら「終日」にチェックを）'; err.hidden = false; start.focus(); return; }
            if (!end.value) { err.textContent = '終了の時刻を選んでください'; err.hidden = false; end.focus(); return; }
            if (end.value <= start.value) { err.textContent = '終了は開始より後にしてください'; err.hidden = false; end.focus(); return; }
        }
        shut({ title: t, allDay: all.checked, start: all.checked ? '' : start.value,
               end: all.checked ? '' : end.value, toGroup: !!toGroup, tags: picked.slice() });
    };
    el('evok').onclick = go;
    el('evcancel').onclick = () => shut(null);
    box.onmousedown = (e) => { if (e.target === box) shut(null); };
    box.onkeydown = (e) => {
        e.stopPropagation();
        if (e.isComposing || e.keyCode === 229) return;
        if (e.code === 'Escape') { e.preventDefault(); shut(null); }
        else if (isEnter(e) && e.target === title) { e.preventDefault(); go(); }
    };
    box.hidden = false;
    title.focus();
    return new Promise((resolve) => { evDone = resolve; });
}

/// **その日に予定を足す。** 足すのは新しいノートで、日付は前書きに書く
/// ── 「ノートに日付を書くと予定になる」（依頼 73）が既にあるので、
/// カレンダーのためだけの保存場所を作らない。
async function calAdd(day, at0) {
    // **この環境の予定表が使えるなら、そちらへ**（依頼 462）── 普通の
    // カレンダーとして期待されるのはそれ。使えないときだけノートを作る。
    // **ボタンと同じ言葉で言う**（依頼 546）── ボタンが「予定を登録する」と
    // 言っているのに、開いたダイアログが「予定を足す」だと、別のことをするデスクトップ版に見える。
    const ev = await askEvent('予定を登録する（' + dayName(day) + '）', at0);
    if (!ev) return;
    const title = ev.title;
    if (hereOn) {
        // **選ばれた行き先に入れる**（依頼 544）。グループなら、そのカレンダーへ
        // 書き、誰の用事かをメモ欄の最後の行に置く（`caltagset` が文字を作る）。
        let notes = '';
        if (ev.toGroup && ev.tags.length) {
            try {
                const made = await window.amber.call('caltagset', { notes: '', tags: ev.tags });
                notes = (made && made.notes) || '';
            } catch { /* タグが書けなくても、予定そのものは足す */ }
        }
        const into = ev.toGroup && groupCal ? groupCal.name : '';
        const got = await window.amber.cal(['add', title, day, ev.start, ev.end, notes, into]);
        if (!got || got.error) {
            say('登録できませんでした: ' + ((got && got.error) || 'カレンダーから応答がありません'));
            return;
        }
        await drawCal();
        say('「' + title + '」を ' + dayName(day) + ' に登録しました'
            + (ev.toGroup ? '（グループと共有します）' : ''));
        return;
    }
    // ノートに持てるのは始まりだけ（`remind:`）── 終わりの時刻はノートには書かない。
    const when = ev.start ? day + ' ' + ev.start : day;
    try {
        const made = await window.amber.call('new', {
            dir: state.root, title: title.trim(),
        });
        const got = await window.amber.call('read', { path: made.path });
        const out = await window.amber.call('setfield', {
            text: got.text, key: 'remind', value: when,
        });
        await window.amber.call('write', {
            path: made.path, text: out.text, stamp: got.stamp,
        });
        await reload({ quiet: true });
        await drawCal();
        say('「' + title.trim() + '」を ' + dayName(day) + ' に登録しました');
    } catch (e) {
        say('足せません: ' + why(e));
    }
}

/// この環境の予定を、直すか消すか。
async function hereEdit(id) {
    const one = calSlots.find((s) => s.kind === 'here' && s.path === id);
    const pick = await askPick('この予定をどうしますか',
        [{ name: '予定を修正する', value: 'rename' }, { name: '予定を削除する', value: 'drop' }],
        one ? one.title : '', true);
    if (pick === null) return;
    if (pick === 'rename') {
        const to = await askText('予定を修正する', one ? one.title : '');
        if (to === null || !to.trim()) return;
        const got = await window.amber.cal(['rename', id, to.trim()]);
        if (!got || got.error) { say('直せません: ' + (got?.error || '返事がありません')); return; }
    } else {
        if (!await askYes('この予定を削除しますか')) return;
        const got = await window.amber.cal(['drop', id]);
        if (!got || got.error) { say('消せません: ' + (got?.error || '返事がありません')); return; }
    }
    await drawCal();
}

el('cal').addEventListener('click', async (e) => {
    const box = el('cal');
    // **地を押しても閉じない**（依頼 478）── ダイアログではなく画面になったので、
    // 何もないところを押すのは「閉じる」ではない。
    if (e.target.closest('.x')) { calShut(); return; }
    // 文字の大きさ（依頼 623）。**`.seg button` より先に** ── 同じ形の枠に入れて
    // あるので、後に置くと「日・週・月」の受け口が拾って何も起きない（踏んだ）。
    if (e.target.closest('.fdown')) { setCalFont(calFontStep - 1); return; }
    if (e.target.closest('.fup')) { setCalFont(calFontStep + 1); return; }
    // **動く幅は、見方に合わせる** ── 週を見ている人の「次」は次の週。
    const step = (n) => {
        if (calView === 'month') {
            let m = calMonth.m + n;
            let y = calMonth.y;
            if (m < 1) { m = 12; y -= 1; }
            if (m > 12) { m = 1; y += 1; }
            calMonth = { y, m };
            return;
        }
        const d = new Date(calDay + 'T00:00:00');
        d.setDate(d.getDate() + n * (calView === 'week' ? 7 : 1));
        calDay = ymd(d.getFullYear(), d.getMonth(), d.getDate());
        calMonth = { y: d.getFullYear(), m: d.getMonth() + 1 };
    };
    if (e.target.closest('.prev')) { step(-1); await drawCal(); return; }
    if (e.target.closest('.next')) { step(1); await drawCal(); return; }
    const sideBtn = e.target.closest('.seg.side button');
    if (sideBtn) {
        calSide = sideBtn.dataset.side;
        window.amber.remember({ calSide });
        await drawCal();
        return;
    }
    const seg = e.target.closest('.seg button');
    if (seg) {
        if (seg.dataset.view === 'crowd') {
            // **並べるのは、時間が横に並ぶときだけ。** 月から押されたら週へ ──
            // 月の表を人ごとに割ると、一人ぶんのセル目が文字より小さくなる。
            calGroup = true;
            if (calView === 'month') calView = 'week';
        } else {
            calView = seg.dataset.view;
            calGroup = false;
        }
        window.amber.remember({ calView, calGroup });
        await drawCal();
        return;
    }
    if (e.target.closest('.groupbtn')) {
        await (groupCal ? cmdInvite() : cmdMakeGroup());
        return;
    }
    if (e.target.closest('.whobtn')) { await cmdWhoPick(); return; }
    if (e.target.closest('.teamnow')) { await cmdTeamNow(); return; }
    if (e.target.closest('.teamat')) { await cmdTeamHere(); return; }
    // 名前を押したら、その人を引っ込める。**戻し方をその場で言う** ──
    // 押して消えたものの戻し方が画面のどこにも無いのが、いちばん困る。
    const who = e.target.closest('.crowd .rows .who');
    if (who) {
        const key = who.dataset.key;
        if (!key) return;
        calHide = calHide.includes(key)
            ? calHide.filter((k) => k !== key) : calHide.concat(key);
        window.amber.remember({ calHide });
        say(who.textContent.trim() + ' を引っ込めました（「人を選ぶ」で戻せます）');
        await drawCal();
        return;
    }
    // みんなの表を押したとき。**読むだけのものは、そう言う。**
    const bar = e.target.closest('.crowd .bar, .crowd .chip');
    if (bar) {
        if (bar.dataset.at) {
            if (bar.classList.contains('here')) { await hereEdit(bar.dataset.at); return; }
            calShut();
            await openNote(bar.dataset.at);
            return;
        }
        // **読むだけのものは、中身を出す**（依頼 574）── 前は「読むだけです」
        // とだけ言っていた。押した人が知りたいのは、**そのラベルに何が書いて
        // あるか**であって、直せるかどうかではない。
        const got = evOf(bar);
        if (got) { popEvent(got, { x: e.clientX, y: e.clientY }); return; }
        say(bar.classList.contains('team')
            ? 'チームの予定表は読むだけです'
            : 'よその予定表のものなので、ここでは直せません');
        return;
    }
    // 週の「みんな」で、空いているセル目を押したらその日を選ぶ。
    const box3 = e.target.closest('.crowd .cell');
    if (box3) { calDay = box3.dataset.day; await drawCal(); return; }
    // 週と日の表で、何もないところを押したら**その日を選ぶ**。足すのは
    // 二度押しか右押し（本人・2026-09-11）── 一度押しでダイアログが開くのは早すぎた。
    const lane = e.target.closest('.lane');
    if (lane && !e.target.closest('.blk')) {
        if (calDay !== lane.dataset.day) { calDay = lane.dataset.day; await drawCal(); }
        return;
    }
    const ad = e.target.closest('.ad');
    if (ad) {
        if (ad.dataset.at) { calShut(); await openNote(ad.dataset.at); }
        else say('よその予定表のものなので、ここでは直せません');
        return;
    }
    const blk = e.target.closest('.blk');
    if (blk) {
        if (blk.classList.contains('here')) { await hereEdit(blk.dataset.at); return; }
        if (!blk.dataset.at) { say('よその予定表のものなので、ここでは直せません'); return; }
        calShut();
        await openNote(blk.dataset.at);
        return;
    }
    if (e.target.closest('.today')) {
        const now = new Date();
        calMonth = { y: now.getFullYear(), m: now.getMonth() + 1 };
        calDay = ymd(now.getFullYear(), now.getMonth(), now.getDate());
        await drawCal();
        return;
    }
    if (e.target.closest('.add')) { await calAdd(calDay); return; }
    const cell = e.target.closest('.d[data-day]');
    if (cell) { calDay = cell.dataset.day; await drawCal(); }
});


/* ── 二度押しと右押し（依頼 494） ──
 *
 * **押した場所に足す。押したものを直す。** 月・週・日・みんなの表、どれでも
 * 同じ二つ ── 何も無いところなら「その日・その時刻に予定を足す」、予定の
 * 上なら「それを直す」（この Mac の予定はタイトルと削除、ノートの予定は
 * そのノートを開く、よそとチームは読むだけ）。二度押しはすぐ、右押しは
 * メニューを出してから。
 */

/// 押した場所が指す日と時刻（`{ day, at }`・at は無いこともある）。
function calSpotAt(e) {
    const lane = e.target.closest('.lane');
    if (lane) {
        const r = lane.getBoundingClientRect();
        const m = Math.max(0, Math.min(23 * 60 + 45, Math.floor((e.clientY - r.top) / HOUR_PX * 4) * 15));
        return { day: lane.dataset.day, at: String(Math.floor(m / 60)).padStart(2, '0') + ':' + String(m % 60).padStart(2, '0') };
    }
    const track = e.target.closest('.crowd .track');
    if (track) {
        if (calView === 'day') {
            const r = track.getBoundingClientRect();
            const m = Math.max(0, Math.min(23 * 60 + 45, Math.floor((e.clientX - r.left) / CROWD_HOUR * 4) * 15));
            return { day: calDay, at: String(Math.floor(m / 60)).padStart(2, '0') + ':' + String(m % 60).padStart(2, '0') };
        }
        const cell = e.target.closest('.crowd .cell');
        return { day: cell ? cell.dataset.day : calDay, at: '' };
    }
    const cell = e.target.closest('.d[data-day]');
    if (cell) return { day: cell.dataset.day, at: '' };
    const col = e.target.closest('.allday > div[data-day]');
    if (col) return { day: col.dataset.day, at: '' };
    return null;
}

/// 押した予定（`.ev` `.blk` `.ad` `.bar` `.chip` `.span`）。無ければ null。
function calItemAt(e) {
    const it = e.target.closest('.ev, .blk, .ad, .crowd .bar, .crowd .chip, .crowd .span');
    if (!it) return null;
    const kind = ['here', 'away', 'team', 'note', 'once', 'repeat'].find((k) => it.classList.contains(k)) || '';
    return { el: it, kind, at: it.dataset.at || '' };
}

/// 押した予定を直す（この Mac の予定）か、開く（ノート）か、読むだけと言うか。
async function calEditItem(item, at) {
    if (item.kind === 'here' && item.at) { await hereEdit(item.at); return; }
    if (item.at) { calShut(); await openNote(item.at); return; }
    const got = evOf(item.el);
    if (got) { popEvent(got, at || { x: 300, y: 300 }); return; }
    say(item.kind === 'team' ? 'チームの予定表は読むだけです' : 'よその予定表のものなので、ここでは直せません');
}

el('cal').addEventListener('dblclick', async (e) => {
    const item = calItemAt(e);
    if (item) { e.preventDefault(); await calEditItem(item, { x: e.clientX, y: e.clientY }); return; }
    const spot = calSpotAt(e);
    if (!spot) return;
    e.preventDefault();
    calDay = spot.day;
    await calAdd(spot.day, spot.at || undefined);
});

el('cal').addEventListener('contextmenu', async (e) => {
    const at = { x: e.clientX, y: e.clientY };
    // 人の名前の上 ── その人の色を選ぶ（依頼 498）。
    const who = e.target.closest('.crowd .rows .who');
    if (who && who.dataset.key) {
        e.preventDefault();
        const key = who.dataset.key;
        const now = who.closest('.ln').style.getPropertyValue('--lane').trim();
        const 名 = who.textContent.trim();
        const rows = [
            { name: '上へ', run: () => moveLane(key, -1) },
            { name: '下へ', sep: true, run: () => moveLane(key, 1) },
        ];
        // **呼び名を変える**（依頼 563）── 書き出す側が「予定表」としか
        // 言わないことがある。**空にすれば元の名前に戻る。**
        rows.push({
            name: '呼び名を変える',
            sub: calNames[key] ? 'いまは「' + calNames[key] + '」' : '空にすると元に戻ります',
            sep: !key.startsWith('team:'),
            run: async () => {
                const got = await askText('この段の呼び名', calNames[key] || 名,
                    '空にすると、書き出された名前に戻ります');
                if (got === null) return;
                const to = got.trim();
                calNames = { ...calNames };
                if (to && to !== 名) calNames[key] = to; else delete calNames[key];
                window.amber.remember({ calNames });
                await drawCal();
            },
        });
        // **「自分はこの人」はチームの段だけ**（依頼 562）── ほかの段は
        // もともと自分のものなので、選ばせても意味が無い。
        if (key.startsWith('team:')) {
            rows.push({
                name: calMe === key ? '自分の指定をやめる' : '自分はこの人',
                sub: calMe === key ? '日・週・月に全員ぶんが戻ります'
                    : '日・週・月は、この人の予定だけになります',
                sep: true,
                run: async () => {
                    calMe = calMe === key ? '' : key;
                    window.amber.remember({ calMe });
                    say(calMe ? 名 + ' を自分にしました（日・週・月はこの人の予定だけ）'
                        : '自分の指定をやめました');
                    await drawCal();
                },
            });
        }
        popMenu(rows.concat(CAL_COLORS.filter(([h]) => LANE_COLORS.includes(h)).map(([h, n]) => ({
            name: (h === now ? '● ' : '　 ') + n + ' ── ' + 名,
            run: async () => { calColors = { ...calColors, [key]: h }; window.amber.remember({ calColors }); await drawCal(); },
        }))), at);
        return;
    }
    const item = calItemAt(e);
    const spot = calSpotAt(e);
    if (!item && !spot) return;
    e.preventDefault();
    const rows = [];
    if (item) {
        if (item.kind === 'here' && item.at) {
            rows.push({ name: '予定を修正する', run: () => hereEdit(item.at) });
            rows.push({ name: '予定を削除する', run: async () => {
                if (!await askYes('この予定を削除しますか')) return;
                const got = await window.amber.cal(['drop', item.at]);
                if (!got || got.error) { say('削除できません: ' + (got?.error || '返事がありません')); return; }
                await drawCal();
            } });
        } else if (item.at) {
            rows.push({ name: 'ノートを開く', run: () => calEditItem(item) });
        } else {
            rows.push({ name: item.kind === 'team' ? 'チームの予定（読むだけ）' : 'よその予定（読むだけ）', dim: true });
        }
    }
    const day = spot ? spot.day : calDay;
    const when = spot && spot.at ? dayName(day) + ' ' + spot.at : dayName(day);
    rows.push({ name: when + ' に予定を追加', sep: rows.length > 0, run: () => { calDay = day; return calAdd(day, spot && spot.at ? spot.at : undefined); } });
    popMenu(rows, at);
});

/* ── 表示の設定（依頼 494）── どの予定表を出すか・土日を出すか ── */

/// **招待された側が、グループカレンダーを見つける**（依頼 538）。
///
/// 作った端末は名前を憶えているが、**招待された人の amber は何も知らない** ──
/// カレンダーは端末に降りてくるのに、amber から見ると「よその予定表」と
/// 見分けが付かず、絞り込みも色分けも出ない。
///
/// 名前が `ambər グループ` で決まっているので、**端末の予定表にその名前が
/// あれば見つけられる**。ノートの共有で決めた「受け取る側は自動で見つける」
/// と同じ形 ── 人に選ばせない。
///
/// **訊くのは一度だけ。** 断った人に毎回訊かない。
async function findGroupCal() {
    // 会社向けのビルドには、グループという話そのものが無い（依頼 602）。
    if (OFFICE) return;
    if (groupCal || groupAsked || !hereOn) return;
    let names = [];
    try {
        const got = await window.amber.cal(['calendars']);
        names = (got && got.calendars) || [];
    } catch { return; }
    if (!names.includes(GROUP_NAME)) return;
    groupAsked = true;
    window.amber.remember({ groupAsked: true });
    if (!await askYes('「' + GROUP_NAME + '」というカレンダーが見つかりました。\n\n'
        + 'グループカレンダーとして使いますか。\n'
        + 'このカレンダーの予定が、グループの予定として色分けされます。')) return;
    // **id は持たない。** 招待された側は持ち主ではないので、Google 側の id を
    // 知らない ── 招待も削除もできない（持ち主の端末からやってもらう）。
    groupCal = { id: '', name: GROUP_NAME };
    window.amber.remember({ group: groupCal });
    await drawCal();
    say('「' + GROUP_NAME + '」をグループカレンダーとして使います');
}

/// 予定表の一覧。この Mac の予定表（無ければ空）・よその予定表・チーム・自分のノート。
async function calSources() {
    const out = [{ key: 'me', name: '自分のノート（日付を書いたノート）' }];
    if (hereOn) {
        const got = await window.amber.cal(['calendars']);
        for (const c of (got && got.calendars) || []) out.push({ key: 'here:' + c, name: c + '（このパソコン）' });
    }
    for (const a of away) out.push({ key: 'away:' + a.name, name: a.name + '（カレンダー設定で足したもの）' });
    for (const w of teamPeople) {
        const key = 'team:' + (w.mail || w.name);
        out.push({ key, name: laneName(key, w.name) + '（チーム）' });
    }
    // 隠しているのに一覧に無いもの（もう無い予定表）も出す ── 戻せないと困る。
    for (const k of calHide) if (!out.some((x) => x.key === k)) out.push({ key: k, name: k.replace(/^[a-z]+:/, '') });
    return out;
}

async function cmdCalSettings() {
    for (;;) {
        const rows = (await calSources()).map((c) => ({
            name: (calHide.includes(c.key) ? '　　' : '✓　') + c.name,
            sub: calHide.includes(c.key) ? '出していません' : '',
            value: c.key,
        }));
        rows.push({ name: (calWeekend ? '✓　' : '　　') + '土日表示', value: '*weekend', sub: calWeekend ? '' : '月〜金だけ出しています' });
        rows.push({ name: 'カラー設定 ── ' + (calHereColor ? colorName(calHereColor) : '緑（既定）'), value: '*color', sub: '押すと選べます' });
        rows.push({ name: '出す時間帯 ── ' + calFrom + '時 〜 ' + calTill + '時', value: '*hours',
            sub: '日・週・並べて に出す幅。狭いほど一つ一つが読みやすくなります' });
        const pick = await askPick('カレンダー表示設定', rows, '押すと出し入れできます。閉じるまで続けて選べます', true);
        if (pick === null) break;
        if (pick === '*weekend') { calWeekend = !calWeekend; window.amber.remember({ calWeekend }); continue; }
        if (pick === '*hours') {
            // **始まりを決めて、終わりを決める。** 一度に両方訊くダイアログを
            // 作らない ── 選ぶものが二つあるダイアログは、押す前に何が起きるか
            // 分からない。終わりは始まりより後だけを出す。
            const hh = (n) => ({ name: n + '時', value: String(n) });
            const a = await askPick('出す時間帯 ── 何時から', Array.from({ length: 13 }, (_, i) => hh(i))
                .map((r) => (Number(r.value) === calFrom ? { ...r, name: '● ' + r.name } : { ...r, name: '　 ' + r.name })), '', true);
            if (a === null) continue;
            const from = Number(a);
            const b = await askPick('出す時間帯 ── 何時まで',
                Array.from({ length: 24 - from }, (_, i) => hh(from + i + 1))
                    .map((r) => (Number(r.value) === calTill ? { ...r, name: '● ' + r.name } : { ...r, name: '　 ' + r.name })), '', true);
            if (b === null) continue;
            calFrom = from;
            calTill = Number(b);
            window.amber.remember({ calFrom, calTill });
            continue;
        }
        if (pick === '*color') {
            const c = await askPick('カラー設定（個人カレンダーの色）', CAL_COLORS.map(([h, n]) => ({
                name: (h === (calHereColor || '#2f8a52') ? '● ' : '　 ') + n, value: h,
            })), '', true);
            if (c !== null) { calHereColor = c; window.amber.remember({ calHereColor }); paintHereColor(); }
            continue;
        }
        calHide = calHide.includes(pick) ? calHide.filter((k) => k !== pick) : calHide.concat(pick);
        window.amber.remember({ calHide });
    }
    if (calOn) await drawCal();
}

/* ── よその予定表（依頼 456） ── */

/// 購読している予定表（`[{ url, name }]`）。**この環境の中だけに持つ。**
///
/// アドレスは、それを知っている人が予定を全部読めるもの ── キーと同じ扱い。
/// ノートにも `.amber/` にも書かない（あそこはフォルダと一緒に旅をする）。
let away = [];
/// 取ってきた予定（月ごとに憶える。閉じれば消える）。
let awaySlots = [];

/// 予定表を一つ増やす。
async function cmdSubscribe() {
    const url = await askText('カレンダー設定追加', '',
        'Google カレンダーなら「設定 → カレンダーの統合 → 非公開 URL（iCal 形式）」');
    if (url === null || !url.trim()) return;
    say('取りに行っています…');
    const got = await window.amber.fetchPage(url.trim(), 'calendar');
    if (!got || got.error) { say('インポートできません: ' + (got?.error || '返事がありません')); return; }
    let name = '';
    try {
        const out = await window.amber.call('ics', {
            text: got.html, year: 2026, month: 1,
        });
        name = out.name || '';
    } catch (e) {
        say('予定表ではないようです: ' + why(e));
        return;
    }
    away = away.filter((a) => a.url !== url.trim());
    away.push({ url: url.trim(), name: name || readableUrl(url.trim()).slice(0, 40) });
    window.amber.remember({ away });
    say('「' + (name || 'よその予定表') + '」を読むようにしました');
    if (!el('cal').hidden) await drawCal();
}

/// 購読しているものを見て、やめる。
async function cmdUnsubscribe() {
    if (!away.length) { say('読んでいる予定表はありません'); return; }
    const pick = await askPick('カレンダー設定解除',
        away.map((a) => ({ name: a.name, sub: readableUrl(a.url).slice(0, 60), value: a.url })),
        '選ぶと、読むのをやめます（向こうの予定表は何も変わりません）', true);
    if (pick === null) return;
    away = away.filter((a) => a.url !== pick);
    window.amber.remember({ away });
    say('読むのをやめました');
    if (!el('cal').hidden) await drawCal();
}

/// その月ぶんを、購読しているところから取ってくる。
///
/// **一つ取れなくても、ほかは出す。** ネットワークの向こうの都合で全部が出ないのは、
/// カレンダーとして使いものにならない。
async function awayFor(y, m) {
    const out = [];
    for (const a of away) {
        try {
            const got = await window.amber.fetchPage(a.url, 'calendar');
            if (!got || got.error) continue;
            const rows = await window.amber.call('ics', { text: got.html, year: y, month: m });
            for (const r of rows.days || []) out.push({ ...r, from: a.name });
        } catch { /* この一つは飛ばす */ }
    }
    return out;
}


/// この環境の予定表（`here`）が使えるか。**訊いたことがあるか**も憶える
/// ── 断られたあとに毎回訊きなおすのは、いちばん嫌われる。
let hereOn = null;

/// 許可を訊く。**開いたときに一度だけ** ── 起きた瞬間に訊くと、何のために
/// 訊かれたのか分からないまま断られる。
async function hereAsk() {
    if (hereOn !== null) return hereOn;
    const got = await window.amber.cal(['ask']);
    hereOn = !!(got && got.ok);
    return hereOn;
}

/// ひと月ぶん。**読めなくても、ほかは出す**（よその予定表と同じ扱い）。
async function hereFor(y, m) {
    if (!hereOn) return [];
    const got = await window.amber.cal(['month', y, m]);
    if (!got || got.error) return [];
    return (got.days || []).map((r) => ({
        day: r.day, at: r.at, title: r.title,
        // **パスの代わりに、OS の言う名札を持つ** ── 直すときに要る。
        path: r.id, kind: 'here', place: r.place, from: r.from,
    }));
}

/* ── デスクトップ版ができること、ひとつの表 ── */

/// **パレットも ⋯ のメニューも、ここを見る。**
///
/// 二か所に書くと、片方にだけ増えた命令ができて、そのうち「あるはずなのに
/// 無い」になる。`need` は要るもの: `note` は開いているノート、`root` は
/// 保存場所（いつもある）。`menu` が真なら、⋯ のメニューにも出る。
/// **合言葉。** これを打つまで、チームの予定表の二行はどこにも出ない
/// （依頼 473）。日本語で打つものにしないのは、変換の途中で偶然出て
/// しまわないため ── ここは「打とうと思った人だけ」が通る道。
const TEAM_WORD = 'csv';

/// **会社向けのビルドでは、外のネットワークに触るものを出さない**（依頼 602・本人
/// 「会社でビルドする際には、『同期』に関する機能や表示はすべてクローズに
/// したい」「カレンダーもフォルダも同期っていう概念は会社のビルドに不要だ」）。
///
/// 閉じるのは**外へ運ぶもの**だけ ── Google Drive の同期、iCal の購読
/// （＝カレンダーの同期）、グループでの共有。**カレンダーの画面そのものと、
/// チームの CSV は残す** ── あれは会社の Outlook のためのパスで、外へは
/// 何も出さない（読むだけ）。
///
/// 出さないだけでなく**走らせない** ── 押せない道具が並ぶより、無いほうがいい。
let OFFICE = false;
const officeReady = (async () => {
    try {
        OFFICE = (await window.amber.edition()) === 'office';
    } catch {
        OFFICE = false;                       // 版を訊けない机では、通常のビルド
    }
    if (OFFICE) document.body.classList.add('office');
    return OFFICE;
})();

const CMDS = [
    { id: 'new', name: '新しいノート', key: '⌘N', run: () => cmdNewNote() },
    { id: 'tmpl', name: 'テンプレートから新しいノート', sub: '「' + TEMPLATES + '」フォルダの中身',
      run: cmdTemplate },
    { id: 'clip', name: 'Web からインポート', sub: 'URL を渡すと、一件のノートに', run: cmdClip },
    { id: 'outside', name: 'ほかの場所のノートを開く', key: '⌘O', app: true,
      run: cmdOpenOutside },
    // **`⌘S` は「現状バージョン保存」が持っている**（受け口は捕捉の段）。
    // ここにも同じキーを書くと、一覧に二つ並んで、どちらが走るのか
    // 画面が答えられない。ふだんは打てば勝手に保存される。
    { id: 'save', name: '保存', sub: '打てば自動でも保存されます', need: 'note', run: () => save() },
    { id: 'read', name: '表示 / コードを入れ替え', key: '⌘E', need: 'note', run: () => toggleRead() },
    { id: 'split', name: '並べて表示', key: '⌘P', need: 'note', run: () => toggleSplit() },
    // **⚙ にも出す。** 鍵（`⌘/`）を覚えていない人には、畳むパスがどこにも
    // 無かった ── Inkdrop はメニューに並べている。
    { id: 'rail', name: '左の列を畳む', key: '⌘/', app: true, run: () => toggleRail() },
    { id: 'list', name: '一覧を畳む', key: '⌘⌥/', app: true, run: () => toggleList() },
    { id: 'find', name: 'ノートを探す', key: '⌘F', run: () => openFind() },
    // **絞り込みは、命令ではなくなった。** タグ・フォルダ・期間の3 つは
    // 一覧の頭に引き出しとして常に出ている ── 命令の表から呼ぶものが
    // 別にあると、同じことを頼むパスが二つになる。
    // **⚙ には出さない**（本人・2026-09-11）── 左の列にカレンダーが居る。表には残す。
    { id: 'cal', name: 'カレンダー', sub: '予定と、その日のノートを 1 つで', run: cmdCalendar },
    { id: 'sub', name: 'カレンダー設定追加', sub: 'Google カレンダーなどの iCal の URL を読みます',
      app: true, net: true, run: cmdSubscribe },
    { id: 'unsub', name: 'カレンダー設定解除', app: true, net: true, run: cmdUnsubscribe },
    { id: 'calset', name: 'カレンダー表示設定', sub: 'どの予定表を出すか・土日を出すか', app: true, run: cmdCalSettings },
    // **合言葉を打つまで、どこにも出ない**（依頼 473）。
    //
    // 会社の Outlook は外から読める形を一つも出さないので、別の道具が
    // 置いた CSV を読む（依頼 471）。読むだけで、取りに行くことも書き戻す
    // こともしない ── ただ、初めて amber を見た人にこの二行が並んで
    // いると、何の話なのかが分からないまま画面食らう。
    //
    // 出し方: 「何をしますか」（⌘⇧P）で `csv` と打つ。合言葉は
    // `TEAM_WORD` の一行 ── 変えたければそこを変える。
    { id: 'team', name: 'チームの予定表を読む（CSV）', word: TEAM_WORD,
      sub: '別の道具が書き出したファイルを、人ごとに並べます', run: cmdTeam },
    { id: 'teamoff', name: 'チームの予定表を読むのをやめる', word: TEAM_WORD,
      run: cmdTeamOff },
    { id: 'when', name: '期間で絞る', run: () => openDrawer('when') },

    // ── このノートにすること（⋯ と、ノートの右押し）
    { id: 'star', name: 'ブックマークに登録する', key: '⌘D', need: 'note', menu: true, run: cmdStar },
    { id: 'tags', name: 'タグ設定', need: 'note', menu: true, run: cmdTags },
    { id: 'move', name: 'フォルダへ移動', need: 'note', menu: true, run: cmdMove },
    { id: 'toshare', name: 'グループと共有する', need: 'note', menu: true, net: true, run: cmdToShare },
    // **メニューには出さない。** 上の帯にベルが居て、押せば同じダイアログが出る
    // ── 同じことを頼むパスが二つあると、片方を直した日にもう片方が
    // 古いまま残る。表には残す（⌘⇧P から名前で探せる）。
    { id: 'remind', name: '通知設定', need: 'note', run: cmdRemind },
    { id: 'dup', name: '複製', sub: '同じ中身のノートをもう一つ', need: 'note', menu: true,
      run: cmdDup },
    { id: 'totmpl', name: 'このノートをテンプレートにする', sub: '「' + TEMPLATES + '」フォルダへ写します',
      need: 'note', menu: true, run: cmdToTemplate },
    { id: 'export', name: 'エクスポート', need: 'note', menu: true, run: cmdExport },
    // **題の右押しにしか無かった四つ**（依頼 612・本人「どちらも同じ
    // ポップアップにできる？」）── ここへ移した。命令の表に置けば、
    // 一覧の右押しからも ⋯ からも ⌘⇧P からも同じものが出る。
    { id: 'rename', name: 'タイトルを直す', need: 'note', menu: true, run: renameTitle },
    { id: 'copyname', name: 'ファイル名を写す', need: 'note', menu: true,
      run: () => copyText(baseOf(state.open.path), 'ファイル名') },
    { id: 'copypath', name: '場所をコピー', need: 'note', menu: true,
      run: () => copyText(state.open.path, '保存場所') },
    { id: 'reveal', name: MAC ? 'Finder で表示' : 'エクスプローラーで表示',
      need: 'note', menu: true, run: () => window.amber.reveal(state.open.path) },
    // **名前で出す。** 前は帯に ☰ と ⤢ が並んでいたが、どちらが目次で
    // どちらが拡大かは記号のどこにも書いていない ── 帯の幅を食っていた
    // うえ、押してみるまで分からなかった。
    { id: 'toc', name: '目次', key: '⌘⇧O', need: 'note', menu: true, run: () => toggleToc() },
    { id: 'closetab', name: 'このタブを閉じる', need: 'note', menu: true,
      run: () => closeTab(showing) },
    { id: 'zen', name: 'ノートだけを大きく', key: 'F12', need: 'note', menu: true, run: () => setZen(!zen) },
    { id: 'delete', name: 'ゴミ箱へ入れる', need: 'note', menu: true, sep: true, run: cmdDelete },

    // ── amber のこと（⚙）
    // **キーは「ショートカット一覧」へ渡した。** ここに `⌘⇧/` と書いて
    // ありながら、受け口はどこにも無く、押すと左の列が畳まれていた
    // ── メニューが嘘をついていた。
    { id: 'keys', name: 'ショートカット一覧', key: '⌘⇧/', app: true, run: cmdKeys },
    { id: 'syntax', name: 'マークダウンの書き方', app: true, run: cmdSyntax },
    { id: 'theme', name: 'テーマ', app: true, run: cmdTheme },
    { id: 'autosave', name: '自動保存', app: true, run: cmdAutoSave },
    { id: 'vim', name: 'vimモード', app: true, run: cmdVim },
    { id: 'lineno', name: '行番号', app: true, run: cmdLineNo },
    // ── ノートを入れる／出す
    // **入れる三つを、並べて置く。** 「サンプルのノートを入れる」は列の
    // いちばん下に一つだけ離れて座っていて、探す人は「amber について」の
    // 下まで来ない ── 同じ行い（ノートを入れる）は同じ場所に。
    // エクスポートと対の言葉（本人・2026-09-12）。
    { id: 'bring', name: 'インポート', sub: 'ほかの .md をノートに', app: true, sep: true, run: cmdBring },
    // **OneNote は「インポート」の隣**（依頼 621・本人に見取り図を見せて通った）。
    // 会社向けのビルドにも出す ── ネットワークに出ず、手元の `.onepkg` を読むだけ。
    { id: 'onenote', name: 'OneNote を取り込む', sub: 'OneNote が書き出した .onepkg / .one から', app: true, run: cmdOneNote },
    { id: 'welcome', name: 'サンプルのノートを入れる', app: true, run: cmdWelcome },
    { id: 'spare', name: '使われていない画像', app: true,
      sub: 'どのノートも使っていない画像を、選んでゴミ箱へ', run: cmdSpare },
    { id: 'backup', name: 'バックアップ', app: true, run: cmdBackup },
    { id: 'restore', name: 'バックアップから戻す', app: true, run: cmdRestore },
    // **足す・変える・外す・同期先、を一つの入口で**（依頼 511・本人「保存
    // ディレクトリを追加・変更・削除っていう表現で全部できるようにしない？」）。
    { id: 'places', name: '保存ディレクトリの追加・変更・削除', app: true, run: cmdPlaces },
    // **作るパスがあるなら、やめるパスもある**（依頼 535）。押す場所は ⚙ ──
    // 一生に一度で、戻せない操作なので、毎日押すものの隣には置かない。
    { id: 'groupdrop', name: 'グループカレンダーを削除する', app: true, need: 'group', net: true, run: cmdDropGroup },
    { id: 'sync', name: '同期', app: true, net: true, sub: '同期していません', run: cmdSync },
    { id: 'refresh', name: '読み直す', key: 'F5', app: true, sub: 'フォルダをもう一度読みます', run: cmdRefresh },
    { id: 'all', name: 'コマンド一覧', key: '⌘⇧P', app: true, sep: true, run: () => palette() },
    { id: 'about', name: 'ambər について', app: true, run: cmdAbout },
    // 錠（依頼 629）。**押す場所はノートのメニュー** ── 開いているノートに
    // することなので、⚙（amber についての設定）ではない。
    { id: 'lock', name: 'このノートをロックする', sub: 'うっかり書き換えないように',
      need: 'note', menu: true, run: () => cmdLock(true) },
    { id: 'unlock', name: 'このノートのロックをやめる', need: 'note', menu: true, run: () => cmdLock(false) },
    { id: 'history', name: '過去バージョン', need: 'note', menu: true, run: () => cmdHistory() },
    { id: 'keepnow', name: 'いまのバージョンを保護', key: '⌘S', need: 'note', menu: true, run: cmdKeepNow },
    // **`back` / `fwd` は上の「前に見たノート」で使っている。** 同じ id を
    // 二つ置くと、パレットから選んだときに先に見つかったほうが走る。
    { id: 'undo', name: '一つ戻す', key: '⌘Z', need: 'note', run: () => stepBack(false) },
    { id: 'redo', name: 'やり直す', key: '⌘⇧Z', need: 'note', run: () => stepBack(true) },

    // ── 表には要るが、メニューには出さないもの
    { id: 'mkbook', name: '新しいフォルダを作る', run: () => cmdMkBook() },
    { id: 'color', name: 'フォルダに色をつける', sub: 'フォルダを右押しでも', run: () => cmdColor() },
    { id: 'tablist', name: '開いているノートの一覧', sub: 'タブが多いときに、名前で選ぶ', run: () => cmdTabList() },
    { id: 'bigger', name: '文字を大きく', key: '⌘+', run: () => setFont(fontStep + 1) },
    { id: 'smaller', name: '文字を小さく', key: '⌘−', run: () => setFont(fontStep - 1) },
    { id: 'font0', name: '文字の大きさを戻す', key: '⌘0', run: () => setFont(0) },
];

/// 命令の表にも、道具の帯にも出ない一打。**ここにしか無い鍵。**
///
/// 一覧を上下する ↑↓ も、表の中の Tab も、押してみるまで分からなかった。
/// 覚えていなくてよいものにするには、まず**どこかに書いてある**必要がある。
const LOOSE_KEYS = [
    ['一覧を上下する', '↑ ↓ / J K', '一覧を見ているとき'],
    ['そのノートを開いて打つ', 'Enter', '一覧を見ているとき'],
    ['ゴミ箱へ入れる', 'Delete', '選んでいるノートを（確認してから）'],
    ['ノートを探す', '/', '一覧を見ているとき'],
    ['閉じる・やめる', 'Esc', 'ポップアップ・ツール・大きい画面から'],
    ['次のマスへ', 'Tab', '表の中で（⇧Tab で前へ、最後で押すと行が増える）'],
];

/// ショートカットの一覧（⌘⇧/）。**探せれば、覚えなくていい。**
///
/// 「どうやって確かめるの」と訊かれた ── キーは帯の吹き出しと ⋯ のメニューに
/// 散らばっていて、**全部を一度に見る場所が無かった**。ここは 1 つにして、
/// 選べばその場で走る（読むだけの紙にすると、見ながら手で打ち直すことに
/// なる）。
async function cmdKeys() {
    const rows = [];
    const put = (name, key, sub, run) => rows.push({ name, key, sub, run });

    rows.push({ name: '── 書く道具（記号）', sub: '「表示」でも「コード」でも', head: true });
    for (const [name, key, run] of MARKS.flat()) {
        if (name === '|' || !run) continue;
        // 見出しだけ、キーとボタンで振る舞いが違う ── 一打はその深さに直し、
        // ボタンは押すたびに深くなる。一覧では両方言う。
        if (name === '見出し') { put(name, '⌘1 ⌘2 ⌘3 ⌘4', '一回でその深さに（下のボタンは押すたび深く）', run); continue; }
        put(name, key || '', key ? '' : '下のボタンから', run);
    }
    put('マークダウンの書き方', '', '記号そのものを見る', cmdSyntax);

    rows.push({ name: '── デスクトップ版のこと', sub: 'どこを打っていても効きます', head: true });
    for (const c of CMDS) {
        if (!c.key || !canRun(c)) continue;
        put(c.name, keyText(c.key), '', () => c.run());
    }

    rows.push({ name: '── そのほか', head: true });
    for (const [name, key, sub] of LOOSE_KEYS) put(name, key, sub);

    const pick = await askPick('ショートカット一覧',
        rows.map((r, n) => ({ name: r.name, sub: r.sub || '', key: keyText(r.key), head: r.head, value: n })),
        '選ぶと、その場で動きます');
    if (pick === null) return;
    const hit = rows[pick];
    if (hit && hit.run) await hit.run();
}

const canRun = (c) => (c.need !== 'note' || !!state.open)
    // グループを持っていない人に「消す」を出さない（依頼 535）。
    // 「削除」は持ち主だけ ── 招待された側は消せない（消すのは持ち主の仕事）。
    && (c.need !== 'group' || !!(groupCal && groupCal.id))
    // **会社向けのビルドでは、外へ運ぶものを一つも出さない。**
    // 表（⌘⇧P）からも消える ── 名前で探せてしまうなら、閉じたことにならない。
    && !(OFFICE && c.net);

/// 命令のパレット（⌘⇧P）。**名前で探せれば、覚えなくていい。**
///
/// **命令だけではない**（依頼 414）。ここに来る人が探しているのは
/// 「やること」ではなく「行き先」のことが多い ── ノート・フォルダ・タグ・
/// いま開いているノートの見出し。名前を打てば全部ここに出る、が
/// いちばん覚えることが少ない（Inkdrop も VS Code もそうしている）。
///
/// **一つの箱に混ぜる。** 種類ごとに別のデスクトップ版を割り当てると、打つ前に
/// 「これは何を探すところか」を思い出す仕事が増える ── 打った文字が
/// どれに当たるかは、こちらが数えればよい。
///
/// **並びは、近いものから。** 命令 → いま開いているノートの見出し →
/// ノート → フォルダ → タグ。見出しが上なのは、いま見ているものの中の
/// 話だから ── 手元の話が画面の下にあると、そこまで目が降りない。
async function palette() {
    const rows = [];
    const head = (name, sub) => rows.push({ name, sub, head: true });

    head('── すること');
    // カレンダーは左の列のいちばん上にある ── ここには出さない（本人・2026-09-12）。
    for (const c of CMDS.filter((c) => canRun(c) && c.id !== 'cal')) {
        rows.push({
            name: c.name, key: keyText(c.key), run: () => c.run(),
            // 合言葉のあるものは、打たれるまで出てこない（依頼 473）。
            word: c.word,
        });
    }

    // 「このノートの見出し」と「ノートを開く」の段は出さない（本人・2026-09-12）──
    // 見出しは目次（⌘⇧O）、ノートは一覧の探す欄が受け持つ。

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
        rows.map((r, n) => ({
            name: r.name, sub: r.sub || '', key: r.key || '', head: r.head,
            word: r.word, value: n,
        })),
        '↑↓ で選び、Enter で決定');
    if (pick === null) return;
    const hit = rows[pick];
    if (hit && hit.run) await hit.run();
}

/// ⋯ のメニュー。**パレットと同じ表の、ノートに関わるところだけ。**
/// `which` が `app` なら設定のメニュー、既定はノートのメニュー。
///
/// **「見た目」をノートの右押しに出さない。** ノートを右押しした人が
/// 訊いているのは「このノートをどうするか」で、アプリの色ではない。
function openMenu(at, which) {
    const key = which === 'app' ? 'app' : 'menu';
    const items = CMDS.filter((c) => c[key] && canRun(c)).map((c) => {
        // **いまどうなっているかを、押す前に見せる。**
        // **「いま:」とは書かない。** メニューに出ている値は、これから選ぶ値では
        // なく**いまの値**しかありえない ── 一行ごとに同じ二文字を読ませる
        // 意味が無い。入切は「オン / オフ」で揃える（片方だけ「出している」
        // のような言い方をすると、同じ形の設定が二つの語彙を持つ）。
        if (c.id === 'theme') return { ...c, sub: themeName() };
        if (c.id === 'vim') return { ...c, sub: vimOn ? 'オン' : 'オフ' };
        if (c.id === 'lineno') return { ...c, sub: lineNo ? 'オン' : 'オフ' };
        if (c.id === 'rail') return { ...c, name: railOff ? '左の列を出す' : '左の列を畳む' };
        if (c.id === 'list') return { ...c, name: listOff ? '一覧を出す' : '一覧を畳む' };
        if (c.id === 'autosave') return { ...c, sub: autoSave ? '入 ── 打てば保存されます' : '切 ── 「保存」を押したときに書きます' };
        if (c.id === 'places') {
            return { ...c, sub: manyPlaces() ? state.places.map((p) => p.name).join('・') : shortPath(state.root) };
        }
        if (c.id === 'sync') return { ...c, sub: syncLabel() };
        if (c.id === 'toshare') {
            if (state.open && state.open.shared) {
                // **押す前に、どこへ戻るかを言う。** 「いちばん上へ」と
                // 出しておいて別のフォルダへ入るのは、黙って動かすのと同じ。
                const home = homeOf(state.open);
                return { ...c, name: 'グループとの共有をやめる',
                    sub: home ? '「' + home.split('/').pop() + '」へ戻します' : 'いちばん上へ戻します' };
            }
            const to = state.shares[0];
            return { ...c, sub: to
                // **無ければ作る。** 「共有する」を押した人に、その前に
                // 「フォルダを作る」を押させない。
                ? '「' + (to.at.split('/').pop() || 'すべて') + '」へ移します'
                : '「グループ」というフォルダを作って、そこへ移します' };
        }
        return c;
    });
    // 区切りは**マークの付いた命令の手前**に置く ── 「最後の一つの前」に
    // すると、命令が増えた日に区切りが勝手に動く。
    popMenu(items, at);
}

/// メニューを出す。**描くのも置くのも、ここ一つ。**
///
/// 前は三か所（⋯ のメニュー・左の列の右押し・色のメニュー）が同じことを書いて
/// いた ── 画面の外へはみ出さない直しを一か所に入れて、残り二つが古い
/// まま、が起こる形。右押しを十か所に増やすので、先に一本にする。
///
/// `items` は `{ name, sub, key, sep, dim, html, run }` の並び。`at` は
/// 押した場所（`{x, y}`）か、ボタンの四角（`{right, bottom}` ── 右端に揃える）。
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
/// **何が見出しかは core が決める**（`note::blocks`）── デスクトップ版が `#` を数え
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
                { name: '見出しをコピー', run: () => copyText(h.text, '見出し') },
            ], { x: e.clientX, y: e.clientY });
        };
    }
}

/// 見出しへ飛ぶ。編集画面ならその行へ、表示画面ならその見出しへ。
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

/// 選べるテーマ。3 つ目は「暗いか」── `null` は OS に訊く。
///
/// **名前で選んだら、選んだとおりに出す。** 「ayu-dark にしたのに昼は
/// 明るい」は、選んだことにならない。既定（空）だけが OS に従う。
/// **cian と同じ配色を、同じ順で**（依頼 495・本人「全テーマを全く同一に」）。
/// 琥珀の3 つは amber が育ててきたものなので頭に残す。そのあとに cian のデスクトップ版の
/// 3 つの装い（白磁・陰翳・端末譲り）と、cian-tui の十八の配色。表は
/// `palettes.js`（cian からの写し・`themes-test` が古くなれば鳴る）。
const THEMES = [
    ['', '琥珀 ── OS に合わせる', null],
    ['amber-light', '琥珀 ── 明るい', false],
    ['amber-dark', '琥珀 ── 暗い', true],
    ['hakuji', '白磁', false],
    ['inei', '陰翳', true],
    ['terminal', '端末譲り', true],
    ...CIAN_PALETTES.map((p) => [p.name, p.name, !lightColor(p.bg)]),
];
let theme = '';

/// amber のデスクトップ版が使う十五の変数。cian の色からの組み替えは `palettes.js` の
/// `amberVarsOf`（iPhone も同じ算数で `Palettes.swift` を作る）。
const THEME_VARS = ['--amber', '--amber-soft', '--amber-deep', '--bg', '--rail', '--list', '--paper',
    '--line', '--line-2', '--ink', '--ink-2', '--ink-3', '--sel', '--hover', '--brand-s'];
const themeVars = (name) => amberVarsOf(name);

/// いま暗いか。**Monaco と mermaid にも同じ答えを渡す** ── 別々に訊くと、
/// テーマを替えた日にエディタだけ前の明暗で残る。
function isDark() {
    const t = THEMES.find(([k]) => k === theme);
    return t && t[2] !== null ? t[2] : matchMedia('(prefers-color-scheme: dark)').matches;
}

function setTheme(name) {
    theme = name || '';
    const root = document.documentElement;
    for (const k of THEME_VARS) root.style.removeProperty(k);
    root.style.removeProperty('color-scheme');
    const got = theme ? themeVars(theme) : null;
    if (got) {
        // cian の配色は、変数を直に差す（琥珀の3 つは `index.html` のラベルで）。
        root.dataset.theme = 'cian';
        for (const [k, v] of Object.entries(got.vars)) root.style.setProperty(k, v);
        root.style.setProperty('color-scheme', got.light ? 'light' : 'dark');
    } else if (theme) {
        root.dataset.theme = theme;
    } else {
        delete root.dataset.theme;
    }
    window.amber.remember({ theme });
    if (window.monaco && editor) monaco.editor.setTheme(isDark() ? 'vs-dark' : 'vs');
    if (Mermaid) {
        Mermaid.initialize(mermaidOpts());
        // 図は初期化し直しただけでは色が変わらない ── 描き直す。
        if (view !== 'write') drawRead();
    }
}

/// いま出ているテーマの名前。メニューに添える。
function themeName() {
    const t = THEMES.find(([k]) => k === theme);
    return t ? t[1].split(' ── ')[0] + (t[1].includes('──') ? '・' + t[1].split('── ')[1] : '') : '琥珀';
}
/// 表に無い名前が憶えに残っていたら（消えた配色）、琥珀に戻す。
function knownTheme(name) {
    return THEMES.some(([k]) => k === name) ? name : '';
}

async function cmdTheme() {
    const at = await askPick('テーマ', THEMES.map(([k, n]) => ({
        name: n, sub: k === theme ? '● いま' : '', value: k,
    })), '琥珀は amber が育ててきたもの');
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
        // **「＋」も文字で書かない。** 全角の記号は行の高さも幅も文字に引かれて、
        // 段の見出しの隣で一つだけ大きく沈む ── マークは線で描く（「新しい
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
        const name = await askText('新しいブックマークグループの名前', '', '仕事/週次 と書けば階層になります');
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

/// 行き先を右押ししたときのメニュー。**フォルダ・タグ・ブックマークを、名前ごと直す。**
function railMenu(kind, what, at) {
    if (!what) return;
    if (kind === 'place') {
        // 保存ディレクトリの右押しは、⚙ のダイアログと同じ四つ＋フォルダ作り（依頼 511）。
        const p = state.places.find((x) => x.dir === what);
        if (!p) return;
        popMenu([
            { name: 'この中にフォルダを作る', run: () => cmdMkBook(what) },
            { name: '過去バージョン', sub: 'この中のノートすべて', run: () => cmdHistory(what, true) },
            { name: '同期先', sub: SYNC_WORDS[p.sync], sep: true, run: () => placeSyncSheet(p) },
            { name: '名前を変える', run: () => placeRename(p) },
            { name: '場所を変える…', sub: shortPath(p.dir), run: () => placeMove(p) },
            { name: '外す', sub: 'ノートはそのまま残ります', run: () => placeDrop(p) },
        ], at);
        return;
    }
    const items = [];
    // **下の階層は、ここから作る。** 名前に「/」を打たせるのは、
    // 書き方を知っている人にしか通じない。
    if (kind === 'book') {
        items.push({ name: 'この中にフォルダを作る', run: () => cmdMkBook(what) });
        items.push({ name: 'フォルダに色をつける', run: () => cmdColor(what) });
        // 錠（依頼 629）── **中のノートとサブフォルダ全部**に効く。
        // **この列だけが目印を持つ** ── 上のフォルダの錠は、上で外す。
        const shut = state.locks.includes(what);
        const above = state.locks.find((l) => l !== what && what.startsWith(l + '/'));
        items.push({
            name: shut ? 'このフォルダのロックをやめる' : 'このフォルダをロックする',
            sub: above ? '「' + leafOf(above) + '」のロックが効いています'
                : (shut ? '' : '中のノートとサブフォルダも、まとめて'),
            dim: !!above && !shut,
            run: () => cmdLock(!shut, what),
        });
        const isShare = state.shares.some((sh) => sh.at === what);
        if (isShare) {
            items.push({
                name: 'グループへ招待',
                sub: 'クラウドの画面が開きます',
                run: () => window.amber.reveal(what),
            });
        }
        items.push({
            name: isShare ? 'グループとの共有をやめる' : 'グループと共有するフォルダにする',
            run: () => cmdShare(what, isShare),
        });
        // フォルダの履歴は、**中のノートの姿をまとめて時系列で** ──
        // 「あのあたりで壊した」は、どのノートかを覚えていないほうが多い。
        items.push({ name: '過去バージョン', sub: 'この中のノートすべて',
                     run: () => cmdHistory(what, true) });
    }
    if (kind === 'star') {
        items.push({ name: 'この中にブックマークグループを作る', run: () => newShelf(what) });
    }
    items.push({ name: '名前を変える', sep: items.length > 0, run: () => railRename(kind, what) });
    items.push({
        name: kind === 'book' ? 'このフォルダを削除'
            : (kind === 'tag' ? 'このタグを全部のノートから外す' : 'このブックマークグループを消す'),
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
    // フォルダは絶対のパスで来る ── 欄に出すのは保存ディレクトリからの相対。
    const shown = kind === 'book' ? relOf(what) : what;
    const to = await askText('新しい名前', shown,
        kind === 'book' ? '仕事/2026 と書けば階層になります' : '');
    if (to === null || !to.trim() || to.trim() === shown) return;
    const name = to.trim();
    const hit = underRail(kind, what);
    const root = kind === 'book' ? rootOf(what) : state.root;
    try {
        if (kind === 'book') {
            // **中のノートを一本ずつ移す。** フォルダはただのディレクトリで、
            // 名前を変えるのは中身を動かすこと ── 途中で止まっても、動いた
            // ぶんは新しい名前の下にちゃんと居る。
            await ask('mkbook', { dir: root + '/' + name });
            for (const n of hit) {
                const sub = (n.book || '').slice(what.length).replace(/^\//, '');
                const dir = root + '/' + name + (sub ? '/' + sub : '');
                await ask('mkbook', { dir });
                await ask('move', { path: n.path, dir, root });
            }
            await window.amber.trash(what);
        } else {
            for (const n of hit) await retagOne(n, kind, what, name);
            // フォルダは空でも core が憶えている ── 中のノートだけ直しても、
            // 前の名前のフォルダが並びに残る（**空のフォルダは名前を変えられない**）。
            if (kind === 'star') {
                // 下の階層ごと付け替える ── `drop` は下も一緒に忘れるので、
                // 先に新しい名前で作り直しておかないと孫のフォルダが消える。
                // フォルダの帳画面は保存ディレクトリごと ── 作るのはいちばん目、
                // 忘れるのはぜんぶ（どこの帳画面に居ても消えるように）。
                for (const sh of state.stars) {
                    if (sh !== what && !sh.startsWith(what + '/')) continue;
                    await ask('shelf', { path: state.root, name: name + sh.slice(what.length) });
                }
                await dropShelf(what);
            }
        }
        state.dest = { kind, what: kind === 'book' ? root + '/' + name : name };
        await reload({ quiet: true });
        say('「' + name + '」に変えました（' + hit.length + ' 件）');
    } catch (e) {
        say('変えられません: ' + why(e));
    }
}

/// ブックマークのグループを忘れる ── **ぜんぶの保存ディレクトリの帳画面から**。
/// どこか一つに残っていると、消したはずのグループが並びに戻ってくる。
async function dropShelf(name) {
    for (const p of state.places) {
        if (state.placeTrouble[p.dir]) continue;
        try { await ask('shelf', { path: p.dir, name, drop: true }); } catch { /* 無い帳画面もある */ }
    }
}

async function railDrop(kind, what) {
    const hit = underRail(kind, what);
    const what2 = kind === 'book' ? 'フォルダ' : (kind === 'tag' ? 'タグ' : 'ブックマーク');
    const ask2 = kind === 'book'
        ? '「' + bookLabel(what) + '」を、中の ' + hit.length + ' 件ごとゴミ箱へ入れますか'
        : kind === 'star'
            ? 'ブックマークグループ「' + what + '」を消しますか'
                + (hit.length ? '（中の ' + hit.length + ' 件はブックマークの直下へ）' : '')
            : '「' + what + '」の' + what2 + 'を ' + hit.length + ' 件から外しますか（ノートは残ります）';
    if (!await askYes(ask2)) return;
    try {
        if (kind === 'book') {
            const gone = await window.amber.trash(what);
            if (gone !== true) {
                say('ゴミ箱へ入れられません' + (gone && gone.why ? ': ' + gone.why : ''));
                return;
            }
        } else if (kind === 'star') {
            // **消したのはフォルダで、しおりではない。** 中に居たノートは
            // ブックマークの直下へ移す ── フォルダを片付けたつもりで、
            // 印まで一緒に消えるのは取り返しがつかない。
            for (const n of hit) await shelveOne(n, '');
            // **棚そのものも忘れる。** 保存場所は空でも残るように core が
            // 憶えている（`notebook::add_star`）── ノートから外すだけでは、
            // 中身の無いフォルダが並び続けて**消せないもの**になっていた。
            await dropShelf(what);
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
/// **開いているノートは、開いたまま直す。** 直接ファイルを書くと、デスクトップ版が
/// 持っている文字と食い違い、次の保存でどちらかが消える。
/// ノートを、指した保存場所へ移す（`''` はブックマークの直下）。
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
/// 保存場所を入れ替えない ── 一本開くたびに一覧が丸ごと変わると、
/// 「さっきまでのノートが消えた」に見える。並べても持たない ── 「フォルダが
/// そのまま索引」という前提の外にあるものを索引に混ぜると、索引が索引で
/// なくなる。
///
/// **異例な開き方だと、画面が言う。** 左の二列を出さず、上に帯を出す ──
/// 出さないと、いつもの一本と見分けが付かないまま別のフォルダへ書く。
/// 閉じれば、さっきまで見ていた一覧とノートが戻る。
let guestBack = null;

/// パスを、読める長さに。**真ん中を落とす** ── 頭（どこの家か）と
/// 末尾（何というファイルか）が、どちらも効く。
/// パスを、一行に収まる形に。
///
/// **Windows のパスも切る**（依頼 603・本人「保存ディレクトリの見た目が
/// 横に長くなりすぎない？」）── 前は `/` でしか割っていなかったので、
/// `C:\\Users\\t502960\\Documents\\OneNote` は**一つも切れずにまるごと**出ていた。
/// 同じ取りこぼしを `leafOf` でも踏んでいる（依頼 596）── Windows のパスを
/// 見るところは、`/` と `\\` の両方で割る。
///
/// 家の下なら頭を `~` に畳む（mac は `/Users/誰か`、Windows は
/// `C:\\Users\\誰か`）。それでも深いものは、頭二つと末尾二つを残して中を `…` に。
function shortPath(at) {
    const sep = /[\\/]/;
    const home = (state.root || '').match(/^(\/Users\/[^/]+|[A-Za-z]:\\Users\\[^\\]+)/i);
    let t = home && at.startsWith(home[1]) ? '~' + at.slice(home[1].length) : at;
    const part = t.split(sep);
    // 区切りは、そのパスが使っているほうに合わせて戻す（混ぜると別のパスに見える）。
    const mark = t.includes('\\') ? '\\' : '/';
    if (part.length > 5) {
        t = part.slice(0, 2).join(mark) + mark + '…' + mark + part.slice(-2).join(mark);
    }
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
    // 戻り先を憶える。**開くより先に。** 途中で失敗してもパスが残る。
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

/// `.md` をデスクトップ版に落としたら、同じ開き方をする。
///
/// **Electron 32 から `File.path` は無い。** `webUtils.getPathForFile` で
/// 訊く（preload の向こう側）── 描く側が勝手にファイルのパスを知れる口は
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
    if (!at) { say('このファイルの場所が分かりません'); return; }
    // 保存場所の中のものは、いつもの一本として開く ── 同じファイルが
    // 一覧と客の両方に居ると、どちらに書いたのか分からなくなる。
    if (placeOf(at)) {
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
/// iPhone で書いた一行はデスクトップ版を開き直すまで出てこなかった ── 同期はしていて、
/// 見ていなかっただけなのに「同期していない」ように見える。
///
/// 開いているノートは、**打っている途中なら触らない** ── いま書いている
/// ものを、向こうの版で黙って置き換えるのが一番悪い。打っていなければ
/// 静かに読み直す（保存のときの衝突検査は、そのまま残っている）。
let churn = null;
window.amber.onChanged((names) => {
    clearTimeout(churn);
    // **自分が書いたぶんで、フォルダを数え直さない。**
    //
    // 保存するとファイルが動くので、見張りが自分の書き込みで起きる ──
    // 保存の中で行を直したすぐあとに、1002 本を数え直していた（370ms）。
    // 動いたのが**いま自分で書いた一本だけ**なら、もう新しい。
    // 名前は `NFC` に揃えて比べる ── mac は濁点を分けて持つことがあり
    // （`が` = `か` + `゛`）、文字の上では同じ名前が一致しなくなる。外れても
    // 数え直すだけで害は無いが、**日本語の名前のノートだけ遅い**になる。
    const nfc = (s) => String(s).normalize('NFC');
    // **打ち消すのは、自分の書き込みの跳ね返り一回だけ。**
    //
    // 憶えたまま消さないと、そのノートが外で何度変わっても無視し続ける。
    // 跳ね返りが来ないこともある（別の動きと一緒に丸められる）ので、
    // 時間でも切る ── 数秒より後に来たものは、もう自分のではない。
    if (names && names.length && lastWrote
        && Date.now() - lastWroteAt < 4000
        && names.every((n) => nfc(n) === nfc(lastWrote))) {
        lastWrote = null;
        return;
    }
    churn = setTimeout(async () => {
        if (state.guest) return;                 // 単発で開いている一本は索引の外
        // **見比べる相手を、先に控える**（依頼 479）。
        //
        // `reload` は開いている行を新しいほうに繋ぎ直す ── そのあとで
        // 「変わったか」を見ると、新しい値どうしを見比べることになり、
        // **いつも「変わっていない」になる**。開いているノートの文字だけが
        // 古いまま残っていたのはこれで、二台で同じフォルダを触ると必ず出る。
        const was = state.open ? state.open.updated : null;
        const at = state.open ? state.open.path : null;
        await reload({});
        if (state.open && !state.dirty) {
            const now = state.notes.find((n) => n.path === state.open.path);
            // 消えていたら、開いたままにしない ── 無いノートを見せ続けると、
            // 次の保存で作り直してしまう。
            if (!now) { state.open = null; applyView(); return; }
            if (now.path === at && now.updated !== was) {
                await openNote(now.path, { quiet: true });
            }
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

/// ノートの文字を書き換える一本道。
///
/// **前書きを戻し、切り直し、いつもの保存を通す。** 星もタグも通知も
/// front matter の一行なので、同じパスを通れば衝突の検査も一度で済む ──
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
/// ── フォルダへ移動と同じ一手で済む話。保存場所が無ければ、その場で作る。
async function cmdStar() {
    if (starred(state.open)) {
        const now = state.open.star;
        const off = await askPick('このノートはブックマークに入っています',
            [{ name: 'ブックマークから外す', value: 'off' },
             { name: 'ブックマークグループを変える', value: 'move' }],
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
        { name: '＋ 新しいブックマークグループを作る', value: ' new' }];
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

/// ブックマークの保存場所を一つ作る。`under` があれば、その下に。
///
/// **「/」を打たせない。** 階層は親を右押しして作る ── 一行に全部書かせる
/// のは、書き方を知っている人にしか通じない。
async function newShelf(under) {
    const name = await askText(under ? '「' + under + '」の下に作る名前' : '新しい保存場所の名前',
        '', under ? '' : '下の階層は、保存場所を右押しして作れます');
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
///   というラベルを置く。名前が違うので一覧に出ない。待てば戻ってくる。
/// * 同時に書いた控え ── クラウドが `買い物リスト (…競合コピー…).md` を作る。
///   これは**ノートとして一覧に出す**（消すと中身を助け出すパスが無くなる）。
///   出したうえで、そういうものだとラベルを貼る。
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
            + ' 件、同時に更新されたコピーがあります</b>'
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
/// 前は「フィルタ」という一つのボタンで、押すとダイアログが開き、タグかフォルダか
/// 期間の**どれか一つ**を選んで閉じる作りだった ── 重ねられないうえ、
/// 選んだ結果は `tag:仕事` という文字になって探す欄に流れ込んだ。押しただけ
/// なのに環境の言葉が現れ、外すには文字を消すことになる。
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
        c.textContent = 'すべて外す';
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

/// 帯の一つに出す文字。
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
        : state.books.map((b) => [b, bookLabel(b), state.notes.filter(
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

/* ── カレンダー（いまは期間を選ぶための月表） ── */

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

/// カレンダーで、**いつからいつまでを、押して決める。**
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

/// 共有のフォルダにする（`off` で、やめる）。
///
/// **分けるのは amber の仕事ではない。** クラウドのフォルダ共有に任せる ──
/// amber がするのは、そのフォルダに**マークを 1 つ置くこと**だけ。マークはフォルダと
/// 一緒に旅をするので、**受け取った人は何も教えなくていい** ── 設定に
/// 書いていた頃は、相手が自分の amber に「これが共有です」と教え直す手が
/// 要り、機種を替えるたびにもう一度要った。
async function cmdShare(folder, off) {
    // `folder` は絶対の道。core には、その保存ディレクトリからの相対で渡す。
    const root = rootOf(folder);
    const shown = bookLabel(folder);
    if (!off) {
        const ok = await askYes('「' + shown + '」を、グループと分けるフォルダにしますか');
        if (!ok) return;
    }
    const by = off ? '' : await myName();
    if (by === null) return;
    try {
        await ask('share', {
            path: root, folder: relOf(folder), off: !!off, by, today: today(),
        });
        // マークの一覧は `reload` が保存ディレクトリごとに読み直す。
        await reload({ quiet: true });
        if (off) { say('共有をやめました（ノートはそのままです）'); return; }
        // **二段あることを言う。** amber がマークを置いただけでは誰にも届かない
        // ── クラウド側で人に分けるのは、まだ人がやる。
        await askYes('「' + shown + '」を共有のフォルダにしました。\n\n'
            + 'あとは、このフォルダをクラウド側でグループの人に分けてください。'
            + '（いま開きますか）')
            ? window.amber.reveal(folder)
            : say('あとで、フォルダを右押し →「グループへ招待」からでもできます');
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
/// **フォルダが無ければ作る。** 「共有する」を押した人に、その前に「フォルダを
/// 作る」と「フォルダにする」を押させない ── 押したいのは共有することであって、
/// フォルダを作ることではない。
async function cmdToShare() {
    if (!state.open) return;
    const back = state.open.shared;
    if (back) {
        // **もといたフォルダへ戻す。** 憶えが無ければ、いままでどおり
        // いちばん上へ ── 共有に入れたのが憶えるより前のノートもある。
        const root = rootOf(state.open.path);
        const home = homeOf(state.open);
        const rel = state.open.rel;
        const top = !home || home === root;
        const ok = await askYes('「' + (state.open.title || stem()) + '」を共有から外しますか（'
            + (top ? 'いちばん上へ戻します' : '「' + home.split('/').pop() + '」へ戻します') + '）');
        if (!ok) return;
        await moveNote(home || root, { home: top ? '' : home, forget: rel });
        return;
    }
    // 共有のフォルダは、**そのノートの保存ディレクトリのもの**を先に ── 別の保存
    // ディレクトリの棚へ渡すと、画像と履歴が付いてこない。
    const root = rootOf(state.open.path);
    const near = state.shares.find((sh) => rootOf(sh.at) === root) || state.shares[0];
    let to = near ? near.at : undefined;
    if (to === undefined) {
        const ok = await askYes('「グループ」というフォルダを作って、そこへ移しますか');
        if (!ok) return;
        const by = await myName();
        if (by === null) return;
        try {
            await ask('share', { path: root, folder: 'グループ', by, today: today() });
            to = root + '/グループ';
        } catch (e) { say('できません: ' + why(e)); return; }
    } else {
        const ok = await askYes('「' + (state.open.title || stem()) + '」を「'
            + (to === rootOf(to) ? 'すべて' : to.split('/').pop()) + '」へ移して共有しますか');
        if (!ok) return;
    }
    await moveNote(to, { from: state.open.book || root });
}

/// このノートがもといたフォルダ（絶対の道）。**憶えていなければ空**（いちばん
/// 上へ戻す、といういままでの形）。
///
/// 憶えていたフォルダが、もう無いことはある（消した・名前を変えた）──
/// **無いところへは戻さない**。移せずに止まるより、いちばん上へ。
function homeOf(note) {
    if (!note || !note.path) return '';
    const home = (state.came || {})[note.path];
    if (!home) return '';
    return home === rootOf(note.path) || state.books.includes(home) ? home : '';
}

/// 共有の棚へ入れる（`to`）／外して戻す（`to` が空ならいちばん上）。
///
/// `opts.from` があれば、**入れる前に居たフォルダを憶える** ── 外すときに
/// そこへ戻せるように。`opts.home` は戻した先で、言葉にするために持つ。
async function moveNote(to, opts) {
    try {
        // `to` は絶対の道（保存ディレクトリそのものなら、そのいちばん上）。
        const was = state.open.path;
        const root = rootOf(was);
        const r = await moveOp(was, to);
        // **憶えるのは移せてから。** 移せなかった回の憶えが残ると、次に
        // 外した人が身に覚えのないフォルダへ連れて行かれる。
        // 帳画面は保存ディレクトリごと ── 別の保存ディレクトリへ渡ったときは
        // 憶えない（向こうの帳画面に、こちらのパスは書けない）。
        try {
            if (opts && opts.from !== undefined && r && r.path && rootOf(r.path) === root
                && rootOf(opts.from) === root) {
                const got = await ask('came', { path: root, rel: relOf(r.path), from: relOf(opts.from) });
                if (got && got.came) await reload({ quiet: true });
            } else if (opts && opts.forget) {
                await ask('came', { path: root, rel: opts.forget, forget: true });
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
    const here = [...bookChoices(), { name: '＋ 新しいフォルダを作る', value: ' new' }];
    let to = await askPick('どのフォルダへ', here);
    if (to === null) return;
    if (to === ' new') {
        const made = await cmdMkBook();
        if (!made) return;
        to = made;
    }
    const dir = to;
    try {
        // 書きかけを置いていかない ── 移した先に古い文字が残る。
        if (state.dirty) await save();
        const r = await moveOp(state.open.path, dir);
        await reload({ quiet: true });
        await openNote(r.path);
        say(dirWords(dir) + '移しました');
    } catch (e) {
        say('移せません: ' + why(e));
    }
}

/// フォルダを一つ作る。`under`（絶対の道）があれば、その下に。無ければ
/// **いま見ている保存ディレクトリ**のいちばん上。返すのは出来た絶対の道。
///
/// **「/」を打たせない。** 「仕事/2026」と書けば階層になる、は書き方を
/// 知っている人にしか通じない ── 下の階層は、親を右押しして作る。
async function cmdMkBook(under) {
    const base = under || rootOf(hereDir());
    const top = !under || under === rootOf(under);
    const where = top && manyPlaces() ? '「' + bookName(base) + '」に作る名前'
        : top ? '新しいフォルダの名前' : '「' + bookName(under) + '」の下に作る名前';
    const name = await askText(where, '', top ? '下の階層は、フォルダを右押しして作れます' : '');
    if (name === null || !name.trim()) return null;
    const leaf = name.trim().replace(/\//g, '／');
    const full = base + '/' + leaf;
    try {
        await ask('mkbook', { dir: full });
        await reload({ quiet: true });
        say('「' + bookLabel(full) + '」を作りました');
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
    say('このフォルダの変更を検知できません' + (why2 ? '（' + why2 + '）' : '')
        + '。外で変えたら、開き直すと出ます');
}

/// ゴミ箱の無い置き場所（憶えたもの）。
///
/// **一度断られたら、次からは初めからそう言う。** `Documents` が OneDrive
/// へ寄せられている机では、断られるのは一度きりではなく毎回 ── そこで
/// 二度訊き続けると、消すたびに二回答えることになる。分かっているなら、
/// 初めの一回で正直に訊けばいい。
///
/// 保存場所ごとに憶える ── 別のフォルダへ移せば、そちらにはゴミ箱がある。
let noBins = [];
const noBin = () => noBins.includes(openRoot());

function markNoBin(yes) {
    const root = openRoot();
    const was = noBins.filter((r) => r !== root);
    noBins = yes ? [...was, root] : was;
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
    // **消さずに、ゴミ箱へ。** core の `delete` は消してしまう（iPhone には
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
        // 保存場所も同じ。ここで黙ると、**そのノートは二度と消せない**。
        //
        // **消すのは、訊いてから。** ゴミ箱が「戻せる」ことの担保だった
        // ので、それが無い以上そう言う ── 言わずに消すほうが強すぎる。
        // **ダイアログは記号を解さない。** ここは `textContent` なので、
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

/// 通知。**仕掛けるのはデスクトップ版でもできる。鳴らすのはiPhone。**
///
/// デスクトップ版は閉じている時間のほうが長く、閉じている間の時刻は誰も見ていない ──
/// ここで鳴るのは「開いているうちに来た分」だけ。同じフォルダを見ている
/// iPhone は、閉じていても鳴らす。
async function cmdRemind() {
    const kind = await askPick('いつ知らせるか', [
        { name: '一度だけ知らせる', value: 'once' },
        { name: '毎日', sub: '例: 09:00', value: 'daily' },
        { name: '毎週', sub: '例: 月 09:00', value: 'weekly' },
        { name: '毎月', sub: '例: 1 09:00', value: 'monthly' },
        { name: '（やめる）', value: 'off' },
    ], '通知は iPhone に届きます（このアプリを開いているあいだは、ここでも通知します）');
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
            .then((r) => r.text))) say('通知を設定しました: ' + v.trim());
        return;
    }
    const hint = { daily: '09:00', weekly: '月 09:00', monthly: '1 09:00' }[kind];
    // 題は日本語で（`daily` がそのまま出ていた・2026-09-12 の見直しで見つけた）。
    const v = await askText('繰り返し（' + ({ daily: '毎日', weekly: '毎週', monthly: '毎月' }[kind] || kind) + '）', hint,
        '毎日は 09:00、毎週は 月 09:00、毎月は 1 09:00');
    if (v === null || !v.trim()) return;
    if (await editNote((t) => ask('setfield', { text: t, key: 'repeat', value: kind + ' ' + v.trim() })
        .then((r) => r.text))) say('繰り返しを設定しました');
}

function ymdNow() {
    const d = new Date();
    const p = (n) => String(n).padStart(2, '0');
    return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) + ' 09:00';
}

async function cmdExport() {
    const how = await askPick('どの形で書き出すか', [
        { name: 'Markdown', sub: 'ノートそのまま（前書きも含む）', value: 'md' },
        { name: 'HTML', sub: '読める形、1 つで完結', value: 'html' },
        { name: 'PDF', sub: '読める形を刷る', value: 'pdf' },
    ]);
    if (how === null) return;
    const name = stem();
    try {
        if (how === 'md') {
            // **ファイルと同じ文字にする。** 画面が持っているのは最後の改行の
            // 無い姿で、そのまま書き出すと**元のノートと一バイト違う**
            // （実際に 99 と 100 になった）── 「そのまま」と言っている以上、
            // そこは合わせる。core も保存のときに同じ一文字を足している。
            const text = whole();
            const at = await window.amber.saveText(
                name + '.md', text.endsWith('\n') ? text : text + '\n');
            if (at) say('書き出しました: ' + at);
            return;
        }
        const body = (await ask('html', { text: whole() })).html || '';
        const page = onePage(state.open.title || name, await inlinePictures(body));
        const at = how === 'html'
            ? await window.amber.saveText(name + '.html', page)
            : await window.amber.savePDF(name + '.pdf', page);
        if (at) say('書き出しました: ' + at);
    } catch (e) {
        say('書き出せません: ' + why(e));
    }
}

/// **画像を、書き出す 1 つの中へ入れる**（依頼 435）。
///
/// メニューは「1 つで完結」と言っているのに、画像は `attachments/…` という
/// **隣を指す道**のままだった ── 書き出した HTML を人に送ると、送られた
/// 側では画像が出ない。言っていることを本当にする。
///
/// 落として来られない画像は、パスのまま残す ── 消すと「あったはずのものが
/// 無い」になり、そちらのほうが分かりにくい。
async function inlinePictures(html) {
    const dir = state.open ? dirOf(state.open.path) : '';
    const doc = new DOMParser().parseFromString(html, 'text/html');
    for (const img of doc.querySelectorAll('img')) {
        const src = img.getAttribute('src') || '';
        if (!src || /^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith('//')) continue;
        try {
            const got = await window.amber.fileBytes(absPath(src, dir));
            if (got && got.b64) img.src = 'data:image/' + (got.ext || 'png') + ';base64,' + got.b64;
        } catch { /* 読めない画像は、パスのまま置いておく */ }
    }
    return doc.body.innerHTML;
}

/// 1 つで完結する HTML。
///
/// **外を参照しない。** 別の環境で開いても文字の形が崩れないように、字体は
/// その環境にあるものだけ。画像は `inlinePictures` が中へ入れてある。
function onePage(title, body) {
    return '<!doctype html><html lang="ja"><head><meta charset="utf-8">'
        + '<title>' + escapeHtml(title) + '</title><style>'
        + 'body{max-width:44rem;margin:3rem auto;padding:0 1.4rem;'
        + 'font:16px/1.9 -apple-system,BlinkMacSystemFont,"Hiragino Sans","Yu Gothic UI",sans-serif;'
        + 'color:#2a2011;background:#fffdf8}'
        + 'h1,h2,h3,h4{line-height:1.4;margin:1.6em 0 .5em}'
        + 'h2{padding-bottom:.2em;border-bottom:1px solid #efe6d4}'
        + 'pre>code.language-amber{color:#b5760f;display:block;line-height:1}'
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
/// 前はここと `Colouring.palette`（iPhone）に同じ表を書いていて、両方の
/// コメントに「同じ並び」と書いてあった ── それでも**十一色のうち六色が
/// ずれていた**。iPhone で付けた青が、Mac では少し違う青で出ていた。
/// 写しを持てば、いつかずれる。
let PALETTE = [];

async function loadPalette() {
    try {
        const r = await ask('palette', {});
        PALETTE = (r.colors || []).map((c) => [c.hex, c.name]);
    } catch {
        // 訊けなくても色は付けられなくていい ── デスクトップ版が開かない理由にはしない。
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
    const hex = await askPick('「' + bookLabel(what) + '」の色', [
        { name: '（色を外す）', value: '' },
        ...PALETTE.map(([h, n]) => ({ name: n, sub: h, value: h })),
    ]);
    if (hex === null) return;
    try {
        await ask('color', { path: rootOf(what), folder: relOf(what), color: hex || null });
        // 色の一覧は保存ディレクトリごと ── 読み直して重ねる。
        await reload({ quiet: true });
    } catch (e) {
        say('色を付けられません: ' + why(e));
    }
}

/// バックアップ。**範囲を訊く。**
///
/// 長いあいだ「すべて」を決め打ちで渡していて、四つあることはエンジンしか
/// 知らなかった ── iPhone には四つとも出ていたので、**同じアプリでiPhone に
/// できてデスクトップ版にできない**ことが一つあった。
///
/// 四つ ── すべて／フォルダ一つ／タグの付いたもの／このノート 1 つ。
/// zip の名前は何が入っているかを言う（`仕事-2026-09-06.zip`）── 名前が
/// `backup.zip` ばかりのフォルダは、「どれがどれか」という一つの問いになる。
async function cmdBackup() {
    const here = state.dest.kind === 'book' ? state.dest.what : '';
    // zip は保存ディレクトリ一つぶん ── 二つ以上あるときは「すべて」も一つずつ。
    const items = state.places.map((p) => ({
        name: 'すべて' + (manyPlaces() ? '（' + p.name + '）' : ''),
        sub: 'ノートも画像も、まるごと一つに', value: ['all', p.dir, p.dir],
    }));
    for (const b of state.books || []) {
        items.push({ name: 'フォルダ: ' + bookLabel(b), sub: b === here ? 'いま見ているところ' : '', value: ['book', relOf(b), rootOf(b)] });
    }
    // タグは使われている順（`tagsOf`）。多いものから並ぶので、
    // 取っておきたいまとまりはたいてい上のほうに居る。
    // タグは**いま見ている保存ディレクトリ**の中から集める。
    const tagRoot = rootOf(hereDir());
    for (const [t, n] of tagsOf(state.notes.filter((x) => x.root === tagRoot)).slice(0, 20)) {
        items.push({ name: 'タグ: #' + t, sub: n + ' 件・フォルダをまたいで集めます'
            + (manyPlaces() ? '（' + bookName(tagRoot) + '）' : ''), value: ['tag', t, tagRoot] });
    }
    if (state.open) {
        items.push({ name: 'このノート 1 つ', sub: shortPath(state.open.path), value: ['note', state.open.path, rootOf(state.open.path)] });
    }
    const pick = await askPick('どこまで取っておきますか', items,
        '一つの zip にまとめます。いまあるノートは動きません');
    if (pick === null) return;
    const [scope, what, root] = pick;
    const into = await window.amber.pickFolder();
    if (!into) return;
    try {
        const r = await ask('backup', { path: root, scope, what, into });
        say(r.files + ' 件を保存しました: ' + shortPath(r.path || into));
    } catch (e) {
        say('保存できません: ' + why(e));
    }
}

/// よそにある .md を、ノート帳へ取り込む。
///
/// **デスクトップ版には無かった。** iPhone には初めからあり、デスクトップ版には「バックアップから
/// 戻す」しかなかった ── zip でなければ入れるパスが無く、ほかのアプリから
/// 書き出した .md を持ってきた人は、Finder でフォルダを開いて自分で写す
/// しかなかった（そのフォルダがどこかも、amber は言わない）。
///
/// **上書きしない・元は動かさない。** 同じ名前があれば `週報-2.md` に
/// して両方残す（判断は core の `notebook::bring`。デスクトップ版と iPhone で二組書くと、
/// 同じノートが端末によって別の名前で入る）。名前を変えたぶんは数えて
/// 言う ── 言わないと、開いたノートが「さっき取り込んだやつ」なのか
/// 「前からあったやつ」なのか見分けが付かない。
async function cmdBring() {
    const files = await window.amber.pickFiles([{ name: 'ノート', extensions: ['md', 'markdown', 'txt'] }]);
    if (!files || !files.length) return;
    try {
        // 入れる先は、いま見ている保存ディレクトリのいちばん上。
        const r = await ask('bring', { files, to: rootOf(hereDir()) });
        await reload({});
        // **入らなかった数も言う。** 十本選んで八本入ったとき、黙って
        // いると人は八本しか選ばなかったと思う ── 気づくのは、あとで
        // 探しても出てこない日。
        const re = r.renamed ? '（' + r.renamed + ' 件は名前を変えました）' : '';
        const no = r.failed ? '。' + r.failed + ' 件は入れられませんでした' : '';
        say(r.put + ' 件をインポートしました' + re + no);
    } catch (e) {
        say('インポートできません: ' + why(e));
    }
}

/* ── OneNote を取り込む（依頼 621）── */

/// いま走っている取り込み。**デスクトップ版を閉じても（ダイアログを閉じても）続く** ──
/// 大きいノートブックは数分かかり、その間ずっとダイアログを見ていろとは言えない。
/// もう一度 ⚙ から開くと、進み具合のダイアログに戻る。
let oneRun = null;
/// 進み具合のダイアログがいま出ているか（出ていれば、一つ書くたびに描き直す）。
let oneOpen = false;
let oneShown = 0;

/// 取り込む。**読むのも書くのもエンジン**（`amber_core::onenote`）──
/// ここは選ばせて、進み具合を見せるだけ。
async function cmdOneNote() {
    if (oneRun && !oneRun.over) { oneShow(); return; }
    const known = await window.amber.knownDirs();
    const from = await pickOneNote(known);
    if (!from) return;
    const to = await oneNoteOut(known);
    if (!to) return;
    await runOneNote(from, to);
}

/// どれを取り込むか。**書き出した場所に置いてあることが多い** ので、
/// ドキュメント・ダウンロード・デスクトップの直下の `.onepkg` を先に並べる。
async function pickOneNote(known) {
    const dirs = [known.documents, known.downloads, known.desktop].filter(Boolean);
    let found = [];
    try { found = (await window.amber.onenote('onenote_find', { dirs })).found || []; } catch { /* 探せなくても選べる */ }
    const where = (at) => (at.startsWith(known.documents) ? 'ドキュメント'
        : at.startsWith(known.downloads) ? 'ダウンロード'
        : at.startsWith(known.desktop) ? 'デスクトップ' : shortPath(at));
    const mb = (n) => (n >= 1048576 ? (n / 1048576).toFixed(1) + ' MB' : Math.max(1, Math.round(n / 1024)) + ' KB');
    const day = (t) => { const d = new Date(t * 1000); return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0'); };
    const items = found.map((f) => ({ name: f.name + '.onepkg', sub: [where(f.path), day(f.modified), mb(f.bytes)].join(' · '), value: f.path }));
    items.push({ name: 'ファイルを選ぶ…', sub: '.onepkg / .one', value: ' file' });
    items.push({ name: 'フォルダを選ぶ…', sub: '.one が入ったフォルダ', value: ' dir' });
    const go = await askPick('OneNote を取り込む', items,
        '書き出し方: OneNote の ファイル → エクスポート → ノートブック → .onepkg', true);
    if (go === null) return null;
    if (go === ' file') return window.amber.pickFile([{ name: 'OneNote', extensions: ['onepkg', 'one', 'onetoc2'] }]);
    if (go === ' dir') return window.amber.pickFolder();
    return go;
}

/// どこへ書き出すか。**一度決めたら憶える**（`onenoteOut`）。
///
/// 憶えた場所が**どの保存ディレクトリの中でもなくなっていたら、もう一度訊く**
/// ── ⚙「保存ディレクトリ」から外した人は、そこに書いてほしくない人。
/// 書き出したのに左の列に出てこないノートは、無いのと同じ。
async function oneNoteOut(known) {
    // **`/` と `\\` の両方で見る**（`placeOf` は `/` だけ ── Windows のパスは
    // 一つも中に入らず、毎回訊くうえに、入れ子の保存ディレクトリを作る）。
    const inside = (dir) => state.places.some((p) => dir === p.dir
        || dir.startsWith(p.dir + '/') || dir.startsWith(p.dir + '\\'));
    const saved = (await window.amber.recall()).onenoteOut;
    if (typeof saved === 'string' && saved && inside(saved)) return saved;
    const sep = known.sep || '/';
    const docs = known.documents ? known.documents + sep + 'OneNote' : '';
    const here = state.places.length ? rootOf(hereDir()) : '';
    const items = [];
    if (docs) items.push({ name: 'ドキュメント／OneNote', sub: '新しい保存ディレクトリとして足します（おすすめ）', value: 'new' });
    if (here) items.push({ name: '「' + bookName(here) + '」の中の「OneNote」フォルダ', sub: 'いまの保存ディレクトリの下に作ります', value: 'in' });
    items.push({ name: 'フォルダを選ぶ…', sub: '選んだフォルダを保存ディレクトリとして足します', value: 'pick' });
    const go = await askPick('どこへ書き出しますか', items,
        '一度決めたら憶えます。あとで ⚙ →「保存ディレクトリの追加・変更・削除」から外すと、次にまた訊きます', true);
    if (go === null) return null;
    let dir = go === 'new' ? docs : go === 'in' ? here + sep + 'OneNote' : await window.amber.pickFolder();
    if (!dir) return null;
    try {
        await ask('place', { dir });
    } catch (e) {
        say('作れません: ' + why(e));
        return null;
    }
    // 保存ディレクトリの外なら、足す（中なら、もう左の列に出る）。
    if (!inside(dir) && !(await putPlace(dir))) return null;
    window.amber.remember({ onenoteOut: dir });
    return dir;
}

async function runOneNote(from, to) {
    oneRun = { name: leafOf(from), rows: [], over: false, pages: 0, pictures: 0, failed: 0, to };
    const run = oneRun;
    run.title = '「' + run.name + '」を読んでいます…';
    oneShow();
    let got;
    try {
        got = await window.amber.onenote('onenote_open', { path: from });
    } catch (e) {
        run.over = true;
        window.amber.onenote('onenote_close', {}).catch(() => {});
        // **読めなかったことは、ダイアログに残す**（本人・メンバーから「何も出なかった。
        // 裏で動いているのか？」）。前はダイアログを閉じて下のラベルで言っていた ──
        // ラベルは 2 秒で消え、ダイアログは一瞬しか出ないので、何も起きなかったのと同じ
        // 顔をしていた。**閉じていても開き直す** ── 結果が失敗なら、それが答え。
        run.title = '「' + run.name + '」を取り込めませんでした';
        run.trouble = oneTrouble(why(e));
        oneShow();
        return;
    }
    run.rows = (got.units || []).map((u) => ({
        name: [...(u.groups || []), u.name].join('／'), pages: u.pages, state: 'wait',
    }));
    if (!run.rows.length) {
        run.over = true;
        window.amber.onenote('onenote_close', { key: got.key }).catch(() => {});
        run.title = '「' + run.name + '」には、セクションがありませんでした';
        run.trouble = { what: '取り込むページが見つかりませんでした', hint: 'OneNote で ファイル → エクスポート → ノートブック → .onepkg を書き出して、それを選んでください', raw: '' };
        oneShow();
        return;
    }
    run.title = 'OneNote を取り込んでいます';
    for (let i = 0; i < run.rows.length; i += 1) {
        const row = run.rows[i];
        row.state = 'now';
        if (oneOpen) oneShow();
        try {
            const w = await window.amber.onenote('onenote_write', { key: got.key, i, to });
            row.state = 'done';
            run.pages += w.pages;
            run.pictures += w.pictures;
            run.dir = run.dir || w.dir;
        } catch (e) {
            // **一つ書けなくても、残りは書く。** どれが書けなかったかは行に残す。
            row.state = 'fail';
            row.why = why(e);
            run.failed += 1;
        }
    }
    await window.amber.onenote('onenote_close', { key: got.key }).catch(() => {});
    run.over = true;
    await reload({});
    const pics = run.pictures ? '・画像 ' + run.pictures + ' 枚' : '';
    const bad = run.failed ? '。' + run.failed + ' セクションは書けませんでした' : '';
    run.title = '取り込みました ── ' + run.pages + ' ページ' + pics + bad;
    say(run.title + '。左の列の「' + bookName(rootOf(to)) + '」に入っています');
    if (oneOpen) oneShow();
}

/// 読めなかった理由を、人の言葉に。**パスは落とす**（`C:\\Users\\…\\x.one を開けません:`
/// が頭に付いていて、ダイアログの幅では肝心の理由が切れて見えない）。
function oneTrouble(text) {
    const raw = String(text || '').replace(/^.*を開けません:\s*/, '');
    const low = raw.toLowerCase();
    // Windows の「別のプロセスが使用中」（os error 32）── OneNote が開いている `.one`。
    if (/os error 32|os error 33|being used by another process|別のプロセス/.test(low)) {
        return { what: 'OneNote がこのファイルを使っているので、読めません',
            hint: 'OneNote で ファイル → エクスポート → ノートブック → .onepkg を書き出して、それを選んでください', raw };
    }
    if (/unexpected end of file|unknown file format|malformed|not a (toc|section)/.test(low)) {
        return { what: 'OneNote のファイルとして読めませんでした',
            hint: '空か途中で切れているか、OneDrive 上にだけあってこのパソコンに落ちていないかもしれません。OneNote から .onepkg を書き出して選ぶのが確実です', raw };
    }
    if (/os error 5\b|permission denied|access is denied|アクセスが拒否/.test(low)) {
        return { what: 'このファイルを読む権限がありません',
            hint: 'ドキュメントなど自分のフォルダに写してから選んでください', raw };
    }
    return { what: '読めませんでした',
        hint: 'OneNote で ファイル → エクスポート → ノートブック → .onepkg を書き出して、それを選ぶのが確実です', raw };
}

/// 進み具合のダイアログ。**閉じても取り込みは止まらない**（そう書いておく）。
function oneShow() {
    const run = oneRun;
    if (!run) return;
    const mark = { done: '✓ ', now: '▸ ', fail: '✗ ', wait: '　' };
    const items = run.rows.map((r) => ({
        name: mark[r.state] + r.name,
        sub: r.state === 'fail' ? '書けませんでした: ' + r.why : r.pages + ' ページ',
        value: ' row',
    }));
    // **読んでいる間も、行を一つ出す** ── 題だけのダイアログは、止まっているのか
    // 動いているのか分からない（「裏で動いているのか？」と訊かれた）。
    if (!run.over && !run.rows.length) {
        items.push({ name: '▸ 読んでいます…', sub: '大きいノートブックは数分かかります', value: ' row' });
    }
    // **理由と元の文言は、横に並べず一行ずつ**（添え書きの欄は一行で切れる ──
    // 肝心の「どうすればいいか」が「…このパソコンに落ち…」で見えなかった）。
    // どうすればいいかは、切れない下の添え書きへ。
    if (run.trouble) {
        items.push({ name: '✗ ' + run.trouble.what, value: ' row' });
        // 元の文言も残す ── 問い合わせのとき、これを打ってもらえば原因が分かる。
        if (run.trouble.raw) items.push({ name: '元の文言: ' + run.trouble.raw, value: ' row' });
    }
    if (run.over && run.dir) items.push({ name: 'フォルダを開く', sub: shortPath(run.to), value: ' reveal' });
    const my = ++oneShown;
    oneOpen = true;
    const foot = run.trouble ? run.trouble.hint : (run.over ? '' : '閉じても、裏で続きます');
    sheet({ title: run.title, items, foot, bare: true }).then((v) => {
        if (my !== oneShown) return;
        oneOpen = false;
        if (v === ' reveal') window.amber.reveal(run.dir);
    });
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
        const r = await ask('restore', { zip, to: rootOf(hereDir()) });
        await reload({});
        const kept = r.kept ? '（' + r.kept + ' 件は、いまのを残しました）' : '';
        say(r.put + ' 件を戻しました' + kept);
    } catch (e) {
        say('戻せません: ' + why(e));
    }
}

/* ── 同期（Google Drive）── */

/// いまの様子（メニューの脇に出す）。**押す前に、いまどうなっているかを見せる。**
let syncAccount = { signedIn: false };
function syncLabel() {
    if (!syncAccount.signedIn) return '同期していません';
    const who = syncAccount.who || {};
    const at = syncLast ? '・最終 ' + new Date(syncLast).toTimeString().slice(0, 5) : '';
    return 'Google Drive' + (who.email ? '（' + who.email + '）' : '') + at;
}
async function loadSync() {
    try { syncAccount = await window.amber.driveAccount(); } catch { syncAccount = { signedIn: false }; }
    // サインインしているなら、時計を回して一度合わせる。
    // 開いた直後は少し待ってから ── 一覧と画面が組み上がる前に裏で運ばない。
    if (syncAccount.signedIn) { syncClock(); syncSoon(6000); } else { clearInterval(syncTick); syncTick = null; }
    drawSyncState();
}
// 開いた直後に一度 ── メニューの脇の「いま」は、押す前から正しくあること。
// **会社向けのビルドでは、訊きにも行かない**（依頼 602）── サインインの
// 有無を訊くこと自体が、外の鍵入れを開けにいくこと。
officeReady.then(() => { if (!OFFICE) loadSync(); });

/* ── ファイル名は題に合わせる（依頼 492） ──
 *
 * **何という名前にするかは core（`settle`）、いつ改名するかはここ。**
 * 題の欄から出たとき（`titleDone`）はその場で。見出しや一行目で題が決まる
 * ノートは、**そのノートから離れたとき**（別のノートを開く・ウィンドウから出る・
 * 打つ手が十秒止まったとき）── 打つたびに改名すると、「牛乳」を「牛乳と
 * パン」に直すあいだにファイルが三度名前を変える。
 */
/// 勝手に改名するか。**総ざらいは切ってから回す** ── 固定のファイル名で
/// 押して回るので、離れるたびに名前が変わると次の段が迷子になる。
let nameAuto = true;
let nameTimer = null;

/// 改名したあと、パスで持っているものを繋ぎ直す（タブ・戻る道・開いている
/// ノート・混ぜた印）。フォルダの中のもの（履歴・憶え）は core が連れて行く。
function afterRename(from, to) {
    for (const t of tabs) {
        if (t.path !== from) continue;
        t.path = to;
        if (t.keep && t.keep.open) t.keep.open = { ...t.keep.open, path: to };
    }
    if (showing === from) showing = to;
    for (let i = 0; i < trail.length; i += 1) if (trail[i] === from) trail[i] = to;
    if (incomings[from]) {
        incomings[to] = { ...incomings[from], path: to };
        delete incomings[from];
        window.amber.remember({ incomings });
    }
    if (incoming && incoming.path === from) incoming.path = to;
    if (state.open && state.open.path === from) {
        state.open = { ...state.open, path: to };
        window.amber.remember({ open: to });
    }
    rememberTabs();
    // 見張りが「自分の書き込み」として読み飛ばせるように。
    lastWrote = to;
    lastWroteAt = Date.now();
}

/// 題に合わせて改名する。改名したら新しい道、しなければ null。
async function settleName(path) {
    if (!nameAuto || !path || state.guest || !placeOf(path)) return null;
    let r;
    try {
        r = await ask('settle', { path: rootOf(path), note: path });
    } catch (e) {
        say('名前を変えられません: ' + why(e));
        return null;
    }
    if (!r || !r.renamed) return null;
    afterRename(path, r.path);
    await reload({ quiet: true });
    // 本文の画像のリンクまで書き直されたなら、開いているものを読み直す。
    if (r.rewrote && state.open && state.open.path === r.path && !state.dirty) {
        await openNote(r.path, { walking: true });
    }
    return r.path;
}

/// 打つ手が止まって十秒したら、開いているノートの名前を揃える。
function nameSoon() {
    if (!nameAuto) return;
    clearTimeout(nameTimer);
    nameTimer = setTimeout(() => {
        if (state.open && !state.dirty && !state.guest) settleName(state.open.path);
    }, 10_000);
}
window.addEventListener('blur', () => {
    if (state.open && !state.dirty && !state.guest) settleName(state.open.path);
});

/* ── 運ぶ ──
 *
 * **判断は core（`syncplan`）、運ぶのはここ。** 向こうの一覧を持ってきて、
 * 手順書をもらい、一つずつやって、運べたぶんだけ憶えてもらう（`synced`）。
 * 途中で切れても、運べたぶんは憶えに残る ── 次に続きから。
 *
 * いつ運ぶか: 保存して三秒後・三十秒ごと・デスクトップ版に戻ったとき・サインインした
 * とき。**打っている最中には触らない** ── 下ろしたものは、ファイルが変わった
 * ときの道（見張り → 拾い直す・打ちかけなら保存のときに混ぜる）で画面に届く。
 */
let syncBusy = false;
/// 勝手に運ぶか。**総ざらいは、これを切ってから順に押す**（保存のたびに裏で
/// 運ぶと、見張っている数が動く）。手で呼ぶ `syncNow('手')` は切っても通る。
let syncAuto = true;
let syncTimer = null;
let syncTick = null;
let syncLast = null;
let syncTrouble = '';
let syncLastReport = null;
/// 「あとで」を押したか（このデスクトップ版を開いているあいだだけ。次に開いたら、また
/// 列を出す ── 本人が決めた・2026-09-11・案い）。
let syncLater = false;
/// 困りはじめた時刻（列に「hh:mm から」と出す）。
let syncTroubleSince = null;
/// 運んだ直後の数（数秒だけ色つきの列に出す）。
let syncFresh = null;
let syncFreshTimer = null;

function syncSoon(ms = 3000) {
    if (!syncAccount.signedIn) return;
    clearTimeout(syncTimer);
    syncTimer = setTimeout(() => syncNow('保存'), ms);
}
function syncClock() {
    clearInterval(syncTick);
    syncTick = setInterval(() => syncNow('時計'), 30_000);
}
window.addEventListener('focus', () => { if (syncAccount.signedIn) syncNow('戻った'); });

/// 一度、合わせる。返すのは何を運んだかの数（試験が見る）。
/// 運ぶ相手 ── Drive にしてある保存ディレクトリ（読めているものだけ・依頼 511）。
const syncTargets = () => state.places.filter((p) => p.sync === 'drive' && !state.placeTrouble[p.dir]);

/// 向こうの一覧のうち、この保存ディレクトリのぶん（パスは保存ディレクトリからの相対に）。
///
/// **いちばん目は `ambər` の直下、二つ目からは `ambər/<at>/`** ── いままでの
/// ノートは 1 つも動かさずに、二つ目が足せる。`at` が '' のもの（いちばん目）は、
/// ほかの保存ディレクトリの頭を持たないぜんぶ。
function remoteOf(all, place) {
    const others = state.places.map((p) => p.at).filter((a) => a && a !== place.at);
    const pre = place.at ? place.at + '/' : '';
    const out = [];
    for (const x of all) {
        if (pre) {
            if (!x.rel.startsWith(pre)) continue;
            out.push({ ...x, rel: x.rel.slice(pre.length) });
        } else {
            if (others.some((o) => x.rel.startsWith(o + '/'))) continue;
            out.push(x);
        }
    }
    return out;
}

/// 一度、合わせる。返すのは何を運んだかの数（試験が見る）。
/// 保存ディレクトリごとに順に運び、数は足す（`places` に一つずつも残す ── 運んだ
/// 直後の列が「仕事: アップロード1本」と言えるように）。
async function syncNow(reason) {
    const targets = syncTargets();
    if (!syncAccount.signedIn || syncBusy || !targets.length || state.guest) return null;
    if (!syncAuto && reason !== '手') return null;
    syncBusy = true;
    drawSyncState();
    const report = { reason, up: 0, down: 0, gone: 0, clash: 0, moved: 0, eyes: 0, trouble: [], places: {} };
    try {
        const all = await window.amber.driveList();
        let touched = false;
        let openTouched = false;
        for (const place of targets) {
            const one = await syncPlace(place, remoteOf(all, place));
            for (const k of ['up', 'down', 'gone', 'clash', 'moved', 'eyes']) report[k] += one[k];
            for (const t of one.trouble) report.trouble.push((manyPlaces() ? place.name + ' › ' : '') + t);
            report.places[place.name] = one;
            touched = touched || one.touched;
            openTouched = openTouched || one.openTouched;
        }
        if (touched) await reload({ quiet: true });
        // 開いているノートが下りてきた ── 打ちかけでなければ、その文字に開き直す
        // （帯と選び口もここで付く）。打ちかけなら、保存のときの混ぜに任せる。
        if (openTouched && state.open && !state.dirty && !calOn) {
            clearTimeout(readTimer);
            await syncRead(true);
            if (!state.dirty) await openNote(state.open.path, { walking: true });
        }
        syncLast = Date.now();
        syncTrouble = report.trouble.length ? report.trouble[0] : '';
        if (report.up + report.down + report.gone + report.clash + report.moved > 0) {
            syncFresh = { at: syncLast, ...report };
            clearTimeout(syncFreshTimer);
            syncFreshTimer = setTimeout(() => { syncFresh = null; drawSyncState(); }, 6000);
        }
    } catch (e) {
        syncTrouble = why(e);
        report.trouble.push(why(e));
    } finally {
        syncBusy = false;
        syncLastReport = report;
        if (syncTrouble) syncTroubleSince = syncTroubleSince || Date.now();
        else syncTroubleSince = null;
        drawSyncState();
    }
    return report;
}

/// 一つの保存ディレクトリを運ぶ。`remote` はそのぶんの一覧（相対）。
///
/// core（`syncplan`・`synced`）は保存ディレクトリ一つしか知らない ── 帳画面
/// （`.amber/sync.json`）もそこにある。Drive の上の道だけ `pre` を頭に付ける。
async function syncPlace(place, remote) {
    const root = place.dir;
    const pre = place.at ? place.at + '/' : '';
    const one = { up: 0, down: 0, gone: 0, clash: 0, moved: 0, eyes: 0, trouble: [], touched: false, openTouched: false };
    const plan = await ask('syncplan', { path: root, who: 'drive', remote });
    const done = [];
    const gone = [];
    const moved = [];
    // **開いているノートを書き換えたか。** 見張り（`onChanged`）は、保存した
    // 直後の数秒はそのノートの変わりを「自分の跳ね返り」として捨てる ──
    // 同期が下ろした文字はそこに紛れて、画面が古いまま残る（実際に残った）。
    // だから同期は自分で開き直す（打ちかけなら触らない ── 保存のときに混ざる）。
    for (const s of plan.steps || []) {
        const at = root + '/' + s.rel;
        const isOpen = !!(state.open && state.open.path === at);
        try {
            if (s.do === 'up') {
                const print = (await ask('syncprint', { path: at })).print;
                // 画像は bytes のまま（描く側を通さない・依頼 497）。
                const r = s.bin
                    ? await window.amber.driveUploadFile({ rel: pre + s.rel, file: at, print, id: s.id || undefined })
                    : await window.amber.driveUpload({ rel: pre + s.rel, text: (await ask('read', { path: at })).text, print, id: s.id || undefined });
                done.push({ rel: s.rel, id: r.id, tag: r.tag });
                one.up += 1;
            } else if (s.do === 'down') {
                if (s.bin) {
                    await window.amber.driveDownloadFile({ id: s.id, to: at });
                } else {
                    const text = await window.amber.driveDownload(s.id);
                    await ask('syncdown', { path: at, text });
                }
                const there = remote.find((x) => x.id === s.id);
                done.push({ rel: s.rel, id: s.id, tag: there ? there.tag : '' });
                one.down += 1;
                one.touched = true;
                if (isOpen) one.openTouched = true;
            } else if (s.do === 'clash' && s.bin) {
                // **画像は混ぜられない。** こちらを残し、向こうのものは `名前.2.png` として
                // 隣に置く（失うよりよい）。隣に置いた 1 つは、次の同期で新しく上がる。
                const dot = at.lastIndexOf('.');
                let beside = at.slice(0, dot) + '.2' + at.slice(dot);
                for (let n = 3; n < 100; n += 1) {
                    try { await ask('syncprint', { path: beside }); } catch { break; }
                    beside = at.slice(0, dot) + '.' + n + at.slice(dot);
                }
                await window.amber.driveDownloadFile({ id: s.id, to: beside });
                const print = (await ask('syncprint', { path: at })).print;
                const r = await window.amber.driveUploadFile({ rel: pre + s.rel, file: at, print, id: s.id });
                done.push({ rel: s.rel, id: r.id, tag: r.tag });
                one.up += 1;
                one.down += 1;
                one.touched = true;
            } else if (s.do === 'drophere') {
                // 向こうで消え、こちらは触っていない ── **ゴミ箱へ**（消さない）。
                await window.amber.trash(at);
                gone.push(s.rel);
                one.gone += 1;
                one.touched = true;
            } else if (s.do === 'dropthere') {
                await window.amber.driveTrash(s.id);
                gone.push(s.rel);
                one.gone += 1;
            } else if (s.do === 'movethere') {
                // こちらで改名した ── 向こうも改名する（ID は同じまま・依頼 492）。
                await window.amber.driveRename({ id: s.id, rel: pre + s.rel });
                const there = remote.find((x) => x.id === s.id);
                done.push({ rel: s.rel, id: s.id, tag: there ? there.tag : '' });
                moved.push(s.rel);
                one.moved += 1;
            } else if (s.do === 'movehere') {
                // 向こうで改名された ── こちらも改名する。
                const from = root + '/' + s.from;
                const wasOpen = !!(state.open && state.open.path === from);
                const r = await ask('syncmove', { path: root, from: s.from, to: s.rel });
                const there = remote.find((x) => x.id === s.id);
                done.push({ rel: r.rel, id: s.id, tag: there ? there.tag : '' });
                afterRename(from, r.path);
                one.moved += 1;
                one.touched = true;
                if (wasOpen) one.openTouched = true;
            } else if (s.do === 'clash') {
                // **両方が変わった ── 混ぜる。** 分かれる前の姿は `synced` が
                // 取っておいたもの（無ければ空 ── ぜんぶがぶつかった場所になり、
                // 両方残って人が選ぶ。失うよりよい）。
                const theirs = await window.amber.driveDownload(s.id);
                const ours = (await ask('read', { path: at })).text;
                const base = s.base ? (await ask('baseread', { path: root, hash: s.base })).text : null;
                const got = await ask('merge', { was: base || '', ours, theirs });
                // 混ぜる前のこちらを履歴に（Git の ORIG_HEAD の写し）。
                try { await ask('keep', { root, path: at, text: ours, gap: 0, force: true }); } catch { /* 履歴が置けなくても混ぜる */ }
                await ask('syncdown', { path: at, text: got.text });
                const print = (await ask('syncprint', { path: at })).print;
                const r = await window.amber.driveUpload({ rel: pre + s.rel, text: got.text, print, id: s.id });
                done.push({ rel: s.rel, id: r.id, tag: r.tag });
                const there = remote.find((x) => x.id === s.id);
                noteIncoming(at, got, there && there.by ? there.by : '向こう');
                one.clash += 1;
                if (got.eyes) one.eyes += 1;
                one.touched = true;
                if (isOpen) one.openTouched = true;
            }
        } catch (e) {
            one.trouble.push(s.rel + ': ' + why(e));
        }
    }
    if (done.length || gone.length) await ask('synced', { path: root, who: 'drive', done, gone, moved });
    return one;
}

/// 向こうと混ぜた印（来た行・ぶつかった場所）を、そのノートに憶えさせる。
/// 開いていれば帯と選び口を出す。
function noteIncoming(path, got, who) {
    const rows = rowsOf(got.text);
    const entry = {
        path,
        came: got.came || [],
        both: got.both || [],
        eyes: !!got.eyes,
        who,
        spots: (got.spots || []).map((sp) => ({
            ours: rows.slice(sp.ours[0], sp.ours[0] + sp.ours[1]),
            theirs: rows.slice(sp.theirs[0], sp.theirs[0] + sp.theirs[1]),
        })),
        fields: got.fields || [],
    };
    incomings[path] = entry;
    window.amber.remember({ incomings });
    if (state.open && state.open.path === path) {
        incoming = entry;
        drawBand();
        paintIncoming();
    }
}

/// 困りごとを、人の言葉と次に押すものに。
function syncTroubleFace(t) {
    const e = String(t || '');
    if (!navigator.onLine || /fetch failed|ENOTFOUND|ECONNREFUSED|ECONNRESET|ETIMEDOUT|ENETUNREACH|EAI_AGAIN|socket hang up|network/i.test(e)) {
        return { text: 'インターネットに繋がっていないようです。次回接続時に同期します。', button: '接続確認する', go: 'retry' };
    }
    if (/サインインが切れ|invalid_grant|unauthorized|401/i.test(e)) {
        return { text: 'Google のサインインが切れています。もう一度サインインしてください。', button: 'Google でサインイン', go: 'signin' };
    }
    if (/insufficient|storage|quota|507/i.test(e)) {
        return { text: 'Google Drive の空きが足りないようです。空けてから、もう一度試してください。', button: 'もう一度試す', go: 'retry' };
    }
    return { text: e, button: 'もう一度試す', go: 'retry' };
}

/// **いまの様子を、一覧の頭に**（依頼 491・本人が決めた案甲・2026-09-11）。
///
/// 色つきの列（`#syncsay`）は3 つのときだけ ── まだ始めていない・困っている・
/// 運んだ直後（数秒）。それ以外は一行（`#syncmark`）。画面の文字は
/// 「アップロード」「ダウンロード」「同期」── 上げる・下ろす・運ぶは中の言葉。
function drawSyncState() {
    const box = el('syncsay');
    const mark = el('syncmark');
    // 会社向けのビルドでは、この二つごと出さない（依頼 602）。
    if (OFFICE) {
        for (const x of [box, mark]) { if (x) { x.hidden = true; x.innerHTML = ''; } }
        return;
    }
    const hhmm = (t) => new Date(t).toTimeString().slice(0, 5);
    const hide = (x) => { x.hidden = true; x.innerHTML = ''; };
    if (state.guest || !state.places.length) { hide(box); hide(mark); return; }
    const who = syncAccount.who || {};

    // **どの保存ディレクトリも「同期しない」なら、灰色の一行**（依頼 511）──
    // サインインを勧めない。同期したくなったら、保存ディレクトリの同期先から。
    if (!state.places.some((p) => p.sync === 'drive')) {
        hide(box);
        el('syncmark').innerHTML = '';
        mark.innerHTML = '<span class="dot off"></span><span class="t">同期していません ・ どの保存ディレクトリも「同期しない」</span>'
            + '<button type="button">同期先を選ぶ</button>';
        mark.querySelector('button').onclick = () => cmdPlaces();
        mark.hidden = false;
        return;
    }

    // 一行のほう。
    const line = (dot, text, act) => {
        mark.innerHTML = '<span class="dot ' + dot + '"></span><span class="t">' + escapeHtml(text) + '</span>'
            + (act ? '<button type="button">' + escapeHtml(act.name) + '</button>' : '');
        if (act) mark.querySelector('button').onclick = act.run;
        mark.hidden = false;
    };
    // 列のほう。
    const column = (cls, head, text, acts) => {
        box.className = cls;
        box.innerHTML = '<b>' + escapeHtml(head) + '</b>' + (text ? '<span>' + escapeHtml(text) + '</span>' : '')
            + (acts && acts.length ? '<div class="act">' + acts.map((a, i) =>
                '<button type="button" data-n="' + i + '"' + (a.quiet ? ' class="quiet"' : '') + '>' + escapeHtml(a.name) + '</button>').join('') + '</div>' : '');
        for (const b of box.querySelectorAll('button')) b.onclick = acts[Number(b.dataset.n)].run;
        box.hidden = false;
    };

    if (!syncAccount.signedIn) {
        if (!syncLater) {
            hide(mark);
            column('before', 'まだ同期していません',
                'ノートはこのパソコンだけにあります。ほかの端末やグループの人と同じノートを使うには、Google でサインインします。',
                [{ name: '同期をはじめる', run: () => cmdSync() },
                 { name: 'あとで', quiet: true, run: () => { syncLater = true; drawSyncState(); } }]);
        } else {
            hide(box);
            line('off', '同期していません ・ ', { name: '同期をはじめる', run: () => cmdSync() });
        }
        return;
    }
    if (syncBusy) {
        hide(box);
        line('busy', '同期しています…');
        return;
    }
    if (syncTrouble) {
        hide(mark);
        const f = syncTroubleFace(syncTrouble);
        column('bad', '同期できません ── ' + hhmm(syncTroubleSince || Date.now()) + ' から', f.text,
            [{ name: f.button, run: async () => {
                if (f.go === 'signin') {
                    try { await window.amber.driveSignOut(); } catch { /* キーはもう死んでいる */ }
                    syncTrouble = ''; syncTroubleSince = null;
                    await cmdSync();
                    return;
                }
                syncNow('手');
            } }]);
        return;
    }
    if (syncFresh) {
        hide(mark);
        const partsOf = (r) => {
            const parts = [];
            if (r.up) parts.push('アップロード' + r.up + '件');
            if (r.down) parts.push('ダウンロード' + r.down + '件');
            if (r.gone) parts.push('ゴミ箱へ' + r.gone + '件');
            if (r.clash) parts.push('同じ行を両方で直したノート' + r.clash + '件');
            if (r.moved) parts.push('名前の変更' + r.moved + '件');
            return parts;
        };
        // 保存ディレクトリが二つ以上なら、**運んだところの名前を頭に**（依頼 511）
        // ── ふだんの一行には出さない。名前が要るのは、何かが動いたときだけ。
        const each = Object.entries(syncFresh.places || {}).filter(([, r]) => partsOf(r).length);
        const text = manyPlaces() && each.length
            ? each.map(([name, r]) => name + ': ' + partsOf(r).join('・')).join('　')
            : partsOf(syncFresh).join('・');
        column('good', '同期しました ── ' + hhmm(syncFresh.at), text, []);
        return;
    }
    hide(box);
    line('', '同期しています' + (syncLast ? ' ・ 最終 ' + hhmm(syncLast) : '') + (who.email ? ' ・ ' + who.email : ''));
}

/// サインインが駄目だったときの言い分を、**人が次に何をすればよいか**の形に。
function signInTrouble(err) {
    const e = String(err || '');
    if (/client_secret/i.test(e)) {
        return 'Google が「クライアント シークレット」を求めています。'
            + '手元のシークレットを ~/Library/Application Support/amber/google.json に'
            + ' {"secret": "…"} の形で置いて、もう一度試してください';
    }
    if (/access_denied/i.test(e)) {
        return 'Google に断られました（アクセスをブロック）。テスト利用者に、いま使ったアカウントを足してください';
    }
    if (/時間切れ/.test(e)) return 'ブラウザで「許可」を押す前に、三分が過ぎました。もう一度試してください';
    if (/安全に置けません/.test(e)) return e;
    return 'サインインできませんでした: ' + e;
}

/// 「同期」。**押すのは3 つ、打つのは Google のパスワードだけ**（本人が決めた・
/// 2026-09-11・案 甲）── amber の中で「Google でサインイン」を押す →
/// ブラウザで「許可」→ デスクトップ版に戻る。URL は打たせない。
async function cmdSync() {
    await loadSync();
    if (!syncAccount.signedIn) {
        const go = await askPick('同期', [
            { name: 'Google でサインイン', sub: 'ブラウザが開きます。「許可」を押したら、このデスクトップ版に戻ってください', value: 'in' },
        ], 'Mac と iPhone で同じノートを使えるようにします。amber が触れるのは、amber が作ったファイルだけです', true);
        if (go !== 'in') return;
        say('ブラウザで Google にサインインしてください…');
        let got;
        try { got = await window.amber.driveSignIn(); } catch (e) { got = { error: why(e) }; }
        if (!got || got.error) {
            // **消えない形で言う。** 帯の一言は数秒で消え、見逃すと「押したのに
            // 何も起きない」にしか見えない（実際に見逃された・2026-09-11）。
            await askPick('サインインできませんでした', [{ name: '閉じる', value: 0 }],
                signInTrouble(got ? got.error : '返事がありません'), true);
            return;
        }
        await loadSync();
        const who = got.who || {};
        say('Google にサインインしました' + (who.email ? '（' + who.email + '）' : ''));
        return;
    }
    const go = await askPick('同期', [
        // iPhone にあってデスクトップ版に無かった一行（2026-09-12 の見直し）。
        { name: 'いま同期する', sub: '三十秒待たずに、いま合わせます', value: 'now' },
        { name: '同期をやめる', sub: 'Google のサインインを外します。ノートは消えません', value: 'out' },
    ], 'いま: ' + syncLabel(), true);
    if (go === 'now') { syncNow('手'); return; }
    if (go !== 'out') return;
    try { await window.amber.driveSignOut(); } catch (e) { say('やめられません: ' + why(e)); return; }
    await loadSync();
    say('同期をやめました');
}

const SYNC_WORDS = { none: '同期しない', drive: 'Google Drive' };

/// ⚙「保存ディレクトリの追加・変更・削除」（依頼 511・本人が決めた三段・2026-09-12）。
/// **一つの入口で全部** ── 一覧 → 一つを選ぶ → 同期先／名前／場所／外す。
/// 同期の入れる切るも、ここ（保存ディレクトリごとに同期先を持つ、という作り）。
async function cmdPlaces() {
    for (;;) {
        // **パスは一行に収める**（依頼 603・本人「保存ディレクトリの見た目が
        // 横に長くなりすぎない？」）── 深いところに置いた人のパスは 80 文字を
        // 越える。3 つ四つ並ぶと、名前よりパスのほうが目立つ。
        // 同期先は、いまの言葉が**「同期しない」でないときだけ**添える
        // ── 何も繋いでいない人の一覧に、同じ文字が四本並ぶ意味は無い。
        const items = state.places.map((p, i) => ({
            name: p.name,
            sub: [shortPath(p.dir),
                  (!OFFICE && p.sync !== 'none') ? SYNC_WORDS[p.sync] : '',
                  state.placeTrouble[p.dir] ? '見つかりません' : '']
                .filter(Boolean).join(' ・ '),
            value: i,
        }));
        items.push({ name: '＋ 保存ディレクトリを追加', sub: 'フォルダを一つ選びます。中の .md がノートになります', value: ' add' });
        const go = await askPick('保存ディレクトリ', items,
            'ノートを置くフォルダ。いくつでも。同期先はフォルダごとに選べます', true);
        if (go === null) return;
        if (go === ' add') { await addPlace(); continue; }
        const p = state.places[go];
        if (!p) return;
        await placeSheet(p);
    }
}

/// 一つの保存ディレクトリのダイアログ（二段目）。
async function placeSheet(p) {
    const go = await askPick(p.name, [
        // 会社向けのビルドには、同期先そのものが無い（依頼 602）。
        ...(OFFICE ? [] : [{ name: '同期先', sub: SYNC_WORDS[p.sync]
            + (p.sync === 'drive' && !syncAccount.signedIn ? '（まだサインインしていません）' : ''), value: 'sync' }]),
        { name: '名前を変える', sub: '一覧での呼び名だけ。フォルダの名前は変わりません', value: 'name' },
        { name: '場所を変える…', sub: 'いままでのノートも一緒に移せます', value: 'dir' },
        { name: '外す', sub: 'ambər の一覧から外します。フォルダとノートはそのまま', value: 'drop' },
    ], shortPath(p.dir), true);
    if (go === 'sync') await placeSyncSheet(p);
    else if (go === 'name') await placeRename(p);
    else if (go === 'dir') await placeMove(p);
    else if (go === 'drop') await placeDrop(p);
}

/// 同期先（三段目）。**同期しない／Google Drive**（iCloud・OneDrive はこれから）。
async function placeSyncSheet(p) {
    const who = syncAccount.who || {};
    const now = (k) => (p.sync === k ? 'いまはこれ' : '');
    const go = await askPick('「' + p.name + '」の同期先', [
        { name: '同期しない', sub: now('none') || 'このパソコンだけに置きます', value: 'none' },
        { name: 'Google Drive', sub: now('drive') || (syncAccount.signedIn ? who.email || '' : 'Google でサインインします'), value: 'drive' },
    ], 'iCloud と OneDrive は、これから', true);
    if (go === null || go === p.sync) return;
    if (go === 'drive' && !syncAccount.signedIn) {
        await cmdSync();
        if (!syncAccount.signedIn) return;
    }
    p.sync = go;
    savePlaces();
    drawRail();
    drawSyncState();
    if (go === 'drive') {
        syncSoon(1000);
        say('「' + p.name + '」を Google Drive と同期します');
    } else {
        say('「' + p.name + '」は同期しません（Drive にあるものはそのままです）');
    }
}

async function placeRename(p) {
    const to = await askText('「' + p.name + '」の新しい呼び名', p.name, 'フォルダの名前は変わりません');
    if (to === null || !to.trim() || to.trim() === p.name) return;
    const name = to.trim();
    if (state.places.some((x) => x !== p && x.name === name)) { say('「' + name + '」はもうあります'); return; }
    p.name = name;
    savePlaces();
    await reload({ quiet: true });
    say('「' + name + '」に変えました');
}

/// 置き場所を選ぶ（足すときも、変えるときも同じダイアログ）。
///
/// **クラウドは名前で選ばせる。** どのサービスも机の上ではただのフォルダ
/// なので、amber は同期の仕組みを一つも知らなくていい ── けれど
/// `~/Library/Mobile Documents/com~apple~CloudDocs` を覚えている人はいない。
/// 入っているものだけ並べる。クラウドの直下には置かない ── 同期フォルダの
/// 根っこにノートをばら撒くと、ほかの物と混ざって二度と分けられない。
async function pickPlaceDir(current) {
    let found = [];
    try { found = await window.amber.clouds(); } catch { /* 一つも無い環境 */ }
    const here = current ? found.find((c) => current.startsWith(c.dir)) : null;
    const items = [
        ...found.map((c) => ({
            name: c.name,
            sub: c.dir === (here || {}).dir ? 'いまここ' : 'この中の「amber」に置く',
            value: c.dir,
        })),
        { name: '別の場所を選ぶ', sub: 'フォルダを一つ選びます', value: ' pick' },
    ];
    const go = await askPick(current ? 'どこへ' : '保存ディレクトリを追加', items,
        current ? 'いま: ' + shortPath(current) + (here ? '（' + here.name + '）' : '')
            : 'クラウドのフォルダを選ぶと、その仕組みで同期されます');
    if (go === null) return null;
    if (go === ' pick') return (await window.amber.pickFolder()) || null;
    const dir = go + '/amber';
    try {
        await ask('place', { dir });
    } catch (e) {
        say('作れません: ' + why(e));
        return null;
    }
    return dir;
}

/// ほかの保存ディレクトリと重なるか（同じ・中・外）。**入れ子にはしない** ──
/// 同じノートが二つの保存ディレクトリから見えると、二度数えて二度運ぶ。
function overlaps(dir, except) {
    return state.places.some((x) => x !== except
        && (x.dir === dir || dir.startsWith(x.dir + '/') || x.dir.startsWith(dir + '/')));
}

async function addPlace() {
    const dir = await pickPlaceDir('');
    if (!dir) return;
    const name = await putPlace(dir);
    if (name) say('「' + name + '」を足しました。同期するなら「' + name + '」→ 同期先 から');
}

/// 保存ディレクトリを一つ足す（名前を返す。足せなければ null）。
/// **足すパスは一つ** ── ⚙ から足すのも、OneNote の書き出し先を足すのも
/// ここを通る（依頼 621）。二本にすると、Drive の上の置き場所の名前を
/// 空けるのを片方だけ忘れる。
async function putPlace(dir) {
    if (state.places.some((x) => x.dir === dir)) { say('「' + bookName(dir) + '」はもう入っています'); return null; }
    if (overlaps(dir)) { say('そこは、ほかの保存ディレクトリと重なります（入れ子にはできません）'); return null; }
    const leaf = leafOf(dir) || 'ambər';
    let name = leaf;
    for (let n = 2; state.places.some((x) => x.name === name); n += 1) name = leaf + ' ' + n;
    // Drive の上の置き場所（`ambər/<at>/`）。いちばん目のフォルダと名前が
    // ぶつかると、向こうの一覧で見分けられない ── 空いている名前にする。
    const taken = new Set([
        ...state.places.map((x) => x.at),
        ...state.books.filter((b) => rootOf(b) === state.root).map((b) => relOf(b).split('/')[0]),
    ]);
    let at = name;
    for (let n = 2; taken.has(at); n += 1) at = name + ' ' + n;
    // **入った直後は同期しない**（本人が決めた・2026-09-12）── 会社の共有
    // フォルダを足した人の一覧を、黙って Drive に上げない。
    state.places.push({ name, dir, sync: 'none', at });
    savePlaces();
    await rewatch();
    await reload({ quiet: true });
    return name;
}

/// 場所を変える（前の「保存場所」と同じ流れ）。
async function placeMove(p) {
    const dir = await pickPlaceDir(p.dir);
    if (!dir || dir === p.dir) return;
    if (overlaps(dir, p) || dir.startsWith(p.dir + '/')) {
        say('そこは、ほかの保存ディレクトリと重なります（入れ子にはできません）');
        return;
    }
    // **いままでのノートは、ひとりでには付いてこない。**
    // 新しいフォルダは空のフォルダで、そうと知らずに移した人は、書いた
    // ものが全部見えなくなったところに立たされる（iPhone は前から訊いて
    // いる ── ウィンドウだけ訊いていなかった）。
    const had = state.notes.filter((n) => n.root === p.dir).length;
    if (had > 0 && await askYes('いままでの ' + had + ' 件を、新しい場所へ移しますか')) {
        try {
            // **数えるのは人が数えるもの。** `migrate` が返すのは動かした
            // ファイルの数（画像も履歴も `.amber` も入る）で、6 件のノートが
            // 「14 件を移しました」になる ── 何が 14 なのか誰も分からない。
            await ask('migrate', { from: p.dir, to: dir });
            say('ノート ' + had + ' 件を、画像と履歴ごと移しました');
        } catch (e) {
            // **移せなくても、保存場所は変えない。** 半分だけ移った状態で
            // 向こうを見せると、残りが消えたようにしか見えない。
            say('移せません: ' + why(e));
            return;
        }
    }
    p.dir = dir;
    savePlaces();
    await rewatch();
    state.open = null;
    applyView();
    await reload({});
    say('保存場所を変えました: ' + shortPath(dir));
}

/// 一覧から外す。**ファイルは消さない**（外すのは ambər の憶えだけ）。
async function placeDrop(p) {
    if (state.places.length < 2) {
        say('最後の一つは外せません（動かすなら「場所を変える…」）');
        return false;
    }
    const n = state.notes.filter((x) => x.root === p.dir).length;
    const ok = await askYes('「' + p.name + '」を ambər から外しますか（' + shortPath(p.dir)
        + ' と、中の ' + n + ' 件のノートはそのまま残ります）');
    if (!ok) return false;
    const inside = (at) => at === p.dir || String(at || '').startsWith(p.dir + '/');
    state.places = state.places.filter((x) => x !== p);
    tabs = tabs.filter((t) => !inside(t.path));
    rememberTabs();
    if (state.open && inside(state.open.path)) { state.open = null; applyView(); }
    if (['book', 'place', 'share'].includes(state.dest.kind) && inside(state.dest.what)) {
        state.dest = { kind: 'all', what: '' };
    }
    savePlaces();
    await rewatch();
    await reload({});
    say('「' + p.name + '」を外しました（フォルダはそのままです）');
    return true;
}

/// いま動いている amber の身元。
///
/// **不具合が人づてに回ってくるから要る。** amber の画面は crmaine の中でも
/// 動いていて、そちらは社内の Windows 端末に配られる ── 戻ってくる報せは
/// たいてい「amber が変です」だけで、どの版のどのエンジンかは書いていない。
/// 押せば読める場所に3 つ（版・エンジン・置き場所）を出しておけば、
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
        // **一つずつ、一行ずつ**（依頼 603）。前はパスを `・` で
        // 繋いだ一本の行だったので、二つ3 つと増えるほど横に伸びて
        // ダイアログからはみ出していた ── 不具合を伝えるときに添える3 つの
        // うち、いちばん読めない一つになっていた。
        ...(state.places.length
            ? state.places.map((p, i) => ({
                name: i === 0 ? '保存ディレクトリ' : '', sub: shortPath(p.dir), value: null }))
            : [{ name: '保存ディレクトリ', sub: '（まだ決めていません）', value: null }]),
    // **`bare` は真を渡す。** 前は渡さず `bare ?? few`（3 つ以下なら隠す）に
    // 任せていたが、保存ディレクトリを一行ずつにした日に**行が四つを越えて
    // 絞り込みの欄が生えた** ── ここは読むだけの紙で、絞る相手がいない。
    // `false` は渡せない（`??` が拾うのは null と undefined だけなので、
    // 偽を渡すと素通りして同じことが起きる）。
    ], '不具合を伝えるときは、この3 つを添えてください', true,
       'Advanced Markdown Browser & Editor for Readability');
}

/* ── 前の姿 ── */

/// **読み直す**（F5・⌘R ── 依頼 604・本人「保存ディレクトリの中身をごっそり削除した
/// ときなど、表示がなかなかアンバー側に伝わらない」）。
///
/// 見張り（`fs.watch`）は、**見張っているフォルダそのものが消えると落ちる**
/// ── そのとき `eyes` から外れるだけで、張り直しはしていない。外で
/// まとめて消した・別の端末の同期が下ろした・ネットワーク越しのフォルダが
/// 一度切れた、のどれでも「画面だけが古いまま」になる。
///
/// **押せば必ず読み直す。** 見張りを張り直してから数え直し、開いている
/// ノートが消えていれば閉じる（無いノートを見せ続けると、次の保存で
/// 作り直してしまう）。
///
/// **打ちかけは触らない。** 読み直しで消えていいのは、まだ書いていない文字
/// ではない ── 打っている最中に押しても、その一本は開いたまま残す。
async function cmdRefresh() {
    say('読み直しています…');
    await rewatch();
    const at = state.open ? state.open.path : null;
    await reload({});
    if (at && !state.dirty && !state.notes.some((n) => n.path === at)) {
        state.open = null;
        applyView();
    }
    say('読み直しました ・ ノート ' + state.notes.length + ' 本');
}

/// サンプルのノートを、いまの保存場所に置く。
///
/// **初回に置けなかった人のための道。** 自動で置くのは、まだ一本も
/// ノートが無いときの一度きり ── 既にノートがある人のフォルダに三枚
/// 落とすと、それはただの散らかし。それでも「入れてくれ」と言える場所が
/// 要る（iPhone の設定にも同じものがある）。
async function cmdWelcome() {
    const go = await askYes('サンプルのノートを、いまの保存場所に入れますか');
    if (!go) return;
    try {
        const r = await window.amber.welcome(state.root);
        await reload({});
        say(r.put ? r.put + ' 件置きました' : 'もう入っています（同じ名前は飛ばしました）');
    } catch (e) {
        say('置けません: ' + why(e));
    }
}

/// ⌘S ── **保存ではなく「ここを残す」。**
///
/// amber は打キーの 0.9 秒後に書いているので、保存という操作が無い。それでも
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
            root: rootOf(state.open.path), path: state.open.path, gap: 0, force: true, kept: true,
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
    const root = rootOf(path);
    let r;
    try {
        r = await ask('history', { root, path });
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
    const note = isBook ? root + '/' + pick.note : path;
    let old;
    try {
        old = (await ask('oldtext', { root, path: note, stamp: pick.stamp })).text;
    } catch (e) {
        say('読めません: ' + why(e));
        return;
    }
    const go = await askPick(pick.when + ' のバージョン', [
        { name: 'このバージョンを見る', sub: '読むだけ。いまのノートは動きません', value: 'peek' },
        { name: 'このバージョンに戻す', sub: 'いまのバージョンも一つ残ります', value: 'back' },
        { name: pick.kept ? '保護をやめる' : 'このバージョンを保護する',
          sub: '保護したものは、古くなっても消えません', value: 'mark' },
    ], shortPath(note) + '  ·  ' + old.length + ' 文字');
    if (go === null) return;
    if (go === 'mark') {
        await ask('keepmark', { root, path: note, stamp: pick.stamp, kept: !pick.kept });
        say(pick.kept ? '保護をやめました' : '保護しました（古くなっても消えません）');
        return;
    }
    if (go === 'peek') {
        await openGuestText(pick.when + ' のバージョン', old);
        return;
    }
    // **戻す前に、いまを一世代残す。** 戻しすぎても戻れるように。
    await ask('keep', { root, path: note, gap: 0, force: true });
    await ask('write', { path: note, text: old, force: true });
    await reload({});
    if (state.open && state.open.path === note) await openNote(note);
    say(pick.when + ' のバージョンに戻しました（いまのバージョンも残してあります）');
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

/* ── ダイアログ ── */

/// 命令を選ぶ・文字を打つ・一つ選ぶ、を 1 つで賄う。
///
/// **3 つ作らない。** 別々に書くと、微妙に違う「Esc で閉じる」が3 つできて、
/// そのうち一つだけ閉じない日が来る。ここが唯一の閉じ方。
///
/// `items` があれば選ぶダイアログ、無ければ文字を打つダイアログ。返すのは選んだ値
/// （または打った文字）で、やめたときは `null`。
let sheetDone = null;

/// 名前を、名前として見せる 1 つ。
///
/// **ここだけ HTML を通す。** 題（`.hd`）と添え書き（`.foot`）は
/// `textContent` のまま ── あそこにはノートの名前が入る（「ゴミ箱へ
/// 入れる」など）ので、HTML にすると人の書いた文字がマークになるパスが開く。
/// 名札はこちらが書いた決め打ちなので、そこだけ通してよい。
const BRAND = 'amb<span class="s">ə</span>r';

/// 一度に描く行の上限。**絞る数ではなく、描く数。**
const SHEET_ROWS = 200;

function sheet({ title, value, placeholder, items, foot, bare, brand }) {
    closeSheet(null);
    const veil = el('veil');
    const input = veil.querySelector('input');
    // **四つまでのときは、文字を打つ欄を見せない。**
    //
    // 四つを絞り込む人はいない。それどころか、「はい／やめる」の二択に
    // 欄が出ていると「何か打つものがある」に見えて、タグを外すだけの返事
    // で人が止まる（実際に止まった ── ゴミ箱へ入れるときも同じだった）。
    //
    // 欄は残す ── 上下と Enter を受けているのはここなので、消すとキーボードで
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
        // 打った文字を、名前のどこかに含むもの。**部分一致** ── 覚えている
        // のはたいてい真ん中の一語で、頭ではない。
        const hit = items.filter((i) => {
            // **合言葉のあるものは、打たれるまで出てこない**（依頼 473）。
            // 初めて amber を見た人の一覧に、会社の話が混ざらないように。
            if (i.word) return q.includes(i.word);
            return !q || (i.name + ' ' + (i.sub || '')).toLowerCase().includes(q);
        });
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
            // **マウスを乗せたら、そこが選び目**（依頼 613・本人「マウスが
            // オンボードされても選択のハイライトが変わらない」）。
            //
            // 前は `.on` をキーボードだけが動かしていたので、「はい」に乗せて
            // 押しているのに、光っているのは「いいえ」のまま ── 押す直前に
            // 画面が言っていることと、押して起きることが食い違う。
            // いちばん怖いのは、まさにここ（ゴミ箱の はい／いいえ）。
            //
            // **描き直さずに、印だけ移す。** 一行ごとに組み直すと、
            // 千行の一覧でマウスを動かすたびに全部を描くことになる。
            row.onmouseenter = () => {
                const k = Number(row.dataset.n);
                if (k === at) return;
                at = k;
                list.querySelector('.it.on')?.classList.remove('on');
                row.classList.add('on');
            };
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
        // 「さかな」→「魚」を Enter で確定した瞬間にダイアログまで閉じ、
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

/// 文字を打つダイアログ。
const askText = (title, value, foot) => sheet({ title, value, foot });
/// 一つ選ぶダイアログ。`items` は `{ name, sub, key, value }`。
/// `bare` が真なら、文字を打つ欄を見せない（押して選ぶだけの一覧）。
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
    // **ボタンの吹き出しに書いてあるキーも、土台の言葉に。**
    //
    // `index.html` に `title="言葉で探す（⌘F）"` と直に書いてあるものが
    // ある ── Windows には `⌘` というキーが無いので、**押しようがない案内**が
    // 出ていた。一度だけ舐めて言い換える（`keyText` と同じ一本を通す）。
    if (!MAC) {
        for (const n of document.querySelectorAll('[title]')) {
            const t = n.getAttribute('title');
            if (!/[⌘⌃⇧⌥]/.test(t)) continue;
            n.setAttribute('title', t.replace(/[⌘⌃⇧⌥][^\s（）()]*/g, (m) => keyText(m)));
        }
    }
    el('blankmark').innerHTML = mark(54);
    // **どの版かを、何かを描く前に決める**（依頼 602）── あとで決めると、
    // 会社向けのビルドでも最初の一瞬だけ Drive のラベルが出る。
    await officeReady;
    const saved = await window.amber.recall();
    // 保存ディレクトリ（依頼 511）。**前の `root` 一つから引き継ぐ** ── 憶えが
    // `places` になっていない机では、いままでの場所が一つ目になる（同期は
    // いままで通り Drive）。ここでは書き戻さない（開いただけで設定を書かない）。
    const places = (Array.isArray(saved.places) ? saved.places : [])
        .filter((p) => p && typeof p.dir === 'string' && p.dir)
        .map((p) => ({
            name: (typeof p.name === 'string' && p.name.trim()) || p.dir.split('/').pop() || 'ambər',
            dir: p.dir,
            sync: p.sync === 'drive' ? 'drive' : 'none',
            at: typeof p.at === 'string' ? p.at : '',
        }));
    if (!places.length && saved.root) {
        places.push({ name: saved.root.split('/').pop() || 'ambər', dir: saved.root, sync: 'drive', at: '' });
    }
    state.places = places;
    state.root = places.length ? places[0].dir : saved.root;
    away = Array.isArray(saved.away) ? saved.away : [];
    if (['month', 'week', 'day'].includes(saved.calView)) calView = saved.calView;
    calGroup = !!saved.calGroup;
    groupCal = saved.group && saved.group.id ? saved.group : null;
    if (['me', 'group', 'both'].includes(saved.calSide)) calSide = saved.calSide;
    groupAsked = !!saved.groupAsked;
    if (Array.isArray(saved.tagsKnown)) tagsKnown = saved.tagsKnown.filter((t) => typeof t === 'string');
    if (Array.isArray(saved.calHide)) calHide = saved.calHide.filter((k) => typeof k === 'string');
    if (saved.calWeekend === false) calWeekend = false;
    // 段の並び順と「自分はこの人」（依頼 562）。
    if (Array.isArray(saved.calOrder)) calOrder = saved.calOrder.filter((k) => typeof k === 'string');
    if (typeof saved.calMe === 'string') calMe = saved.calMe;
    if (saved.calNames && typeof saved.calNames === 'object') calNames = { ...saved.calNames };
    // 出す時間帯（依頼 562）。**筋の通らない組はそのまま受けない** ──
    // 終わりが始まりより前だと、時間が一つも無い表になる。
    if (Number.isInteger(saved.calFrom) && Number.isInteger(saved.calTill)
        && saved.calFrom >= 0 && saved.calTill <= 24 && saved.calTill > saved.calFrom) {
        calFrom = saved.calFrom;
        calTill = saved.calTill;
    }
    if (typeof saved.calHereColor === 'string' && /^#[0-9a-f]{6}$/i.test(saved.calHereColor)) calHereColor = saved.calHereColor;
    if (saved.calColors && typeof saved.calColors === 'object') calColors = saved.calColors;
    paintHereColor();
    if (typeof saved.teamFile === 'string') { teamFile = saved.teamFile; teamClock(); }
    noBins = Array.isArray(saved.noBins) ? saved.noBins : [];
    incomings = (saved.incomings && typeof saved.incomings === 'object') ? saved.incomings : {};
    // 外から動いたら教えてもらう ── 同じフォルダを二つの端末で触るのが
    // このアプリの前提なのに、開き直すまで出てこなかった。
    await rewatch();
    // 時刻の名前のまま残っているノートを、一度だけ題の名前に（依頼 492・決めごと 7）。
    for (const p of state.places) {
        try {
            const got = await ask('tidynames', { path: p.dir });
            for (const r of got.renamed || []) afterRename(r.from, r.to);
        } catch { /* 揃えられなくても開ける */ }
    }
    await loadPalette();
    if (saved.view === 'read' || saved.view === 'split' || saved.view === 'write') {
        view = saved.view;
    }
    moreMarks = !!saved.moreMarks;
    drawMarks();
    // **帯を先に整える。** ノートを一本も開かないまま終わる起動もある
    // （初めて立ち上げた日がそう）── そのとき ⚙ が出ていないと、
    // 保存場所を決めるパスがどこにも無い。
    applyView();
    // 掴んで動かす縦棒（依頼 596）。**憶えた幅を先に戻してから**繋ぐ。
    grabsUp();
    if (typeof saved.railWidth === 'number') setPaneWidth('rail', saved.railWidth);
    if (typeof saved.listWidth === 'number') setPaneWidth('list', saved.listWidth);
    if (saved.railOff) { railOff = true; document.body.classList.add('norail'); }
    if (saved.listOff) { listOff = true; document.body.classList.add('nolist'); }
    // エディタはまだ無い ── 開いたときに入る（`makeEditor` の末尾）。
    if (saved.vim) vimOn = true;
    if (saved.autoSave === false) autoSave = false;
    if (Array.isArray(saved.faces)) usedFaces = saved.faces.slice(0, 24);
    if (typeof saved.fontStep === 'number') fontStep = saved.fontStep;
    // ボタンの薄さは開いた時点で合わせる（エディタができる前から帯は見える）。
    setFont(fontStep, true);
    setCalFont(typeof saved.calFontStep === 'number' ? saved.calFontStep : 0, true);
    if (saved.order) order = saved.order;
    // **向きを憶えていない古い設定でも読める** ── 無ければその物差しの既定。
    if (typeof saved.orderAsc === 'boolean') asc = saved.orderAsc;
    else asc = order === 'title';
    if (saved.tocOn) tocOn = true;
    if (saved.theme) setTheme(knownTheme(saved.theme));
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
    // **開いていた机に戻る。** iPhone が憶えているのと同じ ── 閉じて開いたら
    // 一本きりに戻るのでは、机として使えない。
    //
    // 憶えたパスのうち**いまフォルダにあるものだけ**を並べる（消えた・移した本を
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
