'use strict';
// Google Drive との繋ぎ ── **OS に触る側**（窓の主・Node）。
//
// ここにあるのは「サインインする・鍵を持つ・鍵を新しくする・やめる」だけ。
// 何を上げ下ろしするかの判断は core（`sync::plan`）で、ここは運ぶだけ
// （`PLANS.ja.md` 一章「通信は core に入れない」）。
//
// # 使う人がやること
//
// 「Google でサインイン」を押す → いつものブラウザが開く → 「許可」を押す
// → 窓に戻る。以上（本人が決めた・2026-09-11・案 甲）。URL を打たせない、
// Google の設定画面を触らせない。
//
// # 仕組み（OAuth 2.0・PKCE・折り返しは 127.0.0.1）
//
// amber は「公開クライアント」（RFC 8252）── 秘密を持てない前提の設計で、
// 盗まれて困る鍵は **PKCE**（毎回その場で作る使い捨ての合言葉）に置き換わって
// いる。クライアント ID は秘密ではない（Joplin も同じ形で焼き込んで配っている）。
//
//   1. 使い捨ての合言葉（verifier）と、その要約（challenge）を作る
//   2. 127.0.0.1 の空いている番号で、一度だけ返事を受ける小さな口を開く
//   3. ブラウザで Google の許可の画面へ（challenge と折り返し先を添えて）
//   4. Google がブラウザを折り返し先へ戻す ── そこに「code」が付いている
//   5. code と verifier を Google に渡して、鍵（token）と交換する
//
// # 鍵の置き場所
//
// `userData/drive.token`。**暗号化して置く**（Electron の `safeStorage` ──
// mac はキーチェーン、Windows は DPAPI）。ノートの隣には置かない ── あそこは
// フォルダと一緒に旅をする。
//
// # クライアント シークレット
//
// Google はデスクトップ向けの登録にもシークレットを発行し、「インストール型の
// シークレットは秘密として扱わない」と書いている。**repo には置かない。**
// 要るなら `userData/google.json` の `{"secret": "..."}` から読む（本人の
// 手元にだけある）。PKCE だけで通るなら、置かなくてよい。

const crypto = require('node:crypto');
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');

/// 登録済みのクライアント（`PLANS.ja.md` 一章）。**秘密ではない。**
const CLIENT_ID = '306373349806-bskgnk86ciamokeeblmoqgi3t88sqqmf.apps.googleusercontent.com';
const SCOPE = 'https://www.googleapis.com/auth/drive.file';
// **カレンダーの許可は、要る瞬間に足す**（依頼 525）。ノートの同期しか
// 使わない人に、カレンダーの許可を訊かない ── 同意の画面に並ぶ数が増える
// ほど、押す前に引き返す人が増える。`calendar.app.created` は「アプリが
// 自分で作った二次カレンダーだけ」で、**非機密**（審査が要らない）。
const CAL_SCOPE = 'https://www.googleapis.com/auth/calendar.app.created';
const AUTH_URL = 'https://accounts.google.com/o/oauth2/v2/auth';
const TOKEN_URL = 'https://oauth2.googleapis.com/token';
const REVOKE_URL = 'https://oauth2.googleapis.com/revoke';
const ABOUT_URL = 'https://www.googleapis.com/drive/v3/about?fields=user';
const API_URL = 'https://www.googleapis.com';
/// Drive の中の、amber の置き場所の名前。**名前で選ぶ人のために**（Drive の
/// 画面で見たとき、ここにあると分かる）。
const HOME_NAME = 'ambər';

const b64url = (buf) => Buffer.from(buf).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

/// PKCE の合言葉と要約。
function pkce() {
    const verifier = b64url(crypto.randomBytes(32));
    const challenge = b64url(crypto.createHash('sha256').update(verifier).digest());
    return { verifier, challenge };
}

