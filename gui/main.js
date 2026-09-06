'use strict';
// amber の窓。
//
// **持っているのは三つだけ**: 窓、エンジンの子、憶えごとの一枚。
// ファイラの土台（ペイン・シェル・アーカイブ）は持ってこない ── amber は
// 2画面ファイラではないので、要らないものを継ぐと、そこから太る。

const { app, BrowserWindow, ipcMain, dialog, clipboard, nativeTheme,
        nativeImage, shell, Notification } = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');

const { Engine } = require('./engine');

// **名前を先に決める。** これが `userData` の置き場所を決めるので、
// 決めないと憶えごとが `.../Electron/` に入り、Electron を使う他のものと
// 同じ引き出しを共有することになる。
app.setName('amber');

/// 渡された `.md` は、開いたら**単発で**出す。
///
/// 二つの道から来る:
///
///   * `amber note.md`（`npm start -- note.md` も）── `process.argv`
///   * Finder の「このアプリケーションで開く」── `open-file`（mac だけで、
///     **`whenReady` より先に鳴ることがある**ので、来た順に溜めておく）
///
/// 置き場所は動かさない ── 一本開くたびに一覧が入れ替わると、
/// 「さっきまでのノートが消えた」に見える。
const guests = [];
function noteArg(list) {
    return list.find((a) => typeof a === 'string' && /\.(md|markdown|txt)$/i.test(a)
        && !a.startsWith('-'));
}
const fromArgv = noteArg(process.argv.slice(1));
if (fromArgv) guests.push(path.resolve(fromArgv));
app.on('open-file', (e, at) => {
    e.preventDefault();
    guests.push(at);
    if (win) win.webContents.send('amber:openGuest', at);
});

/// アプリの印。`packaging/amber_icon.py` が焼いたもの。
///
/// **macOS では窓の `icon` は効かない** ── Dock の絵は束ねたときの
/// `.icns` から来る。走らせて確かめている間は Electron の絵が出たままなので、
/// `dock.setIcon` で上書きする。Windows と Linux は窓の `icon` が効く。
///
/// Dock にだけ別の絵を渡すのは**余白のため**。Apple の升目では 1024 のうち
/// 絵は 824 で、まわりの 100 は空けておく決まりになっている。余白の無い
/// `amber.png` を Dock に置くと、隣のアイコンより一回り大きく見える。
///
/// `.icns` ではなく PNG なのは、**Electron が `.icns` を読めない**から ──
/// `createFromPath` は空の画像を黙って返し、`setIcon` は何も言わずに何も
/// しない。空かどうかは下で確かめる。
const ICON = path.join(__dirname, '..', 'packaging', 'amber.png');
const DOCK_ICON = path.join(__dirname, '..', 'packaging', 'amber-dock.png');

let win = null;
let engine = null;

/// 憶えごとの置き場所。**ノートの中には書かない** ── ノートはただの Markdown
/// で、同期先で別の端末と出会っても、窓の都合が混ざらない。
const STATE = path.join(app.getPath('userData'), 'amber.json');

function recall() {
    try {
        return JSON.parse(fs.readFileSync(STATE, 'utf8'));
    } catch {
        return {};
    }
}

function remember(patch) {
    const now = { ...recall(), ...patch };
    try {
        fs.mkdirSync(path.dirname(STATE), { recursive: true });
        fs.writeFileSync(STATE, JSON.stringify(now, null, 2));
    } catch (e) {
        // 憶えられないのは不便だが、書けないことで窓が閉じる理由はない。
        console.error('憶えられませんでした:', e.message);
    }
    return now;
}

