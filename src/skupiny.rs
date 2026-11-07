//! Skupiny bodů – výběr bodů podle kritérií.
//!
//! Bod patří do skupiny, když splní aspoň jedno kritérium zahrnutí (všechny
//! body, kód, číslo / rozsah čísel, soubor importu) a žádné kritérium
//! vyloučení (kód, číslo / rozsah čísel).

use crate::preklad::tr;
use crate::trf;
use crate::body::Bod;

/// Identifikátor vestavěné skupiny „Všechny body“.
pub const VSECHNY: &str = "*";
/// Původ ručně vytvořených bodů (místo jména souboru importu).
pub const RUCNE: &str = "(ručně)";

// ── Seznam čísel bodů ─────────────────────────────────────────────────────────

/// Čísla bodů zapsaná jako „1-50, 101, A12“. Číselné rozsahy se slučují.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeznamCisel {
    rozsahy: Vec<(u64, u64)>,
    jine: Vec<String>,
}

fn ciselne(c: &str) -> Option<u64> {
    let c = c.trim();
    if c.is_empty() || !c.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    c.parse().ok()
}

impl SeznamCisel {
    pub fn z_textu(t: &str) -> Self {
        let mut s = Self::default();
        for tok in t.split([',', ';', ' ', '\t', '\n']).map(str::trim).filter(|x| !x.is_empty()) {
            if let Some((a, b)) = tok.split_once('-') {
                if let (Some(a), Some(b)) = (ciselne(a), ciselne(b)) {
                    s.rozsahy.push((a.min(b), a.max(b)));
                    continue;
                }
            }
            match ciselne(tok) {
                Some(n) => s.rozsahy.push((n, n)),
                None => s.jine.push(tok.to_string()),
            }
        }
        s.normalizuj();
        s
    }

    fn normalizuj(&mut self) {
        self.rozsahy.sort();
        let mut out: Vec<(u64, u64)> = Vec::new();
        for (a, b) in self.rozsahy.drain(..) {
            match out.last_mut() {
                Some(p) if a <= p.1.saturating_add(1) => p.1 = p.1.max(b),
                _ => out.push((a, b)),
            }
        }
        self.rozsahy = out;
        self.jine.sort_by_key(|x| x.to_uppercase());
        self.jine.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    }

    pub fn je_prazdny(&self) -> bool {
        self.rozsahy.is_empty() && self.jine.is_empty()
    }

    pub fn obsahuje(&self, cislo: &str) -> bool {
        match ciselne(cislo) {
            Some(n) => self.rozsahy.iter().any(|&(a, b)| a <= n && n <= b),
            None => {
                let c = cislo.trim();
                !c.is_empty() && self.jine.iter().any(|j| j.eq_ignore_ascii_case(c))
            }
        }
    }

    pub fn pridej(&mut self, cisla: &[String]) {
        for c in cisla {
            match ciselne(c) {
                Some(n) => self.rozsahy.push((n, n)),
                None if !c.trim().is_empty() => self.jine.push(c.trim().to_string()),
                None => {}
            }
        }
        self.normalizuj();
    }

    pub fn odeber(&mut self, cisla: &[String]) {
        for c in cisla {
            match ciselne(c) {
                Some(n) => {
                    let mut nove = Vec::new();
                    for &(a, b) in &self.rozsahy {
                        if n < a || n > b {
                            nove.push((a, b));
                        } else {
                            if a < n {
                                nove.push((a, n - 1));
                            }
                            if n < b {
                                nove.push((n + 1, b));
                            }
                        }
                    }
                    self.rozsahy = nove;
                }
                None => self.jine.retain(|j| !j.eq_ignore_ascii_case(c.trim())),
            }
        }
    }

    pub fn text(&self) -> String {
        let mut casti: Vec<String> = self
            .rozsahy
            .iter()
            .map(|&(a, b)| if a == b { a.to_string() } else { format!("{a}-{b}") })
            .collect();
        casti.extend(self.jine.iter().cloned());
        casti.join(", ")
    }
}

// ── Skupina ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkupinaBodu {
    pub id: String,
    pub jmeno: String,
    pub popis: String,
    /// Zahrnout všechny body.
    pub vse: bool,
    /// Zahrnout body s těmito kódy.
    pub kody: Vec<String>,
    /// Zahrnout body s těmito čísly („1-50, 101“).
    pub cisla: String,
    /// Zahrnout body importované z těchto souborů.
    pub zdroje: Vec<String>,
    /// Vyloučit body s těmito kódy.
    pub bez_kodu: Vec<String>,
    /// Vyloučit body s těmito čísly.
    pub bez_cisel: String,
    /// Vyloučit ručně vytvořené body (např. body pro návrhové plochy).
    pub bez_rucnich: bool,
    /// Styl zobrazení bodů skupiny (prázdné = výchozí).
    pub styl: String,
    /// Bod musí splnit všechna pravidla zahrnutí (jinak stačí jedno).
    pub vsechna: bool,
    /// Ručně přidané body (čísla) – mají přednost před pravidly.
    pub pridane: String,
    /// Ručně odebrané body (čísla) – mají přednost před pravidly.
    pub odebrane: String,
}

