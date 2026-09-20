#!/usr/bin/env node
/* コメントが、普通の日本語のままか（依頼 642）。
 *
 *     node scripts/plain-test.js
 *
 * 本人が 2026-09-20 に決めた: **比喩をやめて、全部普通の日本語にする。**
 * 決め手は数えたこと ── 本人の逐語の引用 94 件（2,289 字）の中に `窓`
 * `電話` `扉` `献立` `札` `印` は一度も出てこず、使っているのはタブ・
 * ボタン・スマホ・iPhone・画面・ファイル・フォルダだった。台帳には
 * 「道 とか 持ち物 とか 独創的だなｗ」という行まで残っていて、あれは
 * こちらの造語を面白がっていたのであって、採用していたわけではなかった。
 *
 * **これが無いと、また生える。** 用語表は覚えているあいだしか効かない。
 * 次に書く人（たいてい僕）が「窓」と書いた日に、ここが鳴る。
 *
 * 見るのは**コメントと、人が読む文字列**だけ。試験データ（ノートの中身・
 * 絵文字の名前・単語の並び）は見ない ── あれは「何を入れたか」であって
 * 説明ではないし、`# 図と字` を直すと試験の入力と期待値がずれる。
 */
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const ROOT = path.join(__dirname, '..');

/// 使わないと決めた語 → 代わりに使う語。**表そのものが決めごと**なので、
/// 増やすときは本人に訊く（依頼 642 で一つずつ決めた）。
const PLAIN = [
    ['窓', '画面／デスクトップ版'], ['電話', 'iPhone'], ['扉', '入口'],
    ['献立', 'メニュー'], ['升', 'セル／チェックボックス'], ['飾り', '書式'],
    ['一枚', 'ファイル／ビルド'], ['一冊', 'ノートブック'], ['機械', '端末／環境'],
    ['持ち物', '自前の型'], ['借り物', '外部クレート'], ['束ね', 'バンドル'],
    ['見本', 'サンプル'], ['字下げ', 'インデント'], ['鍵盤', 'キーボード'],
];

/// 見にいく場所。`crates/vendor` は本家の写しなので触らない（依頼 636）。
const DIRS = ['crates/amber-core', 'crates/amber-ffi', 'crates/amber-server',
              'crates/amber-onenote', 'gui', 'ios/Cian', 'scripts', 'docs',
              'packaging', '.github'];

/// 見ない場所。
///
///   * `packaging/welcome/` ── **利用者が読む本文**で、コメントではない。
///     本人が良いと言ったものだし、松陰の引用も入っている
///   * このファイル自身 ── **使わないと決めた語を並べるのが仕事**なので、
///     自分を見ると必ず鳴る。作った直後は追跡下に無くて通り、コミットした
///     瞬間に鳴った（`git ls-files` は追跡しているものだけ返す）
const SKIP = ['packaging/welcome/', 'scripts/plain-test.js'];

/// 直さないと決めたもの。**一つずつ理由を書く** ── 理由の書けない例外は、
/// 例外ではなく直し忘れ。
const KEEP = [
    // 試験データ。直すと入力と期待値がずれる。
    /# 図と字/, /献立\.md/, /スマホ 電話 phone/, /"電話"/, /\| 面 \| いつ \|/,
    /"面"\.to_string/, /だいだいの字/, /という札です/, /ふつうの字と/,
    // 普通の日本語で、比喩ではない。
    /添え字/, /星印/, /目印/, /印を付け/, /印刷/, /来た道/, /道があれば/,
    /窓口/, /葉に字を書いた/, /画面/, /文字/,
];

