#!/usr/bin/env node
/*
 * **会社で叩く一本**（依頼 639・本人「資材をもちこむだけにして、社内では
 * ビルドするだけにしたい。持ち込む資材は amber-src.zip だけ」）。
 *
 *     node scripts\build-win.js --electron C:\electron-v33.4.11-win32-x64
 *
 * 出来上がるのは三つ（`--out`、既定は `dist`）:
 *
 *   * `amber-gui.zip`                    ── 同梱する側（crmaine）へ渡す画面一式
 *   * `amber-server-win-x64.exe.zip`     ── エンジン一枚（持ち込んだものを包み直す）
 *   * `amber-win-x64-office-<版>.zip`    ── 会社向けの ambər 本体
 *
 * **組むだけ。取りに行かない。** 会社の端末は網の外（依頼 586）なので、
 * ここで要るものは**ぜんぶ持ち込んだ一式の中にある**:
 *
 *   * 画面（`gui/`。`vendor/` に Monaco と mermaid が入っている ── `npm` は要らない）
 *   * 印と見本（`packaging/`）
 *   * エンジン（`amber-server-win-x64.exe`。Rust も要らない）
 *
 * **Electron だけは外から。** 百メガあるものを毎回配るより、会社に既に
 * ある一式（cian と同じもの）を指してもらうほうが速い ── `--electron`。
 * 名前と絵を exe に焼くなら `--rcedit C:\tools\rcedit-x64.exe`（無くても
 * 組める。Electron の名前と絵のままになる）。
 *
 * **ふつうの版も要るなら** `--full` ── `amber-win-x64-<版>.zip` が増える。
 * 既定は会社向け（office）だけ ── 同じ画面で二つ並ぶと、どちらを配ったのか
 * 分からなくなる（依頼 602 と同じ用心）。
 */
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { zipFiles } = require('./zip');

const ROOT = path.join(__dirname, '..');

function arg(name, fallback = null) {
    const i = process.argv.indexOf('--' + name);
    if (i < 0) return fallback;
    const v = process.argv[i + 1];
    return !v || v.startsWith('--') ? true : v;
}
const has = (name) => process.argv.includes('--' + name);

function die(why) {
    console.error(why);
    process.exit(1);
}

/// 版は `gui/package.json` が言う ── 名前に焼くのはここだけ。
function version() {
    return JSON.parse(fs.readFileSync(path.join(ROOT, 'gui', 'package.json'), 'utf8')).version;
}

/// 持ち込んだエンジン。**隣にあるものを最初に見る** ── 一式の中に置いてある
/// ので、ふだんは `--engine` を打たなくていい。
function engine() {
    const given = arg('engine') || arg('server');
    const tries = given && given !== true
        ? [given]
        : [
            path.join(ROOT, 'amber-server-win-x64.exe'),
            path.join(ROOT, 'engine', 'amber-server-win-x64.exe'),
        ];
    const at = tries.find((p) => fs.existsSync(p));
    if (!at) {
        die('エンジンがありません: ' + tries.join(' / ')
            + '\n（持ち込んだ一式の中の amber-server-win-x64.exe を --engine で指してください）');
    }
    return at;
}

