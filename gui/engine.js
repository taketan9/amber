'use strict';
// `amber-server` の子と、その口を約束一本に包むもの。
//
// 一行に JSON 一つ、両向き。返りは呼び出しの `id` を持っているので、何本
// 同時に飛んでも、返る順は当てにしなくていい ── ディレクトリの読み取りが
// 打鍵より遅くなった瞬間に、その順序は崩れる。
//
// cian の `gui/engine.js` と同じ形。**写したのは配管だけで、判断は写して
// いない** ── 答えるのは `amber_core::api` の一枚だけ。

const { spawn } = require('node:child_process');
const readline = require('node:readline');
const path = require('node:path');
const fs = require('node:fs');

/// エンジンの居場所。二通りの走り方それぞれに。
///
/// 配ったものは窓の隣に居る。ソースから走らせるときは `target/` の下で、
/// **新しいほうが勝つ ── release ではなく。**
///
/// release を優先するのは丁寧に見えて逆だった（cian で踏んだ）。朝の release
/// ビルドが残っているのに、午後の `cargo build` は debug に入る。前端は古い
/// エンジンに話しかけ続け、一時間前に書いた操作に「知らない操作」と答える。
function enginePath() {
    const exe = process.platform === 'win32' ? 'amber-server.exe' : 'amber-server';
    const beside = path.join(__dirname, exe);
    if (fs.existsSync(beside)) return beside;
    const built = ['release', 'debug']
        .map((profile) => path.join(__dirname, '..', 'target', profile, exe))
        .filter((p) => fs.existsSync(p))
        .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);
    if (built.length) return built[0];
    throw new Error('amber-server が見つかりません — cargo build -p amber-server');
}

class Engine {
    constructor() {
        this.next = 1;
        this.pending = new Map();
        this.child = spawn(enginePath(), [], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
        readline.createInterface({ input: this.child.stdout }).on('line', (line) => {
            let msg;
            try {
                msg = JSON.parse(line);
            } catch {
                // 約束の外の行。エンジンが何か言ったなら記録に回す ──
                // 捨てると、あとで「何も言わずに黙った」ように見える。
                console.error('engine said:', line);
                return;
            }
            const waiting = this.pending.get(msg.id);
            if (!waiting) return;
            this.pending.delete(msg.id);
            if (msg.error) waiting.reject(new Error(msg.error));
            else waiting.resolve(msg.ok);
        });
        this.child.stderr.on('data', (b) => console.error('engine:', String(b).trimEnd()));
        // 死んだエンジンが、呼び出し側を永久に待たせない。
        this.child.on('exit', (code) => {
            this.gone = true;
            const dead = new Error(`エンジンが止まりました (exit ${code})`);
            for (const { reject } of this.pending.values()) reject(dead);
            this.pending.clear();
        });
        // **窓を閉じるときに書き込むと、相手はもう居ない。** 最後にやるのは
        // 憶えごと（大きさ・見た目・開いていたノート）で、そのどれもが呼び出し。
        // 落ちたパイプへの書き込みは `write EOF` になり、誰も聞いていない
        // `error` として Node が例外に昇格させる。閉じかけのエンジンは
        // 事故ではない ── どのみち答えは捨てるところだった。
        const quiet = () => { this.gone = true; };
        this.child.stdin.on('error', quiet);
        this.child.on('error', quiet);
    }

    call(method, params = {}) {
        const id = this.next++;
        const line = JSON.stringify({ id, method, params });
        return new Promise((resolve, reject) => {
            if (this.gone || !this.child.stdin.writable) {
                reject(new Error('エンジンが動いていません'));
                return;
            }
            this.pending.set(id, { resolve, reject });
            this.child.stdin.write(line + '\n', (e) => {
                if (!e) return;
                this.pending.delete(id);
                reject(e);
            });
        });
    }

    stop() {
        this.gone = true;
        this.child.kill();
    }
}

module.exports = { Engine };
