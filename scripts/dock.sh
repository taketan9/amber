#!/bin/zsh
# **mac の Dock に、ambər を一枚置く**（依頼 446）。
#
#     scripts/dock.sh          # ~/Applications/ambər.app を作る（作り直す）
#
# 押すたびに**そのときの最新**が起きる ── 中身を写すのではなく、この
# repo の `gui/` をそのまま指しているから。エンジンも起動のたびに
# `cargo build` を通すので、午前に直した op が午後の窓に無い、が起きない
# （`gui/engine.js` は**新しいほうを採る**ので、debug で足りる）。
#
# 作り方は electron-packager と同じ ── Electron の一式を写して、名前と
# 絵と中身だけ差し替える。**署名し直すのを忘れない** ── Apple Silicon は
# 中身をいじった束ねを黙って起動しない。
set -e
here="${0:A:h}"
root="${here:h}"
app="$HOME/Applications/ambər.app"
el="$root/gui/node_modules/electron/dist/Electron.app"

[ -d "$el" ] || { echo "Electron がありません ── cd gui && npm install"; exit 2 }
[ -f "$root/packaging/amber.icns" ] || { echo "絵がありません: packaging/amber.icns"; exit 2 }

mkdir -p "$HOME/Applications"
rm -rf "$app"
cp -R "$el" "$app"

res="$app/Contents/Resources"
# **中身は写さない。** 写すと、その日の姿で固まる。
rm -rf "$res/app" "$res/default_app.asar"
ln -s "$root/gui" "$res/app"
cp "$root/packaging/amber.icns" "$res/amber.icns"

# 起きる前に、エンジンを揃える。
cat > "$app/Contents/MacOS/ambər" <<RUN
#!/bin/zsh
cd "$root" && cargo build -q -p amber-server 2>/dev/null || true
exec "\$0:A:h/Electron" "\$@"
RUN
chmod +x "$app/Contents/MacOS/ambər"

plist="$app/Contents/Info.plist"
set_it() { /usr/libexec/PlistBuddy -c "Set :$1 $2" "$plist" 2>/dev/null \
        || /usr/libexec/PlistBuddy -c "Add :$1 string $2" "$plist" }
set_it CFBundleExecutable "ambər"
set_it CFBundleName "ambər"
set_it CFBundleDisplayName "ambər"
set_it CFBundleIdentifier "com.taketan.amber"
set_it CFBundleIconFile "amber.icns"
set_it CFBundleShortVersionString "$(node -p "require('$root/gui/package.json').version" 2>/dev/null || echo 0)"

# **署名し直す。** 中身をいじったので、元の署名はもう合わない。
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || true

# Finder に「絵が変わった」と気づかせる ── これをしないと白い紙のまま。
touch "$app"

echo "できました: $app"
echo "Dock へ: open ~/Applications で出して、ambər を Dock へ引く"
echo "（Dock に入れたら、この repo を動かさないこと ── 中身を指しています）"
