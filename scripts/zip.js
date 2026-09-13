/*
 * **zip を自分で組む**（依頼 550）。
 *
 *     const { zipDir } = require('./zip');
 *     zipDir('/path/to/amber-win-x64', '/path/to/out.zip');
 *
 * 外の道具を呼ばない ── Node に入っている `zlib` だけで組む。
 *
 * **なぜ自前なのか。** 配る zip を組む場所は、会社のネットに出られない
 * Windows でもある（依頼 550）。そこに何が入っているかを当てにできない:
 *
 * - `python3` は**まず無い**（Windows の Python は `python` で、しかも
 *   入っていないことのほうが多い）。前はこれで組んでいた
 * - PowerShell の `Compress-Archive` は有るが、**日本語の名前が化ける** ──
 *   Windows PowerShell 5.1 のあれは UTF-8 の印（汎用ビット 11）を立てない
 *   ので、`はじめにお読みください.txt` が別の字で出てくる端末がある
 *
 * なので**印を必ず立てる**。ここが zip を組む唯一の場所で、Mac も Windows も
 * これを通る（Mac の `.app` だけは `ditto` ── あちらは記号リンクと資源
 * フォークを持っていくので、置き換えると壊れる）。
 */
const fs = require('node:fs');
const path = require('node:path');
const zlib = require('node:zlib');

/** CRC-32（zip が要る形）。表は初回に一度だけ組む。 */
let TABLE = null;
function crc32(buf) {
    if (!TABLE) {
        TABLE = new Int32Array(256);
        for (let n = 0; n < 256; n += 1) {
            let c = n;
            for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
            TABLE[n] = c;
        }
    }
    let c = -1;
    for (let i = 0; i < buf.length; i += 1) c = TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
    return (c ^ -1) >>> 0;
}

/** MS-DOS の日時（zip の形）。1980 より前は 1980-01-01 に寄せる。 */
function dosTime(when) {
    const y = when.getFullYear();
    if (y < 1980) return { time: 0, date: 33 };
    return {
        time: (when.getHours() << 11) | (when.getMinutes() << 5) | (when.getSeconds() >> 1),
        date: ((y - 1980) << 9) | ((when.getMonth() + 1) << 5) | when.getDate(),
    };
}

/** 中を順に歩く（記号リンクは飛ばす ── Windows に持っていけない）。 */
function walk(dir, base, out) {
    for (const name of fs.readdirSync(dir).sort()) {
        const at = path.join(dir, name);
        const st = fs.lstatSync(at);
        if (st.isSymbolicLink()) continue;
        if (st.isDirectory()) walk(at, base, out);
        else out.push({ at, rel: path.relative(base, at).split(path.sep).join('/'), st });
    }
    return out;
}

/**
 * フォルダを一枚の zip にする。**いちばん上のフォルダごと**入れる
 * （`ditto --keepParent` と同じ ── 展開するとフォルダが一つできる。
 * そうしないと、展開した人のデスクトップに二百個のファイルが散る）。
 */
function zipDir(dir, out) {
    const base = path.dirname(dir);
    const files = walk(dir, base, []);
    const parts = [];
    const central = [];
    let at = 0;

    for (const f of files) {
        const name = Buffer.from(f.rel, 'utf8');
        const raw = fs.readFileSync(f.at);
        const sum = crc32(raw);
        const packed = zlib.deflateRawSync(raw, { level: 9 });
        // **縮まないものは、そのまま入れる。** 既に縮んでいるもの（png・
        // exe の中の資源）は deflate をかけると増えることがある。
        const shrank = packed.length < raw.length;
        const body = shrank ? packed : raw;
        const how = shrank ? 8 : 0;
        const { time, date } = dosTime(f.st.mtime);

        const head = Buffer.alloc(30);
        head.writeUInt32LE(0x04034b50, 0);
        head.writeUInt16LE(20, 4);          // 要る版
        head.writeUInt16LE(0x0800, 6);      // **名前は UTF-8**（ビット 11）
        head.writeUInt16LE(how, 8);
        head.writeUInt16LE(time, 10);
        head.writeUInt16LE(date, 12);
        head.writeUInt32LE(sum, 14);
        head.writeUInt32LE(body.length, 18);
        head.writeUInt32LE(raw.length, 22);
        head.writeUInt16LE(name.length, 26);
        head.writeUInt16LE(0, 28);
        parts.push(head, name, body);

        const dir1 = Buffer.alloc(46);
        dir1.writeUInt32LE(0x02014b50, 0);
        dir1.writeUInt16LE(0x031e, 4);      // 組んだ側（Unix・版 3.0）
        dir1.writeUInt16LE(20, 6);
        dir1.writeUInt16LE(0x0800, 8);
        dir1.writeUInt16LE(how, 10);
        dir1.writeUInt16LE(time, 12);
        dir1.writeUInt16LE(date, 14);
        dir1.writeUInt32LE(sum, 16);
        dir1.writeUInt32LE(body.length, 20);
        dir1.writeUInt32LE(raw.length, 24);
        dir1.writeUInt16LE(name.length, 28);
        dir1.writeUInt32LE(0, 30);          // extra + comment
        dir1.writeUInt16LE(0, 34);          // 何枚目の媒体か
        dir1.writeUInt16LE(0, 36);          // 中身の性質
        // **実行の印を残す。** Windows では意味を持たないが、mac や Linux で
        // 展開したときに `amber-server` が実行できないと、そこで止まる。
        dir1.writeUInt32LE(((f.st.mode & 0o7777) || 0o644) << 16, 38);
        dir1.writeUInt32LE(at, 42);
        central.push(dir1, name);

        at += head.length + name.length + body.length;
    }

    const end = Buffer.alloc(22);
    const size = central.reduce((n, b) => n + b.length, 0);
    end.writeUInt32LE(0x06054b50, 0);
    end.writeUInt16LE(0, 4);
    end.writeUInt16LE(0, 6);
    end.writeUInt16LE(files.length, 8);
    end.writeUInt16LE(files.length, 10);
    end.writeUInt32LE(size, 12);
    end.writeUInt32LE(at, 16);
    end.writeUInt16LE(0, 20);

    fs.writeFileSync(out, Buffer.concat([...parts, ...central, end]));
    return files.length;
}

module.exports = { zipDir, crc32 };