/// 許可の画面の URL。
function authUrl({ clientId, redirect, challenge, state, authUrl, scope }) {
    const u = new URL(authUrl || AUTH_URL);
    u.searchParams.set('client_id', clientId || CLIENT_ID);
    u.searchParams.set('redirect_uri', redirect);
    u.searchParams.set('response_type', 'code');
    u.searchParams.set('scope', scope || SCOPE);
    // **前に貰った許可を落とさない。** これが無いと、カレンダーの許可を
    // 足しにいった瞬間に Drive の許可が消えて、同期が黙って止まる。
    u.searchParams.set('include_granted_scopes', 'true');
    u.searchParams.set('code_challenge', challenge);
    u.searchParams.set('code_challenge_method', 'S256');
    // **戻す鍵（refresh token）をもらう。** 無いと一時間で切れて、毎日
    // サインインさせることになる。
    u.searchParams.set('access_type', 'offline');
    u.searchParams.set('prompt', 'consent');
    u.searchParams.set('state', state);
    return u.href;
}

/// ブラウザに見せる、折り返しの一枚。**普通の日本語で、一言だけ。**
function landing(ok) {
    const say = ok
        ? 'サインインできました。ambər に戻ってください。このタブは閉じてかまいません。'
        : 'サインインできませんでした。ambər に戻って、もう一度お試しください。';
    return '<!doctype html><meta charset="utf-8"><title>ambər</title>'
        + '<body style="font-family:-apple-system,Hiragino Sans,sans-serif;padding:3rem;line-height:1.8;color:#2a2011;background:#fffdf8">'
        + '<p style="font-size:1.2rem">' + say + '</p></body>';
}

