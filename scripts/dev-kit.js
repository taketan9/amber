#!/usr/bin/env node
/*
 * **会社で「直して・組んで・試す」ための一式**（`amber-dev.zip`・依頼 640）。
 *
 *     node scripts/dev-kit.js --rust-version 1.98.0 \
 *                             --vendor vendor --engine dist/amber-server-win-x64.exe \
 *                             --out out/amber-dev.zip
 *
 * 配るものは二つに分けてある（本人が決めた・2026-09-19）:
 *
 *   * `amber-src.zip`（10MB・依頼 639）── **ビルドするだけ。** ふだん運ぶのはこちら
 *   * `amber-dev.zip`（40MB ほど・これ）── **直せる。** 判断の側（Rust）まで
 *
 * 中身:
 *
 *   amber-dev/
 *     はじめに読んでください.txt   ← Rust の落とし先と、叩く一行
 *     amber/                       ← ソース一式（画面も判断の側も・試験ごと）
 *       .cargo/config.toml         ← 依存は下の vendor から取る（ネットワークに出ない）
 *       vendor/                    ← 依存の実体（`cargo vendor`）
 *       amber-server-win-x64.exe   ← 出来合いのエンジン（ビルドする前でも画面を動かせる）
 *
 * **Rust 本体（375MB）は入れない**（依頼 641・本人が決めた・2026-09-20）。
 * あれは amber の版が変わっても 1バイトも変わらないので、版ごとに置き直すと
 * 同じ 375MB がラベルの数だけ積み上がる（v3.1.2〜v3.1.7 で実際に 1.66GB 積んで、
 * 消した）。代わりに `はじめに読んでください.txt` へ**版を焼き込んだ URL**を
 * 一行書く ── ネットワークの外なのは会社の端末で、**落とす人の手元は外に出られる**。
 * 落とすのは一度だけで、次からは USB のものを使い回せる。
 *
 * **Linux でしか使わない依存は捨てる**（`linux-raw-sys` ほか・18MB）── あの
 * 環境では一度も開かれない。
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

/// **Rust 本体の落とし先。** 版を焼き込んである ── rust が 1.99 になっても、
/// ここに置かれた 1.98.0 はそのまま残る（rust-lang.org は古い版を消さない）。
/// CI がビルドするのに使った版をそのまま渡すので、**会社で入る Rust と、ここで
/// 「組めた」を見た Rust が同じものになる。**
const rustMsi = (v) =>
    `https://static.rust-lang.org/dist/rust-${v}-x86_64-pc-windows-gnu.msi`;

/// あの環境では開かれない依存。**Windows でビルドするのに要らないもの**だけを挙げる
/// ── 迷ったら残す（捨てて足りないほうが、重いより高くつく）。
const NOT_ON_WINDOWS = [
    'linux-raw-sys',   // Linux の system call の型（18MB）
];

/// ソース一式は **git が知っているものだけ**（依頼 640）。
///
/// 前は「この名前は飛ばす」という引き算で選んでいて、**ビルドした場所に転がって
/// いたものまで入った** ── CI では `vendor/`（依存 100MB）と `rustdist/`
/// （Rust の1 つ 358MB）が根に置かれていて、それを二度包み、746MB になった。
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
    const rustVersion = String(arg('rust-version') || '');
    const vendor = String(arg('vendor') || '');
    const engine = String(arg('engine') || arg('server') || '');
    const out = path.resolve(String(arg('out', 'out/amber-dev.zip')));
    // **版は、こちらで当てずに渡してもらう。** URL に焼き込むものなので、
    // 外した URL を配ると「落としたら無かった」が会社で分かる。
    if (!/^\d+\.\d+\.\d+$/.test(rustVersion)) {
        die('Rust の版がありません（--rust-version 1.98.0 のように渡してください）: '
            + (rustVersion || '(指されていない)'));
    }
    for (const [what, at] of [['依存（cargo vendor）', vendor], ['エンジン', engine]]) {
        if (!at || !fs.existsSync(at)) die(what + 'がありません: ' + (at || '(指されていない)'));
    }

    const rows = [];
    // 一、ソース一式（画面も判断の側も、試験も台帳も）。**git が知っている
    // ものだけ** ── ビルドした場所に転がっているものを巻き込まない。
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
    // 三、出来合いのエンジン（Rust 本体は包まない ── 上の注を見よ）。
    rows.push({ at: engine, rel: 'amber-dev/amber/amber-server-win-x64.exe' });

    // 四、依存の在り処を Cargo に教える1 つ。**ネットワークに出さない**ためのラベルでもある。
    const conf = [
        '# 依存は、隣の vendor から取る（依頼 640）。',
        '# **ネットワークには出ない** ── 会社の端末は外に出られないので、取りに行かせない。',
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
        '■ 一度だけ: Rust を入れる（この一式には入っていません）',
        '',
        '    Rust 本体は 375MB あり、ambər の版が変わっても中身は同じです。',
        '    毎回運ぶ意味がないので、入れていません（依頼 641）。',
        '',
        '    ネットワークにつながる端末で下の1 つを落として、この一式と一緒に USB へ入れて',
        '    ください。落とすのは一度だけで、次からは同じものを使い回せます。',
        '',
        `        ${rustMsi(rustVersion)}`,
        '',
        `    落とした rust-${rustVersion}-x86_64-pc-windows-gnu.msi をダブルクリックして、`,
        '    そのまま進む。',
        '',
        '    Visual Studio は要りません（この1 つに、組むのに要るものが全部入っています）。',
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
        '    node scripts\\win-test.js               デスクトップ版の形',
        '',
        '    ※ jsdom を使う試験（paper-test など）は、npm が要るので会社では回りません。',
        '',
        '■ 配るものを組む',
        '',
        '    node scripts\\build-win.js --electron C:\\electron-v33.4.11-win32-x64',
        '',
        '■ ネットワークには出ません',
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
