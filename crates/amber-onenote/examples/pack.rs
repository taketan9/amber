//! 目次つきのフォルダを `.onepkg`（CAB）に包む（窓で試す用）:
//! `cargo run -p amber-onenote --example pack -- <フォルダ> <出力.onepkg>`
fn main() {
    let mut a = std::env::args().skip(1);
    let (dir, to) = (std::path::PathBuf::from(a.next().unwrap()), a.next().unwrap());
    let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".one") || n.ends_with(".onetoc2")).collect();
    names.sort();
    let mut b = cab::CabinetBuilder::new();
    let f = b.add_folder(cab::CompressionType::MsZip);
    for n in &names { f.add_file(n.clone()); }
    let mut w = b.build(std::fs::File::create(&to).unwrap()).unwrap();
    while let Some(mut f) = w.next_file().unwrap() {
        let bytes = std::fs::read(dir.join(f.file_name())).unwrap();
        std::io::Write::write_all(&mut f, &bytes).unwrap();
    }
    w.finish().unwrap();
}