/// **英語の説明文かどうか。** 語彙だけ直しても、英語のまま残っていては
/// 「普通の日本語で書く」を果たしたことにならない ── 2026-09-20 に「英語は
/// repo から消えた」と書いたが、消えていなかった（`ios-build.sh` の頭など
/// 10 ファイルほどが英語のままだった）。**覚えているあいだしか効かない**
/// ので、機械に見させる。
///
/// **打ち方の見本は英語のまま。** `node scripts/paper-test.js` を日本語に
/// しても誰も打てない。CSS の選び方・記号の名前・引用したエラーの文面も
/// 同じ ── あれは説明ではなく、そう打つもの・そう出るもの。
function englishProse(line) {
    let t = line.replace(/^\s*(\/\/\/|\/\/!|\/\/|#(?!!)|\*|<!--)\s*/, '').trim();
    if (/[ぁ-んァ-ヶ一-龥]/.test(t)) return false;
    // 打ち方の見本・パス・旗。
    if (/^(node|npm|npx|cargo|python3?|bash|sh|zsh|git|gh|curl|scripts[\\/]|\.[\\/]|--|\||dist[\\/]|target[\\/]|package\.json|Error:|MARK:)/.test(t)) return false;
    // CSS の選び方。
    if (/[{};]\s*$|^[#.][\w-]+[\s,.:]|::before|::after|:hover|:not\(/.test(t)) return false;
    // 矢印の入った対応表（`- もの` → `(記号, 中身)` のたぐい）。
    if (/[→←]/.test(t)) return false;
    // 逆引用符の中は書いてあるとおりのもの ── 数える前に外す。
    t = t.replace(/`[^`]*`/g, ' ').replace(/"[^"]*"/g, ' ');
    const words = t.match(/[A-Za-z][A-Za-z'-]{2,}/g) || [];
    return words.length >= 4;
}

/// コメントの行を拾う。**囲みの中かどうかも一緒に返す** ── `.md` の
/// 囲み（``` で挟んだところ）に入っているのは説明ではなく、そう打つもの。
/// 語彙のほうは囲みの中も見る（書いてある言葉は言葉なので）が、英語の
/// 地の文は見ない。
function comments(file, text) {
    const out = [];
    const ext = path.extname(file);
    const mark = ext === '.py' || ext === '.sh' || ext === '.yml' || ext === '.toml'
        ? /^\s*#(?!!)/ : /^\s*(\/\/\/|\/\/!|\/\/|\*|<!--)/;
    let fence = false;
    text.split('\n').forEach((line, i) => {
        if (ext === '.md' && /^\s*(```|~~~)/.test(line)) { fence = !fence; return; }
        if (mark.test(line) || (ext === '.md' && line.trim())) out.push([i + 1, line, fence]);
    });
    return out;
}

function main() {
    const files = execFileSync('git', ['-C', ROOT, 'ls-files', '-z'], { encoding: 'buffer' })
        .toString('utf8').split('\0').filter(Boolean)
        .filter((f) => DIRS.some((d) => f.startsWith(d)))
        .filter((f) => !f.startsWith('gui/node_modules') && !f.includes('/vendor/'))
        .filter((f) => !SKIP.some((s) => f.startsWith(s)))
        .filter((f) => /\.(rs|swift|js|mjs|py|sh|yml|toml|md|html)$/.test(f));

    const bad = [];
    for (const f of files) {
        const text = fs.readFileSync(path.join(ROOT, f), 'utf8');
        for (const [n, line, fence] of comments(f, text)) {
            if (KEEP.some((k) => k.test(line))) continue;
            for (const [word, instead] of PLAIN) {
                if (line.includes(word)) bad.push({ f, n, word, instead, line: line.trim() });
            }
            if (!fence && englishProse(line)) {
                bad.push({ f, n, word: '英語のまま', instead: '日本語で書く', line: line.trim() });
            }
        }
    }
    if (bad.length) {
        console.error(`普通の日本語でないところが ${bad.length} か所あります（依頼 642）:\n`);
        for (const b of bad.slice(0, 20)) {
            console.error(`  ${b.f}:${b.n}  「${b.word}」→ ${b.instead}`);
            console.error(`      ${b.line.slice(0, 90)}`);
        }
        if (bad.length > 20) console.error(`  … ほか ${bad.length - 20} か所`);
        process.exit(1);
    }
    console.log(`コメントは普通の日本語です（${files.length} ファイル・${PLAIN.length} 語と、英語の地の文を見ました）`);
}

main();
