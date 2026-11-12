//! Automatické rozpoznání formátu souboru bodů a kontrola importu.
//!
//! Soubor se zkusmo načte ve všech podporovaných formátech a oddělovačích.
//! Kromě pevných vzorů (PNEZD, PYXZD…) se zkouší i pořadí sloupců podle
//! názvů v záhlaví a podle obsahu sloupců (rozsahy S-JTSK, výšky, čísla
//! bodů) – tak se načte i soubor s nadbytečnými sloupci (datum, kvalita…).
//! Vítězí kombinace, ve které se načte nejvíc řádků a body leží v rozsahu
//! S-JTSK pro Českou republiku. Rozsahy Y (≈ 430–905 km) a X (≈ 935–1230 km)
//! se nepřekrývají, takže se tím spolehlivě pozná i prohození N a E.

use crate::body::{cislo_z_textu, nacti_body, rozdel_radek, Bod, Desetinna, FormatBodu, Oddelovac, Sloupec, VysledekImportu};
use crate::preklad::tr;
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

/// Odkud se vzalo pořadí sloupců kandidáta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Puvod {
    Zahlavi,
    Vzor,
    Obsah,
}

#[derive(Debug, Clone)]
pub struct Kandidat {
    /// Vzor formátu (např. `PYXZD`, `P-YXZ`).
    pub format: String,
    pub sloupce: Vec<Sloupec>,
    pub oddelovac: Oddelovac,
    pub hlavicka: usize,
    pub pocet: usize,
    pub chyby: usize,
    pub v_cr: usize,
    pub podezrele_z: usize,
    /// Desetinný oddělovač, který soubor používá (Tečka/Čárka).
    pub desetinna: Desetinna,
    pub puvod: Puvod,
}

impl Kandidat {
    /// Body leží v ČR (S-JTSK) – alespoň 90 % bodů.
    pub fn sjtsk(&self) -> bool {
        self.pocet > 0 && self.v_cr * 10 >= self.pocet * 9
    }
    fn skore(&self) -> (i64, i64, i64) {
        let celkem = (self.pocet + self.chyby).max(1) as i64;
        // bod s nesmyslnou výškou se nepočítá jako načtený (jinak by vzor,
        // který kontroluje méně sloupců, vyhrál u souboru s chybnými řádky)
        let dobre = self.pocet.saturating_sub(self.podezrele_z) as i64;
        (
            (dobre * 1000) / celkem,
            (self.v_cr as i64 * 1000) / (self.pocet.max(1) as i64),
            -((self.podezrele_z as i64 * 1000) / (self.pocet.max(1) as i64)),
        )
    }
    pub fn format_bodu(&self) -> FormatBodu {
        let mut f = FormatBodu::ze_sloupcu(self.sloupce.clone(), self.oddelovac).expect("platný formát");
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

pub fn nazev_desetinne(d: Desetinna) -> &'static str {
    match d {
        Desetinna::Auto => tr("automaticky"),
        Desetinna::Tecka => tr("tečka"),
        Desetinna::Carka => tr("čárka"),
    }
}

/// Řádky s daty (bez prázdných a komentářů) od řádku `od`: (číslo řádku od 1, text).
fn radky_dat(text: &str, oddelovac: Oddelovac, od: usize) -> impl Iterator<Item = (usize, &str)> {
    text.lines().enumerate().skip(od).filter_map(move |(i, l)| {
        let t = l.trim();
        let komentar = t.is_empty() || t.starts_with('#') || (t.starts_with(';') && oddelovac != Oddelovac::Strednik);
        (!komentar).then_some((i + 1, l))
    })
}

/// Počet úvodních řádků (max. 5), které ve formátu nejdou přečíst.
fn hlavicka_pro(text: &str, f: &FormatBodu) -> usize {
    let mut f = f.clone();
    f.preskocit_radku = 0;
    let prvni = nacti_body(text, &f);
    if prvni.body.is_empty() {
        return 0;
    }
    let radky_chyb: Vec<usize> = prvni.chyby.iter().map(|c| c.radek).collect();
    let mut hlavicka = 0;
    for (r, _) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()).map(|(i, l)| (i + 1, l)).take(5) {
        if radky_chyb.contains(&r) {
            hlavicka = r;
        } else {
            break;
        }
    }
    hlavicka
}

