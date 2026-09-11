#!/usr/bin/env node
/* 総ざらいの**同期の段だけ**。偽の Drive（`fake-drive.js`）を相手に、上げ
 * 下ろし・混ぜ・選び口・消しを順に押す。
 *
 *     scripts/walk.sh walk-sync   # 場所を作り、偽の Drive と窓を出し、これだけ走らせる
 *
 * `walk.mjs` からも同じものが呼ばれる（総ざらいの二十の三）。
 */
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { step, bad, tally, ready, report, NOTES } from './walk-harness.mjs';

/* ── 二十の三。**同期 ── 偽の Drive と上げ下ろし**（依頼 489〜） ──
 *
 * 窓は `AMBER_DRIVE_URL` で偽の Drive を指し、サインインは済んでいる体。
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
        return r.up === notes ? true : r.up + ' 本しか上がりません（' + notes + ' 本のはず）';`, true);
    tally.ran += 1;
    {
        const there = await drive('/_list');
        const rels = there.map((f) => f.appProperties.rel);
        if (!rels.includes('よくばり.md') || !rels.includes('仕事/段取り.md')) {
            bad.push({ name: '同期：向こうに同じ道で並ぶ', why: ['向こうの一覧: ' + rels.slice(0, 8).join(' / ')] });
        }
        const one = await drive('/_get?rel=買い物.md');
        if (!one.text || !one.text.includes('- 牛乳')) bad.push({ name: '同期：向こうの字がこちらと同じ', why: [String(JSON.stringify(one.text)).slice(0, 120)] });
    }
    await step('同期：二度目は何も運ばない', `
        const r = await syncNow('手');
        return r && r.up === 0 && r.down === 0 && r.clash === 0 ? true : JSON.stringify(r);`, true);

    // 向こうが一本置いた。
    await drive('/_put', { rel: '太郎から.md', text: '---\ncreated: 2026-09-11\n---\n\n# 太郎から\n\n電話で書いた。\n', by: '太郎の iPhone' });
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
            if (!got.includes('電話で書いた。')) bad.push({ name: '同期：ファイルにも同じ字', why: [JSON.stringify(got.slice(0, 120))] });
        } catch (e) { bad.push({ name: '同期：ファイルにも同じ字', why: [e.message] }); }
    }

    // 向こうが直した（こちらは触っていない）。
    await drive('/_put', { rel: '太郎から.md', text: '---\ncreated: 2026-09-11\n---\n\n# 太郎から\n\n電話で書いた。\n\n直した。\n', by: '太郎の iPhone' });
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
        // エディタの字は末尾の改行を持たない（前書きを切るときに落ちる）。
        const nl = String.fromCharCode(10);
        const v = editor.getValue();
        loading = true;
        editor.setValue(v + (v.endsWith(nl) ? '' : nl) + '- 卵' + nl);
        loading = false;
        readStale();   // 打ったのと同じ ── 読む面はもう今の字ではない
        state.dirty = true;
        await save();
        clearTimeout(syncTimer);
        const r = await syncNow('手');
        return r && r.up === 1 && r.clash === 0 ? true : JSON.stringify(r);`, true);
    tally.ran += 1;
    {
        const one = await drive('/_get?rel=買い物.md');
        if (!one.text || !one.text.includes('- 卵')) bad.push({ name: '同期：向こうに上がった字', why: [String(JSON.stringify(one.text)).slice(0, 160)] });
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
        readStale();   // 打ったのと同じ ── 読む面はもう今の字ではない
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
            bad.push({ name: '同期：混ぜた字が向こうにも上がる', why: [String(JSON.stringify(one.text)).slice(0, 160)] });
        }
    }
    await step('同期：「こちらを残す」を選ぶと、向こうにもそれが上がる', `
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
            bad.push({ name: '同期：選んだあとの字が向こうにも', why: [String(JSON.stringify(one.text)).slice(0, 160)] });
        }
    }

    // 向こうで消した → こちらはゴミ箱へ。
    await drive('/_trash', { rel: '太郎から.md' });
    await step('同期：向こうで消したノートは、こちらでもゴミ箱へ', `
        const r = await syncNow('手');
        if (!r || r.gone !== 1) return JSON.stringify(r);
        await new Promise((g) => setTimeout(g, 600));
        return state.notes.some((x) => x.title === '太郎から') ? '一覧に残っています' : true;`, true);

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
