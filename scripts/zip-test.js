#!/usr/bin/env node
/*
 * **ビルドした zip が、本当に読めるか**（依頼 550）。
 *
 *     node scripts/zip-test.js
 *
 * `scripts/zip.js` は配るものをビルドする唯一の場所で、**壊れてもビルドした側には
 * 何も起きない** ── 気づくのは、受け取った人が展開したときになる。しかも
 * 相手はネットに出られない会社の端末なので、そこで気づくといちばん高くつく。
 *
 * 見るのは四つ:
 *
 *   一. 展開できる（中央目録と位置が合っている）
 *   二. **日本語の名前が、日本語のまま出てくる**（UTF-8 の印・汎用ビット 11）
 *   三. 中身が一文字も変わっていない（CRC と、読み直した文字そのもの）
 *   四. いちばん上のフォルダごと入っている（展開した人の机に散らない）
 */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');
const crypto = require('node:crypto');
const { zipDir } = require('./zip');

/**
 * CRC-32 を**もう一度、別の書き方で**。
 *
 * `zip.js` のものをそのまま借りると、あれが壊れた日に試験も同じ答えを出して
 * 報告しない（実際に、表を潰しても鳴らなかった）。こちらは表を作らず一ビットずつ
 * 回す ── 遅いが、試すのは数キロなので構わない。
 */
function crcSlow(buf) {
    let c = 0xffffffff;
    for (let i = 0; i < buf.length; i += 1) {
        c ^= buf[i];
        for (let k = 0; k < 8; k += 1) c = c & 1 ? (c >>> 1) ^ 0xedb88320 : c >>> 1;
    }
    return (c ^ 0xffffffff) >>> 0;
}

let bad = 0;
const ok = (yes, what) => {
    console.log((yes ? '  ok   ' : '  NG   ') + what);
    if (!yes) bad += 1;
};

const work = fs.mkdtempSync(path.join(os.tmpdir(), 'amber-zip-'));
const src = path.join(work, 'amber-win-x64');
fs.mkdirSync(path.join(src, 'resources', 'app'), { recursive: true });

// 日本語の名前、よく縮む文字、縮まない出鱈目、空の1 つ ── 四つとも通りパスが違う。
const letter = 'ambər 2.13.0（Windows x64）\r\namber.exe をダブルクリック\r\n';
fs.writeFileSync(path.join(src, 'はじめにお読みください.txt'), letter);
fs.writeFileSync(path.join(src, 'resources', 'app', 'squash.txt'), 'あ'.repeat(5000));
// **本物の出鱈目でないと、この枝は踏めない。** はじめ `i * 2654435761 & 0xff`
// で作っていたが、あれは規則があるので縮んでしまい、「素のまま入れる」枝を
// 一度も通っていなかった（変異テストで、常に deflate に変えても鳴らなかった）。
const noise = crypto.randomBytes(4096);
fs.writeFileSync(path.join(src, 'noise.bin'), noise);
fs.writeFileSync(path.join(src, 'empty.txt'), '');
// 記号リンク ── **持っていかない**（Windows に置けない）。作れない環境
// （権限のいる Windows）では、この一つだけ試さない。
let linked = false;
try {
    fs.symlinkSync('empty.txt', path.join(src, 'link.txt'));
    linked = true;
} catch { /* 作れないなら試さない */ }

const zip = path.join(work, 'out.zip');
const n = zipDir(src, zip);
ok(n === 4, '四枚とも入った（記号リンクは数えない・' + n + '）');

// ── 自分で読み直す。外の道具を当てにしない（会社の端末に unzip は無い）。
const buf = fs.readFileSync(zip);
const end = (() => {
    for (let i = buf.length - 22; i >= 0; i -= 1) if (buf.readUInt32LE(i) === 0x06054b50) return i;
    return -1;
})();
ok(end >= 0, '終わりの印（EOCD）がある');

