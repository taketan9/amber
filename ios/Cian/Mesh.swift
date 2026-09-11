#if DEBUG
import SwiftUI
import WebKit

/// **電話の網** ── 「表示」の面（`WKWebView`）で、位置 × 操作の総当たり。
///
///     scripts/walk-phone.sh      # 総ざらいの最後に、これも走る
///
/// 窓の網（`scripts/grid.mjs`）と同じ考え: 同じ操作を、行頭・行中・行末 ×
/// かたまりの種類ぜんぶで押し、**入れたものが caret のところに入るか・
/// 字に戻せるか・行のどこで押しても同じか・決めごと（`PAPER.ja.md` 六章）の
/// 通りか**を見る。窓で通ったことは電話で通ったことにならない ──
/// `WKWebView` の `contenteditable` は Chromium と癖が違う（四章）。
///
/// **本物の面を建てる。** `Paper.page` を同じ配管（`Paper.Hand`・`amber://`）で
/// 読み、帯の合図（`window.mark`）・絵文字（`window.putFace`）・鍵の受け口
/// （`keydown`・`beforeinput`）を、電話が本当に通す道で押す。鍵そのものは
/// 送れないので、既定の動きは `execCommand`（`insertText`・`delete`・
/// `forwardDelete`・`insertParagraph`）で起こす ── WebKit の既定はこれと同じ。
///
/// 判断は面の中の台本（`script`）が返す。**何が正しいかは窓の網と同じ表**
/// （決めごとを写してある）。決めごとの無いところは落第にしない。
@MainActor
enum Mesh {
    struct Outcome {
        var ran = 0
        var bad: [(String, String)] = []
    }

    /// 行のどこで押しても同じ結果であるべきもの（窓の網の `SAME_ANYWHERE`）。
    private static let sameAnywhere: Set<String> = [
        "見出し", "箇条書き", "チェック", "番号", "引用", "Tab", "⇧Tab",
    ]

    static func run() async -> Outcome {
        var out = Outcome()
        // ── 面を建てる（`Paper.makeUIView` と同じ配管） ──
        let paper = Paper(text: .constant(""), folder: FileManager.default.temporaryDirectory,
                          dark: false, size: 17)
        let hand = Paper.Hand(paper)
        let config = WKWebViewConfiguration()
        for name in ["wrote", "tick", "fix", "menu", "trouble", "at"] {
            config.userContentController.add(hand, name: name)
        }
        config.setURLSchemeHandler(hand, forURLScheme: Paper.Waiter.scheme)
        // **見える大きさで置く。** 隠した面は選び目や `execCommand` が効かない
        // ことがある ── 薄くして端に置く（走査の画面には他に何も無い）。
        let web = WKWebView(frame: CGRect(x: 0, y: 0, width: 390, height: 640), configuration: config)
        web.navigationDelegate = hand
        web.alpha = 0.02
        hand.web = web
        if let on = UIApplication.shared.connectedScenes
            .compactMap({ ($0 as? UIWindowScene)?.keyWindow }).first {
            on.addSubview(web)
        }
        defer { web.removeFromSuperview() }
        web.loadHTMLString(Paper.page, baseURL: URL(string: Paper.Waiter.scheme + "://app/")!)

        var up = false
        for _ in 0..<80 {
            if let ok = try? await web.evaluateJavaScript(
                "typeof paperToMd === 'function' && typeof window.show === 'function'"),
               (ok as? Bool) == true { up = true; break }
            try? await Task.sleep(nanoseconds: 100_000_000)
        }
        out.ran += 1
        guard up else {
            out.bad.append(("電話の網：面を建てる", "paper.js か window.show が居ません"))
            return out
        }
        do {
            _ = try await web.evaluateJavaScript(
                "window.__mmd = \(Paper.mmdOptions(dark: false)); " + script + " true")
        } catch {
            out.bad.append(("電話の網：台本を流し込む", why(error)))
            return out
        }

        // ── 一つずつ押す ──
        guard let plan = (try? await web.callAsyncJavaScript(
            "return __mesh.plan()", contentWorld: .page)) as? [[String: Any]] else {
            out.bad.append(("電話の網：段取り", "plan() が返りません"))
            return out
        }
        var seen: [String: [String: String]] = [:]     // 位置ちがいを見るため
        for c in plan {
            guard let md = c["md"] as? String, let target = c["target"] as? String,
                  let text = c["text"] as? String, let kind = c["kind"] as? String,
                  let whereAt = c["where"] as? String, let op = c["op"] as? String
            else { continue }
            let name = "電話の網：\(target) / \(whereAt) / \(op)"
            out.ran += 1
            let html = (try? Cian.call("html", ["text": md]))?["html"] as? String ?? ""
            if html.isEmpty { out.bad.append((name, "core が組めません")); continue }
            do {
                let got = try await web.callAsyncJavaScript(
                    "return await __mesh.run(html, md, text, kind, where, op)",
                    arguments: ["html": html, "md": md, "text": text, "kind": kind,
                                "where": whereAt, "op": op],
                    contentWorld: .page)
                let d = got as? [String: Any] ?? [:]
                if let bad = d["why"] as? [String], !bad.isEmpty {
                    out.bad.append((name, bad.joined(separator: " / ")))
                }
                if sameAnywhere.contains(op), let after = d["md"] as? String {
                    seen["\(target) / \(op)", default: [:]][whereAt] = after
                }
            } catch {
                out.bad.append((name, "台本が落ちました: " + why(error)))
            }
        }
        // ── 行のどこで押しても同じか ──
        for (key, byWhere) in seen where byWhere.count > 1 {
            out.ran += 1
            let values = Array(Set(byWhere.values.map { $0.trimmingCharacters(in: .newlines) }))
            if values.count > 1 {
                out.bad.append(("電話の網：\(key)",
                                "行のどこで押したかで結果が違います（" + byWhere.keys.sorted().joined(separator: "・") + "）"))
            }
        }
        return out
    }