/// 机の上にある、クラウドのフォルダ。**あるものだけ返す。**
///
/// 入っていないサービスを並べると、押した人は「入れれば使える」のか
/// 「amber が壊れている」のか見分けられない ── 見えるのは、いまこの機械に
/// 実際に置かれているフォルダだけ。
///
/// macOS 12 以降、外のサービスは**ぜんぶ `~/Library/CloudStorage` に並ぶ**
/// （Dropbox も Google Drive も OneDrive も）。昔ながらの `~/Dropbox` も
/// 残っている機械があるので、両方見て同じ場所は一度だけ出す。
function clouds() {
    const home = os.homedir();
    const out = [];
    const seen = new Set();
    const add = (name, dir) => {
        if (!dir) return;
        let real;
        try {
            if (!fs.statSync(dir).isDirectory()) return;
            real = fs.realpathSync(dir);
        } catch {
            return;
        }
        if (seen.has(real)) return;
        seen.add(real);
        out.push({ name, dir });
    };

    if (process.platform === 'darwin') {
        add('iCloud Drive', path.join(home, 'Library', 'Mobile Documents', 'com~apple~CloudDocs'));
        const box = path.join(home, 'Library', 'CloudStorage');
        try {
            for (const name of fs.readdirSync(box).sort()) {
                if (name.startsWith('.')) continue;
                add(cloudName(name), path.join(box, name));
            }
        } catch { /* 一つも入っていない機械 */ }
    } else if (process.platform === 'win32') {
        add('iCloud Drive', path.join(home, 'iCloudDrive'));
        add('OneDrive', process.env.OneDrive || process.env.OneDriveConsumer);
        add('OneDrive（会社・学校）', process.env.OneDriveCommercial);
        add('Google Drive', 'G:\\My Drive');
    }
    // どの土台でも見る、昔ながらの置き場所。
    add('Dropbox', path.join(home, 'Dropbox'));
    add('Google Drive', path.join(home, 'Google Drive'));
    add('OneDrive', path.join(home, 'OneDrive'));
    return out;
}

/// `GoogleDrive-taketan@example.com` のような機械の名前を、人の言葉に。
function cloudName(raw) {
    const [head, ...rest] = raw.split('-');
    const who = rest.join('-');
    const known = {
        GoogleDrive: 'Google Drive',
        OneDrive: 'OneDrive',
        Dropbox: 'Dropbox',
        Box: 'Box',
        pCloud: 'pCloud',
    }[head] || head;
    // `OneDrive-Personal` の「Personal」は、名前ではなく種類 ── 出さない。
    if (!who || who === 'Personal') return known;
    return known + '（' + who + '）';
}

/// 初めて開いたときの置き場所。
///
/// **`~/Documents/cian` が既にあるならそれを使う。** 分ける前からノートは
/// そこにあり、名前が変わったからといって彼のノートを置き去りにする理由は
/// 無い。無ければ `~/Documents/amber` を作る。
function firstRoot() {
    const docs = path.join(os.homedir(), 'Documents');
    const old = path.join(docs, 'cian');
    if (fs.existsSync(old) && fs.statSync(old).isDirectory()) return old;
    const fresh = path.join(docs, 'amber');
    fs.mkdirSync(fresh, { recursive: true });
    return fresh;
}

/// 初めて開いた人に、**空の窓を見せない。**
///
/// 何も無い一覧を前にすると、Markdown を知らない人は「何ができるのか」を
/// どこからも知れない ── 説明を読ませるより、**読めて・押せて・書き換えられる
/// ノートが最初から入っている**ほうが早い（Inkdrop がそうしている）。
///
/// 置くのは一度きり。**消したら二度と戻さない** ── 邪魔だから消したものが
/// 起動のたびに生えるのは、いちばん嫌われる作りかた。だから「空かどうか」
/// ではなく「**置いたことがあるか**」で決める（置いた直後に全部消して閉じた
/// 人にも、次の起動で生えない）。
function seedWelcome() {
    if (recall().seeded) return;
    const from = path.join(__dirname, '..', 'packaging', 'welcome');
    // 見本そのものが無い置かれ方（同梱の仕方によってはありうる）。
    // **憶えないまま帰る** ── 憶えてしまうと、あとで同梱された日に置けない。
    if (!fs.existsSync(from)) return;
    const root = recall().root || firstRoot();
    try {
        // **既にノートがあるなら置かない。** `~/Documents/cian` を引き継いだ
        // 人や、置き場所を自分のフォルダに向けている人の一覧に、見本が
        // 混ざるのはただの散らかし。それでも「置いた」ことにする ──
        // あとで空にした日に生えてこないように。
        if (!hasNotes(root)) {
            for (const at of walk(from)) {
                const to = path.join(root, path.relative(from, at));
                // **上書きはしない。** 同じ名前の自分のノートを消す道は作らない。
                if (fs.existsSync(to)) continue;
                fs.mkdirSync(path.dirname(to), { recursive: true });
                fs.copyFileSync(at, to);
            }
            console.log('見本のノートを置きました:', root);
        }
        // 憶えるのは**置き終えてから** ── 途中で転んだ回は、次の起動で
        // もう一度試せる（既にあるものは飛ばすので、二重にはならない）。
        remember({ seeded: true });
    } catch (e) {
        // 見本が置けないことで、窓が開かない理由はない。
        console.error('見本を置けませんでした:', e.message);
    }
}

