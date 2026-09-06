//! amber が自分で書いた zip を、書いて・数えて・戻す。
//!
//! cian-core の `archive` は 7z も tar も rar も iso も見る 1,521 行で、
//! 文字コードの推定も暗号化 zip も持っている。**amber が要るのは、
//! 自分が書いたバックアップを読み書きすることだけ**なので、引きずらずに
//! ここに置いた ── ノートのアプリがアーカイバを積む理由は無い。
//!
//! 名前は必ず `/` 区切りの相対パスで書く。zip の仕様がそう決めているうえ、
//! Windows で書いた `\` 混じりの名前は Mac で「一つの長いファイル名」になる。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;

/// 中に入っている名前ぜんぶ（ディレクトリの項目は除く）。
pub fn list(zip: &Path) -> Result<Vec<String>> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(zip)?)?;
    let mut out = Vec::new();
    for i in 0..z.len() {
        let f = z.by_index(i)?;
        if !f.is_dir() {
            out.push(f.name().to_string());
        }
    }
    Ok(out)
}

/// `into` の下へ広げる。`strip` が空でなければ、その先頭のひと山を落とす。
///
/// **中身は `into` の外へ出さない。** zip の名前は `../` を含められるので、
/// 素直に繋ぐと外のファイルを書き換えられる（zip slip）。実際に踏んだことは
/// 無いが、戻す先は人のノートの置き場所で、踏んだら気づけない類の事故。
pub fn extract(zip: &Path, into: &Path, strip: &str) -> Result<usize> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(zip)?)?;
    let mut put = 0usize;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().replace('\\', "/");
        let rest = match strip.is_empty() {
            true => name.as_str(),
            false => name
                .strip_prefix(strip)
                .map(|r| r.trim_start_matches('/'))
                .unwrap_or(name.as_str()),
        };
        let Some(dest) = under(into, rest) else { continue };
        if let Some(up) = dest.parent() {
            std::fs::create_dir_all(up)?;
        }
        let mut body = Vec::new();
        f.read_to_end(&mut body)?;
        std::fs::write(&dest, &body)?;
        put += 1;
    }
    Ok(put)
}

/// `root` の下の行き先。`..` で外へ出ようとする名前は `None`。
fn under(root: &Path, rest: &str) -> Option<PathBuf> {
    let mut at = root.to_path_buf();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            p if p.contains('\0') => return None,
            p => at.push(p),
        }
    }
    (at != root).then_some(at)
}

/// `sources` をぜんぶ、それぞれの名前を頭に付けて zip にする。
///
/// 頭を付けるのは、戻すときに「何の zip か」が名前から分かるため
/// （`extract` の `strip` が、全部が一つの山の下にあるときだけ外す）。
/// 付けないと、展開した人の作業ディレクトリにノートが散らばる。
/// 中に入れる「これは何の zip か」の札。
///
/// **戻すときに、頭を外すかどうかがこれで決まる。** ノート帳ぜんぶの zip は
/// `ノート/…` という一つの山の下にあるので頭を外す（外さないと、ノートの中に
/// `ノート` という棚がもう一つできる）。**フォルダ一つの zip も見た目は同じ
/// 形**なので、同じ規則で外すと `仕事/週報.md` が `週報.md` になって根に散る
/// ── 戻したのに元の場所に戻っていない。形からは見分けられないので、
/// 作るときに書いておく。
pub const LABEL: &str = ".amber-backup.json";

pub fn create(sources: &[PathBuf], dest: &Path, ctl: &mut crate::Ctl) -> Result<usize> {
    create_labelled(sources, dest, None, ctl)
}

pub fn create_labelled(
    sources: &[PathBuf],
    dest: &Path,
    label: Option<&str>,
    ctl: &mut crate::Ctl,
) -> Result<usize> {
    let mut jobs: Vec<(PathBuf, String)> = Vec::new();
    for src in sources {
        let head = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if src.is_dir() {
            for (at, rel) in walk(src) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                jobs.push((at, if head.is_empty() { rel } else { format!("{head}/{rel}") }));
            }
        } else if src.is_file() {
            jobs.push((src.clone(), head));
        }
    }
    let mut w = zip::ZipWriter::new(std::fs::File::create(dest)?);
    let opts: zip::write::FileOptions<()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    if let Some(label) = label {
        w.start_file(LABEL, opts)?;
        w.write_all(label.as_bytes())?;
    }
    let mut done = 0usize;
    for (at, name) in &jobs {
        if ctl.stopped() {
            break;
        }
        w.start_file(name.clone(), opts)?;
        let mut body = Vec::new();
        std::fs::File::open(at)?.read_to_end(&mut body)?;
        w.write_all(&body)?;
        done += 1;
        ctl.step(done, jobs.len());
    }
    w.finish()?;
    Ok(done)
}

/// `root` の下の普通のファイルぜんぶと、`root` から見た相対の道。
fn walk(root: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let at = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(at),
                Ok(t) if t.is_file() => {
                    if let Ok(rel) = at.strip_prefix(root) {
                        out.push((at.clone(), rel.to_path_buf()));
                    }
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}

// zip の名前が外へ出ようとしたら断ることと、頭のひと山が落ちること。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 外へ出る名前は断る() {
        let root = Path::new("/tmp/x");
        assert!(under(root, "../../etc/passwd").is_none());
        assert!(under(root, "a/../../b").is_none());
        assert_eq!(under(root, "a/b.md"), Some(root.join("a").join("b.md")));
        assert_eq!(under(root, ""), None);
    }
}