fn shoda_kodu(seznam: &[String], kod: &str) -> bool {
    !kod.is_empty() && seznam.iter().any(|k| k.eq_ignore_ascii_case(kod))
}

/// Seznam kódů z textu „T, HR; STR“ (velkými písmeny, bez opakování).
pub fn kody_z_textu(t: &str) -> Vec<String> {
    let mut v: Vec<String> =
        t.split([',', ';', ' ', '\t']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_uppercase).collect();
    v.dedup();
    v
}

/// Předpřipravený filtr skupiny (čísla rozparsovaná jednou).
pub struct Filtr<'a> {
    s: &'a SkupinaBodu,
    cisla: SeznamCisel,
    bez_cisel: SeznamCisel,
    pridane: SeznamCisel,
    odebrane: SeznamCisel,
}

/// Proč bod do skupiny patří / nepatří.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Clenstvi {
    /// Splňuje pravidla zahrnutí.
    Pravidla,
    /// Přidaný ručně.
    Pridany,
    /// Vyloučený pravidlem (kód, čísla, ručně vytvořené).
    Vylouceny,
    /// Odebraný ručně.
    Odebrany,
    /// Nesplňuje pravidla zahrnutí.
    Mimo,
}

impl Clenstvi {
    pub fn je_ve_skupine(self) -> bool {
        matches!(self, Clenstvi::Pravidla | Clenstvi::Pridany)
    }
}

impl Filtr<'_> {
    pub fn obsahuje(&self, b: &Bod, zdroj: &str) -> bool {
        self.clenstvi(b, zdroj).je_ve_skupine()
    }

    /// Splní bod jednotlivá pravidla zahrnutí? (kódy, čísla, soubory; `None` = pravidlo nezadané)
    pub fn pravidla(&self, b: &Bod, zdroj: &str) -> [Option<bool>; 3] {
        let s = self.s;
        [
            (!s.kody.is_empty()).then(|| shoda_kodu(&s.kody, b.kod())),
            (!self.cisla.je_prazdny()).then(|| self.cisla.obsahuje(&b.cislo)),
            (!s.zdroje.is_empty()).then(|| !zdroj.is_empty() && s.zdroje.iter().any(|z| z.eq_ignore_ascii_case(zdroj))),
        ]
    }

    pub fn clenstvi(&self, b: &Bod, zdroj: &str) -> Clenstvi {
        let s = self.s;
        if self.odebrane.obsahuje(&b.cislo) {
            return Clenstvi::Odebrany;
        }
        if self.pridane.obsahuje(&b.cislo) {
            return Clenstvi::Pridany;
        }
        let p = self.pravidla(b, zdroj);
        let zadana: Vec<bool> = p.iter().flatten().copied().collect();
        let zahrnout = s.vse
            || if s.vsechna { !zadana.is_empty() && zadana.iter().all(|x| *x) } else { zadana.iter().any(|x| *x) };
        if !zahrnout {
            return Clenstvi::Mimo;
        }
        let kod = b.kod();
        if shoda_kodu(&s.bez_kodu, kod) || self.bez_cisel.obsahuje(&b.cislo) || (s.bez_rucnich && zdroj == RUCNE) {
            return Clenstvi::Vylouceny;
        }
        Clenstvi::Pravidla
    }
}

impl SkupinaBodu {
    /// Vestavěná skupina „Všechny body“.
    pub fn vsechny() -> Self {
        Self { id: VSECHNY.into(), jmeno: tr("Všechny body").into(), vse: true, ..Default::default() }
    }

