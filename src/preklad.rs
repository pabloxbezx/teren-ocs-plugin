//! Překlady textů rozhraní.
//!
//! Klíčem je český text, překlad je v souboru `preklady/<jazyk>.txt`
//! (řádek „český text<TAB>překlad“, `\n` = nový řádek). Chybějící překlad
//! = český text. Jazyk se volí v Nastavení (auto / cs / en) a platí od
//! dalšího spuštění Open CAD Studia (pás karet se načítá jen jednou).

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

const EN: &str = include_str!("../preklady/en.txt");


/// Jazyk zvolený při načtení pluginu.
pub fn jazyk() -> &'static str {
    static J: OnceLock<&'static str> = OnceLock::new();
    J.get_or_init(|| match volba_jazyka().as_str() {
        "cs" => "cs",
        "en" => "en",
        _ => {
            if systemovy_jazyk().starts_with("cs") || systemovy_jazyk().starts_with("sk") {
                "cs"
            } else {
                "en"
            }
        }
    })
}

/// Volba jazyka z nastavení pluginu (`jazyk = auto|cs|en`).
pub fn volba_jazyka() -> String {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(std::path::PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
    };
    let Some(text) = base.and_then(|b| std::fs::read_to_string(b.join("OpenCADStudio").join("teren_nastaveni.txt")).ok()) else {
        return "auto".into();
    };
    text.lines()
        .filter_map(|r| r.split_once('='))
        .find(|(k, _)| k.trim() == "jazyk")
        .map_or_else(|| "auto".into(), |(_, v)| v.trim().to_string())
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
    // primární jazyk: 0x05 čeština, 0x1b slovenština
    let id = unsafe { GetUserDefaultUILanguage() } & 0x3ff;
    Some(match id {
        0x05 => "cs".into(),
        0x1b => "sk".into(),
        _ => "en".into(),
    })
}

pub fn anglicky() -> bool {
    jazyk() == "en"
}

fn tabulka() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m = HashMap::new();
        for r in EN.lines() {
            if r.starts_with('#') {
                continue;
            }
            if let Some((cs, en)) = r.split_once('\t') {
                if !en.is_empty() {
                    m.insert(cs, en);
                }
            }
        }
        m
    })
}

/// Text s `\n` z překladového souboru převedený na skutečné nové řádky.
fn s_radky(s: &'static str) -> &'static str {
    if !s.contains("\\n") {
        return s;
    }
    static C: OnceLock<std::sync::Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();
    let mut c = C.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    c.entry(s).or_insert_with(|| Box::leak(s.replace("\\n", "\n").into_boxed_str()))
}

fn klic(cs: &str) -> String {
    cs.replace('\n', "\\n")
}

/// Přeloží text rozhraní.
pub fn tr(cs: &'static str) -> &'static str {
    if !anglicky() {
        return cs;
    }
    let t = tabulka();
    match t.get(cs) {
        Some(en) => s_radky(en),
        None if cs.contains('\n') => t.get(klic(cs).as_str()).map_or(cs, |en| s_radky(en)),
        None => cs,
    }
}

/// Přeloží text, který není `'static` (např. název vestavěného stylu).
pub fn tr_dyn(cs: &str) -> String {
    if !anglicky() {
        return cs.to_string();
    }
    tabulka().get(klic(cs).as_str()).map_or_else(|| cs.to_string(), |en| en.replace("\\n", "\n"))
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

/// `trf!("Bod {} výška {:.2}", a, b)` – přeložená šablona s argumenty.
#[macro_export]
macro_rules! trf {
    ($s:literal $(, $a:expr)* $(,)?) => {
        $crate::preklad::formatuj($crate::preklad::tr($s), &[$(&$a as &dyn ::std::fmt::Display),*])
    };
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn sablona() {
        assert_eq!(formatuj("Bod {} výška {:.2} m", &[&"5", &12.3456]), "Bod 5 výška 12.35 m");
        assert_eq!(formatuj("{1} a {0}", &[&"a", &"b"]), "b a a");
        assert_eq!(formatuj("{{x}} {:>4}|{:<3}|", &[&7, &"ab"]), "{x}    7|ab |");
    }

    #[test]
    fn soubor_prekladu() {
        // každý řádek: český text, tabulátor, překlad; stejný počet {} v obou
        for (n, r) in EN.lines().enumerate().filter(|(_, r)| !r.starts_with('#') && !r.trim().is_empty()) {
            let (cs, en) = r.split_once('\t').unwrap_or_else(|| panic!("řádek {}: chybí tabulátor", n + 1));
            let pocet = |s: &str| s.replace("{{", "").matches('{').count();
            assert_eq!(pocet(cs), pocet(en), "řádek {}: {cs}", n + 1);
        }
    }
}
