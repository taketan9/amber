// エディタの実体を gui/vendor/ に置く。走らせるのに要るものだけ。
//
//     node gui/vendor.js
//
// git には入れない。落としてもこない ── `npm install` が既に node_modules に
// 置いたものを写すだけ。**数メガの、決して変わらないものを、複製のたびに
// 永久に配る理由は無い。**
//
// **language services は入れる。** 7MB あって（TypeScript のコンパイラ込み）、
// 外すのが明らかに正しく見えた。色付けは `basic-languages` から来るので。
// 間違いだった ── Monaco は `.js` を開いた瞬間に `vs/language/typescript/tsMode`
// を要求し、無いと開くたびに例外が出る。**表示も色付けも動くので、被害は
// 開くたび一つの例外**という、本物の例外を隠すまで気づかない類のもの。

const fs = require('node:fs');
const path = require('node:path');

const HERE = __dirname;
const MODULES = path.join(HERE, 'node_modules');
const OUT = path.join(HERE, 'vendor');

const WANTED = [
    ['monaco-editor/min/vs/loader.js', 'monaco/vs/loader.js'],
    ['monaco-editor/min/vs/base', 'monaco/vs/base'],
    ['monaco-editor/min/vs/editor', 'monaco/vs/editor'],
    ['monaco-editor/min/vs/basic-languages', 'monaco/vs/basic-languages'],
    ['monaco-editor/min/vs/language', 'monaco/vs/language'],
    // 日本語だけ。英語は editor.main.js の中にある。
    ['monaco-editor/min/vs/nls.messages.ja.js', 'monaco/vs/nls.messages.ja.js'],
    ['monaco-editor/LICENSE', 'monaco-editor.LICENSE'],
    // vim。**UMD の一枚だけ。** AMD の枝を通るので、Monaco のローダに
    // `monaco-vim` という名前で置けばそのまま読める（`renderer.js` の
    // `require.config`）。中で `monaco-editor/esm/…/editor.api` を要求して
    // くるが、そこは既に読んである `monaco` を返す偽物を先に定義して渡す。
    ['monaco-vim/dist/monaco-vim.umd.js', 'monaco-vim/monaco-vim.umd.js'],
    ['monaco-vim/LICENSE', 'monaco-vim.LICENSE'],
    // 図。**一枚で完結している版**（`min` は chunks を要らない）。3.4MB
    // あるので、読むのは図のあるノートを開いたときだけ ── `renderer.js` の
    // `loadMermaid` が初めて要るまで触らない。
    ['mermaid/dist/mermaid.min.js', 'mermaid/mermaid.min.js'],
    ['mermaid/LICENSE', 'mermaid.LICENSE'],
];

let made = 0;
for (const [from, to] of WANTED) {
    const src = path.join(MODULES, from);
    const dst = path.join(OUT, to);
    if (!fs.existsSync(src)) {
        console.error(`ありません: ${from} — 先に gui で npm install`);
        process.exit(1);
    }
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    fs.cpSync(src, dst, { recursive: true });
    made++;
}
console.log(`${made} 件を gui/vendor へ置きました`);
