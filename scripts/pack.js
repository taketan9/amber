#!/usr/bin/env node
/*
 * **配るものを組む**（依頼 519）── Mac は `ambər.app`、Windows は `amber.exe` の入った
 * フォルダ。cian の `packaging/macos/bundle-gui.sh` と crmaine の `gui/pack.js` と
 * 同じ作り方: **Electron の一式を写して、名前と絵と中身だけ差し替える。**
 *
 *     node scripts/pack.js --mac --out dist [--zip]
 *     node scripts/pack.js --win --out dist --electron ~/workspace/electron-v33.4.11-win32-x64 \
 *                          --server dist/amber-server-win-x64.exe [--zip]
 *
 * **cian の綴りでも通る**（依頼 550・本人が会社の Windows で打つ形）──
 * `--platform win32|darwin` は `--win`／`--mac` と同じ、`--engine` は
 * `--server` と同じ。手が憶えているほうが正しい。
 *
 *     node scripts\pack.js --out dist --platform win32 --electron C:\electron-v33.4.11-win32-x64 ^
 *                          --engine C:\Downloads\amber-server-win-x64.exe --rcedit C:\tools\rcedit-x64.exe --zip
 *
 * **組むあいだ、網に出ない。** electron-builder のような「取りに行く」道具は
 * 使わない（crmaine が whl と vsce で二度やった事故 ── 途中で落ちて半端な生成物が
 * 残り、それが正常に見える）。写すだけで作り、**出口で必ず数えて**、欠けていれば
 * 失敗させる。
 *
 * 出来上がり（Mac）:
 *   dist/ambər.app/Contents/Resources/app/
 *     package.json          ← main: gui/main.js
 *     gui/                  ← 画面（node_modules は入れない。vendor/ に実行時の一式がある）
 *       amber-server        ← エンジン（engine.js は「隣」を最初に見る）
 *       amber-cal           ← この Mac の予定表に話す道具
 *     packaging/            ← 印・見本・テンプレート（main.js は `../packaging` を見る）
 *
 * 出来上がり（Windows）:
 *   dist/amber-win-x64/
 *     amber.exe             ← electron.exe を改名したもの
 *     resources/app/        ← 上と同じ形（gui/amber-server.exe）
 *
 * Windows の exe の絵と名前は、Windows の上で `rcedit` を掛けると替わる
 * （`--rcedit <rcedit-x64.exe>`）。Mac の上では替えられないので、Electron の絵のまま。
 */
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { execFileSync } = require('node:child_process');
const { zipDir } = require('./zip');

const ROOT = path.join(__dirname, '..');

function arg(name, fallback = null) {
    const i = process.argv.indexOf('--' + name);
    if (i < 0) return fallback;
    const v = process.argv[i + 1];
    return !v || v.startsWith('--') ? true : v;
}
const has = (name) => process.argv.includes('--' + name);
/** エンジンの在り処。**`--server` でも `--engine` でも同じ**（依頼 550）。 */
const engineArg = () => arg('server') || arg('engine');

/// **どの版を組むか**（依頼 602・本人「会社でビルドする際には、同期に関する
/// 機能や表示はすべてクローズにしたい」）。
///
/// `--edition office` を渡すと、配る一枚の中に `gui/edition.json` を置く
/// ── 画面はそれを見て、外の網に触るもの（Google Drive の同期・iCal の
/// 購読・グループ共有）を**出さないし、走らせない。**
/// 渡さなければ、その一枚は置かない ＝ ふつうの版。
const edition = (() => {
    const want = String(arg('edition', '') || '').trim();
    if (!want || want === 'full') return 'full';
    if (want !== 'office') {
        console.error('知らない版です: ' + want + '（full か office）');
        process.exit(1);
    }
    return want;
})();

/// 版。**`Cargo.toml` が本**だが、持ち込む一式には入っていない（依頼 639 の
/// `amber-src.zip` は画面と道具だけ）ので、無ければ `gui/package.json` を見る
/// ── 版は四か所で同じ数字なので、どちらを見ても同じものが出る。
const version = (() => {
    const cargo = path.join(ROOT, 'Cargo.toml');
    if (fs.existsSync(cargo)) {
        const m = fs.readFileSync(cargo, 'utf8').match(/^version = "(.+?)"/m);
        if (m) return m[1];
    }
    const pkg = path.join(ROOT, 'gui', 'package.json');
    if (fs.existsSync(pkg)) {
        const v = JSON.parse(fs.readFileSync(pkg, 'utf8')).version;
        if (v) return v;
    }
    return '0.0.0';
})();