/// 下まで見て、`.md` が一枚でもあるか。
function hasNotes(dir) {
    for (const at of walk(dir)) if (at.toLowerCase().endsWith('.md')) return true;
    return false;
}

/// フォルダの中のファイルを、下まで一つずつ。
function* walk(dir, depth = 0) {
    if (depth > 6) return;
    let kids;
    try {
        kids = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
        return;
    }
    for (const k of kids) {
        if (k.name.startsWith('.')) continue;
        const at = path.join(dir, k.name);
        if (k.isDirectory()) yield* walk(at, depth + 1);
        else if (k.isFile()) yield at;
    }
}

function makeWindow() {
    const saved = recall();
    win = new BrowserWindow({
        width: saved.width || 1180,
        height: saved.height || 760,
        minWidth: 720,
        minHeight: 440,
        title: 'amber',
        icon: ICON,
        backgroundColor: nativeTheme.shouldUseDarkColors ? '#17140f' : '#fbf7ef',
        // 題字は窓の中に描く。OS の帯を残すと、三列の上にもう一段増える。
        titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'default',
        webPreferences: {
            preload: path.join(__dirname, 'preload.js'),
            contextIsolation: true,
            nodeIntegration: false,
            spellcheck: false,
        },
    });
    // **描く側の例外を、端末にも出す。**
    //
    // これが無いと、読み込みの途中で落ちたときに窓が真っ白になるだけで、
    // 端末には何も出ない ── 開発者ツールを開くまで、何が起きたのか
    // 分からない（実際に一度、白い窓を前に十分ほど探した）。
    win.webContents.on('console-message', (_e, level, message, line, at) => {
        if (level < 2) return;                 // 0=log 1=info 2=warning 3=error
        console.error(`[窓] ${message}` + (at ? `  (${at}:${line})` : ''));
    });
    win.loadFile(path.join(__dirname, 'index.html'));
    // 描く側が立ち上がってから渡す ── 先に送っても受け取る耳がない。
    win.webContents.once('did-finish-load', () => {
        for (const at of guests.splice(0)) win.webContents.send('amber:openGuest', at);
    });
    // 大きさは閉じるときではなく、変わったときに憶える ── 落ちた回の大きさが
    // 失われるのが惜しいのではなく、閉じ際の書き込みが死んだパイプに当たる
    // 事故を一つ減らせる。
    let t = null;
    const size = () => {
        clearTimeout(t);
        t = setTimeout(() => {
            if (!win || win.isDestroyed()) return;
            const [width, height] = win.getSize();
            remember({ width, height });
        }, 400);
    };
    win.on('resize', size);
    win.on('closed', () => { win = null; });

    // 撮って終わる。**`AMBER_SHOT` が無いと何も起きない** ── 見て確かめる
    // ための口で、人が使う道には出ない。cian の drive.js の代わりに、まずは
    // これだけ（一枚撮れれば「開いたか」「どう見えるか」は分かる）。
    if (process.env.AMBER_SHOT) {
        win.webContents.once('did-finish-load', () => {
            setTimeout(async () => {
                try {
                    // **撮る前に、一手だけ打てる**（`AMBER_DO`）。開いた姿は
                    // 一枚で分かるが、押して出るもの（工房や小窓）は押さないと
                    // 写らない ── 押す道が無いせいで、目で確かめないまま
                    // 出すことになるのがいちばん惜しい。
                    if (process.env.AMBER_DO) {
                        await win.webContents.executeJavaScript(process.env.AMBER_DO, true);
                        await new Promise((go) =>
                            setTimeout(go, Number(process.env.AMBER_DO_WAIT || 900)));
                    }
                    const img = await win.webContents.capturePage();
                    fs.writeFileSync(process.env.AMBER_SHOT, img.toPNG());
                    console.log('撮りました:', process.env.AMBER_SHOT);
                } catch (e) {
                    console.error('撮れません:', e.message);
                }
                app.quit();
            }, Number(process.env.AMBER_SHOT_WAIT || 2500));
        });
    }
}

