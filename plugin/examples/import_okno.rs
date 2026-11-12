//! Zkušební zobrazení okna importu bez Open CAD Studia; uloží snímek.
//! `cargo run -p teren_plugin --example import_okno -- <soubor|""> <snimek.png> [šířka] [výška] [filtr]`
fn main() -> eframe::Result {
    let a: Vec<String> = std::env::args().collect();
    let arg = |i: usize, v: &str| a.get(i).cloned().unwrap_or_else(|| v.to_string());
    teren_plugin::ukazka_importu(&arg(1, ""), &arg(2, "import.png"), arg(3, "1240").parse().unwrap_or(1240.0), arg(4, "800").parse().unwrap_or(800.0), &arg(5, ""))
}
