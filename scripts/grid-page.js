/* 網（位置×操作の総当たり）の、**窓の中で動く側**。
 *
 *     scripts/grid.mjs が CDP で窓に流し込み、`__g.…` として呼ぶ。
 *
 * ここにあるのは「caret をどこに置くか」「どう押すか」「いま何が見えて
 * いるか」だけ ── **何が正しいかは知らない**。判断は `grid.mjs` の側。
 *
 * **切り出しと同じ約束で書く。** 画面のどこにも直に触らず、`el('read')` と
 * 窓の関数（`openNote`・`setView`・`save`・`MARKS`・`putFace`…）だけを
 * 呼ぶ。電話の `WKWebView` に持っていく日に、そのまま流し込めるように。
 *
 * **ここは素の JS のファイル。** `walk.mjs` の頭の注意書き（逆引用符と
 * 円記号をテンプレートの中に書けない）は、ここには当たらない ── 別の
 * ファイルとして読んで、そのまま `Runtime.evaluate` に渡す。
 */
window.__g = {
    /// いま狙っている一行（節）と、その中の caret の位置。
    line: null,
    pos: 0,

    pause(ms) { return new Promise((g) => setTimeout(g, ms)); },

    /// 本文を丸ごと置き換えて、読む面を組み直す。**前書きは残す。**
    ///
    /// `readSourceEdit` と同じ書き方（前書きの後ろに一行空ける）── ここを
    /// 詰めると、置いただけで往復に差分が出る。
    async body(md, note) {
        if (!state.open || state.open.path !== note) {
            await openNote(note);
            await this.pause(150);
        }
        if (view !== 'write') { await setView('write'); await this.pause(120); }
        loading = true;
        editor.setValue(state.head ? '\n' + md : md);
        loading = false;
        state.dirty = true;
        await save();
        await setView('read');
        await this.pause(150);
        this.line = null;
        this.pos = 0;
        return whole();
    },

    /// 読む面を、人が打ったときと同じ合図で一度書き戻させる（正規化）。
    async settle() {
        el('read').dispatchEvent(new Event('input'));
        await this.pause(950);
        return whole();
    },

    /// 読む面の中で、`text` を含む最初の字の節を探す。
    seek(text) {
        const box = el('read');
        const walk = document.createTreeWalker(box, NodeFilter.SHOW_TEXT);
        let node;
        while ((node = walk.nextNode())) {
            const at = node.data.indexOf(text);
            if (at >= 0) return { node, at };
        }
        return null;
    },

    /// 一行として扱う節（項目・段落・見出し・セル）。
    lineOf(node) {
        const n = node.nodeType === 3 ? node.parentElement : node;
        return n ? n.closest('li, p, h1, h2, h3, h4, h5, h6, td, th') : null;
    },

    /// 読む面の直下で、その節を含むかたまりの番号。
    blockIndex(node) {
        const box = el('read');
        let n = node && node.nodeType === 3 ? node.parentElement : node;
        while (n && n.parentElement !== box) n = n.parentElement;
        return n ? [...box.children].indexOf(n) : -1;
    },

    /// `text` の**頭・まん中・末尾**に caret を置く。
    ///
    /// **人が押したときと同じ形にする** ── 面に焦点を渡し、選び目を置き、
    /// `selectionchange` を出す（窓はそれで caret を憶える・依頼 461）。
    /// 返すのは、置いた節とかたまりの番号と、節の字の中で何文字目か。
    spot(text, where) {
        const box = el('read');
        let node;
        let off;
        if (text === '') {
            // 空のノート ── 最初の段落の頭。
            const first = box.firstElementChild;
            if (!first) return { bad: '面に何もありません' };
            const r = document.createRange();
            r.selectNodeContents(first);
            r.collapse(true);
            box.focus();
            const sel = getSelection();
            sel.removeAllRanges();
            sel.addRange(r);
            document.dispatchEvent(new Event('selectionchange'));
            this.line = first;
            this.pos = 0;
            return { block: this.blockIndex(first), tag: first.tagName, pos: 0, text: first.textContent };
        }
        const hit = this.seek(text);
        if (!hit) return { bad: '面に「' + text + '」がありません' };
        node = hit.node;
        const n = text.length;
        off = hit.at + (where === 'head' ? 0 : where === 'mid' ? Math.floor(n / 2) : n);
        const r = document.createRange();
        r.setStart(node, off);
        r.collapse(true);
        box.focus();
        const sel = getSelection();
        sel.removeAllRanges();
        sel.addRange(r);
        document.dispatchEvent(new Event('selectionchange'));
        const line = this.lineOf(node);
        this.line = line;
        this.pos = line ? caretIn(line) : off;
        return {
            block: this.blockIndex(node),
            tag: line ? line.tagName : node.parentElement.tagName,
            pos: this.pos,
            text: line ? line.textContent : node.data,
        };
    },

    /// `a` の頭から `b` の末尾までを選ぶ（`b` が無ければ `a` だけ）。
    select(a, b) {
        const box = el('read');
        const from = this.seek(a);
        if (!from) return { bad: '面に「' + a + '」がありません' };
        const to = b ? this.seek(b) : from;
        if (!to) return { bad: '面に「' + b + '」がありません' };
        const r = document.createRange();
        r.setStart(from.node, from.at);
        r.setEnd(to.node, to.at + (b || a).length);
        box.focus();
        const sel = getSelection();
        sel.removeAllRanges();
        sel.addRange(r);
        document.dispatchEvent(new Event('selectionchange'));
        this.line = this.lineOf(from.node);
        this.pos = this.line ? caretIn(this.line) : from.at;
        return { block: this.blockIndex(from.node), to: this.blockIndex(to.node), picked: sel.toString() };
    },

    /// 面の字をぜんぶ選ぶ（⌘A の既定と同じ ── 編集の箱の中だけ）。
    selectAll() {
        const box = el('read');
        box.focus();
        const r = document.createRange();
        r.selectNodeContents(box);
        const sel = getSelection();
        sel.removeAllRanges();
        sel.addRange(r);
        document.dispatchEvent(new Event('selectionchange'));
        this.line = null;
        this.pos = 0;
        return { picked: sel.toString().length };
    },

    /// 読む面のかたまりを、それぞれ字にしたもの（`paperToMd` と同じ割り方）。
    blocks() {
        const box = el('read');
        return [...box.children].map((n) => (richBlock(n)
            ? (n.dataset.md === undefined ? null : n.dataset.md)
            : blockToMd(n)));
    },

    /// いまの姿。**判断はしない**、見えているものを返すだけ。
    state() {
        const box = el('read');
        const sel = getSelection();
        let n = sel && sel.rangeCount ? sel.anchorNode : null;
        const holder = n && n.nodeType === 3 ? n.parentElement : n;
        const caretRead = !!holder && box.contains(holder);
        let again = null;
        try { again = paperToMd(box, state.head); } catch (e) { again = 'throw: ' + e.message; }
        return {
            md: whole(),
            // **前書きを付けて返す** ── `whole()` と同じ形で比べられるように。
            again: typeof again === 'string' && !again.startsWith('throw:') ? state.head + again : again,
            caretRead,
            focusRead: document.activeElement === box || box.contains(document.activeElement),
            block: caretRead ? this.blockIndex(holder) : -1,
            lineText: this.line && box.contains(this.line) ? this.line.textContent : null,
            veil: !el('veil').hidden,
            emoji: !el('emoji').hidden,
            more: !el('more').hidden,
            said: el('say').classList.contains('on') ? el('say').textContent : '',
            state: el('state').textContent,
            view,
            // 落第を読み解くための DOM（報せには落第のぶんだけ残す）。
            html: box.innerHTML,
        };
    },

    /// 帯の記号を押す（名前で）。
    mark(name) {
        const found = MARKS.flat().find((m) => m[0] === name);
        if (!found || !found[2]) return { bad: '帯に「' + name + '」がありません' };
        const r = found[2]();
        return r && typeof r.then === 'function' ? r : undefined;
    },

    /// 訊いてくる記号（注記）── 小窓が出たら一つめを押す。
    async markAnswering(name) {
        this.mark(name);
        for (let i = 0; i < 6; i += 1) {
            await this.pause(300);
            if (el('veil').hidden) break;
            const first = document.querySelector('#veil #sheet .items .it');
            if (first) { first.click(); continue; }
            const box = document.querySelector('#veil input');
            closeSheet(box && box.value ? box.value : '試し');
        }
        return true;
    },

    /// 絵文字 ── **板を開いてから**入れる（板を開くと焦点が板へ移る。
    /// それが依頼 461 の穴の入口なので、開かずに入れると試しにならない）。
    async face(ch) {
        await openEmoji();
        await this.pause(200);
        putFace(ch);
        await this.pause(120);
        closeEmoji();
        return true;
    },

    /// 貼り付け（読む面の受け口へ、人が貼ったときと同じ形で）。
    paste(plain, html) {
        const box = el('read');
        const dt = new DataTransfer();
        dt.setData('text/plain', plain || '');
        if (html) dt.setData('text/html', html);
        box.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
        return true;
    },

    /// いま狙っている行の升を押す。
    tick() {
        const li = this.line && this.line.closest ? this.line.closest('li') : null;
        const b = li ? li.querySelector(':scope > .box') : null;
        if (!b) return { bad: 'その行に升がありません' };
        b.click();
        return true;
    },

    /// 表の道具。
    table(what) { tableDo(what); return true; },

    /// 面の上の小窓・板・献立を、ぜんぶ閉じる。
    tidy() {
        if (!el('emoji').hidden) closeEmoji();
        if (!el('more').hidden) closeMenu();
        if (!el('veil').hidden) closeSheet(null);
        if (!el('studio').hidden) el('studio').hidden = true;
        el('say').classList.remove('on');
        return true;
    },

    /* ── コードの面 ── */

    /// コードの面で、`text` を含む最初の行の頭・まん中・末尾に caret を置く。
    codeSpot(text, where) {
        const model = editor.getModel();
        for (let ln = 1; ln <= model.getLineCount(); ln += 1) {
            const s = model.getLineContent(ln);
            const at = s.indexOf(text);
            if (at < 0) continue;
            const col = 1 + at + (where === 'head' ? 0 : where === 'mid' ? Math.floor(text.length / 2) : text.length);
            editor.setPosition({ lineNumber: ln, column: col });
            editor.focus();
            return { line: ln, col, text: s };
        }
        return { bad: 'コードの面に「' + text + '」がありません' };
    },

    /// コードの面で、`a` の頭から `b` の末尾までを選ぶ。
    codeSelect(a, b) {
        const model = editor.getModel();
        let from = null;
        let to = null;
        for (let ln = 1; ln <= model.getLineCount(); ln += 1) {
            const s = model.getLineContent(ln);
            if (!from && s.includes(a)) from = { ln, col: 1 + s.indexOf(a) };
            if (from && s.includes(b || a)) {
                to = { ln, col: 1 + s.indexOf(b || a) + (b || a).length };
                if (b === undefined || s.includes(b)) break;
            }
        }
        if (!from || !to) return { bad: 'コードの面に見つかりません' };
        editor.setSelection(new monaco.Selection(from.ln, from.col, to.ln, to.col));
        editor.focus();
        return { from, to };
    },

    codeText() { return editor.getValue(); },
};
true;
