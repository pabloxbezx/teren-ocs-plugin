//! Import COGO bodů z textových souborů.
//!
//! Formát se zadává řetězcem písmen, každé písmeno = jeden sloupec:
//!
//! | písmeno | význam                                                     |
//! |---------|------------------------------------------------------------|
//! | `P`     | číslo bodu                                                  |
//! | `N`     | severní souřadnice (Northing) – souřadnice výkresu, osa y   |
//! | `E`     | východní souřadnice (Easting) – souřadnice výkresu, osa x   |
//! | `Y`     | S-JTSK Y (kladné, jak je dává geodet)                       |
//! | `X`     | S-JTSK X (kladné, jak je dává geodet)                       |
//! | `Z`     | výška                                                       |
//! | `D`     | popis / kód bodu (může obsahovat mezery, pokud je poslední) |
//! | `-`     | sloupec se přeskočí                                         |
//!
//! Sloupce `Y`/`X` se převádějí na souřadnice výkresu podle konvence S-JTSK
//! v CAD výkresech: `E = -Y`, `N = -X`.

use crate::trf;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sloupec {
    Cislo,
    N,
    E,
    SjtskY,
    SjtskX,
    Z,
    Popis,
    Preskocit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Oddelovac {
    /// Jedna nebo více mezer / tabulátorů.
    Mezera,
    Carka,
    Strednik,
    Tabulator,
}

impl Oddelovac {
    pub fn z_nazvu(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mezera" | "space" | " " => Some(Self::Mezera),
            "carka" | "čárka" | "comma" | "," => Some(Self::Carka),
            "strednik" | "středník" | "semicolon" | ";" => Some(Self::Strednik),
            "tab" | "tabulator" | "tabulátor" | "\\t" => Some(Self::Tabulator),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FormatBodu {
    pub sloupce: Vec<Sloupec>,
    pub oddelovac: Oddelovac,
    /// Počet řádků hlavičky, které se přeskočí.
    pub preskocit_radku: usize,
}

impl FormatBodu {
    /// Vytvoří formát z řetězce typu `PNEZD`, `PYXZD`, `ENZ`, `P-NEZD`.
    pub fn z_retezce(vzor: &str, oddelovac: Oddelovac) -> Result<Self, String> {
        let mut sloupce = Vec::new();
        for c in vzor.chars() {
            let s = match c.to_ascii_uppercase() {
                'P' => Sloupec::Cislo,
                'N' => Sloupec::N,
                'E' => Sloupec::E,
                'Y' => Sloupec::SjtskY,
                'X' => Sloupec::SjtskX,
                'Z' => Sloupec::Z,
                'D' => Sloupec::Popis,
                '-' | '_' => Sloupec::Preskocit,
                jiny => return Err(trf!("neznámé písmeno '{}' ve formátu '{}'", jiny, vzor)),
            };
            sloupce.push(s);
        }
        let ma = |s| sloupce.contains(&s);
        let rovina_ne = ma(Sloupec::N) && ma(Sloupec::E);
        let rovina_yx = ma(Sloupec::SjtskY) && ma(Sloupec::SjtskX);
        if !(rovina_ne ^ rovina_yx) {
            return Err(format!(
                "formát '{vzor}' musí obsahovat buď N+E (souřadnice výkresu), nebo Y+X (S-JTSK), ne obojí"
            ));
        }
        if !ma(Sloupec::Z) {
            return Err(trf!("formát '{}' neobsahuje výšku Z", vzor));
        }
        for s in [Sloupec::Cislo, Sloupec::N, Sloupec::E, Sloupec::SjtskY, Sloupec::SjtskX, Sloupec::Z, Sloupec::Popis] {
            if sloupce.iter().filter(|&&x| x == s).count() > 1 {
                return Err(format!("formát '{vzor}' obsahuje sloupec {s:?} vícekrát"));
            }
        }
        Ok(Self { sloupce, oddelovac, preskocit_radku: 0 })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bod {
    pub cislo: String,
    /// Souřadnice výkresu (E).
    pub x: f64,
    /// Souřadnice výkresu (N).
    pub y: f64,
    pub z: f64,
    pub popis: String,
}

impl Bod {
    /// Kód bodu = první slovo popisu (např. `T` z `T okraj cesty`).
    pub fn kod(&self) -> &str {
        self.popis.split_whitespace().next().unwrap_or("")
    }
}

#[derive(Debug, Clone)]
pub struct ChybaRadku {
    pub radek: usize,
    pub text: String,
    pub duvod: String,
}

impl fmt::Display for ChybaRadku {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&trf!("řádek {}: {} ({})", self.radek, self.duvod, self.text.trim()))
    }
}

#[derive(Debug, Default)]
pub struct VysledekImportu {
    pub body: Vec<Bod>,
    pub chyby: Vec<ChybaRadku>,
}

/// Rozdělí řádek na pole podle oddělovače. Je-li popis posledním sloupcem,
/// zbytek řádku (i s mezerami) připadne popisu.
fn rozdel<'a>(radek: &'a str, format: &FormatBodu) -> Vec<&'a str> {
    let n = format.sloupce.len();
    let posledni_popis = format.sloupce.last() == Some(&Sloupec::Popis);
    match format.oddelovac {
        Oddelovac::Mezera => {
            let mut pole = Vec::with_capacity(n);
            let mut zbytek = radek.trim();
            while !zbytek.is_empty() {
                if posledni_popis && pole.len() == n - 1 {
                    pole.push(zbytek);
                    break;
                }
                let konec = zbytek.find(char::is_whitespace).unwrap_or(zbytek.len());
                pole.push(&zbytek[..konec]);
                zbytek = zbytek[konec..].trim_start();
            }
            pole
        }
        jiny => {
            let znak = match jiny {
                Oddelovac::Carka => ',',
                Oddelovac::Strednik => ';',
                _ => '\t',
            };
            let pole: Vec<&str> = if posledni_popis {
                radek.splitn(n, znak).collect()
            } else {
                radek.split(znak).collect()
            };
            pole.into_iter().map(str::trim).collect()
        }
    }
}

fn cislo(s: &str, oddelovac: Oddelovac) -> Option<f64> {
    let s = s.trim();
    // Desetinná čárka je přípustná, pokud čárka není oddělovačem sloupců.
    if oddelovac != Oddelovac::Carka && s.contains(',') && !s.contains('.') {
        s.replace(',', ".").parse().ok()
    } else {
        s.parse().ok()
    }
    .filter(|v: &f64| v.is_finite())
}

/// Načte body z textu. Prázdné řádky a řádky začínající `#` nebo `;`
/// (u oddělovače jiného než středník) se ignorují.
pub fn nacti_body(text: &str, format: &FormatBodu) -> VysledekImportu {
    let mut vysledek = VysledekImportu::default();
    let povinnych = format
        .sloupce
        .iter()
        .rposition(|s| !matches!(s, Sloupec::Popis | Sloupec::Preskocit))
        .map_or(0, |i| i + 1);

    for (i, radek) in text.lines().enumerate().skip(format.preskocit_radku) {
        let cislo_radku = i + 1;
        let orez = radek.trim();
        if orez.is_empty()
            || orez.starts_with('#')
            || (orez.starts_with(';') && format.oddelovac != Oddelovac::Strednik)
        {
            continue;
        }
        let pole = rozdel(radek, format);
        if pole.len() < povinnych {
            vysledek.chyby.push(ChybaRadku {
                radek: cislo_radku,
                text: radek.to_string(),
                duvod: trf!("málo sloupců ({} místo alespoň {})", pole.len(), povinnych),
            });
            continue;
        }

        let mut bod = Bod { cislo: String::new(), x: f64::NAN, y: f64::NAN, z: f64::NAN, popis: String::new() };
        let mut chyba = None;
        for (sloupec, hodnota) in format.sloupce.iter().zip(pole.iter()) {
            let mut num = |cil: &mut f64, jmeno: &str, znamenko: f64| match cislo(hodnota, format.oddelovac) {
                Some(v) => *cil = znamenko * v,
                None => chyba = Some(trf!("neplatná hodnota {} '{}'", jmeno, hodnota)),
            };
            match sloupec {
                Sloupec::Cislo => bod.cislo = hodnota.to_string(),
                Sloupec::E => num(&mut bod.x, "E", 1.0),
                Sloupec::N => num(&mut bod.y, "N", 1.0),
                Sloupec::SjtskY => num(&mut bod.x, "Y", -1.0),
                Sloupec::SjtskX => num(&mut bod.y, "X", -1.0),
                Sloupec::Z => num(&mut bod.z, "Z", 1.0),
                Sloupec::Popis => bod.popis = hodnota.trim().to_string(),
                Sloupec::Preskocit => {}
            }
        }
        match chyba {
            Some(duvod) => vysledek.chyby.push(ChybaRadku { radek: cislo_radku, text: radek.to_string(), duvod }),
            None => vysledek.body.push(bod),
        }
    }
    vysledek
}

/// Výběr bodů podle kódu.
#[derive(Debug, Clone, Default)]
pub struct FiltrKodu {
    /// Je-li neprázdné, berou se jen body s těmito kódy.
    pub zahrnout: Vec<String>,
    /// Body s těmito kódy se vynechají.
    pub vyloucit: Vec<String>,
}

impl FiltrKodu {
    pub fn propusti(&self, bod: &Bod) -> bool {
        let kod = bod.kod();
        let shoda = |seznam: &[String]| seznam.iter().any(|k| k.eq_ignore_ascii_case(kod));
        (self.zahrnout.is_empty() || shoda(&self.zahrnout)) && !shoda(&self.vyloucit)
    }
}

/// Zapíše body do textu v daném formátu (export – opak `nacti_body`).
/// Souřadnice na 3 desetinná místa, desetinná tečka.
pub fn zapis_body(body: &[Bod], format: &FormatBodu) -> String {
    let odd = match format.oddelovac {
        Oddelovac::Mezera => " ",
        Oddelovac::Carka => ",",
        Oddelovac::Strednik => ";",
        Oddelovac::Tabulator => "\t",
    };
    let mut out = String::new();
    for b in body {
        let pole: Vec<String> = format
            .sloupce
            .iter()
            .map(|s| match s {
                Sloupec::Cislo => b.cislo.clone(),
                Sloupec::N => format!("{:.3}", b.y),
                Sloupec::E => format!("{:.3}", b.x),
                Sloupec::SjtskY => format!("{:.3}", -b.x),
                Sloupec::SjtskX => format!("{:.3}", -b.y),
                Sloupec::Z => format!("{:.3}", b.z),
                Sloupec::Popis => b.popis.clone(),
                Sloupec::Preskocit => String::new(),
            })
            .collect();
        out.push_str(pole.join(odd).trim_end());
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod testy {
    use super::*;

    fn pnezd() -> FormatBodu {
        FormatBodu::z_retezce("PNEZD", Oddelovac::Mezera).unwrap()
    }

    #[test]
    fn pnezd_mezera() {
        let t = "1 -1070000.123 -823000.456 345.67 T okraj cesty\n\n# komentar\n2  -1070001.0\t-823001.0  346.0 STR\n";
        let v = nacti_body(t, &pnezd());
        assert!(v.chyby.is_empty(), "{:?}", v.chyby);
        assert_eq!(v.body.len(), 2);
        assert_eq!(v.body[0].x, -823000.456);
        assert_eq!(v.body[0].y, -1070000.123);
        assert_eq!(v.body[0].popis, "T okraj cesty");
        assert_eq!(v.body[0].kod(), "T");
        assert_eq!(v.body[1].kod(), "STR");
    }

    #[test]
    fn bez_popisu_je_ok() {
        let v = nacti_body("5 10 20 30", &pnezd());
        assert_eq!(v.body.len(), 1);
        assert_eq!(v.body[0].popis, "");
    }

    #[test]
    fn sjtsk_kladne_se_otoci() {
        let f = FormatBodu::z_retezce("PYXZD", Oddelovac::Mezera).unwrap();
        let v = nacti_body("1 823000.5 1070000.25 300 T", &f);
        assert_eq!(v.body[0].x, -823000.5);
        assert_eq!(v.body[0].y, -1070000.25);
    }

    #[test]
    fn strednik_a_desetinna_carka() {
        let f = FormatBodu::z_retezce("PENZD", Oddelovac::Strednik).unwrap();
        let v = nacti_body("1;10,5;20,25;300,1;T", &f);
        assert!(v.chyby.is_empty(), "{:?}", v.chyby);
        assert_eq!((v.body[0].x, v.body[0].y, v.body[0].z), (10.5, 20.25, 300.1));
    }

    #[test]
    fn chybne_radky_se_hlasi() {
        let v = nacti_body("1 a 2 3\n2 1 2\n3 1 2 3", &pnezd());
        assert_eq!(v.body.len(), 1);
        assert_eq!(v.chyby.len(), 2);
        assert_eq!(v.chyby[0].radek, 1);
    }

    #[test]
    fn neplatny_format() {
        assert!(FormatBodu::z_retezce("PNZD", Oddelovac::Mezera).is_err());
        assert!(FormatBodu::z_retezce("PNEYXZ", Oddelovac::Mezera).is_err());
        assert!(FormatBodu::z_retezce("PNED", Oddelovac::Mezera).is_err());
    }

    #[test]
    fn export_a_zpet() {
        let body = vec![
            Bod { cislo: "1".into(), x: -823000.5, y: -1070000.25, z: 300.125, popis: "T okraj cesty".into() },
            Bod { cislo: "2".into(), x: -823001.0, y: -1070001.0, z: 301.0, popis: String::new() },
        ];
        for (f, o) in [("PYXZD", Oddelovac::Mezera), ("PNEZD", Oddelovac::Strednik), ("PENZD", Oddelovac::Carka)] {
            let fmt = FormatBodu::z_retezce(f, o).unwrap();
            let text = zapis_body(&body, &fmt);
            let zpet = nacti_body(&text, &fmt);
            assert!(zpet.chyby.is_empty(), "{f}: {:?}", zpet.chyby);
            assert_eq!(zpet.body, body, "{f}");
        }
        let t = zapis_body(&body[..1], &FormatBodu::z_retezce("PYXZD", Oddelovac::Mezera).unwrap());
        assert_eq!(t, "1 823000.500 1070000.250 300.125 T okraj cesty\r\n");
    }

    #[test]
    fn filtr() {
        let b = |p: &str| Bod { cislo: "1".into(), x: 0.0, y: 0.0, z: 0.0, popis: p.into() };
        let f = FiltrKodu { zahrnout: vec![], vyloucit: vec!["STR".into()] };
        assert!(f.propusti(&b("T")));
        assert!(!f.propusti(&b("str x")));
        let f = FiltrKodu { zahrnout: vec!["T".into()], vyloucit: vec![] };
        assert!(!f.propusti(&b("HR")));
    }
}
