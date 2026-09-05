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

app.whenReady().then(() => {
    engine = new Engine();
    ipcMain.handle('amber:call', async (_e, method, params) => engine.call(method, params));
    ipcMain.handle('amber:recall', () => ({ root: firstRoot(), ...recall() }));
    ipcMain.handle('amber:remember', (_e, patch) => remember(patch));
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
