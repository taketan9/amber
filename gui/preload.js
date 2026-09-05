'use strict';
// 描く側と、OS に触れる側の間の、細い一本。
//
// **`contextIsolation` は入れたまま。** 描く側は Node を持たない ── 開くのは
// 人が書いたノートで、その中身が Node に届く経路は作らない。

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('amber', {
    /// エンジンに訊く。`{ ok }` か例外。
    call: (method, params) => ipcRenderer.invoke('amber:call', method, params),
    /// 憶えておくもの（開いていた場所・大きさ・見た目）。窓の側の話で、
    /// ノートの中には書かない ── ノートはただの Markdown のままにする。
    recall: () => ipcRenderer.invoke('amber:recall'),
    remember: (patch) => ipcRenderer.invoke('amber:remember', patch),
    /// OS のダイアログ。描く側からは開けない。
    pickFolder: () => ipcRenderer.invoke('amber:pickFolder'),
    pickFile: (filters) => ipcRenderer.invoke('amber:pickFile', filters),
    saveAs: (name) => ipcRenderer.invoke('amber:saveAs', name),
    /// 貼り付けられた画像の生バイト（base64）。
    clipboardImage: () => ipcRenderer.invoke('amber:clipboardImage'),
});