/// Desetinný oddělovač, který soubor ve sloupcích souřadnic používá.
pub fn desetinna_v_textu(text: &str, f: &FormatBodu) -> Desetinna {
    let (mut carka, mut tecka) = (0, 0);
    let ciselne: Vec<usize> = f
        .sloupce
        .iter()
        .enumerate()
        .filter(|(_, s)| !matches!(s, Sloupec::Cislo | Sloupec::Popis | Sloupec::Preskocit))
        .map(|(i, _)| i)
        .collect();
    for (_, l) in radky_dat(text, f.oddelovac, f.preskocit_radku).take(200) {
        let pole = rozdel_radek(l, f.oddelovac, f.sloupce.len(), f.sloupce.last() == Some(&Sloupec::Popis));
        for &i in &ciselne {
            if let Some(p) = pole.get(i) {
                if p.contains(',') && f.oddelovac != Oddelovac::Carka {
                    carka += 1;
                } else if p.contains('.') {
                    tecka += 1;
                }
            }
        }
    }
    if carka > tecka { Desetinna::Carka } else { Desetinna::Tecka }
}

/// Vyhodnotí formát na textu (včetně rozpoznání hlavičky).
pub fn s_formatem(text: &str, f: &FormatBodu, puvod: Puvod) -> Kandidat {
    let mut f = f.clone();
    f.preskocit_radku = hlavicka_pro(text, &f);
    let v = nacti_body(text, &f);
    Kandidat {
        format: f.vzor(),
        sloupce: f.sloupce.clone(),
        oddelovac: f.oddelovac,
        hlavicka: f.preskocit_radku,
        pocet: v.body.len(),
        chyby: v.chyby.len(),
        v_cr: v.body.iter().filter(|b| v_cr(b.x, b.y)).count(),
        podezrele_z: v.body.iter().filter(|b| podezrela_vyska(b.z)).count(),
        desetinna: desetinna_v_textu(text, &f),
        puvod,
    }
}

fn bez_diakritiky(t: &str) -> String {
    const Z: &str = "áčďéěíňóřšťúůýžÁČĎÉĚÍŇÓŘŠŤÚŮÝŽ";
    const NA: &str = "acdeeinorstuuyzACDEEINORSTUUYZ";
    t.chars().map(|c| Z.chars().position(|x| x == c).and_then(|i| NA.chars().nth(i)).unwrap_or(c)).collect()
}

/// Role sloupce podle názvu v záhlaví (česky i anglicky).
pub fn sloupec_podle_nazvu(nazev: &str) -> Option<Sloupec> {
    let n: String = bez_diakritiky(nazev.trim().trim_matches('"')).to_lowercase().replace([' ', '.', '_', '-'], "");
    Some(match n.as_str() {
        "c" | "cislo" | "cislobodu" | "cb" | "bod" | "point" | "pointid" | "pt" | "id" | "name" | "jmeno" | "nazev" | "pnt" | "p" => Sloupec::Cislo,
        "y" | "ysjtsk" | "sjtsky" => Sloupec::SjtskY,
        "x" | "xsjtsk" | "sjtskx" => Sloupec::SjtskX,
        "e" | "easting" | "east" | "vychod" => Sloupec::E,
        "n" | "northing" | "north" | "sever" => Sloupec::N,
        "z" | "h" | "vyska" | "elevation" | "elev" | "height" | "alt" | "altitude" | "hbpv" | "zbpv" => Sloupec::Z,
        "kod" | "code" | "popis" | "description" | "desc" | "poznamka" | "pozn" | "note" | "kodbodu" | "feature" => Sloupec::Popis,
        _ => return None,
    })
}

