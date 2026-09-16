//! 見本を開いて Markdown を出す（目で見る用）: `cargo run -p amber-onenote --example peek -- <道>`
fn main() -> anyhow::Result<()> {
    let p = std::env::args().nth(1).expect("道を渡してください");
    let got = amber_onenote::open(std::path::Path::new(&p))?;
    println!("book={:?}", got.book);
    for u in &got.units {
        println!("== {:?} / {}", u.groups, u.name);
        for pg in &u.pages {
            println!("--- {} (level {}, {}) 絵{} 添付{}", pg.title, pg.level, pg.created, pg.pictures.len(), pg.files.len());
            println!("{}", pg.body);
        }
    }
    Ok(())
}
