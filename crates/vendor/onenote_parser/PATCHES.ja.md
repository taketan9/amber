# ここは借り物 ── `onenote_parser` 2.0.0 の写し

出どころ: <https://github.com/msiemens/onenote.rs>（MPL-2.0・`LICENSE`）。
crates.io の 2.0.0 をそのまま写し、**下の一つだけ**を直してある。

写しを置いた理由（本人が決めた・2026-09-18）: 会社の `.one` が読めず、直しは
数行だが本家にはまだ入っていない。網に頼らず、こちらで直して配れるようにする。

## 直したところ

| 道 | 何を | なぜ |
|---|---|---|
| `src/onestore/desktop/objects/revision.rs` | `ObjectInfoDependencyOverridesFND` を、リビジョンのどこに出てきても飛ばす（元は全体 ID 表の直後だけ） | 本人の会社の `.one` がここで止まった（「Unexpected node (parsing Revision)」）。この節は参照の数え（MS-ONESTORE 2.5.20）で、中身を読むのに要る値は無い |

## 本家を追うとき

1. crates.io の新しい版を `crates/vendor/onenote_parser` に写し直す
2. 上の表の直しを当て直す（本家に入っていれば、当てなくてよい）
3. `cargo test -p amber-onenote` ── 見本（公開仕様・FSSHTTP 包み・升つき）が通るか
