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
    return win;
}

app.whenReady().then(() => {
    engine = new Engine(process.argv[2] || os.homedir());
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
    createWindow();

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
