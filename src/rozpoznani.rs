//! Automatické rozpoznání formátu souboru bodů a kontrola importu.
//!
//! Soubor se zkusmo načte ve všech podporovaných formátech a oddělovačích.
//! Vítězí kombinace, ve které se načte nejvíc řádků a body leží v rozsahu
//! S-JTSK pro Českou republiku. Rozsahy Y (≈ 430–905 km) a X (≈ 935–1230 km)
//! se nepřekrývají, takže se tím spolehlivě pozná i prohození N a E.

use crate::preklad::tr;
use crate::body::{nacti_body, Bod, FormatBodu, Oddelovac, VysledekImportu};
use std::collections::HashMap;

/// Formáty, které se zkoušejí, v pořadí přednosti při shodném výsledku.
pub const FORMATY: [&str; 6] = ["PNEZD", "PENZD", "PYXZD", "NEZD", "ENZD", "YXZD"];
pub const ODDELOVACE: [Oddelovac; 4] = [Oddelovac::Mezera, Oddelovac::Strednik, Oddelovac::Tabulator, Oddelovac::Carka];

/// Rozsah S-JTSK pro ČR v souřadnicích výkresu (E = −Y, N = −X), s rezervou.
pub fn v_cr(x: f64, y: f64) -> bool {
    (-910_000.0..=-425_000.0).contains(&x) && (-1_235_000.0..=-930_000.0).contains(&y)
}

/// Výška, která v ČR nedává smysl (podezřelé prohození sloupců).
pub fn podezrela_vyska(z: f64) -> bool {
    !(-50.0..=1700.0).contains(&z)
}

#[derive(Debug, Clone)]
pub struct Kandidat {
    pub format: &'static str,
    pub oddelovac: Oddelovac,
    pub hlavicka: usize,
    pub pocet: usize,
    pub chyby: usize,
    pub v_cr: usize,
    pub podezrele_z: usize,
}

impl Kandidat {
    /// Body leží v ČR (S-JTSK) – alespoň 90 % bodů.
    pub fn sjtsk(&self) -> bool {
        self.pocet > 0 && self.v_cr * 10 >= self.pocet * 9
    }
    fn skore(&self) -> (i64, i64, i64) {
        let celkem = (self.pocet + self.chyby).max(1) as i64;
        (
            (self.pocet as i64 * 1000) / celkem,
            (self.v_cr as i64 * 1000) / (self.pocet.max(1) as i64),
            -((self.podezrele_z as i64 * 1000) / (self.pocet.max(1) as i64)),
        )
    }
    pub fn format_bodu(&self) -> FormatBodu {
        let mut f = FormatBodu::z_retezce(self.format, self.oddelovac).expect("platný formát");
        f.preskocit_radku = self.hlavicka;
        f
    }
}

pub fn nazev_oddelovace(o: Oddelovac) -> &'static str {
    match o {
        Oddelovac::Mezera => tr("mezera"),
        Oddelovac::Carka => tr("čárka"),
        Oddelovac::Strednik => tr("středník"),
        Oddelovac::Tabulator => tr("tabulátor"),
    }
}

fn vyhodnot(text: &str, format: &'static str, oddelovac: Oddelovac) -> Kandidat {
    let mut f = FormatBodu::z_retezce(format, oddelovac).expect("platný formát");
    // hlavička: počet úvodních řádků (max. 5), které se nenačtou
    let prvni = nacti_body(text, &f);
    let mut hlavicka = 0;
    if !prvni.body.is_empty() {
        let radky_chyb: Vec<usize> = prvni.chyby.iter().map(|c| c.radek).collect();
        let neprazdne: Vec<usize> = text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, _)| i + 1)
            .collect();
        for r in neprazdne.iter().take(5) {
            if radky_chyb.contains(r) {
                hlavicka = *r;
            } else {
                break;
            }
        }
    }
    f.preskocit_radku = hlavicka;
    let v = nacti_body(text, &f);
    Kandidat {
        format,
        oddelovac,
        hlavicka,
        pocet: v.body.len(),
        chyby: v.chyby.len(),
        v_cr: v.body.iter().filter(|b| v_cr(b.x, b.y)).count(),
        podezrele_z: v.body.iter().filter(|b| podezrela_vyska(b.z)).count(),
    }
}

/// Vyhodnotí všechny formáty a oddělovače. Vrací kandidáty, kteří načetli
/// aspoň jeden bod, seřazené od nejlepšího.
pub fn rozpoznej(text: &str) -> Vec<Kandidat> {
    let mut v: Vec<Kandidat> = Vec::new();
    for &o in ODDELOVACE.iter() {
        for &f in FORMATY.iter() {
            let k = vyhodnot(text, f, o);
            if k.pocet > 0 {
                v.push(k);
            }
        }
    }
    let poradi = |k: &Kandidat| {
        (
            ODDELOVACE.iter().position(|&o| o == k.oddelovac).unwrap_or(9),
            FORMATY.iter().position(|&f| f == k.format).unwrap_or(9),
        )
    };
    v.sort_by(|a, b| b.skore().cmp(&a.skore()).then(poradi(a).cmp(&poradi(b))));
    v
}

