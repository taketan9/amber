#!/usr/bin/env node
/* **偽の Google Drive**。手元に立てて、同期の上げ下ろしを Google 無しで回す。
 *
 *     node scripts/fake-drive.js 8790      # 立てて待つ
 *     const { start } = require('./fake-drive'); const { url, close } = await start();
 *
 * 喋るのは amber が使うぶんだけ（`gui/drive.js` が叩く形そのまま）:
 *
 *   POST  /token                                   鍵の交換（サインインの試験）
 *   GET   /about                                   誰か
 *   GET   /drive/v3/files?q=…                      一覧（appProperties で絞る）
 *   POST  /drive/v3/files                          フォルダを作る（JSON）
 *   POST  /upload/drive/v3/files?uploadType=multipart      新しく上げる
 *   PATCH /upload/drive/v3/files/:id?uploadType=multipart  上書き
 *   GET   /drive/v3/files/:id?alt=media            下ろす
 *   PATCH /drive/v3/files/:id                      ゴミ箱へ（trashed: true）
 *
 * それと、**向こうの端末を演じる口**（Google には無い）:
 *
 *   POST /_put   { rel, text, by }   向こうが一本置いた（あれば上書き）
 *   GET  /_get?rel=…                 いま向こうにある字
 *   GET  /_list                      ぜんぶ
 *   POST /_trash { rel }             向こうで消した
 *   POST /_reset                     まっさらに
 *   POST /_break { on }              繋がらない体（Drive の口だけ、返事をせずに切る）
 *
 * **判断はしない。** 置かれたものを置かれたまま持つだけ。
 */
'use strict';
const http = require('node:http');