/// ノートのフォルダを見張る。
///
/// **同じフォルダを二つの端末で触るのがこのアプリの前提**なのに、外から
/// 書き換えたものは窓を開き直すまで出てこなかった ── iPhone で書いた一行が
/// Mac に現れず、「同期していない」ように見える（同期はしていて、見ていな
/// かっただけ）。
///
/// 細かく数えない。**「何か動いた」だけを伝えて、数え直すのは描く側**の
/// 仕事にする ── どのファイルがどう変わったかを OS ごとに読み解くのは、
/// 出来事の抜けと重複を両方引き受けることになる。
///
/// まとめて一度だけ送る。保存の一回は数十の出来事になる（書いて、名前を
/// 変えて、属性を触って）ので、そのたびに数え直すと打っている最中に一覧が
/// 何度も跳ねる。
let eyes = null;
let eyesAt = '';

function watch(root) {
    if (eyesAt === root && eyes) return;
    if (eyes) { eyes.close(); eyes = null; }
    eyesAt = root;
    if (!root || !fs.existsSync(root)) return;
    let hold = null;
    try {
        // `recursive` は mac と Windows にはあり、Linux には無い ──
        // 無いところでは根の一段だけになる（それでも無いよりよい）。
        eyes = fs.watch(root, { recursive: true, persistent: false }, (_kind, name) => {
            // 自分の一時ファイルで起こさない。
            if (name && /(^|[\\/])\.|\.tmp$|~$/.test(name)) return;
            clearTimeout(hold);
            hold = setTimeout(() => {
                if (win && !win.isDestroyed()) win.webContents.send('amber:changed');
            }, 400);
        });
        eyes.on('error', () => { eyes = null; });
    } catch (e) {
        // 見張れないフォルダ（ネットワーク越しなど）はある。**開かない
        // 理由にはならない** ── 開き直せば読める、という前の姿に戻るだけ。
        console.error('見張れません:', e.message);
    }
}

