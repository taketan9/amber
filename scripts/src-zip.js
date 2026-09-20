#!/usr/bin/env node
/*
 * **会社へ持ち込む一式**（`amber-src.zip`・依頼 639）。
 *
 *     node scripts/src-zip.js --engine dist/amber-server-win-x64.exe --out out/amber-src.zip
 *
 * 会社の端末はネットワークの外（依頼 586）で、Rust も npm も無い。だから**ビルドするのに
 * 要るものを、ぜんぶ1 つに入れて持ち込む**:
 *
 *   * `gui/`        ── 画面。`vendor/`（Monaco・mermaid）も入れる ＝ `npm` が要らない
 *   * `packaging/`  ── 印・サンプルのノート・テンプレート
 *   * `scripts/`    ── ビルドする道具（`build-win.js` / `pack.js` / `zip.js`）
 *   * `amber-server-win-x64.exe` ── エンジン ＝ Rust が要らない
 *   * `はじめに読んでください.txt` ── 叩く一行
 *
 * **Electron は入れない。** 百メガあり、会社には cian のぶんが既にある
 * （ビルドするときに `--electron` で指す）。
 *
 * 名前は `cian-src` / `crmaine-src` に揃えて、中のフォルダは `amber-src`
 * ── 手が憶えているほうが正しい（依頼 550 と同じ）。
 */
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const { zipFiles } = require('./zip');

const ROOT = path.join(__dirname, '..');

function arg(name, fallback = null) {
    const i = process.argv.indexOf('--' + name);
    if (i < 0) return fallback;
    const v = process.argv[i + 1];
    return !v || v.startsWith('--') ? true : v;
}

function die(why) {
    console.error(why);
    process.exit(1);
}

/// 入れる道具。**ビルドするのに要るものだけ** ── 試験や取り込みの道具まで入れると、
/// 会社の人が「どれを叩くのか」から探すことになる。
const TOOLS = ['build-win.js', 'pack.js', 'zip.js'];

function rows(engine, version) {
    const out = [];
    const walk = (dir, into, skip = () => false) => {
        for (const name of fs.readdirSync(dir).sort()) {
            if (skip(name)) continue;
            const at = path.join(dir, name);
            const rel = into + '/' + name;
            if (fs.statSync(at).isDirectory()) walk(at, rel, skip);
            else out.push({ at, rel });
        }
    };
    // **`node_modules` は入れない。** 五百メガあり、ビルドするのには要らない
    // （実行時に要るものは `gui/vendor/` に積んである）。
    walk(path.join(ROOT, 'gui'), 'amber-src/gui', (n) => n === 'node_modules');
    walk(path.join(ROOT, 'packaging'), 'amber-src/packaging');
    for (const t of TOOLS) {
        out.push({ at: path.join(ROOT, 'scripts', t), rel: 'amber-src/scripts/' + t });
    }
    out.push({ at: engine, rel: 'amber-src/amber-server-win-x64.exe' });
    // **許諾も持っていく**（`amber-gui.zip` の中に入る1 つ）── 同梱する側が
    // 配るものの中に、こちらの許諾が無いことになる。
    const lic = path.join(ROOT, 'LICENSE');
    if (fs.existsSync(lic)) out.push({ at: lic, rel: 'amber-src/LICENSE' });
    return out;
}

/// 叩く一行を、一式の中に置く。**読む人はここしか読まない**ので、これだけで
/// 組めるように書く。
function readme(version) {
    return [
        `ambər ${version} ── 会社で組むための一式`,
        '',
        '要るもの: Node（cian と同じもので構いません）と、Windows の Electron 一式。',
        'Rust も npm も要りません（エンジンと画面のパーサーは、この中に入っています）。',
        '',
        '組む:',
        '',
        '    node scripts\\build-win.js --electron C:\\electron-v33.4.11-win32-x64',
        '',
        '名前と絵を exe に焼くなら（無くても組めます）:',
        '',
        '    node scripts\\build-win.js --electron C:\\electron-v33.4.11-win32-x64 --rcedit C:\\tools\\rcedit-x64.exe',
        '',
        'できるもの（dist の下）:',
        '',
        '    amber-gui.zip                  同梱する側へ渡す画面一式',
        '    amber-server-win-x64.exe.zip   エンジン1 つ',
        `    amber-win-x64-office-${version}.zip   会社向けの ambər 本体`,
        '',
        'ふつうの版（同期やカレンダーの同期が入ったもの）も要るときは --full を足します。',
        '',
        '組むあいだ、ネットワークには出ません。取りに行くものは一つもありません。',
        '',
    ].join('\r\n');
}

function main() {
    const engine = String(arg('engine') || arg('server') || '');
    if (!engine || !fs.existsSync(engine)) {
        die('エンジンがありません: ' + (engine || '(--engine が無い)')
            + '\n（リリースの amber-server-win-x64.exe を --engine で指してください）');
    }
    const version = JSON.parse(
        fs.readFileSync(path.join(ROOT, 'gui', 'package.json'), 'utf8')
    ).version;
    const out = path.resolve(String(arg('out', 'out/amber-src.zip')));
    fs.mkdirSync(path.dirname(out), { recursive: true });

    const list = rows(engine, version);
    // **積み忘れは、配ってから分かる。** 画面の部品（Monaco・mermaid）は
    // `npm ci && node vendor.js` を通した環境でしか揃わない ── 数えて言う。
    const must = [
        'amber-src/gui/vendor/monaco/vs/loader.js',
        'amber-src/gui/vendor/mermaid/mermaid.min.js',
        'amber-src/gui/vendor/monaco-vim/monaco-vim.umd.js',
        'amber-src/gui/main.js',
        'amber-src/gui/renderer.js',
        'amber-src/packaging/amber-mark.png',
        'amber-src/scripts/build-win.js',
        'amber-src/amber-server-win-x64.exe',
        'amber-src/LICENSE',
    ];
    const have = new Set(list.map((r) => r.rel));
    const missing = must.filter((m) => !have.has(m));
    if (missing.length) {
        die('一式に足りないものがあります:\n  ' + missing.join('\n  ')
            + '\n（gui で `npm ci && node vendor.js` を通してから組んでください）');
    }
    const notes = list.filter((r) => r.rel.startsWith('amber-src/packaging/welcome/')
        && r.rel.endsWith('.md')).length;
    if (notes < 1) die('サンプルのノートが1 つもありません（packaging/welcome）');

    // 読む1 つは、その場で作って入れる（実物のファイルは持たない）。
    const tmp = path.join(path.dirname(out), 'はじめに読んでください.txt');
    fs.writeFileSync(tmp, readme(version));
    list.push({ at: tmp, rel: 'amber-src/はじめに読んでください.txt' });

    zipFiles(list, out);
    fs.rmSync(tmp, { force: true });
    console.log('組みました: ' + out);
    console.log('  ' + list.length + ' 枚 ・ '
        + Math.round(fs.statSync(out).size / 1024 / 1024) + ' MB ・ サンプル ' + notes + ' 枚');
}

main();