/// Pořadí sloupců podle záhlaví (poslední řádek hlavičky). Neznámé názvy se
/// přeskočí; `None`, když záhlaví nedává platný formát.
pub fn sloupce_podle_zahlavi(radek: &str, oddelovac: Oddelovac) -> Option<Vec<Sloupec>> {
    let pole = rozdel_radek(radek, oddelovac, 0, false);
    if pole.len() < 3 || pole.iter().any(|p| cislo_z_textu(p, oddelovac, Desetinna::Auto).is_some()) {
        return None;
    }
    let mut sloupce: Vec<Sloupec> = pole.iter().map(|p| sloupec_podle_nazvu(p).unwrap_or(Sloupec::Preskocit)).collect();
    // opakovaný název – platí první výskyt
    for i in 0..sloupce.len() {
        if sloupce[i] != Sloupec::Preskocit && sloupce[..i].contains(&sloupce[i]) {
            sloupce[i] = Sloupec::Preskocit;
        }
    }
    FormatBodu::ze_sloupcu(sloupce, oddelovac).ok().map(|f| f.sloupce)
}

/// Pořadí sloupců podle obsahu: sloupce v rozsahu S-JTSK (kladně Y/X nebo
/// záporně E/N), výška, číslo bodu a popis; ostatní se přeskočí. Funguje jen
/// pro S-JTSK (u místních souřadnic nejde role z hodnot poznat).
pub fn sloupce_podle_obsahu(text: &str, oddelovac: Oddelovac, hlavicka: usize) -> Option<Vec<Sloupec>> {
    let radky: Vec<Vec<&str>> =
        radky_dat(text, oddelovac, hlavicka).take(300).map(|(_, l)| rozdel_radek(l, oddelovac, 0, false)).collect();
    if radky.is_empty() {
        return None;
    }
    // nejčastější počet sloupců
    let mut cetnost: HashMap<usize, usize> = HashMap::new();
    for r in &radky {
        *cetnost.entry(r.len()).or_default() += 1;
    }
    let n = cetnost.into_iter().max_by_key(|&(n, c)| (c, n)).map(|(n, _)| n)?;
    if n < 3 {
        return None;
    }
    let hodnoty: Vec<Vec<Option<f64>>> = (0..n)
        .map(|j| radky.iter().map(|r| r.get(j).and_then(|p| cislo_z_textu(p, oddelovac, Desetinna::Auto))).collect())
        .collect();
    let podil = |j: usize, f: &dyn Fn(f64) -> bool| {
        let v = &hodnoty[j];
        v.iter().filter(|h| h.is_some_and(f)).count() * 10 >= v.len() * 9
    };
    let najdi = |f: &dyn Fn(f64) -> bool, obsazeno: &[usize]| (0..n).find(|j| !obsazeno.contains(j) && podil(*j, f));
    let mut role = vec![Sloupec::Preskocit; n];
    let mut obsazeno = Vec::new();
    let ry = |v: f64| (425_000.0..=910_000.0).contains(&v);
    let rx = |v: f64| (930_000.0..=1_235_000.0).contains(&v);
    let (a, b) = match (najdi(&ry, &[]), najdi(&rx, &[])) {
        (Some(y), Some(x)) => {
            role[y] = Sloupec::SjtskY;
            role[x] = Sloupec::SjtskX;
            (y, x)
        }
        _ => match (najdi(&|v| ry(-v), &[]), najdi(&|v| rx(-v), &[])) {
            (Some(e), Some(nn)) => {
                role[e] = Sloupec::E;
                role[nn] = Sloupec::N;
                (e, nn)
            }
            _ => return None,
        },
    };
    obsazeno.extend([a, b]);
    // výška: první číselný sloupec za souřadnicemi s rozumnými hodnotami, jinak kdekoli
    let za = a.max(b);
    let vyska = |v: f64| !podezrela_vyska(v);
    let z = (za + 1..n).find(|j| podil(*j, &vyska)).or_else(|| najdi(&vyska, &obsazeno))?;
    role[z] = Sloupec::Z;
    obsazeno.push(z);
    // číslo bodu: první sloupec před souřadnicemi (číslo nebo krátký text)
    let pred = a.min(b);
    if let Some(c) = (0..pred).find(|j| !obsazeno.contains(j)) {
        role[c] = Sloupec::Cislo;
        obsazeno.push(c);
    }
    // popis: poslední sloupec s textem (ne číslem)
    if let Some(d) = (0..n).rev().find(|j| {
        !obsazeno.contains(j) && hodnoty[*j].iter().filter(|h| h.is_none()).count() * 2 > hodnoty[*j].len()
    }) {
        role[d] = Sloupec::Popis;
    }
    FormatBodu::ze_sloupcu(role, oddelovac).ok().map(|f| f.sloupce)
}

