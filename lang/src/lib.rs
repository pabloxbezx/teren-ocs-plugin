//! Jazyk rozhraní Terénu.
//!
//! Každý text pro uživatele je v kódu jako dvojice `tr("český text", "English text")`
//! (šablony s argumenty `trf!("…{}…", "…{}…", a)`). Jazyk se vybere při načtení
//! pluginu: volba v Nastavení Terénu (`jazyk = auto|cs|en|<kód>`), při `auto`
//! jazyk Open CAD Studia a nakonec jazyk systému.
//!
//! Další jazyk = soubor `preklady/<kód>.txt` (vloží se při sestavení) nebo
//! `%APPDATA%\OpenCADStudio\teren_preklady\<kód>.txt` (načte se za běhu).
//! Řádek `English text<TAB>překlad` nebo `English text = překlad`; klíčem je
//! anglický text, `\n` = nový řádek, `#` = poznámka. Chybějící soubor nebo
//! řádek = angličtina.

use std::collections::HashMap;
use std::fmt::Display;
use std::path::PathBuf;
use std::sync::OnceLock;

mod dvojice;
pub use dvojice::DVOJICE;

include!(concat!(env!("OUT_DIR"), "/vestavene.rs"));

// ── Volba jazyka ────────────────────────────────────────────────────────────

/// Kód jazyka zvoleného při načtení pluginu („cs“, „en“ nebo kód souboru).
pub fn jazyk() -> &'static str {
    static J: OnceLock<String> = OnceLock::new();
    J.get_or_init(|| {
        // TEREN_JAZYK přebije volbu (testy přes .cargo/config.toml – nezávisí
        // na nastavení uživatele)
        let volba = std::env::var("TEREN_JAZYK").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(volba_jazyka);
        let pozadovany = if volba == "auto" || volba.is_empty() {
            jazyk_ocs().filter(|j| j != "system").unwrap_or_else(systemovy_jazyk)
        } else {
            volba
        };
        vyber_kod(&pozadovany, &dostupne_jazyky())
    })
}

/// Převede kód („cs-CZ“, „sk“, „de_DE.UTF-8“) na dostupný jazyk; neznámý = „en“.
pub fn vyber_kod(pozadovany: &str, dostupne: &[String]) -> String {
    let k = pozadovany.trim().to_ascii_lowercase().replace('_', "-");
    let k = k.split('.').next().unwrap_or("").to_string();
    let zaklad = k.split('-').next().unwrap_or("").to_string();
    if zaklad == "cs" || zaklad == "sk" {
        return "cs".into();
    }
    for kandidat in [&k, &zaklad] {
        if !kandidat.is_empty() && dostupne.iter().any(|d| d == kandidat) {
            return kandidat.clone();
        }
    }
    "en".into()
}

/// Je zvolená čeština.
pub fn cesky() -> bool {
    jazyk() == "cs"
}

/// Je zvolená angličtina.
pub fn anglicky() -> bool {
    jazyk() == "en"
}

fn slozka_ocs() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|b| PathBuf::from(b).join("OpenCADStudio"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|b| b.join("OpenCADStudio"))
    }
}

/// Volba jazyka z nastavení pluginu (`jazyk = auto|cs|en|<kód>`).
pub fn volba_jazyka() -> String {
    let Some(text) = slozka_ocs().and_then(|d| std::fs::read_to_string(d.join("teren_nastaveni.txt")).ok()) else {
        return "auto".into();
    };
    text.lines()
        .filter_map(|r| r.split_once('='))
        .find(|(k, _)| k.trim() == "jazyk")
        .map_or_else(|| "auto".into(), |(_, v)| v.trim().to_string())
}

/// Jazyk rozhraní Open CAD Studia (`"language"` v `settings.json`: „system“,
/// „cs-CZ“, „en-US“…).
pub fn jazyk_ocs() -> Option<String> {
    let text = std::fs::read_to_string(slozka_ocs()?.join("settings.json")).ok()?;
    jazyk_z_nastaveni_ocs(&text)
}

