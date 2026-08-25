'use strict';
// The `cian-server` child, and the promise-per-call wrapper over its pipe.
//
// One JSON object per line each way. Replies carry the `id` of the call they
// answer, so several may be in flight and the order they come back in does not
// matter — which it will not, once a directory read is slower than a keypress.

const { spawn } = require('node:child_process');
const readline = require('node:readline');
const path = require('node:path');
const fs = require('node:fs');

/// Where the engine binary is, in each of the two ways this runs.
///
/// Packaged, it sits beside the app; from a checkout it is under `target/`.
/// Release is preferred over debug so a stale debug build from months ago does
/// not quietly become the thing under test.
function enginePath() {
    const exe = process.platform === 'win32' ? 'cian-server.exe' : 'cian-server';
    const beside = path.join(__dirname, exe);
    if (fs.existsSync(beside)) return beside;
    for (const profile of ['release', 'debug']) {
        const built = path.join(__dirname, '..', 'target', profile, exe);
        if (fs.existsSync(built)) return built;
    }
    throw new Error(`cian-server not found — cargo build -p cian-server`);
}

class Engine {
    constructor(cwd) {
        this.next = 1;
        this.pending = new Map();
        this.child = spawn(enginePath(), [cwd], {
            stdio: ['pipe', 'pipe', 'pipe'],
            windowsHide: true,
        });
        readline.createInterface({ input: this.child.stdout }).on('line', (line) => {
            let msg;
            try {
                msg = JSON.parse(line);
            } catch {
                // Not our protocol. Anything the engine prints that is not a
                // reply belongs in the log, not thrown away.
                console.error('engine said:', line);
                return;
            }
            const waiting = this.pending.get(msg.id);
            if (!waiting) return;
            this.pending.delete(msg.id);
            if (msg.error) waiting.reject(new Error(msg.error));
            else waiting.resolve(msg.ok);
        });
        // The engine's stderr is the engine's own trouble; keep it whole.
        this.child.stderr.on('data', (b) => console.error('engine:', String(b).trimEnd()));
        // A dead engine must not leave callers waiting for ever.
        this.child.on('exit', (code) => {
            const dead = new Error(`the engine stopped (exit ${code})`);
            for (const { reject } of this.pending.values()) reject(dead);
            this.pending.clear();
        });
    }

    call(method, params = {}) {
        const id = this.next++;
        const line = JSON.stringify({ id, method, params });
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this.child.stdin.write(line + '\n');
        });
    }

    stop() {
        this.child.kill();
    }
}

module.exports = { Engine };