/// 繋ぎを作る。**OS の部品は外から渡す**（試験では偽物を渡せるように）。
///
/// - `open(url)`      ── ブラウザで開く（窓では `shell.openExternal`）
/// - `vault`          ── 鍵の置き場所（`{ dir, encrypt, decrypt }`）
/// - `secretFile`     ── クライアント シークレットの置き場所（無ければ無し）
/// - `fetch`          ── 既定は Node の fetch
function createDrive(opts) {
    const {
        open,
        vault,
        clientId = CLIENT_ID,
        tokenUrl = TOKEN_URL,
        authUrl: authAt = AUTH_URL,
        revokeUrl = REVOKE_URL,
        aboutUrl = ABOUT_URL,
        apiUrl = API_URL,
        fetch: doFetch = globalThis.fetch,
        patience = 180_000,
        /// この端末の名前（向こうの端末に「誰の版か」と見せる札）。
        by = 'Mac',
        /// 試験のための鍵（あれば、サインイン無しで使える）。
        fakeToken = null,
    } = opts;
    const tokenFile = path.join(vault.dir, 'drive.token');
    const secretFile = path.join(vault.dir, 'google.json');

    function secret() {
        try {
            const j = JSON.parse(fs.readFileSync(secretFile, 'utf8'));
            return typeof j.secret === 'string' && j.secret ? j.secret : null;
        } catch {
            return null;
        }
    }

    /// 持っている鍵（無ければ null）。
    function load() {
        if (fakeToken) {
            return { access: fakeToken, refresh: null, until: Date.now() + 3_600_000,
                     who: { name: '試し', email: 'test@example.com' } };
        }
        try {
            const raw = fs.readFileSync(tokenFile);
            return JSON.parse(vault.decrypt(raw));
        } catch {
            return null;
        }
    }

    function store(tok) {
        fs.mkdirSync(vault.dir, { recursive: true });
        fs.writeFileSync(tokenFile, vault.encrypt(JSON.stringify(tok)));
    }

    function forget() {
        try { fs.unlinkSync(tokenFile); } catch { /* もう無い */ }
    }

    /// Google と鍵を交換する（初回は code、以後は refresh）。
    async function exchange(body) {
        const form = new URLSearchParams({ client_id: clientId, ...body });
        const s = secret();
        if (s) form.set('client_secret', s);
        const r = await doFetch(tokenUrl, {
            method: 'POST',
            headers: { 'content-type': 'application/x-www-form-urlencoded' },
            body: form.toString(),
        });
        const j = await r.json().catch(() => ({}));
        if (!r.ok || !j.access_token) {
            const why = j.error_description || j.error || ('HTTP ' + r.status);
            throw new Error(why);
        }
        return j;
    }

    /// **サインイン。** ブラウザを開き、折り返しを待ち、鍵を交換して仕舞う。
    /// 返すのは `{ ok: true, who }` か `{ error }`（人に見せる言い分）。
    async function signIn(want = {}) {
        const scope = want.scope || SCOPE;
        const { verifier, challenge } = pkce();
        const state = b64url(crypto.randomBytes(16));
        let settle;
        const got = new Promise((go) => { settle = go; });
        // **ブラウザへの返事は、鍵の交換が済んでから。** 先に「できました」と
        // 出しておいて交換で断られると、ブラウザは成功・窓は失敗の顔になる
        // （実際にそうなった・2026-09-11）。
        let browser = null;
        const tell = (ok) => {
            if (!browser) return;
            browser.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
            browser.end(landing(ok));
            browser = null;
        };
        const server = http.createServer((req, res) => {
            const u = new URL(req.url, 'http://127.0.0.1');
            if (u.pathname !== '/') { res.writeHead(404); res.end(); return; }
            const ok = u.searchParams.get('state') === state && u.searchParams.get('code');
            browser = res;
            if (!ok) {
                tell(false);
                settle({ error: u.searchParams.get('error') || '返事の形が違います' });
                return;
            }
            settle({ code: u.searchParams.get('code') });
        });
        await new Promise((go, no) => {
            server.once('error', no);
            server.listen(0, '127.0.0.1', go);
        });
        const port = server.address().port;
        const redirect = 'http://127.0.0.1:' + port;
        const timer = setTimeout(() => settle({ error: '時間切れです（三分待ちました）' }), patience);
        try {
            await open(authUrl({ clientId, redirect, challenge, state, authUrl: authAt, scope }));
            const back = await got;
            if (back.error) return { error: back.error };
            const tok = await exchange({
                code: back.code, code_verifier: verifier,
                grant_type: 'authorization_code', redirect_uri: redirect,
            });
            const now = Date.now();
            const kept = {
                access: tok.access_token,
                refresh: tok.refresh_token || null,
                until: now + Math.max(60, Number(tok.expires_in || 3600) - 60) * 1000,
                // **貰えた許可をそのまま憶える。** こちらが頼んだものではなく
                // 向こうが返したものを持つ ── 頼んだのに断られた許可を
                // 「持っている」と思い込むと、使う瞬間まで気づけない。
                scope: tok.scope || scope,
                who: null,
            };
            kept.who = await whoAmI(kept.access).catch(() => null);
            store(kept);
            tell(true);
            return { ok: true, who: kept.who };
        } catch (e) {
            // 端末にも残す ── 窓の一言は消えるが、`run.sh` の端末には残る。
            console.error('[同期] サインインできません:', e.message);
            tell(false);
            return { error: e.message };
        } finally {
            clearTimeout(timer);
            server.close();
        }
    }

    /// 誰としてサインインしているか（表示名とメール）。
    async function whoAmI(access) {
        const r = await doFetch(aboutUrl, { headers: { authorization: 'Bearer ' + access } });
        if (!r.ok) throw new Error('HTTP ' + r.status);
        const j = await r.json();
        const u = j.user || {};
        return { name: u.displayName || '', email: u.emailAddress || '' };
    }

    /// いま使える鍵。**切れそうなら黙って新しくする。** 無ければ null。
    async function token() {
        const kept = load();
        if (!kept) return null;
        if (Date.now() < kept.until) return kept.access;
        if (!kept.refresh) return null;
        try {
            const tok = await exchange({ refresh_token: kept.refresh, grant_type: 'refresh_token' });
            const next = {
                ...kept,
                access: tok.access_token,
                until: Date.now() + Math.max(60, Number(tok.expires_in || 3600) - 60) * 1000,
            };
            store(next);
            return next.access;
        } catch {
            return null;
        }
    }

    /// その許可を持っているか。**使う前に訊く** ── 持っていないまま叩くと、
    /// 人には「HTTP 403」としか見えない。
    function grants(one) {
        const kept = load();
        if (!kept) return false;
        return String(kept.scope || '').split(/\s+/).includes(one);
    }

    /// サインインの様子。
    function account() {
        const kept = load();
        return kept ? { signedIn: true, who: kept.who, expired: Date.now() >= kept.until && !kept.refresh }
                    : { signedIn: false };
    }

    /// やめる ── Google 側の許可も取り消して、鍵を捨てる。
    async function signOut() {
        const kept = load();
        forget();
        if (kept) {
            try {
                await doFetch(revokeUrl + '?token=' + encodeURIComponent(kept.refresh || kept.access), { method: 'POST' });
            } catch { /* 取り消せなくても、こちらの鍵は捨てた */ }
        }
        return { ok: true };
    }

    /* ── Drive を読み書きする（運ぶだけ。何を運ぶかは core が決める） ── */

    /// Drive の API を一つ叩く。鍵が無ければ、人の言葉で断る。
    async function api(pathAndQuery, init = {}) {
        const access = await token();
        if (!access) throw new Error('同期のサインインが切れています。⚙ の「同期」からもう一度サインインしてください');
        const r = await doFetch(apiUrl + pathAndQuery, {
            ...init,
            headers: { ...(init.headers || {}), authorization: 'Bearer ' + access },
        });
        if (r.status === 204) return null;
        if (init.bytes && r.ok) return Buffer.from(await r.arrayBuffer());
        const text = await r.text();
        if (!r.ok) {
            let why = 'HTTP ' + r.status;
            try { why = JSON.parse(text).error.message || why; } catch { /* 字のまま */ }
            throw new Error(why);
        }
        return init.raw ? text : (text ? JSON.parse(text) : null);
    }

    const q = (s) => encodeURIComponent(s);
    let homeId = null;
    const dirIds = new Map();      // 'フォルダ/入れ子' → id

    /// amber の置き場所（無ければ作る）。
    async function home() {
        if (homeId) return homeId;
        const got = await api('/drive/v3/files?q=' + q(`name='${HOME_NAME}' and mimeType='application/vnd.google-apps.folder' and trashed=false and 'root' in parents`) + '&fields=files(id,name)');
        if (got.files && got.files.length) { homeId = got.files[0].id; return homeId; }
        const made = await api('/drive/v3/files?fields=id', {
            method: 'POST', headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ name: HOME_NAME, mimeType: 'application/vnd.google-apps.folder', parents: ['root'],
                                   appProperties: { amber: 'home' } }),
        });
        homeId = made.id;
        return homeId;
    }

    /// ノートのフォルダ（`グループ/買い物`）を Drive の上にも作る ── Drive の画面で
    /// 見る人のため。札（`rel`）が本物で、フォルダは見た目。
    async function dir(relDir) {
        if (!relDir) return home();
        if (dirIds.has(relDir)) return dirIds.get(relDir);
        if (!dirIds.size) {
            // 一度だけ、amber が作ったフォルダをぜんぶ読む。
            const got = await api('/drive/v3/files?q=' + q(`appProperties has { key='amber' and value='dir' } and trashed=false`) + '&fields=files(id,appProperties)&pageSize=1000');
            for (const f of got.files || []) if (f.appProperties && f.appProperties.rel) dirIds.set(f.appProperties.rel, f.id);
            if (dirIds.has(relDir)) return dirIds.get(relDir);
        }
        const up = relDir.includes('/') ? relDir.slice(0, relDir.lastIndexOf('/')) : '';
        const parent = await dir(up);
        const made = await api('/drive/v3/files?fields=id', {
            method: 'POST', headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ name: relDir.split('/').pop(), mimeType: 'application/vnd.google-apps.folder', parents: [parent],
                                   appProperties: { amber: 'dir', rel: relDir } }),
        });
        dirIds.set(relDir, made.id);
        return made.id;
    }

    /// 向こうにあるノートの一覧 ── `[{ rel, id, tag, by }]`。
    /// `tag` は上げた側が付けた指紋（無ければ Drive の md5）。
    async function list() {
        const out = [];
        let pageToken = '';
        do {
            const got = await api('/drive/v3/files?q=' + q(`appProperties has { key='amber' and value='note' } and trashed=false`)
                + '&fields=nextPageToken,files(id,name,md5Checksum,appProperties,ownedByMe)&pageSize=1000'
                + (pageToken ? '&pageToken=' + q(pageToken) : ''));
            for (const f of got.files || []) {
                const ap = f.appProperties || {};
                if (!ap.rel) continue;
                // **自分のものだけ。** appProperties は同じアプリなら人をまたいで見えるので、
                // グループの人が共有してくれたノートまで「向こうにある」と数えてしまう ── 同じ道に
                // 下りてきて、こちらの上げは自分の ambər へ行く（二つに割れる）。グループと
                // 分けるのは別の道（共有フォルダを保存ディレクトリにする・依頼 521 で）。
                if (f.ownedByMe === false) continue;
                out.push({ rel: ap.rel, id: f.id, tag: ap.print || f.md5Checksum || '', by: ap.by || '' });
            }
            pageToken = got.nextPageToken || '';
        } while (pageToken);
        return out;
    }

    /// 一本上げる（`id` があれば上書き）。返すのは `{ id, tag }`。
    /// 画像の種類（拡張子から）。知らなければ octet-stream。
    function mimeOf(name) {
        const e = (name.split('.').pop() || '').toLowerCase();
        return { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp',
                 heic: 'image/heic', bmp: 'image/bmp', svg: 'image/svg+xml' }[e] || 'application/octet-stream';
    }

    /// 一本上げる（`id` があれば上書き）。字（`text`）か、画像（`bytes`・依頼 497）。返すのは `{ id, tag }`。
    async function upload({ rel, text, bytes, print, id }) {
        const relDir = rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : '';
        const parent = id ? null : await dir(relDir);
        const mime = bytes ? mimeOf(rel) : 'text/markdown';
        const meta = { name: rel.split('/').pop(), mimeType: mime,
                       appProperties: { amber: 'note', rel, print, by } };
        if (parent) meta.parents = [parent];
        const boundary = 'amber' + crypto.randomBytes(8).toString('hex');
        const head = Buffer.from('--' + boundary + '\r\ncontent-type: application/json; charset=UTF-8\r\n\r\n'
            + JSON.stringify(meta) + '\r\n--' + boundary + '\r\ncontent-type: ' + mime
            + (bytes ? '' : '; charset=UTF-8') + '\r\n\r\n', 'utf8');
        const tail = Buffer.from('\r\n--' + boundary + '--', 'utf8');
        const body = Buffer.concat([head, bytes ? Buffer.from(bytes) : Buffer.from(text, 'utf8'), tail]);
        const path = '/upload/drive/v3/files' + (id ? '/' + q(id) : '') + '?uploadType=multipart&fields=id,md5Checksum';
        const got = await api(path, { method: id ? 'PATCH' : 'POST',
            headers: { 'content-type': 'multipart/related; boundary=' + boundary }, body });
        return { id: got.id, tag: print };
    }

    /// 一本下ろす（字）。
    async function download(id) {
        return api('/drive/v3/files/' + q(id) + '?alt=media', { raw: true });
    }

    /// 一本下ろす（画像・そのままの bytes）。
    async function downloadBytes(id) {
        return api('/drive/v3/files/' + q(id) + '?alt=media', { bytes: true });
    }

    /// 向こうの名前を変える（依頼 492）── 中身は運ばない。フォルダが変わる
    /// なら親も付け替える。**ID は同じまま**（履歴も向こうの版も繋がったまま）。
    async function rename({ id, rel }) {
        const name = rel.split('/').pop();
        const relDir = rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : '';
        const now = await api('/drive/v3/files/' + q(id) + '?fields=parents');
        const want = await dir(relDir);
        const had = (now && now.parents) || [];
        let query = '';
        if (want && !had.includes(want)) {
            query = '&addParents=' + q(want) + (had.length ? '&removeParents=' + q(had.join(',')) : '');
        }
        await api('/drive/v3/files/' + q(id) + '?fields=id' + query, { method: 'PATCH',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ name, appProperties: { rel } }) });
        return { ok: true };
    }

    /// 向こうで消す ── **ゴミ箱へ**（消さない。人が Drive で拾える）。
    async function trash(id) {
        await api('/drive/v3/files/' + q(id), { method: 'PATCH',
            headers: { 'content-type': 'application/json' }, body: JSON.stringify({ trashed: true }) });
        return { ok: true };
    }

    return { signIn, signOut, account, token, whoAmI, grants, tokenFile, secretFile,
             list, upload, download, downloadBytes, rename, trash, home, by };
}

module.exports = { createDrive, pkce, authUrl, landing, CLIENT_ID, SCOPE, CAL_SCOPE };
