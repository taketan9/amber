//! amber ── Markdown のノートについての判断だけを持つ。
//!
//! **cian を知らない。** 向きは `amber ← cian` の一方向で、cian-core が
//! ここに依存し、昔からの `crate::note::…` が動き続けるように再輸出している。
//!
//! `cian-core` と同じ規則: I/O の都合と UI に依存しない純ロジックを置く。
//! 前端（iPhone の Swift、窓の JS、端末の Rust）は描くだけ。**同じ判断を
//! 二か所に書いたら、それは一度の編集で食い違う二つの答えになる。**

pub mod markdown;
pub mod note;
pub mod notebook;
pub mod stamp;
pub mod text;
pub mod survey;
pub mod zipbox;

use std::sync::atomic::{AtomicBool, Ordering};

/// この版の下に机があるか。**iPhone には無い。**
///
/// ゴミ箱がそう。`trash` は Windows と macOS と Linux 向けで、電話には
/// `NSFileManager trashItemAtURL` が無い。消すなら消すと言うのが答えで、
/// ゴミ箱へ入れたふりをするのはいちばん悪い。cian-core にも同じ名前の
/// ものがあるが、あちらは向こうの話 ── **amber は cian を知らない。**
pub const DESKTOP: bool = cfg!(feature = "desktop");

/// 途中でやめるための旗と、進んだぶんを伝える口。
///
/// cian-core の `progress::Ctl` と同じ形だが、あちらは `elevate` と `ops` を
/// 引きずっている（管理者権限への昇格とファイル操作）── ノートを戻すのに
/// 昇格は要らないので、写さずに持ち直した。**同じ形の型が二つあるのは
/// 重複ではなく、境界。** 向こうが太ってもこちらは太らない。
pub struct Ctl<'a> {
    pub cancel: &'a AtomicBool,
    pub on_progress: &'a mut dyn FnMut(usize, usize),
}

impl Ctl<'_> {
    pub fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn step(&mut self, done: usize, all: usize) {
        (self.on_progress)(done, all);
    }
}
