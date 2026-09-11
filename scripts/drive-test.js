#!/usr/bin/env node
/* Google Drive のサインイン（`gui/drive.js`）を、**Google 無しで**一周させる。
 *
 *     node scripts/drive-test.js
 *
 * 偽の Google（鍵を交換する口・誰かを答える口）を手元に立て、「ブラウザで
 * 開く」の代わりに折り返し先へ自分で戻る。見るのは:
 *
 *   一。折り返しに `state` が付いていて、違う `state` は受けないこと
 *   二。鍵の交換に **PKCE の合言葉（code_verifier）** が付いていること
 *   三。鍵が置かれ、切れたら refresh で黙って新しくなること
 *   四。やめると鍵が消え、取り消しの口が叩かれること
 */
'use strict';
const http = require('node:http');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createDrive, pkce } = require('../gui/drive');

let bad = 0;
const ok = (yes, what, got) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (got !== undefined) console.log('      ' + JSON.stringify(got)); }
};

(async () => {
    // ── 偽の Google ──
    const seen = { token: [], revoke: [], about: 0 };
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
    /// 鍵の交換が済んでから来る。
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
    ok((await page).includes('サインインできました'), 'ブラウザには、交換が済んでから「できました」の一枚が出る');
    ok(lastAuth.searchParams.get('code_challenge_method') === 'S256', '許可の URL に PKCE の要約が付く');
    ok(lastAuth.searchParams.get('access_type') === 'offline' && lastAuth.searchParams.get('prompt') === 'consent', '戻す鍵をもらう頼み方');
    ok(lastAuth.searchParams.get('scope') === 'https://www.googleapis.com/auth/drive.file', 'スコープは drive.file だけ');
    const ex = seen.token[0] || {};
    ok(ex.grant_type === 'authorization_code' && ex.code === 'the-code', '折り返しの code で鍵を交換する', ex);
    ok(typeof ex.code_verifier === 'string' && ex.code_verifier.length >= 43, '交換に PKCE の合言葉が付く', ex.code_verifier);
    ok(!('client_secret' in ex), 'シークレットの置き場所が無ければ、送らない');
    ok(got.who && got.who.email === 'taro@example.com', '誰としてか分かる', got.who);
    ok(fs.existsSync(drive.tokenFile), '鍵が置かれる');
    ok(String(fs.readFileSync(drive.tokenFile)).startsWith('v1:'), '鍵は暗号化の口を通して置かれる');
    const acct = drive.account();
    ok(acct.signedIn && acct.who.email === 'taro@example.com', '様子を訊ける', acct);

    console.log('鍵');
    ok(await drive.token() === 'acc-1', 'まだ切れていなければ、そのまま');
    // 切れた鍵に書き換えて、黙って新しくなるか。
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
    ok(out.ok === true && !fs.existsSync(drive.tokenFile), '鍵が消える');
    ok(seen.revoke.length === 1 && seen.revoke[0] === 'ref-1', 'Google 側の許可も取り消す（戻す鍵で）', seen.revoke);
    ok(drive.account().signedIn === false, 'やめたあとの様子');

    console.log('PKCE');
    const p1 = pkce(); const p2 = pkce();
    ok(p1.verifier !== p2.verifier, '合言葉は毎回違う');
    ok(/^[A-Za-z0-9_-]{43,}$/.test(p1.verifier) && /^[A-Za-z0-9_-]{43}$/.test(p1.challenge), 'base64url の形', p1);

    fake.close();
    console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
    process.exit(bad ? 1 : 0);
})();
