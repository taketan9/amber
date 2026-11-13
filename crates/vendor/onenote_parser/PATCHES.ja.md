# ここは借り物 ── `onenote_parser` 2.0.0 の写し

出どころ: <https://github.com/msiemens/onenote.rs>（MPL-2.0・`LICENSE`）。
crates.io の 2.0.0 をそのまま写し、**下の一つだけ**を直してある。

写しを置いた理由（本人が決めた・2026-09-18）: 会社の `.one` が読めず、直しは
数行だが本家にはまだ入っていない。網に頼らず、こちらで直して配れるようにする。

## 直したところ

| 道 | 何を | なぜ |
|---|---|---|
| `src/onestore/desktop/objects/revision.rs` | `ObjectInfoDependencyOverridesFND` を、リビジョンのどこに出てきても飛ばす（元は全体 ID 表の直後だけ） | 本人の会社の `.one` がここで止まった（「Unexpected node (parsing Revision)」）。この節は参照の数え（MS-ONESTORE 2.5.20）で、中身を読むのに要る値は無い |

## 読めない `.one` が来たときの見方（2026-11-13）

**まず「大きいから」を疑わない。** 本人が「五十メガを超えると取り込めない」と
見ていたので測った（依頼 715）:

| | |
|---|---|
| 記憶 | 元の大きさの **6〜13 倍**（土台の 1MB を除く）── 50MB なら 300〜650MB |
| 時間 | **1KB あたり 0.048 ミリ秒**・大きさに対してまっすぐ ── 50MB で約 2.5 秒 |

測り方: `cargo build --release --example profile_parse -p onenote_parser` のあと
`/usr/bin/time -l ./target/release/examples/profile_parse <道> 50`。

つまり**大きさは壁ではない**。大きいセクションは古くて改訂が積もっている
セクションで、小さい新しいセクションには出てこない構造（長い取引記録・
多い改訂・参照の数え）を通る ── 上の表の直しと同じ形で、**知らない節に
当たって止まる**。だから要るのは大きさの歯止めではなく、**どの節で
止まったかの文言**。

画面はそれを出す ── フォルダを取り込むと、読めなかった `.one` が
名前・大きさ・理由つきで並ぶ（`gui/renderer.js` の `oneShow`）。
一本だけ選んだときは「元の文言」の行に出る。

## 本家を追うとき

1. crates.io の新しい版を `crates/vendor/onenote_parser` に写し直す
2. 上の表の直しを当て直す（本家に入っていれば、当てなくてよい）
3. `cargo test -p amber-onenote` ── 見本（公開仕様・FSSHTTP 包み・升つき）が通るか