/// Hodnota `"language"` v nejvyšší úrovni JSON objektu (jazyk kontroly
/// pravopisu je vnořený a nepočítá se).
pub fn jazyk_z_nastaveni_ocs(json: &str) -> Option<String> {
    let b = json.as_bytes();
    let (mut hloubka, mut i) = (0i32, 0usize);
    while i < b.len() {
        match b[i] {
            b'{' | b'[' => hloubka += 1,
            b'}' | b']' => hloubka -= 1,
            b'"' => {
                let (konec, s) = retezec(json, i)?;
                i = konec;
                if hloubka == 1 && s == "language" {
                    let mut j = i;
                    while j < b.len() && (b[j] == b' ' || b[j] == b':' || b[j] == b'\n' || b[j] == b'\r' || b[j] == b'\t') {
                        j += 1;
                    }
                    if j < b.len() && b[j] == b'"' {
                        return retezec(json, j).map(|(_, v)| v);
                    }
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Řetězec JSON od uvozovky na `i`: (index za ním, obsah bez escapování).
fn retezec(s: &str, i: usize) -> Option<(usize, String)> {
    let b = s.as_bytes();
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Some((j + 1, s[i + 1..j].to_string())),
            _ => j += 1,
        }
    }
    None
}

/// Jazyk systému („cs“, „en_US“…).
pub fn systemovy_jazyk() -> String {
    for v in ["OCS_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(s) = std::env::var(v) {
            if !s.is_empty() && s != "C" && s != "POSIX" {
                return s.to_ascii_lowercase();
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(s) = jazyk_windows() {
            return s;
        }
    }
    "cs".into()
}

#[cfg(windows)]
fn jazyk_windows() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // primární jazyk: 0x05 čeština, 0x1b slovenština, 0x07 němčina, 0x15 polština
    let id = unsafe { GetUserDefaultUILanguage() } & 0x3ff;
    Some(
        match id {
            0x05 => "cs",
            0x1b => "sk",
            0x07 => "de",
            0x15 => "pl",
            0x0c => "fr",
            0x10 => "it",
            0x0a => "es",
            0x0e => "hu",
            0x1f => "tr",
            0x13 => "nl",
            _ => "en",
        }
        .into(),
    )
}

// ── Další jazyky ze souborů ─────────────────────────────────────────────────

/// Složka s jazykovými soubory načítanými za běhu.
pub fn slozka_prekladu() -> Option<PathBuf> {
    slozka_ocs().map(|d| d.join("teren_preklady"))
}

fn soubory_za_behu() -> Vec<(String, String)> {
    let Some(d) = slozka_prekladu() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(d) else { return Vec::new() };
    let mut v = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("txt") {
            continue;
        }
        let Some(kod) = p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()) else { continue };
        if kod == "cs" || kod == "en" {
            continue;
        }
        if let Ok(t) = std::fs::read_to_string(&p) {
            v.push((kod, t));
        }
    }
    v
}

/// Dostupné jazyky: „cs“, „en“ a kódy jazykových souborů.
pub fn dostupne_jazyky() -> Vec<String> {
    let mut v: Vec<String> = vec!["cs".into(), "en".into()];
    for (k, _) in SOUBORY.iter().map(|(k, t)| (k.to_string(), *t)).chain(soubory_za_behu().iter().map(|(k, _)| (k.clone(), ""))) {
        if !v.contains(&k) {
            v.push(k);
        }
    }
    v
}

/// Načte jazykový soubor: anglický text → překlad.
pub fn nacti_soubor(text: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for r in text.lines() {
        let r = r.trim_end_matches('\r');
        if r.trim_start().starts_with('#') || r.trim().is_empty() {
            continue;
        }
        let dvojice = r.split_once('\t').or_else(|| r.split_once(" = "));
        if let Some((en, cil)) = dvojice {
            let (en, cil) = (en.trim().replace("\\n", "\n"), cil.trim().replace("\\n", "\n"));
            if !en.is_empty() && !cil.is_empty() {
                m.insert(en, cil);
            }
        }
    }
    m
}

/// Tabulka zvoleného dalšího jazyka (prázdná pro cs/en). Soubor za běhu
/// přepisuje vestavěný.
fn tabulka_jazyka() -> &'static HashMap<String, &'static str> {
    static T: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    T.get_or_init(|| {
        let j = jazyk();
        let mut m: HashMap<String, &'static str> = HashMap::new();
        if j == "cs" || j == "en" {
            return m;
        }
        let mut texty: Vec<String> = SOUBORY.iter().filter(|(k, _)| *k == j).map(|(_, t)| t.to_string()).collect();
        texty.extend(soubory_za_behu().into_iter().filter(|(k, _)| k == j).map(|(_, t)| t));
        for t in texty {
            for (en, cil) in nacti_soubor(&t) {
                m.insert(en, Box::leak(cil.into_boxed_str()));
            }
        }
        m
    })
}

fn dalsi(en: &'static str) -> &'static str {
    tabulka_jazyka().get(en).copied().unwrap_or(en)
}

// ── Překlad ─────────────────────────────────────────────────────────────────

/// Text rozhraní ve zvoleném jazyce.
pub fn tr(cs: &'static str, en: &'static str) -> &'static str {
    match jazyk() {
        "cs" => cs,
        "en" => en,
        _ => dalsi(en),
    }
}

fn tabulka_dvojic() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| DVOJICE.iter().copied().collect())
}