function start(port = 0) {
    let files = new Map();     // id → { id, name, mimeType, parents, appProperties, text, trashed, md5 }
    let seq = 0;
    let broken = false;        // 繋がらない体（`/_break`）
    const md5 = (t) => require('node:crypto').createHash('md5').update(t).digest('hex');
    const id = () => 'f' + (++seq).toString(36).padStart(6, '0');

    const byRel = (rel) => [...files.values()].find((f) => f.appProperties && f.appProperties.rel === rel && !f.trashed);
    const meta = (f) => ({ id: f.id, name: f.name, mimeType: f.mimeType, parents: f.parents,
        appProperties: f.appProperties, md5Checksum: f.md5, trashed: f.trashed, version: String(f.version || 1) });

    /// multipart/related を、metadata（JSON）と中身（字）に割る。
    function parts(body, type) {
        const m = /boundary="?([^";]+)"?/.exec(type || '');
        if (!m) return null;
        const chunks = body.split('--' + m[1]).filter((c) => c.trim() && c.trim() !== '--');
        const out = [];
        for (const c of chunks) {
            const at = c.indexOf('\r\n\r\n');
            const head = c.slice(0, at);
            let text = c.slice(at + 4);
            if (text.endsWith('\r\n')) text = text.slice(0, -2);
            out.push({ head, text });
        }
        return out;
    }

    const server = http.createServer(async (req, res) => {
        let body = '';
        for await (const c of req) body += c;
        const u = new URL(req.url, 'http://127.0.0.1');
        const json = (code, obj) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(obj)); };
        try {
            // ── サインインの試験のための口 ──
            if (u.pathname === '/token') {
                const form = Object.fromEntries(new URLSearchParams(body));
                return json(200, { access_token: 'fake-' + (form.grant_type || ''), refresh_token: 'fake-refresh', expires_in: 3600, scope: 'drive.file' });
            }
            if (u.pathname === '/about') return json(200, { user: { displayName: '試し 太郎', emailAddress: 'taro@example.com' } });

            // ── 向こうの端末を演じる口 ──
            if (u.pathname === '/_reset') { files = new Map(); broken = false; return json(200, { ok: true }); }
            if (u.pathname === '/_break') { broken = !!JSON.parse(body || '{}').on; return json(200, { broken }); }
            if (u.pathname === '/_list') return json(200, [...files.values()].filter((f) => !f.trashed).map(meta));
            if (u.pathname === '/_get') {
                const f = byRel(u.searchParams.get('rel'));
                return f ? json(200, { text: f.text, ...meta(f) }) : json(404, { error: 'ありません' });
            }
            if (u.pathname === '/_put') {
                const { rel, text, by } = JSON.parse(body || '{}');
                let f = byRel(rel);
                const print = 'x' + md5(text);
                if (f) { f.text = text; f.md5 = md5(text); f.version = (f.version || 1) + 1; f.appProperties = { ...f.appProperties, print, by: by || '太郎の iPhone' }; }
                else {
                    f = { id: id(), name: rel.split('/').pop(), mimeType: 'text/markdown', parents: ['root'], version: 1,
                          appProperties: { amber: 'note', rel, print, by: by || '太郎の iPhone' }, text, md5: md5(text), trashed: false };
                    files.set(f.id, f);
                }
                return json(200, meta(f));
            }
            if (u.pathname === '/_trash') {
                const { rel } = JSON.parse(body || '{}');
                const f = byRel(rel);
                if (f) f.trashed = true;
                return json(200, { ok: !!f });
            }

            // ── Drive の API（使うぶんだけ） ──
            // 繋がらない体 ── 返事をせずに切る（窓には fetch failed に見える）。
            if (broken) { req.socket.destroy(); return; }
            if (req.method === 'GET' && u.pathname === '/drive/v3/files') {
                const q = u.searchParams.get('q') || '';
                let out = [...files.values()];
                if (q.includes("value='note'")) out = out.filter((f) => f.appProperties && f.appProperties.amber === 'note');
                if (q.includes("value='dir'")) out = out.filter((f) => f.appProperties && f.appProperties.amber === 'dir');
                if (q.includes('trashed=false') || q.includes('trashed = false')) out = out.filter((f) => !f.trashed);
                const nm = /name\s*=\s*'([^']+)'/.exec(q);
                if (nm) out = out.filter((f) => f.name === nm[1]);
                return json(200, { files: out.map(meta) });
            }
            if (req.method === 'POST' && u.pathname === '/drive/v3/files') {
                const m = JSON.parse(body || '{}');
                const f = { id: id(), name: m.name, mimeType: m.mimeType, parents: m.parents || ['root'],
                            appProperties: m.appProperties || {}, text: '', md5: '', trashed: false, version: 1 };
                files.set(f.id, f);
                return json(200, meta(f));
            }
            const up = /^\/upload\/drive\/v3\/files(?:\/([^/?]+))?$/.exec(u.pathname);
            if (up && (req.method === 'POST' || req.method === 'PATCH')) {
                const ps = parts(body, req.headers['content-type']);
                if (!ps || ps.length < 2) return json(400, { error: { message: 'multipart が読めません' } });
                const m = JSON.parse(ps[0].text || '{}');
                const text = ps[1].text;
                let f = up[1] ? files.get(up[1]) : null;
                if (up[1] && !f) return json(404, { error: { message: 'ありません' } });
                if (!f) {
                    f = { id: id(), name: m.name, mimeType: m.mimeType || 'text/markdown', parents: m.parents || ['root'],
                          appProperties: m.appProperties || {}, text: '', md5: '', trashed: false, version: 0 };
                    files.set(f.id, f);
                }
                if (m.name) f.name = m.name;
                if (m.appProperties) f.appProperties = { ...f.appProperties, ...m.appProperties };
                f.text = text;
                f.md5 = md5(text);
                f.version = (f.version || 0) + 1;
                return json(200, meta(f));
            }
            const one = /^\/drive\/v3\/files\/([^/?]+)$/.exec(u.pathname);
            if (one) {
                const f = files.get(one[1]);
                if (!f) return json(404, { error: { message: 'ありません' } });
                if (req.method === 'GET' && u.searchParams.get('alt') === 'media') {
                    res.writeHead(200, { 'content-type': 'text/markdown; charset=utf-8' });
                    return res.end(f.text);
                }
                if (req.method === 'GET') return json(200, meta(f));
                if (req.method === 'PATCH') {
                    const m = JSON.parse(body || '{}');
                    if (m.trashed !== undefined) f.trashed = !!m.trashed;
                    if (m.name) f.name = m.name;
                    return json(200, meta(f));
                }
                if (req.method === 'DELETE') { files.delete(f.id); res.writeHead(204); return res.end(); }
            }
            return json(404, { error: { message: '知らない道: ' + req.method + ' ' + u.pathname } });
        } catch (e) {
            return json(500, { error: { message: e.message } });
        }
    });
    return new Promise((go) => {
        server.listen(port, '127.0.0.1', () => {
            const url = 'http://127.0.0.1:' + server.address().port;
            go({ url, close: () => server.close(), files: () => files });
        });
    });
}

module.exports = { start };

if (require.main === module) {
    start(Number(process.argv[2] || 0)).then(({ url }) => {
        console.log(url);
    });
}
