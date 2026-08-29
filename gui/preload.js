'use strict';
// The only thing the renderer can reach. One function, and it can only ask the
// engine — no filesystem, no child processes, no `require`.

const { contextBridge, ipcRenderer, webUtils } = require('electron');

contextBridge.exposeInMainWorld('cian', {
    /// Call an engine method. Rejects with the engine's own message, which is
    /// written for a person and goes straight into a dialog.
    call: async (method, params) => {
        const reply = await ipcRenderer.invoke('cian', method, params);
        if (reply.error) throw new Error(reply.error);
        return reply.ok;
    },
    /// Where a dropped file actually is.
    ///
    /// A `File` from a drop no longer carries `.path` — Electron took that
    /// away, and rightly: a page that can read the path of anything dragged
    /// over it is a page that can read anything. `webUtils.getPathForFile` is
    /// the replacement, and it lives here rather than in the page because the
    /// page must not hold `webUtils` itself.
    pathOf: (file) => {
        try {
            return webUtils.getPathForFile(file);
        } catch {
            return null;
        }
    },
    /// Listen for what the engine says unasked. The callback is handed the
    /// message itself and nothing else — no event object, which would carry a
    /// sender the renderer has no business holding.
    onEvent: (fn) => {
        ipcRenderer.on('cian-event', (_e, msg) => fn(msg));
    },
});