    private static func why(_ error: Error) -> String {
        ((error as NSError).userInfo["WKJavaScriptExceptionMessage"] as? String)
            ?? error.localizedDescription
    }

    /// 面の中で動く台本。**窓の網の `grid-page.js` と `grid.mjs` の写し**だが、
    /// 押し方は電話の入口（`window.mark`・`window.putFace`・鍵の受け口）に
    /// 合わせてある。決めごとを足したら、窓の `expect` と一緒にここも直す。
    static let script = #"""
    window.__mesh = (() => {
      const NORMAL = [
        '# 見出し', '',
        '飾りのない長い段落です。まん中に置けるくらいには長い。', '',
        '- ひとつ', '- ふたつ', '  - 入れ子', '- みっつ', '',
        '1. 一番', '2. 二番', '',
        '- [ ] やること', '- [x] やった', '',
        '> 引用の一行目', '> 引用の二行目', '',
        '> [!NOTE]', '> 注記の本文', '',
        '| 朝 | 夕 |', '| --- | --- |', '| 掃除 | 片づけ |', '| 洗濯 | 買い出し |', '',
        '上の段落。', '',
        '```rust', 'fn main() {}', '```', '',
        '下の段落。', '',
        '　字下げた段落。', '',
        '最後の段落。',
      ].join('\n');
      const TARGETS = [
        ['見出し（頭）', '見出し', 'h'],
        ['段落', '飾りのない長い段落です。まん中に置けるくらいには長い。', 'p'],
        ['一覧の一つめ', 'ひとつ', 'li1'],
        ['一覧の途中', 'ふたつ', 'li'],
        ['入れ子', '入れ子', 'nest'],
        ['一覧の最後', 'みっつ', 'liN'],
        ['番号', '二番', 'ol'],
        ['升', 'やること', 'task'],
        ['済んだ升', 'やった', 'done'],
        ['引用の一行目', '引用の一行目', 'q1'],
        ['引用の二行目', '引用の二行目', 'q2'],
        ['注記', '注記の本文', 'alert'],
        ['表のセル', '掃除', 'cell'],
        ['表の最後のセル', '買い出し', 'cellN'],
        ['枠の上の段落', '上の段落。', 'before'],
        ['枠の下の段落', '下の段落。', 'after'],
        ['字下げ', '　字下げた段落。', 'pad'],
        ['末尾の段落', '最後の段落。', 'last'],
      ];
      const WHERES = ['行頭', '行中', '行末'];
      const OPS = ['あ', '絵文字', '太字→あ', 'Backspace', 'Delete', 'Return', '段落',
                   'Tab', '⇧Tab', '見出し', '箇条書き', 'チェック', '番号', '引用'];
      const PARA = ['p', 'before', 'after', 'pad', 'last'];
      const LIST = ['li1', 'li', 'nest', 'liN', 'ol', 'task', 'done'];
      const CELL = ['cell', 'cellN'];
      const ALL = ['ひとつ', 'ふたつ', '入れ子', 'みっつ', '一番', '二番', 'やること', 'やった'];

      const pause = (ms) => new Promise((g) => setTimeout(g, ms));
      const same = (a, b) => String(a).replace(/\n+$/, '') === String(b).replace(/\n+$/, '');
      const show = (s) => JSON.stringify(String(s)).replace(/\\n/g, '⏎');
      const mdLine = (md, t) => String(md).split('\n').find((l) => l.includes(t)) ?? null;
      const indentOf = (l) => (l === null ? -1 : l.length - l.trimStart().length);
      const shape = (md) => String(md).split('\n').filter((l) => l.trim().startsWith('|'))
        .map((l) => l.split('|').length - 2).join('/');
      function diff(a, b) {
        const x = String(a).replace(/\n+$/, '').split('\n');
        const y = String(b).replace(/\n+$/, '').split('\n');
        let i = 0;
        while (i < x.length && i < y.length && x[i] === y[i]) i += 1;
        let j = 0;
        while (j < x.length - i && j < y.length - i && x[x.length - 1 - j] === y[y.length - 1 - j]) j += 1;
        return (i + 1) + ' 行目: ' + show(x.slice(i, x.length - j).join('\n')) + ' → ' + show(y.slice(i, y.length - j).join('\n'));
      }

      let errors = [];
      const before = window.onerror;
      window.onerror = (m, s, l) => { errors.push(String(m)); if (before) before(m, s, l); };

      function plan() {
        const out = [];
        for (const [target, text, kind] of TARGETS) {
          for (const op of OPS) {
            for (const where of WHERES) out.push({ md: NORMAL, target, text, kind, where, op });
          }
        }
        return out;
      }

      function seek(text) {
        const walk = document.createTreeWalker(box, NodeFilter.SHOW_TEXT);
        let node;
        while ((node = walk.nextNode())) {
          const at = node.data.indexOf(text);
          if (at >= 0) return { node, at };
        }
        return null;
      }
      const lineOf = (n) => (n.nodeType === 3 ? n.parentElement : n).closest('li, p, h1, h2, h3, h4, h5, h6, td, th');
      let line = null;
      let pos = 0;
      function spot(text, where) {
        const hit = seek(text);
        if (!hit) return '面に「' + text + '」がありません';
        const n = text.length;
        const off = hit.at + (where === '行頭' ? 0 : where === '行中' ? Math.floor(n / 2) : n);
        const r = document.createRange();
        r.setStart(hit.node, off);
        r.collapse(true);
        box.focus();
        const sel = getSelection();
        sel.removeAllRanges();
        sel.addRange(r);
        document.dispatchEvent(new Event('selectionchange'));
        line = lineOf(hit.node);
        pos = line ? caretIn(line) : off;
        return null;
      }
      function caretInBox() {
        const sel = getSelection();
        let n = sel && sel.rangeCount ? sel.anchorNode : null;
        if (n && n.nodeType === 3) n = n.parentElement;
        return !!n && box.contains(n);
      }
      function key(k) {
        const e = new KeyboardEvent('keydown', { key: k, code: k, bubbles: true, cancelable: true });
        box.dispatchEvent(e);
        return e.defaultPrevented;
      }
      async function doOp(op) {
        switch (op) {
          case 'あ': document.execCommand('insertText', false, 'あ'); return;
          case '絵文字': window.putFace('😀'); return;
          case '太字→あ': window.mark('bold'); await pause(30); document.execCommand('insertText', false, 'あ'); return;
          // 外付けの鍵盤と同じ順 ── まず keydown（受け口が受ければそこまで）、
          // 受けなければ既定（`execCommand`）。WebKit は消すものが無いとき
          // `beforeinput` を出さないので、keydown を先に通さないと文書の頭の
          // 見出しの `#` が外れない。
          case 'Backspace': if (!key('Backspace')) document.execCommand('delete'); return;
          case 'Delete': if (!key('Delete')) document.execCommand('forwardDelete'); return;
          case 'Return': if (!key('Enter')) document.execCommand('insertParagraph'); return;
          case '段落': window.mark('para'); return;
          case 'Tab': window.mark('in'); return;
          case '⇧Tab': window.mark('out'); return;
          case '見出し': window.mark('head'); return;
          case '箇条書き': window.mark('ul'); return;
          case 'チェック': window.mark('check'); return;
          case '番号': window.mark('ol'); return;
          case '引用': window.mark('quote'); return;
          default: throw new Error('知らない操作: ' + op);
        }
      }

      /// 決めごとの通りか。true・字（落第）・undefined（見たまま）。
      function expect(c) {
        const { op, kind, where, text, bt, lt, md, was, blocks, block } = c;
        const unchanged = same(md, was);
        const stay = (why) => (unchanged ? true : why + 'のに字が変わりました: ' + diff(was, md));
        const line = (want) => {
          const got = mdLine(md, text.replace(/^　+/, ''));
          return got === want ? true : 'その行が ' + show(want) + ' になるはずが ' + show(got) + ' です';
        };
        const keepList = (r) => {
          if (r !== true) return r;
          for (const t of ALL) if (was.includes(t) && !md.includes(t)) return '「' + t + '」が消えました: ' + diff(was, md);
          return true;
        };
        const dent = (d) => {
          const a = indentOf(mdLine(was, text));
          const b = indentOf(mdLine(md, text));
          if (b === -1) return 'その行が字から消えました';
          return b === a + d ? true : '字下げが ' + a + ' → ' + b + '（' + (a + d) + ' のはず）';
        };
        const ins = (s) => {
          if (lt === null) return '入れたあと、狙った行が面から消えました';
          const want = bt.slice(0, pos) + s + bt.slice(pos);
          if (lt === want) return true;
          const at = lt.indexOf(s);
          if (at < 0) return '「' + s + '」が入っていません: ' + show(bt) + ' → ' + show(lt);
          return '「' + s + '」が caret のところに入っていません（' + pos + ' 文字目のはずが ' + at + ' 文字目）: ' + show(lt);
        };
        const k = pos - bt.indexOf(text);
        switch (op) {
          case 'あ': return ins('あ');
          case '絵文字': return ins('😀');
          case '太字→あ': {
            const r = ins('あ');
            if (r !== true) return r;
            if (kind === 'h') return true;
            return md.includes('**あ**') ? true : '字は入りましたが太字になっていません';
          }
          case 'Tab': {
            if (kind === 'h') return stay('見出しで Tab');
            if (CELL.includes(kind)) return stay('セルで Tab');
            if (['li1', 'nest', 'task'].includes(kind)) return stay('上に項目が無い');
            if (LIST.includes(kind)) return dent(2);
            return lt === '　' + bt ? true : '字下げが付いていません: ' + show(bt) + ' → ' + show(lt);
          }
          case '⇧Tab': {
            if (kind === 'nest') return dent(-2);
            if (kind === 'pad') return lt === bt.slice(1) ? true : '字下げが外れていません: ' + show(lt);
            return stay('外す字下げが無い');
          }
          case 'Backspace': {
            if (where !== '行頭') {
              if (pos === 0) return stay('消す字が無い');
              const want = bt.slice(0, pos - 1) + bt.slice(pos);
              return lt === want ? true : '一文字消えるはずが: ' + show(bt) + ' → ' + show(lt);
            }
            switch (kind) {
              case 'h': return line(text);
              case 'nest': return dent(-2);
              case 'li1': case 'li': case 'liN': case 'ol': case 'task': case 'done': return keepList(line(text));
              case 'q1': return md.includes('引用の一行目\n\n> 引用の二行目') ? true : '一行目だけ引用から出るはずが: ' + diff(was, md);
              case 'q2': return md.includes('引用の一行目引用の二行目') ? true : '前の行と繋がるはずが: ' + diff(was, md);
              case 'alert': return line(text) === true && !md.includes('[!NOTE]') ? true : '注記から出るはずが: ' + diff(was, md);
              case 'after': return stay('枠のすぐ下の行頭で Backspace');
              case 'cell': case 'cellN': return shape(md) === shape(was) ? undefined : '表の形が変わりました';
              case 'p': case 'last': case 'pad': {
                const prev = blocks.slice(0, block).filter((b) => b).pop();
                if (prev === undefined) return stay('前の行が無い');
                const tail = String(prev).split('\n').pop();
                return md.includes(tail + bt) ? true : '前の行と繋がるはずが: ' + diff(was, md);
              }
              default: return undefined;
            }
          }
          case 'Delete': {
            if (where === '行末') {
              if (['before', 'last'].includes(kind) || LIST.includes(kind) || CELL.includes(kind)) return stay('行末で Delete');
              if (kind === 'h') return md.includes('# 見出し飾りのない') ? true : '次の段落と繋がるはずが: ' + diff(was, md);
              if (kind === 'q1') return md.includes('引用の一行目引用の二行目') ? true : '同じ箱の次の行と繋がるはずが: ' + diff(was, md);
              if (['p', 'q2', 'alert', 'after', 'pad'].includes(kind)) {
                const next = blocks.slice(block + 1).find((b) => b !== null && b !== '');
                const plain = next !== undefined && !/^(#|-|\d+\.|>|\||`|!\[)/.test(next);
                if (!plain) return stay('次が記号付きの行');
                return md.includes(bt + next.split('\n')[0]) ? true : '次の段落と繋がるはずが: ' + diff(was, md);
              }
              return undefined;
            }
            const want = bt.slice(0, pos) + bt.slice(pos + 1);
            return lt === want ? true : '一文字消えるはずが: ' + show(bt) + ' → ' + show(lt);
          }
          case 'Return': {
            // 電話の Return は改行。升・見出し・表・箱は窓の Enter と同じ答え。
            if (kind === 'h') {
              if (where !== '行中') return stay('見出しの端で Return');
              return md.includes('# ' + text.slice(0, k) + '\n\n' + text.slice(k)) ? true : '前は見出し・後ろは段落のはずが: ' + diff(was, md);
            }
            if (kind === 'cell') return stay('セルで Return（下のセルへ）');
            if (kind === 'cellN') return md.includes('| 　 | 　 |') ? true : '最後の行のセルで Return を押しても、行が増えません';
            if (['task', 'done'].includes(kind)) {
              if (where !== '行中') return stay('升の端で Return');
              return undefined;
            }
            // 入れ子を持つ項目（ふたつ）の行末は、既定が入れ子を新しい項目へ移す ── 見たまま（窓と同じ）。
            if (kind === 'li' && where === '行末') return undefined;
            if (LIST.includes(kind)) { if (where !== '行中') return stay('項目の端で Return'); return undefined; }
            if (['q1', 'q2', 'alert'].includes(kind)) {
              if (where === '行中') return md.includes('> ' + text.slice(0, k) + '\n> ' + text.slice(k)) ? true : '改行になるはずが: ' + diff(was, md);
              return undefined;
            }
            if (PARA.includes(kind)) {
              if (where !== '行中') return stay('段落の端で Return');
              return md.includes(bt.slice(0, pos) + '\n' + bt.slice(pos)) ? true : '段落の中の改行になるはずが: ' + diff(was, md);
            }
            return undefined;
          }
          case '段落': {
            if (PARA.includes(kind)) {
              if (where !== '行中') return stay('段落の端で「新しい段落」');
              return md.includes(bt.slice(0, pos) + '\n\n' + bt.slice(pos)) ? true : '段落が二つに割れるはずが: ' + diff(was, md);
            }
            return undefined;
          }
          case '見出し': {
            if (PARA.includes(kind)) return line('# ' + bt.replace(/^　+/, ''));
            if (kind === 'h') return line('## ' + text);
            if (LIST.includes(kind)) return keepList(line('# ' + text));
            if (CELL.includes(kind)) return stay('セルで見出し');
            return undefined;
          }
          case '箇条書き': {
            if (PARA.includes(kind)) return line('- ' + bt.replace(/^　+/, ''));
            if (kind === 'h') return line('- ' + text);
            if (['li1', 'li', 'liN', 'nest', 'task', 'done'].includes(kind)) return keepList(line(text));
            if (kind === 'ol') return keepList(line('- ' + text));
            if (CELL.includes(kind)) return stay('セルで箇条書き');
            return undefined;
          }
          case '番号': {
            if (PARA.includes(kind)) return line('1. ' + bt.replace(/^　+/, ''));
            if (kind === 'h') return line('1. ' + text);
            if (kind === 'ol') return keepList(line(text));
            if (['li1', 'li', 'liN'].includes(kind)) return keepList(line('1. ' + text));
            if (kind === 'nest') return keepList(line('  1. ' + text));
            if (kind === 'task') return keepList(line('1. [ ] ' + text));
            if (kind === 'done') return keepList(line('1. [x] ' + text));
            if (CELL.includes(kind)) return stay('セルで番号');
            return undefined;
          }
          case 'チェック': {
            if (PARA.includes(kind)) return line('- [ ] ' + bt.replace(/^　+/, ''));
            if (kind === 'h') return line('- [ ] ' + text);
            if (['li1', 'li', 'liN'].includes(kind)) return line('- [ ] ' + text);
            if (kind === 'nest') return line('  - [ ] ' + text);
            if (kind === 'ol') return line('2. [ ] ' + text);
            if (['task', 'done'].includes(kind)) return line('- ' + text);
            if (kind === 'q1') return md.includes('- [ ] 引用の一行目') ? true : '引用から出て升になるはずが: ' + diff(was, md);
            if (kind === 'alert') return line('- [ ] ' + text) === true && !md.includes('[!NOTE]') ? true : '注記から出て升になるはずが: ' + diff(was, md);
            if (CELL.includes(kind)) return stay('セルでチェック');
            return undefined;
          }
          case '引用': {
            if (PARA.includes(kind)) return line('> ' + bt);
            if (LIST.includes(kind)) return keepList(line('> ' + text));
            if (kind === 'q1' || kind === 'q2') return mdLine(md, '引用の一行目') === '引用の一行目' ? true : '引用の中で押したら外れるはずが: ' + diff(was, md);
            return undefined;
          }
          default: return undefined;
        }
      }

      async function run(html, md, text, kind, where, op) {
        errors = [];
        window.show(html, md, false);
        await pause(20);
        const was = paperToMd(box, head);
        const blocks = [...box.children].map((n) => (richBlock(n) ? (n.dataset.md ?? null) : blockToMd(n)));
        const missing = spot(text, where);
        if (missing) return { md: was, why: [missing] };
        const bt = line ? line.textContent : '';
        const block = (() => { let n = line; while (n && n.parentElement !== box) n = n.parentElement; return n ? [...box.children].indexOf(n) : -1; })();
        const why = [];
        try { await doOp(op); } catch (e) { why.push('押したら落ちました: ' + e.message); }
        await pause(40);
        if (errors.length) why.push(...errors.map((e) => '例外: ' + e));
        let now = null;
        try { now = paperToMd(box, head); } catch (e) { why.push('字に戻すとき落ちます: ' + e.message); }
        if (now === null && !why.length) why.push('もう字に戻せません（paperToMd が null）');
        if (!caretInBox()) why.push('焦点が面から外れました');
        const lt = line && box.contains(line) ? line.textContent : null;
        if (now !== null) {
          const want = expect({ op, kind, where, text, bt, lt, md: now, was, blocks, block });
          if (typeof want === 'string') why.push(want);
        }
        return { md: now, why };
      }

      return { plan, run };
    })();
    """#
}
#endif
