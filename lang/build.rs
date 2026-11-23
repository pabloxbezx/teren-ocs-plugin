//! Vloží do pluginu soubory dalších jazyků `preklady/<kód>.txt` (kořen repozitáře).

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("preklady");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut soubory: Vec<(String, String)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            println!("cargo:rerun-if-changed={}", p.display());
            let Some(kod) = p.file_stem().and_then(|s| s.to_str()) else { continue };
            if p.extension().and_then(|s| s.to_str()) != Some("txt") || !platny_kod(kod) {
                continue;
            }
            soubory.push((kod.to_ascii_lowercase(), p.canonicalize().unwrap_or(p.clone()).display().to_string()));
        }
    }
    soubory.sort();
    let mut out = String::from("/// Vestavěné soubory dalších jazyků (kód, obsah).\npub static SOUBORY: &[(&str, &str)] = &[\n");
    for (kod, cesta) in &soubory {
        let _ = writeln!(out, "    ({kod:?}, include_str!({cesta:?})),");
    }
    out.push_str("];\n");
    let cil = Path::new(&std::env::var("OUT_DIR").unwrap()).join("vestavene.rs");
    std::fs::write(cil, out).unwrap();
}

fn platny_kod(k: &str) -> bool {
    (2..=8).contains(&k.len()) && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && k != "cs" && k != "en"
}
