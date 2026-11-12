//! Převod bodů nakreslených ve výkrese (POINT, bloky s atributy, texty
//! výšek a čísel u značky) na body Terénu.
//!
//! Výsledkem je text se záhlavím a tabulátory (souřadnice výkresu), který
//! prochází stejným náhledem jako soubor – body bez výšky jsou v náhledu
//! vidět jako chybné řádky.

use crate::body::{cislo_z_textu, Desetinna, Oddelovac, Sloupec};
use crate::rozpoznani::sloupec_podle_nazvu;

/// Značka bodu (POINT nebo vložený blok).
#[derive(Debug, Clone, Default)]
pub struct Znacka {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Atributy bloku (tag, hodnota).
    pub atributy: Vec<(String, String)>,
}

/// Text ve výkrese (TEXT, MTEXT).
#[derive(Debug, Clone, Default)]
pub struct Text {
    pub x: f64,
    pub y: f64,
    pub vyska: f64,
    pub hodnota: String,
}

/// Co se z čeho vzalo.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Statistika {
    pub znacek: usize,
    pub vyska_z_atributu: usize,
    pub vyska_z_textu: usize,
    pub vyska_ze_znacky: usize,
    pub bez_vysky: usize,
    pub cislo_z_textu: usize,
    /// Použitá tolerance párování [jednotky výkresu].
    pub tolerance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DruhTextu {
    Vyska,
    Cislo,
    Kod,
}

fn druh_textu(t: &str) -> DruhTextu {
    let t = t.trim();
    if (t.contains('.') || t.contains(',')) && cislo_z_textu(t, Oddelovac::Strednik, Desetinna::Auto).is_some() {
        DruhTextu::Vyska
    } else if !t.is_empty() && t.len() <= 12 && t.chars().any(|c| c.is_ascii_digit()) && t.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '/') {
        DruhTextu::Cislo
    } else {
        DruhTextu::Kod
    }
}