/** 写す（フォルダは丸ごと・`skip` に当たる名前は飛ばす）。 */
function copy(from, to, skip = () => false) {
    const st = fs.statSync(from);
    if (st.isDirectory()) {
        fs.mkdirSync(to, { recursive: true });
        for (const name of fs.readdirSync(from)) {
            if (skip(name, path.join(from, name))) continue;
            copy(path.join(from, name), path.join(to, name), skip);
        }
    } else {
        fs.mkdirSync(path.dirname(to), { recursive: true });
        fs.copyFileSync(from, to);
        fs.chmodSync(to, st.mode);
    }
}

/**
 * **先に足りないものを言う**（依頼 550）。
 *
 * `gui/vendor/`（Monaco と mermaid・16MB）は **git に入っていない** ──
 * `npm install` が node_modules に置いたものを `gui/vendor.js` が写す形なので、
 * **ネットに出られない機械では作れない**。出口の `verify` でも捕まるが、
 * それでは Electron を二百メガ写したあとで落ちる。**写す前に言う。**
 */
function preflight() {
    const vendor = path.join(ROOT, 'gui', 'vendor');
    if (fs.existsSync(path.join(vendor, 'monaco', 'vs', 'loader.js'))
        && fs.existsSync(path.join(vendor, 'mermaid'))) return;
    console.error('NG: gui/vendor/ がありません（エディタと図の実体）。');
    console.error('');
    console.error('  ネットに出られる機械で:  cd gui && npm install && node vendor.js');
    console.error('  出られない機械では:      その gui/vendor/ を丸ごと持ち込んで、同じ場所に置く');
    console.error('');
    console.error('  （持ち込む一式は  node scripts/offline-kit.js  がまとめます）');
    process.exit(1);
}

/** `resources/app/` の中身 ── Mac も Windows も同じ形。 */
function fillApp(appDir, serverExe, exeName, calBin) {
    fs.mkdirSync(appDir, { recursive: true });
    fs.writeFileSync(path.join(appDir, 'package.json'), JSON.stringify({
        name: 'amber', version, private: true, main: 'gui/main.js',
    }, null, 2));
    // 画面。node_modules は入れない（Electron 本体が居るし、実行時の一式は vendor/）。
    // 走査が残した試しのノート（`*.md`）も入れない。
    copy(path.join(ROOT, 'gui'), path.join(appDir, 'gui'), (name) =>
        name === 'node_modules' || name === 'package-lock.json' || name.endsWith('.md')
        || name === 'run.sh' || name === 'run.bat' || name === 'vendor.js');
    // main.js が `../packaging` で見るもの。
    for (const name of ['amber.png', 'amber-dock.png', 'amber-mark.png', 'amber.icns', 'amber.ico']) {
        const at = path.join(ROOT, 'packaging', name);
        if (fs.existsSync(at)) copy(at, path.join(appDir, 'packaging', name));
    }
    copy(path.join(ROOT, 'packaging', 'welcome'), path.join(appDir, 'packaging', 'welcome'));
    copy(path.join(ROOT, 'packaging', 'templates'), path.join(appDir, 'packaging', 'templates'));
    // **会社向けの一枚だけ、版の札を置く**（依頼 602）。無ければふつうの版。
    if (edition !== 'full') {
        fs.writeFileSync(path.join(appDir, 'gui', 'edition.json'),
            JSON.stringify({ edition }, null, 2));
    }
    // エンジン ── engine.js は gui/ の隣（`__dirname`）を最初に見る。
    copy(serverExe, path.join(appDir, 'gui', exeName));
    if (calBin && fs.existsSync(calBin)) copy(calBin, path.join(appDir, 'gui', 'amber-cal'));
}

/** 出口で数える。欠けていれば止める ── 「入れたつもりで空」を配らないため。 */
function verify(appDir, exe) {
    const must = [
        'package.json', 'gui/main.js', 'gui/index.html', 'gui/renderer.js', 'gui/preload.js',
        'gui/palettes.js', 'gui/drive.js', 'gui/engine.js', 'gui/vendor/monaco/vs/loader.js',
        'gui/vendor/mermaid', 'packaging/amber.png', 'packaging/welcome', 'packaging/templates',
        'gui/' + exe,
    ];
    const lost = must.filter((rel) => !fs.existsSync(path.join(appDir, rel)));
    if (lost.length) {
        console.error('NG: 配るものに欠けがあります:');
        for (const l of lost) console.error('  - ' + l);
        process.exit(1);
    }
}

