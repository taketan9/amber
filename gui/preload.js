'use strict';
// 描く側と、OS に触れる側の間の、細い一本。
//
// **`contextIsolation` は入れたまま。** 描く側は Node を持たない ── 開くのは
// 人が書いたノートで、その中身が Node に届く経路は作らない。

const { contextBridge, ipcRenderer, webUtils } = require('electron');

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
    /// 読める形の中のリンクを、外のブラウザへ。`http`/`https` だけ通る。
    openLink: (url) => ipcRenderer.invoke('amber:openLink', url),
    /// 選んでもらったファイルの中身（base64）と拡張子。
    fileBytes: (file) => ipcRenderer.invoke('amber:fileBytes', file),
    /// 外から渡された `.md`（コマンドラインか「このアプリで開く」）。
    onGuest: (fn) => ipcRenderer.on('amber:openGuest', (_e, at) => fn(at)),
    /// 窓に落とされたファイルの場所。
    ///
    /// **Electron 32 から `File.path` は無い。** 描く側に「好きなファイルの
    /// 道を知る」口は作らず、**落とされた `File` を渡して訊く**だけにする。
    pathOf: (file) => {
        try {
            return webUtils.getPathForFile(file) || null;
        } catch {
            return null;
        }
    },
    /// ゴミ箱へ入れる（消さない）。
    trash: (at) => ipcRenderer.invoke('amber:trash', at),
    /// 書き出す。行き先は人が選ぶ。
    saveText: (name, text) => ipcRenderer.invoke('amber:saveText', name, text),
    savePDF: (name, html) => ipcRenderer.invoke('amber:savePDF', name, html),
    /// 期日の来た通知（走っている間だけ）。
    ring: (title, body) => ipcRenderer.invoke('amber:ring', title, body),
    /// 貼り付けられた画像の生バイト（base64）。
    clipboardImage: () => ipcRenderer.invoke('amber:clipboardImage'),
});