/// Vyhodnotí všechny formáty a oddělovače. Vrací kandidáty, kteří načetli
/// aspoň jeden bod, seřazené od nejlepšího.
pub fn rozpoznej(text: &str) -> Vec<Kandidat> {
    let mut v: Vec<Kandidat> = Vec::new();
    for &o in ODDELOVACE.iter() {
        let pridej = |sloupce: Vec<Sloupec>, puvod: Puvod, v: &mut Vec<Kandidat>| {
            if v.iter().any(|k| k.oddelovac == o && k.sloupce == sloupce) {
                return;
            }
            if let Ok(f) = FormatBodu::ze_sloupcu(sloupce, o) {
                let k = s_formatem(text, &f, puvod);
                if k.pocet > 0 {
                    v.push(k);
                }
            }
        };
        // záhlaví: první neprázdný řádek
        if let Some(s) = text.lines().find(|l| !l.trim().is_empty()).and_then(|l| sloupce_podle_zahlavi(l.trim_start_matches('#'), o)) {
            pridej(s, Puvod::Zahlavi, &mut v);
        }
        for &f in FORMATY.iter() {
            let f = FormatBodu::z_retezce(f, o).expect("platný formát");
            pridej(f.sloupce, Puvod::Vzor, &mut v);
        }
        for hl in 0..=1 {
            if let Some(s) = sloupce_podle_obsahu(text, o, hl) {
                pridej(s, Puvod::Obsah, &mut v);
            }
        }
    }
    // oddělovač, který je skoro na každém řádku (tabulátor, středník), má
    // při shodě přednost před mezerou
    let datove: Vec<&str> = radky_dat(text, Oddelovac::Mezera, 0).take(200).map(|(_, l)| l).collect();
    let vsude = |c: char| !datove.is_empty() && datove.iter().filter(|l| l.contains(c)).count() * 10 >= datove.len() * 8;
    let prednost = |o: Oddelovac| match o {
        Oddelovac::Tabulator if vsude('\t') => 0,
        Oddelovac::Strednik if vsude(';') => 0,
        o => 1 + ODDELOVACE.iter().position(|&x| x == o).unwrap_or(9),
    };
    let poradi = |k: &Kandidat| {
        (
            prednost(k.oddelovac),
            match k.puvod {
                Puvod::Zahlavi => 0,
                Puvod::Vzor => 1 + FORMATY.iter().position(|&f| f == k.format).unwrap_or(9),
                Puvod::Obsah => 20,
            },
            k.sloupce.iter().filter(|s| **s == Sloupec::Preskocit).count(),
        )
    };
    v.sort_by(|a, b| b.skore().cmp(&a.skore()).then(poradi(a).cmp(&poradi(b))));
    v
}

/// Oprava pořadí nebo znaménka souřadnic, kterou stojí za to nabídnout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navrh {
    /// Y a X (N a E) jsou prohozené.
    Prohodit,
    /// Sloupce Y/X obsahují souřadnice výkresu (záporné) nebo naopak.
    Znamenko,
    /// Obojí.
    ProhoditZnamenko,
}