function sizeOf(dir) {
    let n = 0;
    const walk = (at) => {
        const st = fs.lstatSync(at);
        if (st.isSymbolicLink()) return;
        if (st.isDirectory()) { for (const c of fs.readdirSync(at)) walk(path.join(at, c)); } else n += st.size;
    };
    walk(dir);
    return (n / 1024 / 1024).toFixed(1) + ' MB';
}

// ── Mac ──────────────────────────────────────────────────────────────
function mac(out) {
    preflight();
    const electron = arg('electron') || path.join(ROOT, 'gui', 'node_modules', 'electron', 'dist', 'Electron.app');
    if (!fs.existsSync(electron)) { console.error('Electron.app がありません: ' + electron + '（cd gui && npm install）'); process.exit(1); }
    const server = engineArg() || path.join(ROOT, 'target', 'release', 'amber-server');
    if (!fs.existsSync(server)) { console.error('amber-server がありません: ' + server + '（cargo build --release -p amber-server）'); process.exit(1); }
    const cal = path.join(ROOT, 'target', 'mac', 'amber-cal');
    if (!fs.existsSync(cal)) console.error('注意: amber-cal がありません（scripts/mac-build.sh）── この Mac の予定表は読めない一枚になります');
    const app = path.join(out, 'ambər.app');
    fs.rmSync(app, { recursive: true, force: true });
    fs.mkdirSync(out, { recursive: true });
    // `cp -R` で写す ── フレームワークの中の記号リンクを、リンクのまま持っていく。
    execFileSync('cp', ['-R', electron, app]);
    fs.rmSync(path.join(app, 'Contents', '_CodeSignature'), { recursive: true, force: true });
    const res = path.join(app, 'Contents', 'Resources');
    fs.rmSync(path.join(res, 'default_app.asar'), { force: true });
    fs.rmSync(path.join(res, 'electron.icns'), { force: true });
    copy(path.join(ROOT, 'packaging', 'amber.icns'), path.join(res, 'amber.icns'));
    // 名前と絵。Electron の plist を土台に、amber のものだけ書き換える。
    const plist = path.join(app, 'Contents', 'Info.plist');
    const set = (k, v) => {
        try { execFileSync('/usr/libexec/PlistBuddy', ['-c', `Set :${k} ${v}`, plist], { stdio: 'ignore' }); }
        catch { execFileSync('/usr/libexec/PlistBuddy', ['-c', `Add :${k} string ${v}`, plist], { stdio: 'ignore' }); }
    };
    set('CFBundleName', 'ambər');
    set('CFBundleDisplayName', 'ambər');
    set('CFBundleIdentifier', 'com.taketan.amber');
    set('CFBundleIconFile', 'amber.icns');
    set('CFBundleShortVersionString', version);
    // 予定表を読む言い分（amber-cal が求める）。無いと macOS は小窓も出さずに断る。
    set('NSCalendarsFullAccessUsageDescription', 'カレンダーに予定を並べ、登録するため');
    set('NSCalendarsUsageDescription', 'カレンダーに予定を並べ、登録するため');
    fillApp(path.join(res, 'app'), server, 'amber-server', cal);
    verify(path.join(res, 'app'), 'amber-server');
    // 署名（その場のもの）。配布の署名や公証には Apple の証明書が要る。
    // **中身を全部置いたあとで**（署名してから差し替えると破損と読まれる）。
    try { execFileSync('codesign', ['--force', '--deep', '--sign', '-', app], { stdio: 'ignore' }); }
    catch { console.error('注意: 署名できませんでした（初回に警告が出ることがあります）'); }
    console.log('できました: ' + app + '  (ambər ' + version + '・' + sizeOf(app) + ')');
    if (has('zip')) {
        const zip = path.join(out, `amber-mac-${version}.zip`);
        fs.rmSync(zip, { force: true });
        execFileSync('ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', app, zip]);
        console.log('zip: ' + zip + '  (' + sizeOf(zip) + ')');
    }
}

