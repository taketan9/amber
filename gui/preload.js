'use strict';
// The only thing the renderer can reach. One function, and it can only ask the
// engine — no filesystem, no child processes, no `require`.

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('cian', {
    /// Call an engine method. Rejects with the engine's own message, which is
    /// written for a person and goes straight into a dialog.
    call: async (method, params) => {
        const reply = await ipcRenderer.invoke('cian', method, params);
        if (reply.error) throw new Error(reply.error);
        return reply.ok;
    },
});