    pub fn filtr(&self) -> Filtr<'_> {
        Filtr {
            s: self,
            cisla: SeznamCisel::z_textu(&self.cisla),
            bez_cisel: SeznamCisel::z_textu(&self.bez_cisel),
            pridane: SeznamCisel::z_textu(&self.pridane),
            odebrane: SeznamCisel::z_textu(&self.odebrane),
        }
    }

    pub fn obsahuje(&self, b: &Bod, zdroj: &str) -> bool {
        self.filtr().obsahuje(b, zdroj)
    }

    /// Krátký popis kritérií pro seznam.
    pub fn kriteria(&self) -> String {
        let mut c = Vec::new();
        if self.vse {
            c.push(tr("všechny body").to_string());
        }
        if !self.kody.is_empty() {
            c.push(trf!("kódy {}", self.kody.join(", ")));
        }
        if !self.cisla.trim().is_empty() {
            c.push(trf!("čísla {}", self.cisla.trim()));
        }
        if !self.zdroje.is_empty() {
            c.push(format!("import {}", self.zdroje.join(", ")));
        }
        let mut t = if c.is_empty() { tr("žádné kritérium").to_string() } else { c.join("; ") };
        if !self.bez_kodu.is_empty() {
            t.push_str(&trf!(" – bez kódů {}", self.bez_kodu.join(", ")));
        }
        if !self.bez_cisel.trim().is_empty() {
            t.push_str(&trf!(" – bez čísel {}", self.bez_cisel.trim()));
        }
        if self.bez_rucnich {
            t.push_str(tr(" – bez ručně vytvořených"));
        }
        if !self.pridane.trim().is_empty() {
            t.push_str(&trf!(" + ručně {}", self.pridane.trim()));
        }
        if !self.odebrane.trim().is_empty() {
            t.push_str(&trf!(" − ručně {}", self.odebrane.trim()));
        }
        t
    }

    /// Zápis do řetězců `klíč=hodnota` (XDATA). Dlouhé seznamy čísel se dělí.
    pub fn zapis(&self) -> Vec<String> {
        let mut out = vec![
            format!("id={}", self.id),
            format!("jmeno={}", self.jmeno),
            format!("vse={}", self.vse as u8),
            format!("kody={}", self.kody.join(",")),
            format!("bezkodu={}", self.bez_kodu.join(",")),
        ];
        if self.bez_rucnich {
            out.push("bezrucnich=1".into());
        }
        if self.vsechna {
            out.push("vsechna=1".into());
        }
        if !self.styl.is_empty() {
            out.push(format!("styl={}", self.styl));
        }
        if !self.popis.is_empty() {
            out.push(format!("popis={}", self.popis));
        }
        for z in &self.zdroje {
            out.push(format!("zdroj={z}"));
        }
        let mut po_castech = |klic: &str, text: &str| {
            let mut kus = String::new();
            for tok in text.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                if kus.len() + tok.len() > 200 {
                    out.push(format!("{klic}={kus}"));
                    kus.clear();
                }
                if !kus.is_empty() {
                    kus.push(',');
                }
                kus.push_str(tok);
            }
            if !kus.is_empty() {
                out.push(format!("{klic}={kus}"));
            }
        };
        po_castech("cisla", &SeznamCisel::z_textu(&self.cisla).text());
        po_castech("bezcisel", &SeznamCisel::z_textu(&self.bez_cisel).text());
        po_castech("pridane", &SeznamCisel::z_textu(&self.pridane).text());
        po_castech("odebrane", &SeznamCisel::z_textu(&self.odebrane).text());
        out
    }

    pub fn precti<S: AsRef<str>>(radky: &[S]) -> Option<Self> {
        let mut s = Self::default();
        let mut cisla = Vec::new();
        let mut bez = Vec::new();
        let mut pridane = Vec::new();
        let mut odebrane = Vec::new();
        for r in radky {
            let Some((k, v)) = r.as_ref().split_once('=') else { continue };
            match k {
                "id" => s.id = v.into(),
                "jmeno" => s.jmeno = v.into(),
                "popis" => s.popis = v.into(),
                "vse" => s.vse = v == "1",
                "kody" => s.kody = kody_z_textu(v),
                "bezkodu" => s.bez_kodu = kody_z_textu(v),
                "zdroj" => s.zdroje.push(v.into()),
                "bezrucnich" => s.bez_rucnich = v == "1",
                "styl" => s.styl = v.into(),
                "cisla" => cisla.push(v.to_string()),
                "bezcisel" => bez.push(v.to_string()),
                "pridane" => pridane.push(v.to_string()),
                "odebrane" => odebrane.push(v.to_string()),
                "vsechna" => s.vsechna = v == "1",
                _ => {}
            }
        }
        s.cisla = cisla.join(", ");
        s.bez_cisel = bez.join(", ");
        s.pridane = pridane.join(", ");
        s.odebrane = odebrane.join(", ");
        (!s.id.is_empty()).then_some(s)
    }
}

/// Další volné číslo bodu (největší číselné + 1).
pub fn dalsi_cislo<'a>(cisla: impl Iterator<Item = &'a str>) -> u64 {
    cisla.filter_map(ciselne).max().map_or(1, |m| m + 1)
}

#[cfg(test)]
mod testy {
    use super::*;

    fn bod(c: &str, popis: &str) -> Bod {
        Bod { cislo: c.into(), x: 0.0, y: 0.0, z: 0.0, popis: popis.into() }
    }