// ── Windows ──────────────────────────────────────────────────────────
function win(out) {
    preflight();
    // Electron の win32 の一式（GitHub の Release の zip を展開したもの）。
    // 引数 → リポジトリの隣の `electron-v*-win32-x64` の順に探す。
    let electron = arg('electron');
    if (!electron) {
        const beside = fs.readdirSync(path.join(ROOT, '..')).filter((n) => /^electron-v.*win32-x64$/.test(n)).sort().pop();
        if (beside) electron = path.join(ROOT, '..', beside);
    }
    if (!electron || !fs.existsSync(path.join(electron, 'electron.exe'))) {
        console.error('Windows の Electron がありません。https://github.com/electron/electron/releases/download/v33.4.11/electron-v33.4.11-win32-x64.zip を落として展開し、--electron で指してください');
        process.exit(1);
    }
    const server = engineArg();
    if (!server || !fs.existsSync(server)) {
        console.error('Windows のエンジンがありません。Release の amber-server-win-x64.exe を --engine（--server でも可）で指してください（gh release download --pattern amber-server-win-x64.exe）');
        process.exit(1);
    }
    // **名前で見分けられるようにする**（依頼 602）── リリースの資材は
    // 一つの籠に平らに並ぶので、同じ名前だと片方が片方を上書きする。
    const dir = path.join(out, 'amber-win-x64' + (edition === 'full' ? '' : '-' + edition));
    fs.rmSync(dir, { recursive: true, force: true });
    copy(electron, dir);
    fs.renameSync(path.join(dir, 'electron.exe'), path.join(dir, 'amber.exe'));
    fs.rmSync(path.join(dir, 'resources', 'default_app.asar'), { force: true });
    fillApp(path.join(dir, 'resources', 'app'), server, 'amber-server.exe', null);
    verify(path.join(dir, 'resources', 'app'), 'amber-server.exe');
    const rcedit = arg('rcedit');
    if (rcedit && process.platform === 'win32') {
        execFileSync(rcedit, [path.join(dir, 'amber.exe'),
            '--set-icon', path.join(ROOT, 'packaging', 'amber.ico'),
            '--set-version-string', 'ProductName', 'ambər',
            '--set-version-string', 'FileDescription', 'ambər',
            '--set-file-version', version, '--set-product-version', version]);
    }
    fs.writeFileSync(path.join(dir, 'はじめにお読みください.txt'), [
        'ambər ' + version + '（Windows x64）',
        '',
        '1. この zip を右クリック →「プロパティ」→「セキュリティ: 許可する」に印 → OK（先に外しておくと、展開したものに印が残りません）',
        '2. 右クリック →「すべて展開」',
        '3. amber.exe をダブルクリック',
        '',
        'ノートは「ドキュメント\\amber」に置かれます。⚙ → 保存ディレクトリの追加・変更・削除 で変えられます。',
        '',
    ].join('\r\n'));
    console.log('できました: ' + dir + '  (ambər ' + version + '・' + sizeOf(dir) + ')');
    if (has('zip')) {
        const zip = path.join(out, path.basename(dir) + '-' + version + '.zip');
        fs.rmSync(zip, { force: true });
        // **組むのは自前**（`scripts/zip.js`・依頼 550）── 前は `python3` を
        // 呼んでいたが、**組む場所は会社の Windows でもある**。あそこに
        // python3 は無いし、PowerShell の `Compress-Archive` は日本語の
        // 名前に UTF-8 の印を立てない（`はじめにお読みください.txt` が化ける）。
        zipDir(dir, zip);
        console.log('zip: ' + zip + '  (' + sizeOf(zip) + ')');
    }
}

const out = path.resolve(arg('out') || path.join(ROOT, 'dist'));
// **cian と同じ綴りでも通す**（依頼 550）── 本人が会社の Windows で打つのは
// `--platform win32` のほう。同じことを二通りで書けるのは普通は避けるが、
// ここは**手が憶えている綴り**のほうが正しい（打ち直させない）。
const plat = arg('platform');
const wantMac = has('mac') || plat === 'darwin' || plat === 'mac' || plat === 'macos';
const wantWin = has('win') || plat === 'win32' || plat === 'win' || plat === 'windows';
if (wantMac && wantWin) {
    console.error('どちらか一つにしてください（--mac と --win の両方が指されています）');
    process.exit(2);
}
if (wantMac) mac(out);
else if (wantWin) win(out);
else {
    console.error('どちらを組むか: --mac か --win（--platform darwin｜win32 でも同じ）');
    console.error('例: node scripts/pack.js --mac --out dist --zip');
    process.exit(2);
}
