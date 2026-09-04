# cian on iPhone

ノートの半分だけを電話に持っていったもの。2画面もシェルも SFTP も無い ──
電話に意味がないから。判断は全部 `cian-core` から C ABI 越しに来る。

## 建て方

```sh
./scripts/ios-build.sh                 # Rust を3ターゲット建てて XCFramework に
xcodebuild -project ios/Cian.xcodeproj -target Cian \
  -sdk iphonesimulator -configuration Debug -arch x86_64 build
```

実機は署名が要る:

```sh
xcodebuild -project ios/Cian.xcodeproj -target Cian \
  -sdk iphoneos -configuration Debug -allowProvisioningUpdates build
xcrun devicectl device install app --device <UDID> ios/build/Debug-iphoneos/Cian.app
```

## 憶えておくこと

**`project.pbxproj` にコメントを書かない。** Xcode はプロジェクトを開いて保存
するたびに整形し直し、書いたコメントを黙って消す。だからこのファイルがある。

**プロジェクトが小さいのは Xcode 16 の同期グループのおかげ。** ソースを一つも
列挙していない ── `Cian` フォルダがそのままターゲットのソースになる。列挙する
形式は、マージのたびに衝突して、誰かが直し損ねた日にファイルが1つ消える。

**`Info.plist` は自前。** `GENERATE_INFOPLIST_FILE` は Xcode が build setting を
持っているキーしか知らず、`UIFileSharingEnabled` にはそれが無い ── 指定しても
黙って落ちる。隣の `LSSupportsOpeningDocumentsInPlace` は通るので、「ファイル」
アプリ側の問題に見えてしまう。両方揃って初めてノートフォルダが場所になる。

**`[profile.ios]` を使う（`release` ではなく）。** `release` は
`strip = "symbols"` で `_cian_call` と `_cian_free` を消し、`lto = "thin"` が
Xcode より新しい LLVM のビットコードを残す。どちらもリンクエラーとしてしか
現れず、原因がここを指さない。

**Xcode のビルドは Rust を建て直さない。** リンクするのは出来合いの
`target/ios/CianFFI.xcframework` なので、`cian-core` や `cian-ffi` を直した
あとに Xcode だけ回すと、ビルドは通り、アプリは入り、起動もして、**新しい
ボタンを押した瞬間に「知らない操作: remind」と答える**。証拠が Swift を
指すので、そこを何時間でも読める。だから `Engine freshness` というビルド
フェーズ（`scripts/ios-fresh.sh`）が最初に走り、ソースがライブラリより
新しければ**その場でビルドを落とす**。id は使っていない24桁を選ぶこと ──
既存と衝突すると Xcode は「プロジェクトを読めません」としか言わない。

**アクセントは `Assets.xcassets/AccentColor`**（明暗それぞれ）。
`ASSETCATALOG_COMPILER_GLOBAL_ACCENT_COLOR_NAME` と `CianApp` の `.tint` の
両方で効かせている。前者は Xcode のプレビューやシステム UI 用、後者が本番。

**無料の Apple ID の署名は7日で切れる。** 切れたら上の install をやり直す。
ノートは同期先にあるので何も失われない。

## 実機に入れるまでに一度だけ要るもの

1. Xcode → Settings → Accounts に Apple ID（無料でよい）
2. プロジェクトの Signing & Capabilities で Team を選ぶ（`DEVELOPMENT_TEAM`)
3. iPhone: 設定 → プライバシーとセキュリティ → **デベロッパモード** → オン → 再起動
4. iPhone: 設定 → 一般 → VPN とデバイス管理 → そのデベロッパを **信頼**