/// Český text z tabulky ([`DVOJICE`]) ve zvoleném jazyce – pro texty
/// v tabulkách a konstantách, které nejdou zapsat jako `tr(cs, en)`.
pub fn tr_tab(cs: &'static str) -> &'static str {
    if cesky() {
        return cs;
    }
    match tabulka_dvojic().get(cs) {
        Some(en) if anglicky() => en,
        Some(en) => dalsi(en),
        None => cs,
    }
}

/// Jako [`tr_tab`] pro text, který není `'static` (např. název vestavěného
/// stylu uložený ve výkrese).
pub fn tr_dyn(cs: &str) -> String {
    if cesky() {
        return cs.to_string();
    }
    match tabulka_dvojic().get(cs) {
        Some(en) if anglicky() => en.to_string(),
        Some(en) => dalsi(en).to_string(),
        None => cs.to_string(),
    }
}

/// Doplní argumenty do šablony: `{}`, `{:.N}`, `{:>N}`, `{:<N}`, `{{`, `}}`.
pub fn formatuj(sablona: &str, arg: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(sablona.len() + 16);
    let mut i = 0;
    let mut ch = sablona.char_indices().peekable();
    while let Some((_, c)) = ch.next() {
        match c {
            '{' if ch.peek().map(|x| x.1) == Some('{') => {
                ch.next();
                out.push('{');
            }
            '}' if ch.peek().map(|x| x.1) == Some('}') => {
                ch.next();
                out.push('}');
            }
            '{' => {
                let mut spec = String::new();
                for (_, c) in ch.by_ref() {
                    if c == '}' {
                        break;
                    }
                    spec.push(c);
                }
                let (pozice, spec) = match spec.split_once(':') {
                    Some((p, s)) => (p.to_string(), s.to_string()),
                    None => (spec, String::new()),
                };
                let a = match pozice.parse::<usize>() {
                    Ok(p) => arg.get(p),
                    Err(_) => {
                        let a = arg.get(i);
                        i += 1;
                        a
                    }
                };
                let Some(a) = a else { continue };
                out.push_str(&uprav(*a, &spec));
            }
            c => out.push(c),
        }
    }
    out
}

fn uprav(a: &dyn Display, spec: &str) -> String {
    if spec.is_empty() {
        return a.to_string();
    }
    let (zarovnani, zbytek) = match spec.chars().next() {
        Some(z @ ('<' | '>' | '^')) => (Some(z), &spec[1..]),
        _ => (None, spec),
    };
    let (sirka, presnost) = match zbytek.split_once('.') {
        Some((s, p)) => (s.parse::<usize>().ok(), p.parse::<usize>().ok()),
        None => (zbytek.parse::<usize>().ok(), None),
    };
    let s = match presnost {
        Some(p) => format!("{a:.p$}"),
        None => a.to_string(),
    };
    match (sirka, zarovnani) {
        (Some(w), Some('<')) => format!("{s:<w$}"),
        (Some(w), Some('^')) => format!("{s:^w$}"),
        (Some(w), _) if s.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-') => format!("{s:>w$}"),
        (Some(w), Some('>')) => format!("{s:>w$}"),
        (Some(w), _) => format!("{s:<w$}"),
        _ => s,
    }
}

/// `trf!("Bod {} výška {:.2}", "Point {} elevation {:.2}", a, b)` – šablona
/// ve zvoleném jazyce s argumenty.
#[macro_export]
macro_rules! trf {
    ($cs:literal, $en:literal $(, $a:expr)* $(,)?) => {
        $crate::formatuj($crate::tr($cs, $en), &[$(&$a as &dyn ::std::fmt::Display),*])
    };
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn sablona() {
        assert_eq!(formatuj("Bod {} výška {:.2} m", &[&"5", &12.3456]), "Bod 5 výška 12.35 m");
        assert_eq!(formatuj("{1} {0}", &[&"a", &"b"]), "b a");
    }

    #[test]
    fn kod_jazyka() {
        let d: Vec<String> = ["cs", "en", "de"].iter().map(|s| s.to_string()).collect();
        assert_eq!(vyber_kod("cs-CZ", &d), "cs");
        assert_eq!(vyber_kod("sk_SK.UTF-8", &d), "cs");
        assert_eq!(vyber_kod("de-DE", &d), "de");
        assert_eq!(vyber_kod("fr-FR", &d), "en");
        assert_eq!(vyber_kod("en-US", &d), "en");
    }

    #[test]
    fn jazyk_z_nastaveni() {
        let j = r#"{"spell":{"language":""},"theme":"dark","language":"cs-CZ"}"#;
        assert_eq!(jazyk_z_nastaveni_ocs(j).as_deref(), Some("cs-CZ"));
        assert_eq!(jazyk_z_nastaveni_ocs(r#"{"language": "system"}"#).as_deref(), Some("system"));
        assert_eq!(jazyk_z_nastaveni_ocs(r#"{"spell":{"language":"de"}}"#), None);
    }

    #[test]
    fn soubor_jazyka() {
        let m = nacti_soubor("# poznámka\nImport points = Punkte importieren\nEnter = finish\tEnter = fertig\nLine\\nTwo = Zeile\\nZwei\n");
        assert_eq!(m.get("Import points").map(String::as_str), Some("Punkte importieren"));
        assert_eq!(m.get("Enter = finish").map(String::as_str), Some("Enter = fertig"));
        assert_eq!(m.get("Line\nTwo").map(String::as_str), Some("Zeile\nZwei"));
    }

    #[test]
    fn dvojice_maji_anglictinu() {
        for (cs, en) in DVOJICE {
            assert!(!cs.is_empty() && !en.is_empty(), "prázdná dvojice: {cs:?}");
        }
    }
}