    #[test]
    fn seznam_cisel() {
        let mut s = SeznamCisel::z_textu("5-10, 1, 2,3 ; 11 A12 7");
        assert_eq!(s.text(), "1-3, 5-11, A12");
        assert!(s.obsahuje("8") && s.obsahuje("008") && s.obsahuje("a12"));
        assert!(!s.obsahuje("4") && !s.obsahuje("") && !s.obsahuje("A1"));
        s.pridej(&["4".into(), "B".into()]);
        assert_eq!(s.text(), "1-11, A12, B");
        s.odeber(&["6".into(), "1".into(), "A12".into()]);
        assert_eq!(s.text(), "2-5, 7-11, B");
    }

    #[test]
    fn kriteria_skupiny() {
        let g = SkupinaBodu {
            id: "G".into(),
            kody: vec!["T".into()],
            cisla: "100-110".into(),
            zdroje: vec!["zamereni.txt".into()],
            bez_kodu: vec!["STR".into()],
            bez_cisel: "105".into(),
            ..Default::default()
        };
        assert!(g.obsahuje(&bod("1", "t okraj"), ""));
        assert!(!g.obsahuje(&bod("1", "HR"), ""));
        assert!(g.obsahuje(&bod("101", "HR"), ""));
        assert!(!g.obsahuje(&bod("105", "HR"), ""));
        assert!(g.obsahuje(&bod("7", "HR"), "Zamereni.TXT"));
        assert!(!g.obsahuje(&bod("7", "STR"), "zamereni.txt"));
        assert!(SkupinaBodu::vsechny().obsahuje(&bod("", ""), ""));
        let z = SkupinaBodu { vse: true, bez_rucnich: true, ..Default::default() };
        assert!(z.obsahuje(&bod("1", ""), "a.txt") && z.obsahuje(&bod("1", ""), ""));
        assert!(!z.obsahuje(&bod("1", ""), RUCNE));
        assert!(SkupinaBodu::precti(&z.zapis().iter().map(|r| r.replace("id=", "id=X")).collect::<Vec<_>>()).unwrap().bez_rucnich);
    }

    #[test]
    fn zapis_a_cteni() {
        let mut g = SkupinaBodu {
            id: "Plosina".into(),
            jmeno: "Plošina, návrh".into(),
            kody: vec!["T".into(), "HR".into()],
            zdroje: vec!["a.txt".into(), "b.txt".into()],
            bez_cisel: "3".into(),
            ..Default::default()
        };
        let cisla: Vec<String> = (1..400).step_by(2).map(|n| n.to_string()).collect();
        g.cisla = cisla.join(", ");
        let z = g.zapis();
        assert!(z.iter().all(|r| r.len() < 230), "{z:?}");
        assert!(z.iter().filter(|r| r.starts_with("cisla=")).count() > 1);
        let zpet = SkupinaBodu::precti(&z).unwrap();
        assert_eq!(SeznamCisel::z_textu(&zpet.cisla), SeznamCisel::z_textu(&g.cisla));
        assert_eq!((zpet.jmeno.as_str(), &zpet.kody, &zpet.zdroje), (g.jmeno.as_str(), &g.kody, &g.zdroje));
        assert_eq!(zpet.bez_cisel, "3");
    }

    #[test]
    fn rucne_a_vsechna() {
        let g = SkupinaBodu {
            id: "G".into(),
            kody: vec!["T".into()],
            cisla: "1-10".into(),
            vsechna: true,
            bez_kodu: vec!["STR".into()],
            pridane: "50, 7".into(),
            odebrane: "5".into(),
            ..Default::default()
        };
        let f = g.filtr();
        assert_eq!(f.clenstvi(&bod("3", "T"), ""), Clenstvi::Pravidla);
        assert_eq!(f.clenstvi(&bod("30", "T"), ""), Clenstvi::Mimo); // jen jedno pravidlo
        assert_eq!(f.clenstvi(&bod("5", "T"), ""), Clenstvi::Odebrany);
        assert_eq!(f.clenstvi(&bod("50", "HR"), ""), Clenstvi::Pridany);
        assert_eq!(f.clenstvi(&bod("7", "STR"), ""), Clenstvi::Pridany); // ručně má přednost
        assert_eq!(f.clenstvi(&bod("8", "STR"), ""), Clenstvi::Mimo);
        let zpet = SkupinaBodu::precti(&g.zapis()).unwrap();
        assert!(zpet.vsechna && SeznamCisel::z_textu(&zpet.pridane).text() == "7, 50" && zpet.odebrane == "5");
    }

    #[test]
    fn dalsi() {
        assert_eq!(dalsi_cislo(["1", "A7", "120", "", "15"].into_iter()), 121);
        assert_eq!(dalsi_cislo(std::iter::empty()), 1);
    }
}
