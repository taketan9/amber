#!/usr/bin/env node
/* Google Drive のサインイン（`gui/drive.js`）を、**Google 無しで**一周させる。
 *
 *     node scripts/drive-test.js
 *
 * 偽の Google（キーを交換する口・誰かを答える口）を手元に立て、「ブラウザで
 * 開く」の代わりに折り返し先へ自分で戻る。見るのは:
 *
 *   一。折り返しに `state` が付いていて、違う `state` は受けないこと
 *   二。キーの交換に **PKCE の合言葉（code_verifier）** が付いていること
 *   三。キーが置かれ、切れたら refresh で黙って新しくなること
 *   四。やめるとキーが消え、取り消しの口が叩かれること
 */
'use strict';
const http = require('node:http');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createDrive, pkce, CAL_SCOPE } = require('../gui/drive');
const { createCal, shareUrl, SETTINGS_URL } = require('../gui/gcal');

let bad = 0;
const ok = (yes, what, got) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (got !== undefined) console.log('      ' + JSON.stringify(got)); }
};

(async () => {
    // ── 偽の Google ──
    const seen = { token: [], revoke: [], about: 0, cals: new Map(), made: 0 };
    let expiresIn = 3600;
    const fake = http.createServer(async (req, res) => {
        let body = '';
        for await (const c of req) body += c;
        if (req.url.startsWith('/token')) {
            const form = Object.fromEntries(new URLSearchParams(body));
            seen.token.push(form);
            res.writeHead(200, { 'content-type': 'application/json' });
            res.end(JSON.stringify({ access_token: 'acc-' + seen.token.length, refresh_token: form.grant_type === 'authorization_code' ? 'ref-1' : undefined, expires_in: expiresIn, scope: 'drive.file' }));
        } else if (req.url.startsWith('/revoke')) {
            seen.revoke.push(new URL(req.url, 'http://x').searchParams.get('token'));
            res.writeHead(200); res.end('{}');
        } else if (req.url.startsWith('/about')) {
            seen.about++;
            res.writeHead(200, { 'content-type': 'application/json' });
            res.end(JSON.stringify({ user: { displayName: '試し 太郎', emailAddress: 'taro@example.com' } }));
        } else if (req.url.startsWith('/cal/calendars')) {
            const id = decodeURIComponent(req.url.slice('/cal/calendars'.length).replace(/^\//, ''));
            if (req.method === 'POST') {
                const made = JSON.parse(body || '{}');
                const newId = 'c' + (++seen.made) + '@group.calendar.google.com';
                seen.cals.set(newId, { id: newId, summary: made.summary || '' });
                res.writeHead(200, { 'content-type': 'application/json' });
                res.end(JSON.stringify(seen.cals.get(newId)));
            } else if (req.method === 'DELETE') {
                if (!seen.cals.has(id)) {
                    res.writeHead(404, { 'content-type': 'application/json' });
                    res.end('{"error":{"message":"Not Found"}}'); return;
                }
                seen.cals.delete(id);
                res.writeHead(204); res.end();
            } else if (req.method === 'PATCH') {
                const c = seen.cals.get(id);
                if (!c) { res.writeHead(404); res.end('{"error":{"message":"Not Found"}}'); return; }
                c.summary = JSON.parse(body || '{}').summary || c.summary;
                res.writeHead(200, { 'content-type': 'application/json' });
                res.end(JSON.stringify(c));
            } else {
                const c = seen.cals.get(id);
                if (!c) { res.writeHead(404, { 'content-type': 'application/json' }); res.end('{"error":{"message":"Not Found"}}'); return; }
                res.writeHead(200, { 'content-type': 'application/json' });
                res.end(JSON.stringify(c));
            }
        } else { res.writeHead(404); res.end(); }
    });
    await new Promise((go) => fake.listen(0, '127.0.0.1', go));
    const at = 'http://127.0.0.1:' + fake.address().port;

    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'amber-drive-'));
    const vault = { dir, encrypt: (t) => Buffer.from('v1:' + t), decrypt: (b) => String(b).slice(3) };
    let lastAuth = null;
    let page = null;
    /// 「ブラウザ」── 許可の画面の URL を受け取って、折り返し先へ自分で戻る。
    /// **待たない**（本物のブラウザも別の生き物）── 折り返しへの返事は、
    /// キーの交換が済んでから来る。
    const open = async (url) => {
        lastAuth = new URL(url);
        const back = new URL(lastAuth.searchParams.get('redirect_uri'));
        back.searchParams.set('code', 'the-code');
        back.searchParams.set('state', lastAuth.searchParams.get('state'));
        page = fetch(back).then((r) => r.text());
    };
    const drive = createDrive({ open, vault, tokenUrl: at + '/token', revokeUrl: at + '/revoke', aboutUrl: at + '/about', authUrl: at + '/auth' });

    console.log('サインイン');
    const got = await drive.signIn();
    ok(got.ok === true, 'サインインできる', got);
    ok((await page).includes('サインインできました'), 'ブラウザには、交換が済んでから「できました」の 1 つが出る');
    ok(lastAuth.searchParams.get('code_challenge_method') === 'S256', '許可の URL に PKCE の要約が付く');
    ok(lastAuth.searchParams.get('access_type') === 'offline' && lastAuth.searchParams.get('prompt') === 'consent', '戻すキーをもらう頼み方');
    ok(lastAuth.searchParams.get('scope') === 'https://www.googleapis.com/auth/drive.file', 'スコープは drive.file だけ');
    const ex = seen.token[0] || {};
    ok(ex.grant_type === 'authorization_code' && ex.code === 'the-code', '折り返しの code でキーを交換する', ex);
    ok(typeof ex.code_verifier === 'string' && ex.code_verifier.length >= 43, '交換に PKCE の合言葉が付く', ex.code_verifier);
    ok(!('client_secret' in ex), 'シークレットの置き場所が無ければ、送らない');
    ok(got.who && got.who.email === 'taro@example.com', '誰としてか分かる', got.who);
    ok(fs.existsSync(drive.tokenFile), 'キーが置かれる');
    ok(String(fs.readFileSync(drive.tokenFile)).startsWith('v1:'), 'キーは暗号化の口を通して置かれる');
    const acct = drive.account();
    ok(acct.signedIn && acct.who.email === 'taro@example.com', '様子を訊ける', acct);

    console.log('鍵');
    ok(await drive.token() === 'acc-1', 'まだ切れていなければ、そのまま');
    // 切れたキーに書き換えて、黙って新しくなるか。
    const kept = JSON.parse(vault.decrypt(fs.readFileSync(drive.tokenFile)));
    kept.until = Date.now() - 1000;
    fs.writeFileSync(drive.tokenFile, vault.encrypt(JSON.stringify(kept)));
    const fresh = await drive.token();
    ok(fresh === 'acc-2', '切れていたら refresh で新しくなる', fresh);
    ok(seen.token[1] && seen.token[1].grant_type === 'refresh_token' && seen.token[1].refresh_token === 'ref-1', 'refresh の頼み方', seen.token[1]);

    console.log('違う state は受けない');
    {
        let bogusPage = null;
        const bogus = async (url) => {
            const u = new URL(url);
            const back = new URL(u.searchParams.get('redirect_uri'));
            back.searchParams.set('code', 'x');
            back.searchParams.set('state', 'wrong');
            bogusPage = fetch(back).then((r) => r.text());
        };
        const d2 = createDrive({ open: bogus, vault: { ...vault, dir: fs.mkdtempSync(path.join(os.tmpdir(), 'amber-drive2-')) }, tokenUrl: at + '/token', patience: 2000 });
        const r = await d2.signIn();
        ok(!!r.error, '受けずに断る', r);
        ok((await bogusPage).includes('できませんでした'), 'ブラウザにも「できませんでした」と出る');
    }

    console.log('やめる');
    const out = await drive.signOut();
    ok(out.ok === true && !fs.existsSync(drive.tokenFile), 'キーが消える');
    ok(seen.revoke.length === 1 && seen.revoke[0] === 'ref-1', 'Google 側の許可も取り消す（戻すキーで）', seen.revoke);
    ok(drive.account().signedIn === false, 'やめたあとの様子');

    console.log('PKCE');
    const p1 = pkce(); const p2 = pkce();
    ok(p1.verifier !== p2.verifier, '合言葉は毎回違う');
    ok(/^[A-Za-z0-9_-]{43,}$/.test(p1.verifier) && /^[A-Za-z0-9_-]{43}$/.test(p1.challenge), 'base64url の形', p1);

    console.log('グループカレンダー（依頼 525）');
    {
        // **キーは Drive と同じ一本。** サインインを二度させない。
        const cal = createCal({ token: () => Promise.resolve('acc-1'), apiUrl: at + '/cal' });
        const made = await cal.make('ambər グループ');
        ok(!!made.id && made.name === 'ambər グループ', '1 つ作れる', made);
        const again = await cal.get(made.id);
        ok(again && again.id === made.id, '作ったものを訊ける', again);
        await cal.rename(made.id, '家の予定');
        ok((await cal.get(made.id)).name === '家の予定', '名前を変えられる');
        await cal.drop(made.id);
        // **投げさせない。** ここで例外が出ると、この試験は落ちるのではなく
        // **死ぬ** ── 後ろの検査が一つも走らないまま、✗ が一つも出ない。
        const gone = await cal.get(made.id).catch((e) => 'なげた: ' + e.message);
        ok(gone === null, '消えたら null ── 黙って空の月を出さない', gone);

        // **もう無いものを消すのは、消し終わっていること**（依頼 536）。
        // 人が Google の画面で先に消していることはある ── そこで止まると
        // amber の憶えだけが永久に外せなくなる。
        const twice = await cal.make('二度消す');
        await cal.drop(twice.id);
        const again2 = await cal.drop(twice.id).catch((e) => ({ error: e.message }));
        ok(again2 && again2.ok === true && again2.gone === true,
           'もう無いものを消しても、落ちずに「もう無かった」と返す', again2);

        // **名前から探し直さない。** `calendar.app.created` に一覧を読む力は
        // 無いので、二度押せば2 つできる ── 憶えるのは呼ぶ側の仕事。
        const a = await cal.make('同じ名前');
        const b = await cal.make('同じ名前');
        ok(a.id !== b.id, '同じ名前でも別の 1 つ（憶えるのは呼ぶ側）', [a.id, b.id]);

        // キーが無ければ、**人の言葉で**断る。
        const none = createCal({ token: () => Promise.resolve(null), apiUrl: at + '/cal' });
        const why = await none.make('x').then(() => '', (e) => e.message);
        ok(why.includes('サインイン'), 'キーが無いときは人の言葉で断る', why);

        // 許可が足りないときも、「HTTP 403」で終わらせない。
        const deny = createCal({
            token: () => Promise.resolve('acc-1'),
            apiUrl: at + '/cal',
            fetch: async () => new Response('{"error":{"message":"Insufficient Permission"}}', { status: 403 }),
        });
        const no403 = await deny.make('x').then(() => '', (e) => e.message);
        ok(no403.includes('許可されていません'), '許可が足りないときは、そう言う', no403);

        ok(shareUrl('') === SETTINGS_URL, 'id が無ければ、設定の入口へ');
        ok(shareUrl('abc@group.calendar.google.com').startsWith(SETTINGS_URL + '/calendar/'),
           '共有設定のページの URL を組める（**本物で確かめていない**）', shareUrl('abc@group.calendar.google.com'));
    }

    console.log('カレンダーの許可は、要る瞬間に足す（依頼 525）');
    {
        const dir2 = fs.mkdtempSync(path.join(os.tmpdir(), 'amber-scope-'));
        const v2 = { dir: dir2, encrypt: (t) => Buffer.from('v1:' + t), decrypt: (b) => String(b).slice(3) };
        const d2 = createDrive({ open, vault: v2, tokenUrl: at + '/token', revokeUrl: at + '/revoke',
                                 aboutUrl: at + '/about', authUrl: at + '/auth' });
        await d2.signIn();
        ok(lastAuth.searchParams.get('scope') === 'https://www.googleapis.com/auth/drive.file',
           'はじめのサインインでは、カレンダーの許可を訊かない', lastAuth.searchParams.get('scope'));
        ok(lastAuth.searchParams.get('include_granted_scopes') === 'true',
           '**前に貰った許可を落とさない** ── 無いと、足しにいった瞬間に同期が黙って止まる');
        ok(d2.grants(CAL_SCOPE) === false, 'まだカレンダーの許可は持っていない');
        await d2.signIn({ scope: CAL_SCOPE });
        ok(lastAuth.searchParams.get('scope') === CAL_SCOPE, '要る瞬間に、カレンダーの許可だけを足しにいく');
    }

    fake.close();

    console.log('運ぶ ── 偽の Drive に、上げて・並べて・下ろして・ゴミ箱へ');
    {
        const { start } = require('./fake-drive');
        const fd = await start();
        const d3 = createDrive({ open: async () => {}, vault: { ...vault, dir: fs.mkdtempSync(path.join(os.tmpdir(), 'amber-drive3-')) },
            apiUrl: fd.url, fakeToken: 'fake', by: '試しの Mac' });
        ok((await d3.list()).length === 0, 'はじめは空');
        const a = await d3.upload({ rel: '買い物.md', text: '# 買い物\n\n- 牛乳\n', print: 'p1' });
        ok(!!a.id && a.tag === 'p1', '一本上げると id と指紋が返る', a);
        const b = await d3.upload({ rel: '家族/週末.md', text: '# 週末\n', print: 'p2' });
        const seen = await d3.list();
        ok(seen.length === 2, '二本並ぶ', seen);
        const fam = seen.find((x) => x.rel === '家族/週末.md');
        ok(fam && fam.id === b.id && fam.tag === 'p2' && fam.by === '試しの Mac', '入れ子のパスと、誰が上げたかがラベルに', fam);
        const dirs = [...fd.files().values()].filter((f) => f.appProperties.amber === 'dir');
        ok(dirs.length === 1 && dirs[0].appProperties.rel === '家族', 'Drive の上にもフォルダができる', dirs.map((x) => x.appProperties));
        ok(await d3.download(a.id) === '# 買い物\n\n- 牛乳\n', '下ろすと同じ文字');
        const c = await d3.upload({ rel: '買い物.md', text: '# 買い物\n\n- 牛乳 2本\n', print: 'p3', id: a.id });
        ok(c.id === a.id && (await d3.list()).find((x) => x.rel === '買い物.md').tag === 'p3', '上書きすると同じ id で指紋が変わる');
        await d3.trash(b.id);
        ok((await d3.list()).length === 1, 'ゴミ箱に入れると一覧から消える');
        // 改名（依頼 492）── 同じ ID のまま名前とパスが変わり、フォルダが変われば親も付け替わる。
        await d3.rename({ id: a.id, rel: '家族/買いもの.md' });
        const moved = (await d3.list()).find((x) => x.id === a.id);
        ok(moved && moved.rel === '家族/買いもの.md' && moved.tag === 'p3', '改名しても同じ id で、パスが変わる', moved);
        const raw = fd.files().get(a.id);
        const famDir = [...fd.files().values()].find((f) => f.appProperties.amber === 'dir' && f.appProperties.rel === '家族');
        ok(raw.name === '買いもの.md' && famDir && raw.parents.length === 1 && raw.parents[0] === famDir.id, 'Drive の名前と親フォルダも付け替わる', { name: raw.name, parents: raw.parents });
        ok(await d3.download(a.id) === '# 買い物\n\n- 牛乳 2本\n', '中身はそのまま');
        // 画像（依頼 497）── bytes のまま上がって、bytes のまま下りる。
        const png = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 1, 2, 255, 254, 0x0d, 0x0a, 0x2d, 0x2d]);
        const pic = await d3.upload({ rel: 'attachments/画像.png', bytes: png, print: 'p9' });
        const back = await d3.downloadBytes(pic.id);
        ok(Buffer.isBuffer(back) && back.equals(png), '画像は bytes のまま往復する（改行や境界に似た bytes があっても）', back && back.length);
        const raw2 = fd.files().get(pic.id);
        ok(raw2.mimeType === 'image/png' && raw2.appProperties.rel === 'attachments/画像.png', '画像の種類とパスがラベルに', raw2 && raw2.mimeType);
        const got = await fetch(fd.url + '/_get?rel=' + encodeURIComponent('家族/買いもの.md')).then((r) => r.json());
        ok(got.text === '# 買い物\n\n- 牛乳 2本\n', '向こうの端末を演じる口からも同じ文字が見える');
        fd.close();
    }

    console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
    process.exit(bad ? 1 : 0);
})();
