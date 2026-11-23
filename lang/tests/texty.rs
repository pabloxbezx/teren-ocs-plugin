//! Kontrola lokalizace celého repozitáře:
//! - každý český text pro uživatele je v `tr(cs, en)` / `trf!(cs, en, …)` nebo
//!   v tabulce `DVOJICE` (texty z tabulek), jinak řádek nese `// i18n:ne`,
//! - každá dvojice má neprázdnou angličtinu bez české diakritiky a stejné
//!   zástupné symboly `{}` v obou jazycích,
//! - klíče v souborech dalších jazyků (`preklady/<kód>.txt`) jsou anglické
//!   texty, které v kódu opravdu jsou.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

const CZ: &str = "áčďéěíňóřšťúůýžÁČĎÉĚÍŇÓŘŠŤÚŮÝŽ";

fn ceske(s: &str) -> bool {
    // název pluginu se nepřekládá; zkratky hlavních bodů trasy ZÚ/KÚ zůstávají
    // i v angličtině (zatím nerozhodnuto)
    s.replace("Terén", "").replace("TERÉN", "").replace("ZÚ", "").replace("KÚ", "").chars().any(|c| CZ.contains(c))
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Punct(char),
    Str(String, usize),
}

/// Jednoduchý lexer Rustu: identifikátory, interpunkce a řetězce (bez komentářů).
fn tokeny(src: &str) -> Vec<Tok> {
    let c: Vec<char> = src.chars().collect();
    let (mut i, mut radek, mut out) = (0usize, 1usize, Vec::new());
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            radek += 1;
            i += 1;
        } else if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                if c[i] == '\n' {
                    radek += 1;
                }
                i += 1;
            }
            i += 2;
        } else if ch == '"' || ((ch == 'r' || ch == 'b') && matches!(c.get(i + 1), Some('"') | Some('#') | Some('r'))) {
            let start_radek = radek;
            let mut j = i;
            if c[j] == 'b' {
                j += 1;
            }
            let raw = c[j] == 'r';
            if raw {
                j += 1;
            }
            let mut hashes = 0;
            while j < c.len() && c[j] == '#' {
                hashes += 1;
                j += 1;
            }
            if j >= c.len() || c[j] != '"' {
                // identifikátor začínající r/b
                let s = i;
                while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                    i += 1;
                }
                out.push(Tok::Ident(c[s..i].iter().collect()));
                continue;
            }
            j += 1;
            let mut obsah = String::new();
            loop {
                if j >= c.len() {
                    break;
                }
                if !raw && c[j] == '\\' {
                    match c.get(j + 1) {
                        Some('n') => obsah.push('\n'),
                        Some('t') => obsah.push('\t'),
                        Some('\n') => {
                            radek += 1;
                            j += 2;
                            while j < c.len() && c[j].is_whitespace() {
                                if c[j] == '\n' {
                                    radek += 1;
                                }
                                j += 1;
                            }
                            continue;
                        }
                        Some(x) => obsah.push(*x),
                        None => {}
                    }
                    j += 2;
                    continue;
                }
                if c[j] == '"' && (0..hashes).all(|h| c.get(j + 1 + h) == Some(&'#')) {
                    j += 1 + hashes;
                    break;
                }
                if c[j] == '\n' {
                    radek += 1;
                }
                obsah.push(c[j]);
                j += 1;
            }
            out.push(Tok::Str(obsah, start_radek));
            i = j;
        } else if ch == '\'' {
            // znak ('a', '\n', '\u{..}') nebo životnost ('a)
            if c.get(i + 1) == Some(&'\\') {
                i += 2;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if c.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if ch.is_alphabetic() || ch == '_' {
            let s = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(c[s..i].iter().collect()));
        } else if ch.is_whitespace() {
            i += 1;
        } else {
            out.push(Tok::Punct(ch));
            i += 1;
        }
    }
    out
}