const found = new Map();
if (end >= 0) {
    let at = buf.readUInt32LE(end + 16);
    const count = buf.readUInt16LE(end + 10);
    for (let i = 0; i < count; i += 1) {
        if (buf.readUInt32LE(at) !== 0x02014b50) { ok(false, '中央目録が読めない'); break; }
        const flags = buf.readUInt16LE(at + 8);
        const how = buf.readUInt16LE(at + 10);
        const sum = buf.readUInt32LE(at + 16);
        const csize = buf.readUInt32LE(at + 20);
        const usize = buf.readUInt32LE(at + 24);
        const nlen = buf.readUInt16LE(at + 28);
        const elen = buf.readUInt16LE(at + 30);
        const clen = buf.readUInt16LE(at + 32);
        const head = buf.readUInt32LE(at + 42);
        const name = buf.subarray(at + 46, at + 46 + nlen).toString('utf8');
        // 本体の側も読む ── 位置が一つでもずれていれば、ここでマークが合わない。
        const hnlen = buf.readUInt16LE(head + 26);
        const helen = buf.readUInt16LE(head + 28);
        const from = head + 30 + hnlen + helen;
        const body = buf.subarray(from, from + csize);
        found.set(name, {
            utf8: !!(flags & 0x800),
            // **局所ヘッダのマークも見る。** 展開する側は、名前をこちらから読む
            // ことがある ── 中央目録にだけマークを立てても、化ける相手が残る
            // （中央だけ見ていて、変異テストに素通りされた）。
            utf8Local: !!(buf.readUInt16LE(head + 6) & 0x800),
            localName: buf.subarray(head + 30, head + 30 + hnlen).toString('utf8'),
            local: buf.readUInt32LE(head) === 0x04034b50,
            // **局所ヘッダの数も見る。** 流しながら展開する道具はこちらを
            // 読むので、中央目録とだけ合っていても足りない（局所の側の
            // 寸法を潰しても鳴らなかった）。
            localSame: buf.readUInt32LE(head + 14) === sum
                && buf.readUInt32LE(head + 18) === csize
                && buf.readUInt32LE(head + 22) === usize,
            // **解けなかったら、そう記す。** 手法だけ「縮めた」と書いてあって
            // 中身が素のまま、という壊れ方をここで捕まえる（例外で落ちると、
            // 試験は「NG」ではなく「落ちた」になって数に入らない）。
            text: (() => {
                try { return how === 8 ? zlib.inflateRawSync(body) : body; }
                catch { return null; }
            })(),
            sum, usize, csize,
        });
        at += 46 + nlen + elen + clen;
    }
}

const letterName = 'amber-win-x64/はじめにお読みください.txt';
const got = found.get(letterName);
ok(!!got, '日本語の名前が、日本語のまま読める');
ok(!!got && got.utf8, 'UTF-8 の印（汎用ビット 11）が立っている');
ok(!!got && got.utf8Local, '局所ヘッダにも UTF-8 のマークが立っている');
ok(!!got && got.localName === 'amber-win-x64/はじめにお読みください.txt',
   '局所ヘッダの名前も日本語のまま');
ok(!!got && got.text && got.text.toString('utf8') === letter, '中身が一字も変わっていない');
ok(!!got && got.local, '本体の頭の位置が合っている');
ok([...found.values()].every((f) => f.localSame),
   '局所ヘッダの寸法と CRC が、中央目録と合っている');

const squash = found.get('amber-win-x64/resources/app/squash.txt');
ok([...found.values()].every((f) => f.text !== null), '中身がぜんぶ解ける（手法と中身が食い違っていない）');
// **CRC を照らし合わせる。** ここを見ていなかったので、表を潰しても
// 鳴らなかった（変異テストで分かった）── 展開する側は必ず見るところで、
// 合わないと「壊れています」とだけ言われる。
ok([...found.values()].every((f) => f.text !== null && crcSlow(f.text) === f.sum),
   'CRC が中身と合っている');
ok(!!squash && squash.csize < squash.usize, 'よく縮む文字は縮んでいる');
ok(!!squash && squash.text && squash.text.toString('utf8') === 'あ'.repeat(5000), '縮めたものが元に戻る');

// **縮まないものを膨らませない。** 既に縮んでいるもの（png・exe の資源）を
// deflate に通すと増えることがあるので、素のまま入れる枝がある。
const bin = found.get('amber-win-x64/noise.bin');
ok(!!bin && bin.csize === bin.usize, '縮まないものは、素のまま入っている（膨らませない）');
ok(!!bin && bin.text && Buffer.compare(bin.text, noise) === 0, '出鱈目な中身がそのまま戻る');

const empty = found.get('amber-win-x64/empty.txt');
ok(!!empty && empty.usize === 0, '空の1 つも入る');

ok([...found.keys()].every((k) => k.startsWith('amber-win-x64/')),
   'いちばん上のフォルダごと入っている');
if (linked) ok(!found.has('amber-win-x64/link.txt'), '記号リンクは入っていない');

fs.rmSync(work, { recursive: true, force: true });
if (bad) { console.error('zip: ' + bad + ' 件だめでした'); process.exit(1); }
console.log('zip は、組んで、読み直して、元に戻ります');
