#!/usr/bin/env node
/*
 * **会社で「直して・組んで・試す」ための一式**（`amber-dev.zip`・依頼 640）。
 *
 *     node scripts/dev-kit.js --rust rust-x86_64-pc-windows-gnu.msi \
 *                             --vendor vendor --engine dist/amber-server-win-x64.exe \
 *                             --out out/amber-dev.zip
 *
 * 配るものは二つに分けてある（本人が決めた・2026-09-19）:
 *
 *   * `amber-src.zip`（10MB・依頼 639）── **組むだけ。** ふだん運ぶのはこちら
 *   * `amber-dev.zip`（450MB ほど・これ）── **直せる。** 判断の側（Rust）まで
 *
 * 中身:
 *
 *   amber-dev/
 *     はじめに読んでください.txt   ← 入れ方と、叩く一行
 *     rust/…-windows-gnu.msi       ← Rust 本体（**自己完結版** ＝ Visual Studio が要らない）
 *     amber/                       ← ソース一式（画面も判断の側も・試験ごと）
 *       .cargo/config.toml         ← 依存は下の vendor から取る（網に出ない）
 *       vendor/                    ← 依存の実体（`cargo vendor`）
 *       amber-server-win-x64.exe   ← 出来合いのエンジン（組む前でも画面を動かせる）
 *
 * **Linux でしか使わない依存は捨てる**（`linux-raw-sys` ほか・18MB）── あの
 * 機械では一度も開かれない。
 */
'use strict';
const fs = require('node:fs');
const { execFileSync } = require('node:child_process');
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

/// あの機械では開かれない依存。**Windows で組むのに要らないもの**だけを挙げる
/// ── 迷ったら残す（捨てて足りないほうが、重いより高くつく）。
const NOT_ON_WINDOWS = [
    'linux-raw-sys',   // Linux の system call の型（18MB）
];

/// ソース一式は **git が知っているものだけ**（依頼 640）。
///
/// 前は「この名前は飛ばす」という引き算で選んでいて、**組んだ場所に転がって
/// いたものまで入った** ── CI では `vendor/`（依存 100MB）と `rustdist/`
/// （Rust の一枚 358MB）が根に置かれていて、それを二度包み、746MB になった。
/// 足し算で選べば、知らないものは入りようがない。
function tracked() {
    const out = execFileSync('git', ['-C', ROOT, 'ls-files', '-z'], { encoding: 'buffer' });
    return out.toString('utf8').split('\0').filter(Boolean);
}

function walk(dir, into, out, skip = () => false) {
    for (const name of fs.readdirSync(dir).sort()) {
        if (skip(name)) continue;
        const at = path.join(dir, name);
        const st = fs.lstatSync(at);
        if (st.isSymbolicLink()) continue;
        const rel = into + '/' + name;
        if (st.isDirectory()) walk(at, rel, out, skip);
        else out.push({ at, rel });
    }
}

