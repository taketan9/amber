'use strict';
// The Electron side: one window, and the bridge from it to the engine.
//
// The renderer gets no Node at all — `contextIsolation` on, `nodeIntegration`
// off, and one narrow channel through the preload. A file manager runs whatever
// the disk hands it; the window that draws the listing has no business being
// able to read the disk itself.

const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('node:path');
const os = require('node:os');
const { Engine } = require('./engine');

let engine = null;

function createWindow() {
    const win = new BrowserWindow({
        width: 1200,
        height: 800,
        // The listing is dark before the stylesheet loads either way; without
        // this the frame flashes white on the way up.
        backgroundColor: '#11131a',
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

app.whenReady().then(() => {
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
    const win = createWindow();
    // The engine's unasked lines go straight to the window. Nothing here
    // interprets them; a progress count is the renderer's business.
    engine.onEvent = (msg) => {
        if (!win.isDestroyed()) win.webContents.send('cian-event', msg);
    };

    app.on('activate', () => {
        if (BrowserWindow.getAllWindows().length === 0) createWindow();
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