fn uprav_sloupce(sloupce: &[Sloupec], n: Navrh) -> Vec<Sloupec> {
    sloupce
        .iter()
        .map(|&s| match (n, s) {
            (Navrh::Prohodit, Sloupec::SjtskY) => Sloupec::SjtskX,
            (Navrh::Prohodit, Sloupec::SjtskX) => Sloupec::SjtskY,
            (Navrh::Prohodit, Sloupec::E) => Sloupec::N,
            (Navrh::Prohodit, Sloupec::N) => Sloupec::E,
            (Navrh::Znamenko, Sloupec::SjtskY) => Sloupec::E,
            (Navrh::Znamenko, Sloupec::SjtskX) => Sloupec::N,
            (Navrh::Znamenko, Sloupec::E) => Sloupec::SjtskY,
            (Navrh::Znamenko, Sloupec::N) => Sloupec::SjtskX,
            (Navrh::ProhoditZnamenko, Sloupec::SjtskY) => Sloupec::N,
            (Navrh::ProhoditZnamenko, Sloupec::SjtskX) => Sloupec::E,
            (Navrh::ProhoditZnamenko, Sloupec::E) => Sloupec::SjtskX,
            (Navrh::ProhoditZnamenko, Sloupec::N) => Sloupec::SjtskY,
            (_, s) => s,
        })
        .collect()
}

/// Když většina bodů leží mimo ČR a po prohození / změně znaménka by ležela
/// v S-JTSK, vrátí návrh a opravené pořadí sloupců.
pub fn navrh_opravy(body: &[Bod], sloupce: &[Sloupec]) -> Option<(Navrh, Vec<Sloupec>)> {
    let n = body.len();
    if n == 0 || body.iter().filter(|b| v_cr(b.x, b.y)).count() * 2 > n {
        return None;
    }
    let pocet = |f: &dyn Fn(&Bod) -> bool| body.iter().filter(|b| f(b)).count();
    [
        (Navrh::Prohodit, pocet(&|b| v_cr(b.y, b.x))),
        (Navrh::Znamenko, pocet(&|b| v_cr(-b.x, -b.y))),
        (Navrh::ProhoditZnamenko, pocet(&|b| v_cr(-b.y, -b.x))),
    ]
    .into_iter()
    .filter(|(_, c)| c * 10 >= n * 9)
    .max_by_key(|(_, c)| *c)
    .map(|(navrh, _)| (navrh, uprav_sloupce(sloupce, navrh)))
}