/// Souhrn importu pro kontrolu před vložením do výkresu.
#[derive(Debug, Clone)]
pub struct Souhrn {
    pub pocet: usize,
    pub chyby: usize,
    pub e: (f64, f64),
    pub n: (f64, f64),
    pub z: (f64, f64),
    pub mimo_cr: usize,
    pub podezrele_z: usize,
    /// Čísla bodů, která se v souboru opakují.
    pub duplicitni_cisla: Vec<String>,
}

pub fn souhrn(v: &VysledekImportu) -> Souhrn {
    let mut s = Souhrn {
        pocet: v.body.len(),
        chyby: v.chyby.len(),
        e: (f64::INFINITY, f64::NEG_INFINITY),
        n: (f64::INFINITY, f64::NEG_INFINITY),
        z: (f64::INFINITY, f64::NEG_INFINITY),
        mimo_cr: 0,
        podezrele_z: 0,
        duplicitni_cisla: Vec::new(),
    };
    let mut cisla: HashMap<&str, usize> = HashMap::new();
    for b in &v.body {
        let rozsir = |r: &mut (f64, f64), h: f64| {
            r.0 = r.0.min(h);
            r.1 = r.1.max(h);
        };
        rozsir(&mut s.e, b.x);
        rozsir(&mut s.n, b.y);
        rozsir(&mut s.z, b.z);
        if !v_cr(b.x, b.y) {
            s.mimo_cr += 1;
        }
        if podezrela_vyska(b.z) {
            s.podezrele_z += 1;
        }
        if !b.cislo.is_empty() {
            *cisla.entry(b.cislo.as_str()).or_default() += 1;
        }
    }
    let mut dup: Vec<String> = cisla.into_iter().filter(|(_, n)| *n > 1).map(|(c, _)| c.to_string()).collect();
    dup.sort();
    s.duplicitni_cisla = dup;
    s
}

/// Body, které by se ve výkrese překrývaly s již existujícími (do 1 mm).
pub fn shodne_s_existujicimi(nove: &[Bod], existujici: &[Bod]) -> usize {
    let mut mrizka: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let k = |x: f64, y: f64| ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64);
    for (i, b) in existujici.iter().enumerate() {
        mrizka.entry(k(b.x, b.y)).or_default().push(i);
    }
    nove.iter().filter(|b| mrizka.contains_key(&k(b.x, b.y))).count()
}

#[cfg(test)]
mod testy {
    use super::*;

    const PNEZD: &str = "1 -1070000.123 -823000.456 345.67 T okraj\n2 -1070010.0 -823005.0 346.10 T\n3 -1070020.5 -823002.0 347.00 HR\n";

    #[test]
    fn pnezd_mezera() {
        let k = rozpoznej(PNEZD);
        assert_eq!(k[0].format, "PNEZD");
        assert_eq!(k[0].oddelovac, Oddelovac::Mezera);
        assert!(k[0].sjtsk());
        assert_eq!(k[0].pocet, 3);
    }

    #[test]
    fn penzd_se_pozna_podle_rozsahu() {
        let t = "1 -823000.456 -1070000.123 345.67 T\n2 -823005.0 -1070010.0 346.1 T\n";
        assert_eq!(rozpoznej(t)[0].format, "PENZD");
    }

    #[test]
    fn geodet_kladne_yx_strednik_hlavicka() {
        let t = "cislo;Y;X;Z;kod\n1;823000,456;1070000,123;345,67;T\n2;823005,0;1070010,0;346,1;HR\n";
        let k = &rozpoznej(t)[0];
        assert_eq!((k.format, k.oddelovac, k.hlavicka), ("PYXZD", Oddelovac::Strednik, 1));
        assert!(k.sjtsk());
    }

    #[test]
    fn carka_csv() {
        let t = "1,-1070000.1,-823000.4,345.6,T\n2,-1070010.0,-823005.0,346.1,T\n";
        let k = &rozpoznej(t)[0];
        assert_eq!((k.format, k.oddelovac), ("PNEZD", Oddelovac::Carka));
    }

    #[test]
    fn bez_cisel_bodu() {
        let t = "-1070000.1 -823000.4 345.6\n-1070010.0 -823005.0 346.1\n";
        assert_eq!(rozpoznej(t)[0].format, "NEZD");
    }

    #[test]
    fn mistni_soustava_neni_sjtsk() {
        let t = "1 100.0 200.0 10.0\n2 110.0 205.0 11.0\n";
        let k = &rozpoznej(t)[0];
        assert_eq!(k.format, "PNEZD");
        assert!(!k.sjtsk());
    }

    #[test]
    fn souhrn_duplicity() {
        let f = FormatBodu::z_retezce("PNEZD", Oddelovac::Mezera).unwrap();
        let v = nacti_body("1 -1070000 -823000 300\n1 -1070001 -823001 300\n2 10 10 5000\n", &f);
        let s = souhrn(&v);
        assert_eq!(s.duplicitni_cisla, vec!["1".to_string()]);
        assert_eq!(s.mimo_cr, 1);
        assert_eq!(s.podezrele_z, 1);
    }
}
