//! The pipe itself: what goes over it, and who is allowed to write.
//!
//! Until an operation took longer than a keypress, this was request and reply
//! and nothing else. A copy of four thousand files is not that — it has to say
//! how far it has got while it is still going — so the server now talks
//! unasked as well.
//!
//! ```text
//! → {"id":7,"method":"copy","params":{…}}
//! ← {"id":7,"ok":{"op":1}}                     the call returns at once
//! ← {"event":"progress","op":1,"done":12,…}    …and the work reports
//! ← {"event":"progress","op":1,"done":260,…}
//! ← {"event":"done","op":1,"ok":260,…}
//! ```
//!
//! A reply carries `id`; an event carries `event` and never an `id`. A front
//! end that only understands replies can ignore every line without one and
//! still work, which is what let the first milestone exist before this did.

use std::io::Write;
use std::sync::mpsc::{channel, Receiver, Sender};

/// One line, already rendered.
///
/// **Two producers, one file descriptor.** Replies come from the thread
/// reading stdin; events come from however many workers are running. Letting
/// both call `println!` would eventually interleave two half-lines into one
/// unparseable one — rare enough to survive testing and certain to happen at
/// a customer's desk.
pub type Line = String;

/// The end of the pipe the whole process writes through.
#[derive(Clone)]
pub struct Out(Sender<Line>);

impl Out {
    /// Start the writer thread. Everything written from anywhere goes through
    /// the returned handle, which is cheap to clone and safe to hand to a
    /// worker.
    pub fn start() -> Out {
        let (tx, rx): (Sender<Line>, Receiver<Line>) = channel();
        std::thread::spawn(move || {
            let mut out = std::io::stdout();
            for line in rx {
                // A closed pipe means the front end is gone. Nothing to report
                // it to, so stop rather than spin.
                if writeln!(out, "{line}").is_err() || out.flush().is_err() {
                    break;
                }
            }
        });
        Out(tx)
    }

    pub fn send(&self, value: serde_json::Value) {
        let _ = self.0.send(value.to_string());
    }

    /// An answer to a call.
    pub fn reply(&self, id: u64, ok: serde_json::Value) {
        self.send(serde_json::json!({ "id": id, "ok": ok }));
    }

    /// A call that could not be carried out. The message is going to a person,
    /// through a dialog, so it is a sentence rather than a code.
    pub fn fail(&self, id: u64, message: impl std::fmt::Display) {
        self.send(serde_json::json!({ "id": id, "error": message.to_string() }));
    }

    /// Something the front end did not ask for: progress, or a finish.
    pub fn event(&self, name: &str, body: serde_json::Value) {
        let mut m = body;
        m["event"] = serde_json::Value::String(name.to_string());
        self.send(m);
    }

    /// An `Out` a test can read back, instead of one that writes to stdout.
    ///
    /// The events *are* the interface for anything long-running — a queued
    /// job that never says "done" is a front end waiting for ever — so a test
    /// of that has to be able to hear them.
    #[cfg(test)]
    pub fn piped() -> (Out, Receiver<Line>) {
        let (tx, rx) = channel();
        (Out(tx), rx)
    }
}