function main() {
    const rust = String(arg('rust') || '');
    const vendor = String(arg('vendor') || '');
    const engine = String(arg('engine') || arg('server') || '');
    const out = path.resolve(String(arg('out', 'out/amber-dev.zip')));
    for (const [what, at] of [['Rust の一枚', rust], ['依存（cargo vendor）', vendor], ['エンジン', engine]]) {
        if (!at || !fs.existsSync(at)) die(what + 'がありません: ' + (at || '(指されていない)'));
    }

    const rows = [];
    // 一、ソース一式（画面も判断の側も、試験も台帳も）。**git が知っている
    // ものだけ** ── 組んだ場所に転がっているものを巻き込まない。
    for (const rel of tracked()) {
        const at = path.join(ROOT, rel);
        if (fs.existsSync(at) && fs.statSync(at).isFile()) {
            rows.push({ at, rel: 'amber-dev/amber/' + rel });
        }
    }
    // 画面の部品だけは git に入っていない（`npm` が置いたものの写し）。
    walk(path.join(ROOT, 'gui', 'vendor'), 'amber-dev/amber/gui/vendor', rows);
    // 二、依存の実体。**Linux 専用は捨てる。**
    const dropped = [];
    walk(vendor, 'amber-dev/amber/vendor', rows, (n) => {
        if (NOT_ON_WINDOWS.some((x) => n === x || n.startsWith(x + '-'))) {
            dropped.push(n);
            return true;
        }
        return false;
    });
    // 三、Rust 本体と、出来合いのエンジン。
    rows.push({ at: rust, rel: 'amber-dev/rust/' + path.basename(rust) });
    rows.push({ at: engine, rel: 'amber-dev/amber/amber-server-win-x64.exe' });

    // 四、依存の在り処を Cargo に教える一枚。**網に出さない**ための札でもある。
    const conf = [
        '# 依存は、隣の vendor から取る（依頼 640）。',
        '# **網には出ない** ── 会社の端末は外に出られないので、取りに行かせない。',
        '[source.crates-io]',
        'replace-with = "vendored-sources"',
        '',
        '[source.vendored-sources]',
        'directory = "vendor"',
        '',
    ].join('\n');
    const tmpDir = path.join(path.dirname(out), '.kit-tmp');
    fs.rmSync(tmpDir, { recursive: true, force: true });
    fs.mkdirSync(tmpDir, { recursive: true });
    const confAt = path.join(tmpDir, 'config.toml');
    fs.writeFileSync(confAt, conf);
    rows.push({ at: confAt, rel: 'amber-dev/amber/.cargo/config.toml' });

    const version = JSON.parse(
        fs.readFileSync(path.join(ROOT, 'gui', 'package.json'), 'utf8')
    ).version;
    const readAt = path.join(tmpDir, 'はじめに読んでください.txt');
    fs.writeFileSync(readAt, [
        `ambər ${version} ── 会社で直して・組んで・試すための一式`,
        '',
        '（組むだけでよければ、軽いほうの amber-src.zip で足ります）',
        '',
        '■ 一度だけ: Rust を入れる',
        '',
        `    rust\\${path.basename(rust)} をダブルクリックして、そのまま進む`,
        '',
        '    Visual Studio は要りません（この一枚に、組むのに要るものが全部入っています）。',
        '    入ったか見る:  cargo --version',
        '',
        '■ 直して、組む',
        '',
        '    cd amber',
        '    cargo build --release -p amber-server --target x86_64-pc-windows-gnu',
        '',
        '    出来たもの: target\\x86_64-pc-windows-gnu\\release\\amber-server.exe',
        '    これを gui\\amber-server.exe として置くと、画面がそれを使います。',
        '',
        '■ 試す',
        '',
        '    cargo test --workspace                 判断の側ぜんぶ（242 件ほど）',
        '    python scripts\\requests.py             台帳（依頼が守られているか）',
        '    node scripts\\win-test.js               窓の形',
        '',
        '    ※ jsdom を使う試験（paper-test など）は、npm が要るので회社では回りません。',
        '',
        '■ 配るものを組む',
        '',
        '    node scripts\\build-win.js --electron C:\\electron-v33.4.11-win32-x64',
        '',
        '■ 網には出ません',
        '',
        '    依存は amber\\vendor\\ から取ります（amber\\.cargo\\config.toml がそう指しています）。',
        '    cargo が外を見にいくことはありません。',
        '',
        '■ 許諾',
        '',
        '    Rust は MIT / Apache-2.0、依存はそれぞれの crate の許諾に従います',
        '    （vendor\\<名前>\\ の中に入っています）。',
        '',
    ].join('\r\n'));
    rows.push({ at: readAt, rel: 'amber-dev/はじめに読んでください.txt' });

    // **入れ忘れは、運んでから分かる。** 数えて言う。
    const have = new Set(rows.map((r) => r.rel));
    const must = [
        'amber-dev/amber/Cargo.toml',
        'amber-dev/amber/crates/amber-core/src/api.rs',
        'amber-dev/amber/crates/vendor/onenote_parser/Cargo.toml',
        'amber-dev/amber/gui/vendor/monaco/vs/loader.js',
        'amber-dev/amber/scripts/build-win.js',
        'amber-dev/amber/REQUESTS.ja.md',
        'amber-dev/amber/.cargo/config.toml',
        'amber-dev/amber/amber-server-win-x64.exe',
    ];
    const missing = must.filter((m) => !have.has(m));
    if (missing.length) die('一式に足りないものがあります:\n  ' + missing.join('\n  '));

    zipFiles(rows, out);
    fs.rmSync(tmpDir, { recursive: true, force: true });
    const mb = (n) => Math.round(n / 1024 / 1024);
    console.log('組みました: ' + out);
    console.log('  ' + rows.length + ' 枚 ・ ' + mb(fs.statSync(out).size) + ' MB');
    if (dropped.length) console.log('  捨てた依存（Windows では開かない）: ' + dropped.join(' / '));
}

main();