fn koren() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn soubory(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            soubory(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

struct Nalez {
    dvojice: Vec<(String, String, String)>,
    volne: Vec<String>,
}

fn projdi() -> Nalez {
    let mut seznam = Vec::new();
    for d in ["src", "plugin/src", "trasy/src"] {
        soubory(&koren().join(d), &mut seznam);
    }
    let tabulka: HashSet<&str> = teren_lang::DVOJICE.iter().map(|d| d.0).collect();
    let mut n = Nalez { dvojice: Vec::new(), volne: Vec::new() };
    for p in seznam {
        // příkazový nástroj pro vývoj (src/bin) je jen česky
        if p.components().any(|c| c.as_os_str() == "bin") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap().replace("\r\n", "\n");
        // testy pluginu se nekontrolují
        let text = match text.find("#[cfg(test)]") {
            Some(i) => text[..i].to_string(),
            None => text,
        };
        let vyjimky: HashSet<usize> =
            text.lines().enumerate().filter(|(_, r)| r.contains("i18n:ne")).map(|(i, _)| i + 1).collect();
        let t = tokeny(&text);
        let mut prelozene: HashSet<usize> = HashSet::new();
        for i in 0..t.len() {
            let je_tr = matches!(&t[i], Tok::Ident(s) if s == "tr") && t.get(i + 1) == Some(&Tok::Punct('('));
            let je_trf = matches!(&t[i], Tok::Ident(s) if s == "trf")
                && t.get(i + 1) == Some(&Tok::Punct('!'))
                && t.get(i + 2) == Some(&Tok::Punct('('));
            let z = if je_tr { i + 2 } else if je_trf { i + 3 } else { continue };
            if let (Some(Tok::Str(cs, r)), Some(Tok::Punct(',')), Some(Tok::Str(en, _))) = (t.get(z), t.get(z + 1), t.get(z + 2)) {
                prelozene.insert(z);
                prelozene.insert(z + 2);
                if !vyjimky.contains(r) {
                    n.dvojice.push((format!("{}:{}", p.display(), r), cs.clone(), en.clone()));
                }
            }
        }
        for (i, tok) in t.iter().enumerate() {
            if let Tok::Str(s, r) = tok {
                if !prelozene.contains(&i) && ceske(s) && !tabulka.contains(s.as_str()) && !vyjimky.contains(r) {
                    n.volne.push(format!("{}:{}  {:?}", p.display(), r, s.chars().take(70).collect::<String>()));
                }
            }
        }
    }
    n
}

fn zastupne(s: &str) -> usize {
    s.replace("{{", "").matches('{').count()
}

#[test]
fn zadny_cesky_text_mimo_tr() {
    let n = projdi();
    assert!(n.dvojice.len() > 1000, "lexer nenašel dvojice tr(cs, en)");
    assert!(n.volne.is_empty(), "{} textů mimo tr(cs, en) (doplňte překlad nebo // i18n:ne):\n{}", n.volne.len(), n.volne.join("\n"));
}

#[test]
fn kazda_dvojice_ma_oba_jazyky() {
    let mut chyby = Vec::new();
    for (kde, cs, en) in projdi().dvojice {
        if en.trim().is_empty() && !cs.trim().is_empty() {
            chyby.push(format!("{kde}: chybí angličtina: {cs:?}"));
        } else if ceske(&en) {
            chyby.push(format!("{kde}: angličtina s diakritikou: {en:?}"));
        } else if zastupne(&cs) != zastupne(&en) {
            chyby.push(format!("{kde}: jiný počet {{}}: {cs:?} / {en:?}"));
        }
    }
    for (cs, en) in teren_lang::DVOJICE {
        if ceske(en) || zastupne(cs) != zastupne(en) {
            chyby.push(format!("DVOJICE: {cs:?} / {en:?}"));
        }
    }
    assert!(chyby.is_empty(), "{}", chyby.join("\n"));
}

#[test]
fn soubory_dalsich_jazyku_maji_platne_klice() {
    let anglicke: HashSet<String> = projdi().dvojice.into_iter().map(|d| d.2).chain(teren_lang::DVOJICE.iter().map(|d| d.1.to_string())).collect();
    let mut chyby = Vec::new();
    for (kod, text) in teren_lang::SOUBORY {
        for (en, cil) in teren_lang::nacti_soubor(text) {
            if !anglicke.contains(&en) {
                chyby.push(format!("{kod}.txt: neznámý anglický text {en:?}"));
            } else if zastupne(&en) != zastupne(&cil) {
                chyby.push(format!("{kod}.txt: jiný počet {{}}: {en:?}"));
            }
        }
    }
    assert!(chyby.is_empty(), "{}", chyby.join("\n"));
}
