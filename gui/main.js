'use strict';
// The Electron side: one window, and the bridge from it to the engine.
//
// The renderer gets no Node at all — `contextIsolation` on, `nodeIntegration`
// off, and one narrow channel through the preload. A file manager runs whatever
// the disk hands it; the window that draws the listing has no business being
// able to read the disk itself.

const { app, BrowserWindow, Menu, ipcMain } = require('electron');
const path = require('node:path');
const os = require('node:os');
const { Engine } = require('./engine');

let engine = null;

/// The frame's colour before the page paints, matched to the saved look so
/// the window does not flash the wrong ground on the way up. It was a fixed
/// dark — right for a dark theme and exactly wrong for 白磁, the default.
const GROUNDS = { hakuji: '#f7f8f8', inei: '#14110f', terminal: '#0c0c0c' };

function createWindow(ground) {
    const win = new BrowserWindow({
        width: 1200,
        height: 800,
        backgroundColor: ground,
        title: 'cian',
        webPreferences: {
            preload: path.join(__dirname, 'preload.js'),
            contextIsolation: true,
            nodeIntegration: false,
            sandbox: false,
        },
    });
    win.loadFile(path.join(__dirname, 'index.html'));

    // The window's own console, in the terminal that started it.
    //
    // A renderer throws into a devtools nobody has open, and the process that
    // launched it prints nothing at all — so a key that quietly does nothing
    // looks the same as a key that is not bound. In-house there may be no
    // devtools habit at all, and this is the only trail.
    win.webContents.on('console-message', (_e, level, message, line, source) => {
        const where = source ? `${source.split('/').pop()}:${line}` : 'renderer';
        const how = level >= 2 ? console.error : console.log;
        how(`[${where}] ${message}`);
    });
    return win;
}

/// The menu bar, decided rather than inherited.
///
/// **Electron installs a default menu when none is set, and on Windows that
/// menu owns Ctrl+A, Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+Z, Ctrl+Y and Ctrl+R.**
/// Those are seven of cian's keys, and a menu accelerator takes the keystroke
/// before the page ever sees it — so mark-all, the file clipboard and redo
/// would have been dead on the only platform this build is for. It cannot be
/// seen from a Mac, where that same default menu is on Cmd instead.
///
/// So: no menu at all off macOS. cian is a full-screen keyboard program and
/// the bar is a row of pixels it has no use for.
///
/// macOS keeps one, and keeps Edit in it. Not for the look — without an Edit
/// menu, Cmd+C and Cmd+V stop working inside text fields on macOS, which is a
/// platform behaviour rather than a choice. Cmd there means "the text in this
/// field", which is what a Mac user means by it; cian's own bindings take
/// Ctrl as well, and Ctrl is what the hands this is built for use.
function installMenu() {
    if (process.platform !== 'darwin') {
        Menu.setApplicationMenu(null);
        return;
    }
    Menu.setApplicationMenu(Menu.buildFromTemplate([
        { role: 'appMenu' },
        { role: 'editMenu' },
        { role: 'windowMenu' },
    ]));
}

app.whenReady().then(async () => {
    installMenu();
    // The first plain argument is where to start; anything beginning with a
    // dash belongs to Chromium and may turn up anywhere in the line. Taking
    // argv[2] whatever it was meant that adding `--remote-debugging-port` gave
    // the engine a switch as its starting directory, and the window came up
    // empty with the reason only in a stream nobody was reading.
    const where = process.argv.slice(2).find((a) => !a.startsWith('-'));
    engine = new Engine(where || os.homedir());
    // Every call from the renderer, forwarded whole. The engine names its own
    // methods; this does not want a case per method that would need editing
    // each time one is added.
    ipcMain.handle('cian', async (_event, method, params) => {
        try {
            return { ok: await engine.call(method, params) };
        } catch (e) {
            return { error: String(e.message || e) };
        }
    });
    // One quick question before the frame exists: which ground was saved.
    // Bounded, because a window that waits on a wedged engine is worse than a
    // window that flashes.
    let ground = GROUNDS.hakuji;
    try {
        const s = await Promise.race([
            engine.call('settings', {}),
            new Promise((_, no) => setTimeout(() => no(new Error('slow')), 1500)),
        ]);
        ground = GROUNDS[s && s.look] || ground;
    } catch { /* the default ground */ }
    const win = createWindow(ground);
    // The engine's unasked lines go straight to the window. Nothing here
    // interprets them; a progress count is the renderer's business.
    engine.onEvent = (msg) => {
        if (!win.isDestroyed()) win.webContents.send('cian-event', msg);
    };

    app.on('activate', () => {
        if (BrowserWindow.getAllWindows().length === 0) createWindow(ground);
    });
});

app.on('window-all-closed', () => {
    if (engine) engine.stop();
    // macOS keeps an app alive with no windows; everywhere else this is the end.
    if (process.platform !== 'darwin') app.quit();
});

app.on('before-quit', () => {
    if (engine) engine.stop();
});
