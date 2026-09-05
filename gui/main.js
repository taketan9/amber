'use strict';
// amber の窓。
//
// **持っているのは三つだけ**: 窓、エンジンの子、憶えごとの一枚。
// ファイラの土台（ペイン・シェル・アーカイブ）は持ってこない ── amber は
// 2画面ファイラではないので、要らないものを継ぐと、そこから太る。

const { app, BrowserWindow, ipcMain, dialog, clipboard, nativeTheme } = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');

const { Engine } = require('./engine');

// **名前を先に決める。** これが `userData` の置き場所を決めるので、
// 決めないと憶えごとが `.../Electron/` に入り、Electron を使う他のものと
// 同じ引き出しを共有することになる。
app.setName('amber');

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
    win.loadFile(path.join(__dirname, 'index.html'));
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
    ipcMain.handle('amber:clipboardImage', () => {
        const img = clipboard.readImage();
        if (img.isEmpty()) return null;
        return { b64: img.toPNG().toString('base64'), ext: 'png' };
    });
    makeWindow();
    app.on('activate', () => {
        if (BrowserWindow.getAllWindows().length === 0) makeWindow();
    });
});

app.on('window-all-closed', () => {
    if (engine) engine.stop();
    if (process.platform !== 'darwin') app.quit();
});
