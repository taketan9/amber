#!/usr/bin/env python3
"""つながっている実機を、一台一行で出す（依頼 541）。

    名前<TAB>入れるときの識別子<TAB>ビルドに使う識別子<TAB>機種<TAB>困りごと

**名前で突き合わせない。** `devicectl list devices` を文字で読むと、名前の
「の」が `?` になって `xcodebuild -showdestinations` 側の名前と一致しない
── それで一台が黙って飛ばされた（2026-09-13）。JSON で訊けば、**入れる
ときの識別子（identifier）と、ビルドに使う識別子（udid）が両方**そのまま
取れるので、取り違えようがない。
"""

import json
import subprocess
import sys
import tempfile


def main() -> int:
    with tempfile.NamedTemporaryFile(suffix=".json") as f:
        got = subprocess.run(
            ["xcrun", "devicectl", "list", "devices", "--json-output", f.name],
            capture_output=True,
        )
        if got.returncode != 0:
            return 0
        try:
            d = json.load(open(f.name))
        except Exception:
            return 0
    for x in d.get("result", {}).get("devices", []):
        props = x.get("deviceProperties") or {}
        name = props.get("name") or "(名前なし)"
        ident = x.get("identifier") or ""
        hw = x.get("hardwareProperties") or {}
        udid = hw.get("udid") or ""
        model = hw.get("marketingName") or hw.get("productType") or ""
        # **入れられない理由は、端末が知っている**（依頼 542）。こちらで
        # 「ロックされているか、つながっていません」と当て推量を並べても、
        # 相手は何をすればいいか分からない ── 端末が言っていることを渡す。
        trouble = ""
        if (props.get("developerModeStatus") or "") == "disabled":
            trouble = ("デベロッパモードが切れています（設定 → プライバシーとセキュリティ"
                       " → デベロッパモード → オン → 再起動）")
        elif (x.get("connectionProperties") or {}).get("pairingState") != "paired":
            trouble = "まだペアリングが済んでいません（端末で「信頼」を押してください）"
        if ident and udid:
            print("\t".join([name, ident, udid, model, trouble]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
