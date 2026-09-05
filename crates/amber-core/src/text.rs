//! ノートを読んで、**あった通りに**書き戻す。
//!
//! cian-core の `grepedit::read_text` / `write_text` と同じ仕事だが、写して
//! きたのは向こうが `crate::cloud`（OneDrive のプレースホルダ判定）と
//! `crate::viewer`（さらに `crate::office` = SharePoint の文書から本文を
//! 抜き出す 895 行）を引きずっているから ── ノートを開くのにそこまでは要らない。
//!
//! **一つだけ振る舞いを変えた。** 向こうはクラウドにしか無いファイルを
//! 「まだ落ちていない」と断る。grep が何百ものファイルを触るとき、そのたびに
//! ダウンロードを起こさないための断りで、正しい。**開けと言われた一本の
//! ノートでは逆**で、そこで断ると「同期しているのに開けない」になる。
//! 落として開くのが答え。

use std::path::Path;

use anyhow::{Context, Result};

/// 開いたときの姿。保存はここに書いてあるものを、そのまま戻す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    pub lines: Vec<String>,
    pub encoding: Encoding,
    pub bom: bool,
    pub eol: Eol,
    /// 末尾が改行で終わっていた。**憶えていないと、保存のたびに全行が差分に
    /// なる** ── 無かった改行を足すか、あった改行を落とすかのどちらかを、
    /// 触ってもいないファイルに対してやることになる。
    pub trailing_eol: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    ShiftJis,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    pub fn decode(self, bytes: &[u8]) -> String {
        let enc = match self {
            Encoding::Utf8 => encoding_rs::UTF_8,
            Encoding::ShiftJis => encoding_rs::SHIFT_JIS,
            Encoding::Utf16Le => encoding_rs::UTF_16LE,
            Encoding::Utf16Be => encoding_rs::UTF_16BE,
        };
        enc.decode(bytes).0.into_owned()
    }

    pub fn encode(self, text: &str) -> Vec<u8> {
        match self {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::ShiftJis => encoding_rs::SHIFT_JIS.encode(text).0.into_owned(),
            // encoding_rs は UTF-16 を書けない（読むだけ）ので自分で並べる。
            Encoding::Utf16Le => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            Encoding::Utf16Be => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
    /// 昔の Mac。滅多に無いが、二つ運ぶなら三つ目もほぼ只。
    Cr,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::Crlf => "\r\n",
            Eol::Cr => "\r",
        }
    }

    /// **最初の一つで決める。** 混ざっているファイルもあるが、そのときは
    /// 「最初に出てきたもの」が書いた人の意図にいちばん近い。
    pub fn detect(text: &str) -> Eol {
        match text.find('\n') {
            Some(0) => Eol::Lf,
            Some(i) if text.as_bytes()[i - 1] == b'\r' => Eol::Crlf,
            Some(_) => Eol::Lf,
            None if text.contains('\r') => Eol::Cr,
            None => Eol::Lf,
        }
    }
}

/// 大きすぎるものは断る。ノートは人が書いた文章で、これを超えるなら
/// それはノートではない（写真を貼り込んだ .md でも桁が違う）。
const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// `path` を文章として読む。タブはタブのまま ── 画面で桁を揃えるための
/// 展開は表示側の仕事で、保存に混ぜると触ってもいない行が書き換わる。
pub fn read(path: &Path) -> Result<TextFile> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if meta.is_dir() {
        anyhow::bail!("is a directory");
    }
    if meta.len() > MAX_BYTES {
        anyhow::bail!("larger than {} MB", MAX_BYTES / (1024 * 1024));
    }
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        anyhow::bail!("looks binary");
    }
    let (encoding, bom) = match () {
        _ if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) => (Encoding::Utf8, true),
        _ if bytes.starts_with(&[0xFF, 0xFE]) => (Encoding::Utf16Le, true),
        _ if bytes.starts_with(&[0xFE, 0xFF]) => (Encoding::Utf16Be, true),
        _ => {
            // UTF-8 を先に、駄目なら Shift_JIS。grep と同じ順で、理由も同じ ──
            // 出会う古いファイルはまだ SJIS のことがある。
            let (_, _, bad) = encoding_rs::UTF_8.decode(&bytes);
            if bad {
                let (_, _, sjis_bad) = encoding_rs::SHIFT_JIS.decode(&bytes);
                if sjis_bad {
                    anyhow::bail!("neither UTF-8 nor Shift_JIS");
                }
                (Encoding::ShiftJis, false)
            } else {
                (Encoding::Utf8, false)
            }
        }
    };
    let text = encoding.decode(&bytes);
    let eol = Eol::detect(&text);
    let trailing_eol = text.ends_with('\n') || text.ends_with('\r');
    Ok(TextFile { lines: text.lines().map(str::to_string).collect(), encoding, bom, eol, trailing_eol })
}

/// 読んだときの文字コード・BOM・改行のまま書き戻す。
///
/// **Shift_JIS で CRLF のノートは、そのまま返す。** iPhone が UTF-8 と LF で
/// 保存し直すと、Mac 側では一行も編集していないファイルが全行差分になる。
pub fn write(path: &Path, file: &TextFile) -> Result<()> {
    let sep = file.eol.as_str();
    let mut text = file.lines.join(sep);
    if file.trailing_eol {
        text.push_str(sep);
    }
    let mut bytes = Vec::new();
    if file.bom {
        bytes.extend_from_slice(match file.encoding {
            Encoding::Utf8 => &[0xEF, 0xBB, 0xBF][..],
            Encoding::Utf16Le => &[0xFF, 0xFE][..],
            Encoding::Utf16Be => &[0xFE, 0xFF][..],
            Encoding::ShiftJis => &[][..],
        });
    }
    bytes.extend_from_slice(&file.encoding.encode(&text));
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

/// まだ無いノートを書くための、まっさらな姿。UTF-8 と LF。
impl Default for TextFile {
    fn default() -> Self {
        TextFile {
            lines: Vec::new(),
            encoding: Encoding::Utf8,
            bom: false,
            eol: Eol::Lf,
            trailing_eol: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 読んだ姿のまま書き戻す() {
        let dir = tempfile::tempdir().unwrap();
        for (bytes, enc, eol, bom) in [
            (b"\xe3\x81\x82\r\n\xe3\x81\x84\r\n".to_vec(), Encoding::Utf8, Eol::Crlf, false),
            (b"\xef\xbb\xbfa\nb\n".to_vec(), Encoding::Utf8, Eol::Lf, true),
            (b"\x82\xa0\r\n\x82\xa2\r\n".to_vec(), Encoding::ShiftJis, Eol::Crlf, false),
        ] {
            let at = dir.path().join("n.md");
            std::fs::write(&at, &bytes).unwrap();
            let f = read(&at).unwrap();
            assert_eq!((f.encoding, f.eol, f.bom), (enc, eol, bom));
            write(&at, &f).unwrap();
            assert_eq!(std::fs::read(&at).unwrap(), bytes, "書き戻して同じ byte にならない");
        }
    }

    #[test]
    fn 末尾の改行の有無を憶えている() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("n.md");
        std::fs::write(&at, b"a\nb").unwrap();
        let f = read(&at).unwrap();
        assert!(!f.trailing_eol);
        write(&at, &f).unwrap();
        assert_eq!(std::fs::read(&at).unwrap(), b"a\nb");
    }
}
