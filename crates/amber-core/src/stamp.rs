//! 「このファイルは、読んだときから変わっているか？」
//!
//! **cian の保存は無条件に書いていた。** 文字コードも BOM も改行コードも
//! 読み込んだときのまま維持していた ── *どう書くか*は全部気にしていた ──
//! のに、いま上書きしようとしている中身が、読んだときと同じものかどうかは
//! 一度も確かめていなかった。共有ドライブ上の 1 つのノートを 2 人で編集して
//! 両方が保存すると、後の保存が先の保存を黙って消す。画面には何も出ない。
//! 誰も見ていなかったから。
//!
//! これは共有フォルダなら必ずある危険で（同期された OneDrive、WebDAV で
//! マウントした SharePoint、NFS のホーム）、気づくのに必要なのは `metadata`
//! を 1 回呼ぶことだけ。
//!
//! **これで捕まえられないもの**: 同じ 1 秒の中で行われ、長さも変わらない変更。
//! ボリュームによっては mtime が秒単位までしか保持されないので、その組み合わせは
//! 実際に起こりうる。捕まえるには読み込みのたびに中身をハッシュすることになり、
//! この小さな穴を塞ぐために開くたびファイルサイズぶんのコストを払うことになる。
//! ごまかさずに書いておく ── 長さとタイムスタンプが両方一致したら、ここは
//! 「変わっていない」と答えるが、それは間違っていることがある。

use std::path::Path;
use std::time::SystemTime;

/// 読み込んだ時点でのファイルの状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    /// ファイルシステムが時刻を返さないときは `None`。時刻の無いスタンプは
    /// 長さだけで比較することになり、判定は弱くなるが、よくあるケースは
    /// それでも捕まえられる。
    pub modified: Option<SystemTime>,
}

/// ファイルのスタンプを取る。存在しなければ `None` ── それ自体が答えになる。
/// 何も無かった場所にファイルができていれば、それは変更である。
pub fn of(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some(Stamp { len: m.len(), modified: m.modified().ok() })
}

/// 知らないあいだにファイルが変わったか。
///
/// *消えている*ファイルも変更として扱う。そのまま書くと誰にも断らずに復活させる
/// ことになるし、消したのは誰かが意図してやったことだから。
pub fn changed(path: &Path, since: &Stamp) -> bool {
    match of(path) {
        Some(now) => now != *since,
        None => true,
    }
}

/// 人に伝えるための文言。「何かが違う」ではなく「何が違うか」を言う。
pub fn describe(path: &Path, since: &Stamp) -> String {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    match of(path) {
        None => format!("{name} は消えています"),
        Some(now) if now.len != since.len => {
            let (a, b) = (since.len, now.len);
            format!("{name} は開いたあとで変わっています（{a} → {b} バイト）")
        }
        Some(_) => format!("{name} は開いたあとで更新されています"),
    }
}

/// スタンプを 1 つの文字列にする。あとで返してもらう必要がある呼び出し側のため。
///
/// **秒に丸めない。** iPhone はノートを読み、スタンプを JSON で受け取り、保存時に
/// それを返す。出力時に秒へ丸めたスタンプは、元のファイルと等しくなくなるので、
/// 保存のたびに相手のいない競合が報告される。これは仮の話ではなく、FFI の最初の
/// 版が実際にそうなっていて、往復テストが捕まえた。
///
/// 意図的に不透明にしてある。呼び出し側は保存して返すだけで、このモジュールの
/// 外が「長さと時刻である」ことを知る必要はない。
pub fn token(s: &Stamp) -> String {
    match s.modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) {
        Some(d) => format!("{}:{}.{}", s.len, d.as_secs(), d.subsec_nanos()),
        // 時刻を返さないファイルシステム。長さだけでは判定が弱くなるが、
        // よくあるケースは捕まえられる。絶対に一致しない時刻をでっち上げるより、
        // 無いと言うほうがいい。
        None => format!("{}:-", s.len),
    }
}

/// [`token`] が書いたものを読み戻す。それ以外は `None`。
pub fn from_token(t: &str) -> Option<Stamp> {
    let (len, time) = t.split_once(':')?;
    let len: u64 = len.parse().ok()?;
    if time == "-" {
        return Some(Stamp { len, modified: None });
    }
    let (secs, nanos) = time.split_once('.')?;
    let d = std::time::Duration::new(secs.parse().ok()?, nanos.parse().ok()?);
    Some(Stamp { len, modified: Some(std::time::UNIX_EPOCH + d) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(p: &Path, s: &str) {
        let mut f = std::fs::File::create(p).unwrap();
        f.write_all(s.as_bytes()).unwrap();
    }

    #[test]
    fn 触っていないファイルは変わっていない() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("note.md");
        write(&p, "one\n");
        let s = of(&p).unwrap();
        assert!(!changed(&p, &s));
    }

    /// これが存在する理由そのもの: 開いているあいだに誰かが書き換えた。
    #[test]
    fn 長さが違えば変更() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("note.md");
        write(&p, "one\n");
        let s = of(&p).unwrap();
        write(&p, "one\ntwo\n");
        assert!(changed(&p, &s));
        assert!(describe(&p, &s).contains("4 → 8"), "{}", describe(&p, &s));
    }

    /// **消えたファイルは変更である。** 保存すると復活してしまうし、消したのは
    /// 誰かが意図してやったこと。
    #[test]
    fn 消えたファイルは変更() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("note.md");
        write(&p, "one\n");
        let s = of(&p).unwrap();
        std::fs::remove_file(&p).unwrap();
        assert!(changed(&p, &s));
        assert!(describe(&p, &s).contains("消えています"));
    }

    #[test]
    fn 往復したスタンプは同じスタンプ() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("note.md");
        write(&p, "one\n");
        let s = of(&p).unwrap();
        // 「ほぼ等しい」ではなく完全一致。`changed` は等価比較なので、往復の
        // 途中で 1 ナノ秒でも落ちたスタンプは「変わった」と言ってしまう。
        assert_eq!(from_token(&token(&s)), Some(s.clone()));
        assert!(!changed(&p, &from_token(&token(&s)).unwrap()));
        // 時刻を返さないファイルシステムでも、往復はできる。
        let no_time = Stamp { len: 12, modified: None };
        assert_eq!(from_token(&token(&no_time)), Some(no_time));
        // 壊れた入力は、推測せずに拒否する。
        assert_eq!(from_token("あ"), None);
        assert_eq!(from_token("12:x.y"), None);
    }

    /// 同じ長さでの書き換えを、見分けられるだけ時間を空けて行う。
    /// （1 秒以内では見分けられない ── その穴はモジュールの冒頭に書いてある。）
    #[test]
    fn 長さが同じでも時刻が違えば変更() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("note.md");
        write(&p, "one\n");
        let s = of(&p).unwrap();
        // sleep せずに時刻を戻す。1 秒待つテストは、そのうち飛ばされるようになる。
        let old = std::fs::metadata(&p).unwrap().modified().unwrap()
            - std::time::Duration::from_secs(120);
        std::fs::OpenOptions::new().write(true).open(&p).unwrap().set_modified(old).unwrap();
        assert!(changed(&p, &s));
    }
}