/// Text MTEXT bez formátovacích kódů (`{\fArial;…}`, `\P` …).
pub fn cisty_mtext(t: &str) -> String {
    let mut out = String::new();
    let mut zn = t.chars().peekable();
    while let Some(c) = zn.next() {
        match c {
            '{' | '}' => {}
            '\\' => match zn.next() {
                Some('P') | Some('~') => out.push(' '),
                Some('\\') => out.push('\\'),
                Some('{') => out.push('{'),
                Some('}') => out.push('}'),
                // kód s parametrem až po středník (\f…; \H…; \C…; …)
                Some(_) => {
                    for d in zn.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

fn cisty(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ").trim().to_string()
}

/// Převede značky a texty na text bodů. `tolerance` ≤ 0 = 3× medián výšky textu.
pub fn preved(znacky: &[Znacka], texty: &[Text], tolerance: f64) -> (String, Statistika) {
    let tol = if tolerance > 0.0 {
        tolerance
    } else {
        let mut v: Vec<f64> = texty.iter().map(|t| t.vyska).filter(|v| *v > 0.0).collect();
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).map_or(2.0, |m| m * 3.0)
    };
    let mut st = Statistika { znacek: znacky.len(), tolerance: tol, ..Default::default() };
    // vlastnosti z atributů
    struct Bod {
        cislo: Option<String>,
        z: Option<f64>,
        kod: Option<String>,
    }
    let mut body: Vec<Bod> = znacky
        .iter()
        .map(|m| {
            let mut b = Bod { cislo: None, z: None, kod: None };
            for (tag, hodnota) in &m.atributy {
                let h = hodnota.trim();
                if h.is_empty() {
                    continue;
                }
                match sloupec_podle_nazvu(tag) {
                    Some(Sloupec::Cislo) => b.cislo = Some(h.to_string()),
                    Some(Sloupec::Z) => b.z = cislo_z_textu(h, Oddelovac::Strednik, Desetinna::Auto),
                    Some(Sloupec::Popis) => b.kod = Some(h.to_string()),
                    _ => {}
                }
            }
            if b.z.is_some() {
                st.vyska_z_atributu += 1;
            }
            b
        })
        .collect();
    // páry značka–text v toleranci, od nejbližších
    let mut pary: Vec<(f64, usize, usize)> = Vec::new();
    for (i, m) in znacky.iter().enumerate() {
        for (j, t) in texty.iter().enumerate() {
            let d = (m.x - t.x).hypot(m.y - t.y);
            if d <= tol {
                pary.push((d, i, j));
            }
        }
    }
    pary.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut pouzity = vec![false; texty.len()];
    for (_, i, j) in pary {
        if pouzity[j] {
            continue;
        }
        let t = &texty[j];
        let b = &mut body[i];
        match druh_textu(&t.hodnota) {
            DruhTextu::Vyska if b.z.is_none() => {
                b.z = cislo_z_textu(t.hodnota.trim(), Oddelovac::Strednik, Desetinna::Auto);
                st.vyska_z_textu += 1;
            }
            DruhTextu::Cislo if b.cislo.is_none() => {
                b.cislo = Some(t.hodnota.trim().to_string());
                st.cislo_z_textu += 1;
            }
            DruhTextu::Kod if b.kod.is_none() => b.kod = Some(t.hodnota.trim().to_string()),
            _ => continue,
        }
        pouzity[j] = true;
    }
    let mut out = String::from("Point\tEasting\tNorthing\tElevation\tCode\n");
    for (m, b) in znacky.iter().zip(&body) {
        let z = match b.z {
            Some(z) => format!("{z}"),
            None if m.z != 0.0 => {
                st.vyska_ze_znacky += 1;
                format!("{}", m.z)
            }
            None => {
                st.bez_vysky += 1;
                String::new()
            }
        };
        out.push_str(&format!(
            "{}\t{:.4}\t{:.4}\t{}\t{}\n",
            cisty(b.cislo.as_deref().unwrap_or("")),
            m.x,
            m.y,
            z,
            cisty(b.kod.as_deref().unwrap_or(""))
        ));
    }
    (out, st)
}

#[cfg(test)]
mod testy {
    use super::*;
    use crate::body::nacti_body;
    use crate::rozpoznani::rozpoznej;

    fn t(x: f64, y: f64, h: &str) -> Text {
        Text { x, y, vyska: 0.5, hodnota: h.into() }
    }

    #[test]
    fn parovani_textu_nejblizsi() {
        let znacky = vec![Znacka { x: 0.0, y: 0.0, ..Default::default() }, Znacka { x: 10.0, y: 0.0, ..Default::default() }];
        // u každého křížku výška a číslo; texty druhého bodu jsou blíž jemu
        let texty = vec![t(0.4, 0.3, "312,48"), t(0.4, -0.4, "4001"), t(9.6, 0.3, "313.05"), t(9.6, -0.4, "4002"), t(10.5, 0.0, "OB")];
        let (text, st) = preved(&znacky, &texty, 0.0);
        let r: Vec<&str> = text.lines().collect();
        assert_eq!(r[1], "4001\t0.0000\t0.0000\t312.48\t");
        assert_eq!(r[2], "4002\t10.0000\t0.0000\t313.05\tOB");
        assert_eq!((st.vyska_z_textu, st.cislo_z_textu, st.bez_vysky), (2, 2, 0));
        assert_eq!(st.tolerance, 1.5);
        let k = &rozpoznej(&text)[0];
        assert_eq!((k.format.as_str(), k.pocet), ("PENZD", 2));
    }

    #[test]
    fn parovani_mimo_toleranci() {
        let znacky = vec![Znacka { x: 0.0, y: 0.0, ..Default::default() }];
        let (text, st) = preved(&znacky, &[t(5.0, 0.0, "312.48")], 1.0);
        assert_eq!(st.bez_vysky, 1);
        let v = nacti_body(&text, &rozpoznej("Point\tEasting\tNorthing\tElevation\tCode\n1\t0\t0\t1\t\n")[0].format_bodu());
        assert_eq!((v.body.len(), v.chyby.len()), (0, 1));
    }

    #[test]
    fn atribut_podle_tagu_a_vyska_znacky() {
        let znacky = vec![
            Znacka { x: 1.0, y: 2.0, z: 0.0, atributy: vec![("CISLO".into(), "7".into()), ("VYSKA".into(), "305,20".into()), ("KOD".into(), "ŠA".into())] },
            Znacka { x: 3.0, y: 4.0, z: 301.5, atributy: vec![] },
        ];
        let (text, st) = preved(&znacky, &[], 1.0);
        let r: Vec<&str> = text.lines().collect();
        assert_eq!(r[1], "7\t1.0000\t2.0000\t305.2\tŠA");
        assert_eq!(r[2], "\t3.0000\t4.0000\t301.5\t");
        assert_eq!((st.vyska_z_atributu, st.vyska_ze_znacky), (1, 1));
    }

    #[test]
    fn vyska_z_textu_carka_tecka_a_mtext() {
        assert_eq!(druh_textu("312,48"), DruhTextu::Vyska);
        assert_eq!(druh_textu("312.5"), DruhTextu::Vyska);
        assert_eq!(druh_textu("4001"), DruhTextu::Cislo);
        assert_eq!(druh_textu("A12"), DruhTextu::Cislo);
        assert_eq!(druh_textu("okraj cesty"), DruhTextu::Kod);
        assert_eq!(cisty_mtext(r"{\fArial|b0;\H0.5;312,48}"), "312,48");
        assert_eq!(cisty_mtext(r"okraj\Pcesty"), "okraj cesty");
    }
}
