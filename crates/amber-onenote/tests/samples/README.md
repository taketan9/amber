# テスト用のサンプル ── 外部から借りた実データの `.one`

自作のデータではなく、OneNote が実際に書き出したもの。自前のパーサーは自作の
サンプルでは全部通っていたのに、実データでは 0 ページだった（依頼 620・621）。
サンプルが実データより甘いと、テストは嘘をつく。

| パス | 形式 | 出どころ | ライセンス |
|---|---|---|---|
| `desktop/OneWithFileData.one` | 公開仕様（MS-ONESTORE 2.3）── 会社の環境からエクスポートしたものと同じ形式 | Microsoft の [Interop-TestSuites](https://github.com/OfficeDev/Interop-TestSuites)（[onenote.rs](https://github.com/msiemens/onenote.rs) 経由） | MIT（`desktop/LICENSE`） |
| `fsshttp/New Section 1.one` | FSSHTTP 形式（MS-ONESTORE 2.8）── SharePoint／OneDrive から落としたもの | [onenote.rs](https://github.com/msiemens/onenote.rs) のテストデータ | MPL-2.0（`fsshttp/LICENSE`） |
| `notebook/`（目次 `Open Notebook.onetoc2` と 2 セクション） | 同上 ── 目次つきのノートブック。テストの中で CAB にまとめて `.onepkg` にする | 同上（`New Section Group/`） | MPL-2.0（`notebook/LICENSE`） |

| `checks/handwriting_recognition.one` | 同上 ── チェックボックスと手書きが入っている | 同上 | MPL-2.0（`checks/LICENSE`） |

AGPL のサンプル（onenote.rs の `joplin/`）は取り込まない。
