# 試験の見本 ── よそから借りた本物の `.one`

**作り物ではなく、OneNote が書いたもの。** 自前の読み手は作り物の見本で
全部通っていたのに、本物では 0 ページだった（依頼 620・621）── 見本が
本物より甘いと、試験は嘘をつく。

| 道 | 形式 | 出どころ | 許諾 |
|---|---|---|---|
| `desktop/OneWithFileData.one` | 公開仕様（MS-ONESTORE 2.3）── **会社の書き出しと同じ形** | Microsoft の [Interop-TestSuites](https://github.com/OfficeDev/Interop-TestSuites)（[onenote.rs](https://github.com/msiemens/onenote.rs) 経由） | MIT（`desktop/LICENSE`） |
| `fsshttp/New Section 1.one` | FSSHTTP 包み（MS-ONESTORE 2.8）── SharePoint／OneDrive から落とした形 | [onenote.rs](https://github.com/msiemens/onenote.rs) の試験資材 | MPL-2.0（`fsshttp/LICENSE`） |
| `notebook/`（目次 `Open Notebook.onetoc2` と 2 セクション） | 同上 ── **目次つきの一冊**。試験の中で CAB に包んで `.onepkg` にする | 同上（`New Section Group/`） | MPL-2.0（`notebook/LICENSE`） |

| `checks/handwriting_recognition.one` | 同上 ── **升（チェックボックス）と手書き**が入っている | 同上 | MPL-2.0（`checks/LICENSE`） |

**AGPL の見本（onenote.rs の `joplin/`）は持ち込まない。**
