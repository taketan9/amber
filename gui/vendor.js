// Put the editor's runtime under gui/vendor/, trimmed to what is actually run.
//
//     node gui/vendor.js
//
// Not committed, and not downloaded either — copied out of node_modules, which
// is where `npm install` has already put it. The same reasoning as the bundled
// font: several megabytes that never change do not belong in every clone
// forever, and the release workflow is the place that assembles them.
//
// **What is left out is most of it.** `min/vs/language` is 6.9 MB of language
// *services* — the whole TypeScript compiler, and the CSS, HTML and JSON
// analysers — which exist to give an IDE its red squiggles. cian is a file
// manager with an editor in it, and shipping a compiler to colour a batch file
// would be absurd. Syntax highlighting comes from `basic-languages`, which is
// 640 KB for eighty-one of them.

const fs = require('node:fs');
const path = require('node:path');

const HERE = __dirname;
const MODULES = path.join(HERE, 'node_modules');
const OUT = path.join(HERE, 'vendor');

/// Everything the editor loads at runtime, and nothing else.
const WANTED = [
    ['monaco-editor/min/vs/loader.js', 'monaco/vs/loader.js'],
    ['monaco-editor/min/vs/base', 'monaco/vs/base'],
    ['monaco-editor/min/vs/editor', 'monaco/vs/editor'],
    ['monaco-editor/min/vs/basic-languages', 'monaco/vs/basic-languages'],
    // Japanese only. The other eight locales are 1.5 MB for languages this
    // is not offered in; English is built into editor.main.js.
    ['monaco-editor/min/vs/nls.messages.ja.js', 'monaco/vs/nls.messages.ja.js'],
    ['monaco-vim/dist/monaco-vim.umd.js', 'monaco-vim.js'],
    // The licences travel with the code they cover. Both are MIT, and a
    // release that shipped the minified JavaScript without them would be
    // distributing someone's work with the terms filed off.
    ['monaco-editor/LICENSE', 'monaco-editor.LICENSE'],
    ['monaco-editor/ThirdPartyNotices.txt', 'monaco-editor.ThirdPartyNotices.txt'],
    ['monaco-vim/LICENSE', 'monaco-vim.LICENSE'],
];

/// Source maps double the size and are read by nobody here — this is a
/// dependency being run, not one being debugged.
function copy(from, to) {
    const stat = fs.statSync(from);
    if (stat.isDirectory()) {
        fs.mkdirSync(to, { recursive: true });
        for (const name of fs.readdirSync(from)) copy(path.join(from, name), path.join(to, name));
        return;
    }
    if (from.endsWith('.map')) return;
    fs.mkdirSync(path.dirname(to), { recursive: true });
    fs.copyFileSync(from, to);
}

function bytes(dir) {
    let n = 0;
    for (const name of fs.readdirSync(dir)) {
        const at = path.join(dir, name);
        const s = fs.statSync(at);
        n += s.isDirectory() ? bytes(at) : s.size;
    }
    return n;
}

const missing = WANTED.filter(([from]) => !fs.existsSync(path.join(MODULES, from)));
if (missing.length) {
    console.error('node_modules に無いものがあります:');
    for (const [from] of missing) console.error(`  ${from}`);
    console.error('\ngui/ で npm install を先に走らせてください。');
    process.exit(1);
}

fs.rmSync(OUT, { recursive: true, force: true });
for (const [from, to] of WANTED) copy(path.join(MODULES, from), path.join(OUT, to));
console.log(`gui/vendor/  ${(bytes(OUT) / 1024 / 1024).toFixed(1)} MB`);