app.whenReady().then(() => {
    // 一覧を訊かれる前に置く ── あとから置くと、最初の一覧が空のまま出る。
    seedWelcome();
    engine = new Engine();
    ipcMain.handle('amber:call', async (_e, method, params) => engine.call(method, params));
    ipcMain.handle('amber:recall', () => ({ root: firstRoot(), ...recall() }));
    ipcMain.handle('amber:appVersion', () => app.getVersion());
    // 描く側が置き場所を決めたら、そこを見張る。
    ipcMain.handle('amber:watch', (_e, root) => { watch(root); });
    /// 見本のノートを、言われた場所へ。**上書きはしない。**
    ipcMain.handle('amber:welcome', (_e, root) => {
        const from = path.join(__dirname, '..', 'packaging', 'welcome');
        if (!fs.existsSync(from)) throw new Error('見本が入っていません');
        let put = 0;
        for (const at of walk(from)) {
            const to = path.join(root, path.relative(from, at));
            if (fs.existsSync(to)) continue;
            fs.mkdirSync(path.dirname(to), { recursive: true });
            fs.copyFileSync(at, to);
            put++;
        }
        return { put };
    });
    ipcMain.handle('amber:remember', (_e, patch) => remember(patch));
    // **どのクラウドに置くかを、選べるようにする。**
    //
    // どのサービスも机の上では「ただのフォルダ」なので、amber は同期の
    // 仕組みを一つも知らなくていい ── 知る必要があるのは**そのフォルダが
    // どこにあるか**だけ。`~/Library/Mobile Documents/com~apple~CloudDocs`
    // を覚えている人はいない。
    ipcMain.handle('amber:clouds', () => clouds());

    // **「あとはクラウドで分けてください」を、押せる形にする。**
    // フォルダを人に分けるのはクラウドの画面の仕事だが、そこまで人に
    // 探させない ── その場所を開いて、選ばれた状態で見せる。
    ipcMain.handle('amber:reveal', (_e, at) => {
        try { shell.showItemInFolder(at); return true; } catch { return false; }
    });

    // 名乗りの下書き。**この機械が既に知っていることを、もう一度打たせない。**
    ipcMain.handle('amber:userName', () => {
        try { return os.userInfo().username || ''; } catch { return ''; }
    });

    ipcMain.handle('amber:pickFolder', async () => {
        const r = await dialog.showOpenDialog(win, {
            title: 'ノートの置き場所を選ぶ',
            properties: ['openDirectory', 'createDirectory'],
        });
        return r.canceled ? null : r.filePaths[0];
    });
    ipcMain.handle('amber:pickFile', async (_e, filters) => {
        const r = await dialog.showOpenDialog(win, {
            properties: ['openFile'],
            filters: filters || [],
        });
        return r.canceled ? null : r.filePaths[0];
    });
    ipcMain.handle('amber:saveAs', async (_e, name) => {
        const r = await dialog.showSaveDialog(win, { defaultPath: name });
        return r.canceled ? null : r.filePath;
    });
    /// 読める形の中のリンクを、既定のブラウザで開く。
    ///
    /// **窓の中で開かせない。** `<a href>` をそのまま踏ませると、窓が
    /// ノートから離れて別の頁になり、戻る道が無い（題字は窓の中に描いて
    /// いるので、ブラウザの戻るボタンも無い）。開くのは `http`/`https`
    /// だけ ── ノートは人が書いたもので、`file:` や独自の scheme を
    /// 押しただけで何かが起きるのは、ノートに許して良い力ではない。
    ipcMain.handle('amber:openLink', (_e, url) => {
        try {
            const u = new URL(String(url));
            if (u.protocol !== 'http:' && u.protocol !== 'https:') return false;
            shell.openExternal(u.href);
            return true;
        } catch {
            return false;
        }
    });
    /// 選んでもらったファイルの中身。**開く側が選んだものだけ** ──
    /// 描く側から好きな道を読ませない（ノートは人が書いたもので、その中身が
    /// ファイルを読む力を持つ理由は無い）。
    ipcMain.handle('amber:fileBytes', async (_e, file) => {
        try {
            const b = await fs.promises.readFile(String(file));
            return { b64: b.toString('base64'), ext: path.extname(String(file)).slice(1) };
        } catch (e) {
            return null;
        }
    });
    /// ゴミ箱へ入れる。
    ///
    /// **`core` の `delete` は消してしまう**（`remove_file`）。机のある
    /// ところでは、消したものが戻せないのは強すぎる ── 電話には
    /// ゴミ箱が無いので core は消すが、窓はここを通す。ディレクトリも
    /// そのまま入る。
    ipcMain.handle('amber:trash', async (_e, at) => {
        try {
            await shell.trashItem(String(at));
            return true;
        } catch (e) {
            console.error('ゴミ箱へ入れられません:', e.message);
            return false;
        }
    });

    /// 字をファイルに書き出す。**行き先は人が選ぶ** ── 描く側が道を
    /// 決められると、ノートの中身が好きなところへ書ける口になる。
    /// 読むだけの一本を、その場に置く。
    ///
    /// **前の姿を「見てから決める」ために要る。** 保存の小窓を出すのは
    /// 違う ── 見たいだけなのに、どこに置くかを訊かれる。ノートの外の
    /// 一時置き場なので、索引にも見張りにも入らない。
    ipcMain.handle('amber:scratch', async (_e, name, text) => {
        const dir = path.join(os.tmpdir(), 'amber-peek-' + process.pid);
        await fs.promises.mkdir(dir, { recursive: true });
        const at = path.join(dir, String(name).replace(/[/\\:]/g, '-'));
        await fs.promises.writeFile(at, String(text), 'utf8');
        return at;
    });

    ipcMain.handle('amber:saveText', async (_e, name, text) => {
        const r = await dialog.showSaveDialog(win, { defaultPath: String(name) });
        if (r.canceled || !r.filePath) return null;
        await fs.promises.writeFile(r.filePath, String(text), 'utf8');
        return r.filePath;
    });

    /// 読める形を PDF にする。
    ///
    /// **窓そのものを刷らない。** `win.webContents.printToPDF` は三列ごと
    /// 刷ってしまう（左の一覧まで PDF に入る）。見えない窓をもう一つ建てて、
    /// 読める形だけを入れて刷る。
    ///
    /// data: の URL ではなく一時ファイルを読ませる ── Chromium は data: を
    /// 最上位の遷移として渡すと黙って空の頁にすることがある。
    ipcMain.handle('amber:savePDF', async (_e, name, html) => {
        const r = await dialog.showSaveDialog(win, { defaultPath: String(name) });
        if (r.canceled || !r.filePath) return null;
        const tmp = path.join(app.getPath('temp'), `amber-${Date.now()}.html`);
        await fs.promises.writeFile(tmp, String(html), 'utf8');
        const sheet = new BrowserWindow({
            show: false,
            webPreferences: { offscreen: true, javascript: false },
        });
        try {
            await sheet.loadFile(tmp);
            const pdf = await sheet.webContents.printToPDF({
                printBackground: true,
                margins: { marginType: 'custom', top: 0.6, bottom: 0.6, left: 0.6, right: 0.6 },
            });
            await fs.promises.writeFile(r.filePath, pdf);
        } finally {
            sheet.destroy();
            fs.promises.unlink(tmp).catch(() => {});
        }
        return r.filePath;
    });

    /// 期日の来た通知を、走っている間だけ鳴らす。
    ///
    /// **仕掛けるのは窓でもできるが、鳴らすのは電話の仕事。** 窓は閉じて
    /// いる時間のほうが長く、閉じている間の時刻は誰も見ていない ── ここで
    /// 鳴るのは「開いているうちに来た分」だけだと、はっきり言っておく。
    ipcMain.handle('amber:ring', (_e, title, body) => {
        if (!Notification.isSupported()) return false;
        new Notification({ title: String(title), body: String(body) }).show();
        return true;
    });

    ipcMain.handle('amber:clipboardImage', () => {
        const img = clipboard.readImage();
        if (img.isEmpty()) return null;
        return { b64: img.toPNG().toString('base64'), ext: 'png' };
    });
    if (process.platform === 'darwin' && app.dock) {
        try {
            const img = nativeImage.createFromPath(DOCK_ICON);
            if (img.isEmpty()) throw new Error(`読めません: ${DOCK_ICON}`);
            app.dock.setIcon(img);
        } catch (e) {
            // 印が出ないのは不便だが、そのために窓が開かない理由はない。
            console.error('印を置けませんでした:', e.message);
        }
    }
    makeWindow();
    app.on('activate', () => {
        if (BrowserWindow.getAllWindows().length === 0) makeWindow();
    });
});

app.on('window-all-closed', () => {
    if (engine) engine.stop();
    if (process.platform !== 'darwin') app.quit();
});