/// 同梱する側へ渡す一枚（`amber-gui.zip`）。**中身は `packaging/gui_zip.py`
/// と同じ顔ぶれ** ── 画面と、印と、見本のノート。会社の端末に python は
/// 無いことのほうが多いので、こちらは Node だけで組む（`scripts/zip.js`）。
function guiZip(out) {
    const rows = [];
    const walk = (dir, into) => {
        for (const name of fs.readdirSync(dir).sort()) {
            // **`node_modules` は入れない。** あそこには Electron 本体が居て、
            // 同梱する側は自分の Electron の中で動かす。
            if (name === 'node_modules') continue;
            const at = path.join(dir, name);
            const rel = into + '/' + name;
            if (fs.statSync(at).isDirectory()) walk(at, rel);
            else rows.push({ at, rel });
        }
    };
    walk(path.join(ROOT, 'gui'), 'gui');
    rows.push({
        at: path.join(ROOT, 'packaging', 'amber-mark.png'),
        rel: 'packaging/amber-mark.png',
    });
    walk(path.join(ROOT, 'packaging', 'welcome'), 'packaging/welcome');
    // **`LICENSE` も渡す**（`packaging/gui_zip.py` と同じ顔ぶれ）── 同梱する
    // 側が配るものの中に、こちらの許諾が無いことになる。
    const lic = path.join(ROOT, 'LICENSE');
    if (fs.existsSync(lic)) rows.push({ at: lic, rel: 'LICENSE' });
    for (const r of rows) {
        if (!fs.existsSync(r.at)) die('ありません: ' + r.at);
    }
    // **フォルダの項目も入れる**（Python 版と同じ）── 入れないと、Unix で
    // 展開したときに入れないフォルダができることがある。
    const dirs = new Set();
    for (const r of rows) {
        const parts = r.rel.split('/');
        for (let i = 1; i < parts.length; i += 1) dirs.add(parts.slice(0, i).join('/'));
    }
    const all = [...[...dirs].sort().map((d) => ({ rel: d, dir: true })), ...rows];
    zipFiles(all, out);
    return rows.length;
}

function main() {
    const outDir = path.join(process.cwd(), String(arg('out', 'dist')));
    const electron = arg('electron');
    if (!electron || electron === true) {
        die('Windows の Electron の一式を --electron で指してください'
            + '\n（cian と同じもので構いません: 例 C:\\electron-v33.4.11-win32-x64）');
    }
    if (!fs.existsSync(path.join(String(electron), 'electron.exe'))) {
        die('electron.exe がありません: ' + electron);
    }
    fs.mkdirSync(outDir, { recursive: true });
    const eng = engine();
    const v = version();

    // 一、画面の一式。
    const gui = path.join(outDir, 'amber-gui.zip');
    const n = guiZip(gui);
    console.log('組みました: ' + gui + '（' + n + ' 枚）');

    // 二、エンジンを包み直す。**中は一枚、名前はそのまま** ── 取り出した人が
    // 名前を直さずに置ける（リリースの並びと同じ形）。
    const engZip = path.join(outDir, 'amber-server-win-x64.exe.zip');
    zipFiles([{ at: eng, rel: 'amber-server-win-x64.exe' }], engZip);
    console.log('組みました: ' + engZip);

    // 三、本体。**組むのは `pack.js`** ── 会社でもリリースでも同じ道具が
    // 組む（二つ書くと、配った一枚と手元の一枚が別物になりうる）。
    const pack = (edition, into) => {
        const args = [
            path.join(__dirname, 'pack.js'), '--out', into, '--platform', 'win32',
            '--electron', String(electron), '--engine', eng, '--zip',
        ];
        if (edition === 'office') args.push('--edition', 'office');
        const rcedit = arg('rcedit');
        if (rcedit && rcedit !== true) args.push('--rcedit', String(rcedit));
        execFileSync(process.execPath, args, { stdio: 'inherit', cwd: process.cwd() });
    };
    pack('office', outDir);
    if (has('full')) pack('full', outDir);

    // **出口で数える。** 組んだつもりで無い、を配らないため（`pack.js` も
    // 中身を数えるが、こちらは「三つ揃ったか」を見る）。
    const want = [
        'amber-gui.zip',
        'amber-server-win-x64.exe.zip',
        `amber-win-x64-office-${v}.zip`,
        ...(has('full') ? [`amber-win-x64-${v}.zip`] : []),
    ];
    const missing = want.filter((f) => !fs.existsSync(path.join(outDir, f)));
    if (missing.length) die('出来ていません: ' + missing.join(' / '));
    console.log('');
    console.log('できました（' + outDir + '）:');
    for (const f of want) {
        console.log('  ' + f + '  ' + Math.round(fs.statSync(path.join(outDir, f)).size / 1024) + ' KB');
    }
}

main();
