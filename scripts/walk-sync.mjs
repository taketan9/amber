#!/usr/bin/env node
/* 総ざらいの**同期の段だけ**。偽の Drive（`fake-drive.js`）を相手に、上げ
 * 下ろし・混ぜ・選び口・消しを順に押す。
 *
 *     scripts/walk.sh walk-sync   # 場所を作り、偽の Drive とデスクトップ版を出し、これだけ走らせる
 *
 * `walk.mjs` からも同じものが呼ばれる（総ざらいの二十の三）。
 */
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { step, bad, tally, ready, report, NOTES } from './walk-harness.mjs';

/* ── 二十の三。**同期 ── 偽の Drive と上げ下ろし**（依頼 489〜） ──
 *
 * デスクトップ版は `AMBER_DRIVE_URL` で偽の Drive を指し、サインインは済んでいる体。
 * 向こうの端末（太郎の iPhone）は node の側が演じる（`/_put` `/_trash`）。
 */
const DRIVE = process.env.DRIVE || '';
const drive = async (p, body) => {
    const r = await fetch(DRIVE + p, body === undefined ? {} : { method: 'POST', body: JSON.stringify(body) });
    return r.json();
};
export async function syncWalk() {
    if (!DRIVE) return;
    await drive('/_reset', {});
    await step('同期：サインイン済みに見える', `
        await loadSync();
        return syncAccount.signedIn ? true : 'サインインしていないことになっています';`, true);
    await step('同期：一度目でこちらのノートがぜんぶ上がる', `
        const r = await syncNow('手');
        if (!r) return '運びませんでした（' + JSON.stringify({ signed: syncAccount.signedIn, busy: syncBusy, root: !!state.root }) + '）';
        if (r.trouble.length) return '困りごと: ' + r.trouble[0];
        const notes = state.notes.filter((n) => !n.guest).length;
        // 絵も乗る（依頼 497）ので、上がる数はノートの数より多い。
        return r.up >= notes ? true : r.up + ' 本しか上がりません（' + notes + ' 本のはず）';`, true);
    tally.ran += 1;
    {
        const there = await drive('/_list');
        const rels = there.map((f) => f.appProperties.rel);
        if (!rels.includes('よくばり.md') || !rels.includes('仕事/段取り.md')) {
            bad.push({ name: '同期：向こうに同じパスで並ぶ', why: ['向こうの一覧: ' + rels.slice(0, 8).join(' / ')] });
        }
        if (!rels.includes('attachments/amber.png')) bad.push({ name: '同期：絵も上がる', why: ['向こうの一覧に attachments/amber.png がありません'] });
        const pic = await drive('/_get?rel=' + encodeURIComponent('attachments/amber.png'));
        if (NOTES) {
            try {
                const mine = readFileSync(NOTES + '/attachments/amber.png').toString('base64');
                if (pic.b64 !== mine) bad.push({ name: '同期：絵は bytes のまま上がる', why: ['向こうの bytes が違います（' + String(pic.b64 || '').length + ' / ' + mine.length + '）'] });
            } catch (e) { bad.push({ name: '同期：絵は bytes のまま上がる', why: [e.message] }); }
        }
        const one = await drive('/_get?rel=買い物.md');
        if (!one.text || !one.text.includes('- 牛乳')) bad.push({ name: '同期：向こうの文字がこちらと同じ', why: [String(JSON.stringify(one.text)).slice(0, 120)] });
    }
    await step('同期の様子：運んだ直後は色つきの列に「アップロードN本」', `
        const box = el('syncsay');
        if (box.hidden) return '列が出ていません';
        if (!box.classList.contains('good')) return '色が ' + box.className + ' です';
        const t = box.textContent;
        if (!t.includes('同期しました') || !/アップロード\\d+件/.test(t)) return JSON.stringify(t);
        if (/上げ|下ろ|運/.test(t)) return '中の言葉が出ています: ' + t;
        return true;`, true);
    await step('同期の様子：数秒で一行に縮み、最終の時刻とメールアドレスが出る', `
        for (let i = 0; i < 40 && !el('syncsay').hidden; i += 1) await new Promise((g) => setTimeout(g, 250));
        if (!el('syncsay').hidden) return '列が消えません';
        const m = el('syncmark');
        if (m.hidden) return '一行が出ていません';
        const t = m.textContent;
        return t.includes('同期しています') && t.includes('最終') && t.includes('@') ? true : JSON.stringify(t);`, true);
    await step('同期：二度目は何も運ばない', `
        const r = await syncNow('手');
        return r && r.up === 0 && r.down === 0 && r.clash === 0 ? true : JSON.stringify(r);`, true);

    // 向こうが絵を1 つ置いた → こちらに bytes のまま下りてくる（依頼 497）。
    const PNG1 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
    await drive('/_put', { rel: 'attachments/太郎の絵.png', b64: PNG1, by: '太郎の iPhone' });
    await step('同期：向こうが置いた絵が、こちらに bytes のまま下りてくる', `
        const r = await syncNow('手');
        return r && r.down === 1 && !r.trouble.length ? true : JSON.stringify(r);`, true);
    if (NOTES) {
        tally.ran += 1;
        try {
            const got = readFileSync(NOTES + '/attachments/太郎の絵.png').toString('base64');
            if (got !== PNG1) bad.push({ name: '同期：下りた絵の bytes', why: ['違います'] });
        } catch (e) { bad.push({ name: '同期：下りた絵の bytes', why: [e.message] }); }
    }

    // 向こうが一本置いた。
    await drive('/_put', { rel: '太郎から.md', text: '---\ncreated: 2026-09-11\n---\n\n# 太郎から\n\niPhone で書いた。\n', by: '太郎の iPhone' });
    await step('同期：向こうが置いたノートが、こちらに来る', `
        const r = await syncNow('手');
        if (!r || r.down !== 1) return JSON.stringify(r);
        await new Promise((g) => setTimeout(g, 600));
        const n = state.notes.find((x) => x.title === '太郎から');
        return n ? true : '一覧に出ません';`, true);
    if (NOTES) {
        tally.ran += 1;
        try {
            const got = readFileSync(NOTES + '/太郎から.md', 'utf8');
            if (!got.includes('iPhone で書いた。')) bad.push({ name: '同期：ファイルにも同じ文字', why: [JSON.stringify(got.slice(0, 120))] });
        } catch (e) { bad.push({ name: '同期：ファイルにも同じ文字', why: [e.message] }); }
    }

    // 向こうが直した（こちらは触っていない）。
    await drive('/_put', { rel: '太郎から.md', text: '---\ncreated: 2026-09-11\n---\n\n# 太郎から\n\niPhone で書いた。\n\n直した。\n', by: '太郎の iPhone' });
    await step('同期：向こうが直したぶんが、黙って下りてくる', `
        const r = await syncNow('手');
        if (!r || r.down !== 1 || r.clash !== 0) return JSON.stringify(r);
        const got = await ask('read', { path: state.root + '/太郎から.md' });
        return got.text.includes('直した。') ? true : '古いままです';`, true);

    // こちらが直した → 上がる。
    await step('同期：こちらで直すと、向こうに上がる', `
        await openNote(state.root + '/買い物.md');
        await new Promise((g) => setTimeout(g, 300));
        // 開くときに、前のノートの打ちかけが保存されることがある（総ざらいの
        // 途中では）── それを先に運んでおき、ここで上がるのを一本だけにする。
        clearTimeout(syncTimer);
        await syncNow('手');
        setView('write');
        await new Promise((g) => setTimeout(g, 200));
        // エディタの文字は末尾の改行を持たない（前書きを切るときに落ちる）。
        const nl = String.fromCharCode(10);
        const v = editor.getValue();
        loading = true;
        editor.setValue(v + (v.endsWith(nl) ? '' : nl) + '- 卵' + nl);
        loading = false;
        readStale();   // 打ったのと同じ ── 表示画面はもう今の文字ではない
        state.dirty = true;
        await save();
        clearTimeout(syncTimer);
        const r = await syncNow('手');
        return r && r.up === 1 && r.clash === 0 ? true : JSON.stringify(r);`, true);
    tally.ran += 1;
    {
        const one = await drive('/_get?rel=買い物.md');
        if (!one.text || !one.text.includes('- 卵')) bad.push({ name: '同期：向こうに上がった文字', why: [String(JSON.stringify(one.text)).slice(0, 160)] });
    }

    // 同じ行を両方で直した → 混ぜて、選び口。
    {
        const one = await drive('/_get?rel=買い物.md');
        await drive('/_put', { rel: '買い物.md', text: String(one.text || '').replace('- 卵', '- 卵（向こうは六個）'), by: '太郎の iPhone' });
    }
    await step('同期：同じ行を両方で直すと、両方残って選び口が出る', `
        loading = true;
        editor.setValue(editor.getValue().replace('- 卵', '- 卵（こちらは十個）'));
        loading = false;
        readStale();   // 打ったのと同じ ── 表示画面はもう今の文字ではない
        state.dirty = true;
        await save();
        clearTimeout(syncTimer);
        const r = await syncNow('手');
        if (!r || r.clash !== 1) return JSON.stringify(r);
        await new Promise((g) => setTimeout(g, 1200));
        const text = whole();
        if (!text.includes('十個') || !text.includes('六個')) return '両方残っていません: ' + JSON.stringify(text.slice(-80));
        if (!incoming || !incoming.spots || incoming.spots.length !== 1) return 'ぶつかった場所が ' + JSON.stringify(incoming && incoming.spots) + ' です';
        if (incoming.who !== '太郎の iPhone') return '相手の名前が ' + incoming.who + ' です';
        if (el('band').hidden || !el('band').textContent.includes('太郎の iPhone')) return '帯に相手の名前が出ません: ' + el('band').textContent;
        return true;`, true);
    tally.ran += 1;
    {
        const one = await drive('/_get?rel=買い物.md');
        if (!one.text || !one.text.includes('十個') || !one.text.includes('六個')) {
            bad.push({ name: '同期：混ぜた文字が向こうにも上がる', why: [String(JSON.stringify(one.text)).slice(0, 160)] });
        }
    }
    await step('同期：「こちらの記載を反映する」を選ぶと、向こうにもそれが上がる', `
        setView('read');
        await new Promise((g) => setTimeout(g, 600));
        const g = el('read').querySelector('.gadget');
        if (!g) {
            const rows = rowsOf(whole());
            const sp = incoming && incoming.spots && incoming.spots[0];
            const at = sp ? spotAt(rows, sp) : -2;
            return '選び口が出ていません: ' + JSON.stringify({ view, sp, at, block: at >= 0 ? !!blockOfLine(el('read'), at) : null,
                lines: [...el('read').children].map((b) => b.tagName + ':' + b.dataset.line + '+' + (b.dataset.span || 1)).slice(0, 12) });
        }
        g.querySelector('button[data-w="ours"]').click();
        await new Promise((g) => setTimeout(g, 1200));
        clearTimeout(syncTimer);
        const r = await syncNow('手');
        if (!r || r.up !== 1) return JSON.stringify(r);
        const text = whole();
        return text.includes('十個') && !text.includes('六個') ? true : JSON.stringify(text.slice(-80));`, true);
    tally.ran += 1;
    {
        const one = await drive('/_get?rel=買い物.md');
        if (!one.text || one.text.includes('六個') || !one.text.includes('十個')) {
            bad.push({ name: '同期：選んだあとの文字が向こうにも', why: [String(JSON.stringify(one.text)).slice(0, 160)] });
        }
    }

    // 繋がらないとき → 赤い列と「接続確認する」。繋がれば、押して直る。
    await drive('/_break', { on: true });
    await step('同期の様子：繋がらないと赤い列に「接続確認する」', `
        const r = await syncNow('手');
        if (!r || !r.trouble.length) return '困りごとになりません: ' + JSON.stringify(r);
        const box = el('syncsay');
        if (box.hidden || !box.classList.contains('bad')) return '赤い列が出ていません: ' + box.className;
        const t = box.textContent;
        if (!t.includes('同期できません') || !t.includes('インターネットに繋がっていないようです')) return JSON.stringify(t);
        const b = box.querySelector('button');
        return b && b.textContent === '接続確認する' ? true : 'ボタンが ' + (b && b.textContent) + ' です';`, true);
    await drive('/_break', { on: false });
    await step('同期の様子：「接続確認する」を押すと、繋がっていれば直る', `
        el('syncsay').querySelector('button').click();
        for (let i = 0; i < 40 && (syncBusy || !el('syncsay').hidden); i += 1) await new Promise((g) => setTimeout(g, 250));
        if (!el('syncsay').hidden) return '赤い列が残っています: ' + el('syncsay').textContent;
        const t = el('syncmark').textContent;
        return t.includes('同期しています') ? true : JSON.stringify(t);`, true);

    // サインインする前の姿（このデスクトップ版ではサインイン済みなので、様子だけ作って見る）。
    await step('同期の様子：始める前は「まだ同期していません」の列と二つのボタン', `
        const was = syncAccount;
        syncAccount = { signedIn: false };
        drawSyncState();
        const box = el('syncsay');
        const bs = [...box.querySelectorAll('button')].map((b) => b.textContent);
        const t = box.textContent;
        let out = true;
        if (box.hidden || !box.classList.contains('before')) out = '列が出ていません: ' + box.className;
        else if (!t.includes('まだ同期していません')) out = JSON.stringify(t);
        else if (bs.join('/') !== '同期をはじめる/あとで') out = 'ボタンが ' + bs.join('/') + ' です';
        else {
            box.querySelectorAll('button')[1].click();
            if (!box.hidden) out = '「あとで」で列が消えません';
            else if (!el('syncmark').textContent.includes('同期していません')) out = '一行が ' + el('syncmark').textContent;
        }
        syncAccount = was;
        syncLater = false;
        drawSyncState();
        return out;`, true);

    // こちらで改名 → 向こうも改名（ID は同じまま・依頼 492）。
    await step('同期：こちらで題を直すと、向こうのファイル名も変わる', `
        nameAuto = true;
        try {
            await openNote(state.root + '/買い物.md');
            el('title').textContent = '買いもの';
            await titleDone(true);
            if (!state.open.path.endsWith('/買いもの.md')) return 'パスが ' + state.open.path;
            clearTimeout(syncTimer);
            const r = await syncNow('手');
            return r && r.moved === 1 ? true : JSON.stringify(r);
        } finally { nameAuto = false; }`, true);
    tally.ran += 1;
    {
        const there = await drive('/_list');
        const rels = there.map((f) => f.appProperties.rel);
        if (!rels.includes('買いもの.md') || rels.includes('買い物.md')) bad.push({ name: '同期：向こうも新しい名前', why: ['向こう: ' + rels.join(' / ')] });
        const one = await drive('/_get?rel=買いもの.md');
        if (!one.text || !one.text.includes('十個')) bad.push({ name: '同期：改名しても中身は同じ', why: [String(JSON.stringify(one.text)).slice(0, 120)] });
    }
    await step('同期：題を戻すと、向こうも戻る', `
        nameAuto = true;
        try {
            el('title').textContent = '買い物';
            await titleDone(true);
            clearTimeout(syncTimer);
            const r = await syncNow('手');
            return r && r.moved === 1 ? true : JSON.stringify(r);
        } finally { nameAuto = false; }`, true);

    // フォルダへ移す → 向こうも同じ ID のままパスが変わる（依頼 496）。
    await step('同期：フォルダへ移すと、向こうも同じ ID のままパスが変わる', `
        // 総ざらいの途中では、このノートは別のフォルダに居たり 買い物.3.md だったり
        // する（同じ題が増えると番号は加算）── 開いている一本（題が 買い物）を使う。
        const note = state.open;
        if (!note || note.title !== '買い物') return '買い物のノートが開いていません: ' + (note && note.title);
        const rel0 = note.path.slice(state.root.length + 1);
        const before = (await window.amber.driveList()).find((x) => x.rel === rel0);
        if (!before) return '向こうに ' + rel0 + ' がありません';
        const r0 = await ask('move', { path: note.path, dir: state.root + '/家族', root: state.root });
        await reload({ quiet: true });
        clearTimeout(syncTimer);
        const r = await syncNow('手');
        if (!r || r.moved !== 1 || r.gone !== 0 || r.up !== 0) return JSON.stringify(r);
        const name = note.path.split('/').pop();
        const after = (await window.amber.driveList()).find((x) => x.rel === '家族/' + name);
        if (!after || after.id !== before.id) return '向こうの道か ID が違います: ' + JSON.stringify(after);
        // 戻す（もといたフォルダへ）。
        await ask('move', { path: r0.path, dir: note.path.slice(0, note.path.lastIndexOf('/')), root: state.root });
        await reload({ quiet: true });
        const r2 = await syncNow('手');
        return r2 && r2.moved === 1 ? true : JSON.stringify(r2);`, true);

    // 向こうで改名 → こちらも改名。
    await drive('/_move', { rel: '太郎から.md', to: '太郎のメモ.md' });
    await drive('/_put', { rel: '太郎のメモ.md', text: '---\ncreated: 2026-09-11\n---\n\n# 太郎のメモ\n\niPhone で書いた。\n\n直した。\n', by: '太郎の iPhone' });
    await step('同期：向こうで名前が変わると、こちらのファイルも変わる', `
        const r = await syncNow('手');
        if (!r || r.moved !== 1 || r.down !== 1) return JSON.stringify(r);
        await new Promise((g) => setTimeout(g, 600));
        if (state.notes.some((x) => x.path.endsWith('/太郎から.md'))) return '古い名前が残っています';
        const n = state.notes.find((x) => x.path.endsWith('/太郎のメモ.md'));
        return n && n.title === '太郎のメモ' ? true : '新しい名前が無い';`, true);
    if (NOTES) {
        tally.ran += 1;
        try {
            const got = readFileSync(NOTES + '/太郎のメモ.md', 'utf8');
            if (!got.includes('# 太郎のメモ')) bad.push({ name: '同期：改名したファイルに新しい文字', why: [JSON.stringify(got.slice(0, 80))] });
        } catch (e) { bad.push({ name: '同期：改名したファイルに新しい文字', why: [e.message] }); }
    }

    // 向こうで消した → こちらはゴミ箱へ。
    await drive('/_trash', { rel: '太郎のメモ.md' });
    await step('同期：向こうで消したノートは、こちらでもゴミ箱へ', `
        const r = await syncNow('手');
        if (!r || r.gone !== 1) return JSON.stringify(r);
        await new Promise((g) => setTimeout(g, 600));
        return state.notes.some((x) => x.title === '太郎のメモ') ? '一覧に残っています' : true;`, true);

    // こちらで消した → 向こうもゴミ箱へ。
    await step('同期：こちらで消したノートは、向こうでもゴミ箱へ', `
        const n = state.notes.find((x) => x.title === 'からっぽ');
        if (!n) return '「からっぽ」がありません';
        await ask('delete', { path: n.path });
        await reload({ quiet: true });
        const r = await syncNow('手');
        return r && r.gone === 1 ? true : JSON.stringify(r);`, true);
    tally.ran += 1;
    {
        const there = await drive('/_list');
        if (there.some((f) => f.appProperties.rel === 'からっぽ.md')) bad.push({ name: '同期：向こうからも消える', why: ['向こうに残っています'] });
    }
}


if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
    await ready();
    await syncWalk();
    report();
}