/// Body daleko od ostatních (překlep v souřadnici): víc než 20× medián
/// vzdálenosti od středu a zároveň víc než 500 m. Vrací indexy.
pub fn odlehle(body: &[Bod]) -> Vec<usize> {
    if body.len() < 4 {
        return Vec::new();
    }
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let cx = median(body.iter().map(|b| b.x).collect());
    let cy = median(body.iter().map(|b| b.y).collect());
    let d: Vec<f64> = body.iter().map(|b| (b.x - cx).hypot(b.y - cy)).collect();
    let md = median(d.clone()).max(1.0);
    d.iter().enumerate().filter(|(_, v)| **v > 20.0 * md && **v > 500.0).map(|(i, _)| i).collect()
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
    /// Indexy bodů daleko od ostatních.
    pub odlehle: Vec<usize>,
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
        odlehle: odlehle(&v.body),
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
        assert_eq!((k.format.as_str(), k.oddelovac, k.hlavicka), ("PYXZD", Oddelovac::Strednik, 1));
        assert_eq!(k.desetinna, Desetinna::Carka);
        assert!(k.sjtsk());
    }

    #[test]
    fn carka_csv() {
        let t = "1,-1070000.1,-823000.4,345.6,T\n2,-1070010.0,-823005.0,346.1,T\n";
        let k = &rozpoznej(t)[0];
        assert_eq!((k.format.as_str(), k.oddelovac), ("PNEZD", Oddelovac::Carka));
        assert_eq!(k.desetinna, Desetinna::Tecka);
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
    fn yxzd_bez_cisla() {
        let t = "823000.4 1070000.1 345.6\n823005.0 1070010.0 346.1\n";
        assert_eq!(rozpoznej(t)[0].format, "YXZD");
    }

    #[test]
    fn ignorovany_sloupec_datum_podle_obsahu() {
        // GNSS: číslo, datum, Y, X, Z, PDOP, kód
        let t = "1;2026-11-03;745210.12;1043520.45;312.48;1.8;T teren\n2;2026-11-03;745215.80;1043518.02;312.61;1.9;OB\n3;2026-11-03;745221.33;1043516.90;312.75;2.1;OB\n";
        let k = &rozpoznej(t)[0];
        assert_eq!(k.format, "P-YXZ-D");
        assert_eq!((k.pocet, k.chyby), (3, 0));
        assert!(k.sjtsk());
    }

    #[test]
    fn zahlavi_cz_en_nazvy() {
        let o = Oddelovac::Strednik;
        assert_eq!(sloupce_podle_zahlavi("Číslo;Y;X;Z;Kód", o).map(|s| FormatBodu::ze_sloupcu(s, o).unwrap().vzor()), Some("PYXZD".into()));
        assert_eq!(
            sloupce_podle_zahlavi("Point;Easting;Northing;Elevation;Code", o).map(|s| FormatBodu::ze_sloupcu(s, o).unwrap().vzor()),
            Some("PENZD".into())
        );
        assert_eq!(
            sloupce_podle_zahlavi("č.b.;datum;Y;X;H;pozn.", o).map(|s| FormatBodu::ze_sloupcu(s, o).unwrap().vzor()),
            Some("P-YXZD".into())
        );
        assert_eq!(sloupce_podle_zahlavi("1;745210;1043520;312", o), None);
    }

    #[test]
    fn zahlavi_urci_poradi_mistni_soustavy() {
        // místní soustava: podle obsahu nejde poznat, záhlaví rozhodne Z před E/N
        let t = "bod,vyska,east,north\n1,10.5,100.0,200.0\n2,11.0,110.0,205.0\n";
        let k = &rozpoznej(t)[0];
        assert_eq!((k.format.as_str(), k.hlavicka, k.pocet), ("PZEN", 1, 2));
    }

    #[test]
    fn pevny_format_rozpozna_hlavicku() {
        let t = "Číslo;Y;X;Z;Kód\n1;823000,456;1070000,123;345,67;T\n";
        let f = FormatBodu::z_retezce("PYXZD", Oddelovac::Strednik).unwrap();
        let k = s_formatem(t, &f, Puvod::Vzor);
        assert_eq!((k.hlavicka, k.pocet, k.chyby), (1, 1, 0));
    }

    #[test]
    fn prohozene_yx_navrh() {
        let f = FormatBodu::z_retezce("PYXZ", Oddelovac::Mezera).unwrap();
        let v = nacti_body("1 1043520.4 745210.1 312\n2 1043518.0 745215.8 313\n", &f);
        let (n, s) = navrh_opravy(&v.body, &f.sloupce).unwrap();
        assert_eq!(n, Navrh::Prohodit);
        assert_eq!(FormatBodu::ze_sloupcu(s, Oddelovac::Mezera).unwrap().vzor(), "PXYZ");
    }

    #[test]
    fn zaporne_sjtsk_navrh() {
        let f = FormatBodu::z_retezce("PYXZ", Oddelovac::Mezera).unwrap();
        let v = nacti_body("1 -745210.1 -1043520.4 312\n2 -745215.8 -1043518.0 313\n", &f);
        let (n, s) = navrh_opravy(&v.body, &f.sloupce).unwrap();
        assert_eq!(n, Navrh::Znamenko);
        assert_eq!(FormatBodu::ze_sloupcu(s, Oddelovac::Mezera).unwrap().vzor(), "PENZ");
        let v = nacti_body("1 -1043520.4 -745210.1 312\n", &f);
        assert_eq!(navrh_opravy(&v.body, &f.sloupce).unwrap().0, Navrh::ProhoditZnamenko);
        // správně zadané body návrh nedostanou
        let v = nacti_body("1 745210.1 1043520.4 312\n", &f);
        assert!(navrh_opravy(&v.body, &f.sloupce).is_none());
    }

    #[test]
    fn odlehly_bod() {
        let b = |x: f64, y: f64| Bod { cislo: String::new(), x, y, z: 0.0, popis: String::new() };
        let body = vec![b(-745210.0, -1043520.0), b(-745215.0, -1043518.0), b(-745221.0, -1043516.0), b(-74521.0, -1043512.0), b(-745226.0, -1043515.0)];
        assert_eq!(odlehle(&body), vec![3]);
        assert!(odlehle(&body[..3]).is_empty());
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
