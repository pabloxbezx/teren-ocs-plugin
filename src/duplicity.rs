//! Duplicitní čísla bodů při importu: v souboru i proti bodům ve výkrese.
//!
//! Čísla se porovnávají bez ohledu na velikost písmen, body bez čísla
//! nejsou nikdy duplicitní.

use crate::body::Bod;
use std::collections::{HashMap, HashSet};

/// Co udělat s bodem, jehož číslo už existuje.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Politika {
    /// Ponechat bod ve výkrese; z dvojic v souboru vzít první.
    #[default]
    Preskocit,
    /// Aktualizovat bod ve výkrese podle souboru; v souboru platí poslední.
    Prepsat,
    /// Duplicitám dát nová čísla od zadaného.
    Precislovat(u64),
    /// Importovat i tak (dvě stejná čísla).
    Ponechat,
}

impl Politika {
    pub fn klic(self) -> &'static str {
        match self {
            Politika::Preskocit => "preskocit",
            Politika::Prepsat => "prepsat",
            Politika::Precislovat(_) => "precislovat",
            Politika::Ponechat => "ponechat",
        }
    }
    pub fn z_klice(s: &str, od: u64) -> Self {
        match s.trim() {
            "prepsat" => Politika::Prepsat,
            "precislovat" => Politika::Precislovat(od),
            "ponechat" => Politika::Ponechat,
            _ => Politika::Preskocit,
        }
    }
}

fn klic(c: &str) -> String {
    c.trim().to_lowercase()
}

/// Nalezené duplicity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Duplicity {
    /// Indexy bodů, jejichž číslo se v souboru už vyskytlo dřív.
    pub v_souboru: Vec<usize>,
    /// (index nového bodu, index existujícího bodu) se shodným číslem.
    pub ve_vykresu: Vec<(usize, usize)>,
}

impl Duplicity {
    pub fn pocet(&self) -> usize {
        self.v_souboru.len() + self.ve_vykresu.len()
    }
    /// Druh duplicity bodu (pro zvýraznění v náhledu).
    pub fn je_v_souboru(&self, i: usize) -> bool {
        self.v_souboru.contains(&i)
    }
    pub fn ve_vykresu_pro(&self, i: usize) -> Option<usize> {
        self.ve_vykresu.iter().find(|(n, _)| *n == i).map(|(_, e)| *e)
    }
}

pub fn najdi(nove: &[Bod], existujici: &[Bod]) -> Duplicity {
    let mut d = Duplicity::default();
    let ve_vykresu: HashMap<String, usize> =
        existujici.iter().enumerate().filter(|(_, b)| !b.cislo.trim().is_empty()).map(|(i, b)| (klic(&b.cislo), i)).collect();
    let mut videno = HashSet::new();
    for (i, b) in nove.iter().enumerate() {
        if b.cislo.trim().is_empty() {
            continue;
        }
        let k = klic(&b.cislo);
        if !videno.insert(k.clone()) {
            d.v_souboru.push(i);
        } else if let Some(&e) = ve_vykresu.get(&k) {
            d.ve_vykresu.push((i, e));
        }
    }
    d
}

/// Výsledek uplatnění politiky.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Nové body do výkresu.
    pub pridat: Vec<Bod>,
    /// (index existujícího bodu, nová podoba) – přepsat ve výkrese.
    pub prepsat: Vec<(usize, Bod)>,
    /// Kolik bodů se vynechá.
    pub preskoceno: usize,
    /// (původní číslo, nové číslo) přečíslovaných bodů.
    pub precislovano: Vec<(String, String)>,
}

/// Nejmenší volné číslo ≥ `od`, které není mezi obsazenými.
fn volne(od: &mut u64, obsazeno: &mut HashSet<String>) -> String {
    loop {
        let c = od.to_string();
        *od += 1;
        if obsazeno.insert(c.clone()) {
            return c;
        }
    }
}

pub fn naplanuj(nove: &[Bod], existujici: &[Bod], politika: Politika) -> Plan {
    let d = najdi(nove, existujici);
    let mut plan = Plan::default();
    let mut obsazeno: HashSet<String> =
        existujici.iter().chain(nove.iter()).map(|b| klic(&b.cislo)).filter(|c| !c.is_empty()).collect();
    let mut dalsi = match politika {
        Politika::Precislovat(od) => od.max(1),
        _ => 1,
    };
    // u Přepsat platí v souboru poslední výskyt čísla
    let mut posledni: HashMap<String, usize> = HashMap::new();
    if politika == Politika::Prepsat {
        for (i, b) in nove.iter().enumerate() {
            if !b.cislo.trim().is_empty() {
                posledni.insert(klic(&b.cislo), i);
            }
        }
    }
    for (i, b) in nove.iter().enumerate() {
        let v_souboru = d.je_v_souboru(i);
        let vykres = d.ve_vykresu_pro(i);
        match politika {
            Politika::Ponechat => plan.pridat.push(b.clone()),
            _ if !v_souboru && vykres.is_none() => {
                if politika == Politika::Prepsat && posledni.get(&klic(&b.cislo)).is_some_and(|&p| p != i) {
                    // první výskyt přebije pozdější řádek se stejným číslem
                    plan.preskoceno += 1;
                } else {
                    plan.pridat.push(b.clone());
                }
            }
            Politika::Preskocit => plan.preskoceno += 1,
            Politika::Prepsat => {
                let k = klic(&b.cislo);
                if posledni.get(&k) != Some(&i) {
                    plan.preskoceno += 1;
                    continue;
                }
                // poslední výskyt čísla: přepíše bod ve výkrese, nebo se přidá
                let e = vykres.or_else(|| existujici.iter().position(|x| klic(&x.cislo) == k));
                match e {
                    Some(e) => {
                        plan.prepsat.push((e, b.clone()));
                    }
                    None => plan.pridat.push(b.clone()),
                }
            }
            Politika::Precislovat(_) => {
                let mut n = b.clone();
                n.cislo = volne(&mut dalsi, &mut obsazeno);
                plan.precislovano.push((b.cislo.clone(), n.cislo.clone()));
                plan.pridat.push(n);
            }
        }
    }
    plan
}

#[cfg(test)]
mod testy {
    use super::*;

    fn b(c: &str, x: f64) -> Bod {
        Bod { cislo: c.into(), x, y: 0.0, z: 0.0, popis: String::new() }
    }

    fn soubor() -> Vec<Bod> {
        vec![b("1", 1.0), b("2", 2.0), b("2", 22.0), b("3", 3.0), b("", 9.0), b("", 9.5)]
    }
    fn vykres() -> Vec<Bod> {
        vec![b("3", 30.0), b("10", 100.0)]
    }

    #[test]
    fn najde_v_souboru_i_ve_vykresu() {
        let d = najdi(&soubor(), &vykres());
        assert_eq!(d.v_souboru, vec![2]);
        assert_eq!(d.ve_vykresu, vec![(3, 0)]);
        assert_eq!(d.pocet(), 2);
        // velikost písmen se nerozlišuje
        assert_eq!(najdi(&[b("A1", 0.0), b("a1", 0.0)], &[]).v_souboru, vec![1]);
    }

    #[test]
    fn duplicity_preskocit_prvni_plati() {
        let p = naplanuj(&soubor(), &vykres(), Politika::Preskocit);
        assert_eq!(p.pridat.iter().map(|b| b.x).collect::<Vec<_>>(), vec![1.0, 2.0, 9.0, 9.5]);
        assert_eq!(p.preskoceno, 2);
        assert!(p.prepsat.is_empty());
    }

    #[test]
    fn duplicity_proti_vykresu_prepsat() {
        let p = naplanuj(&soubor(), &vykres(), Politika::Prepsat);
        // v souboru platí poslední „2“, bod 3 přepíše bod ve výkrese
        assert_eq!(p.pridat.iter().map(|b| b.x).collect::<Vec<_>>(), vec![1.0, 22.0, 9.0, 9.5]);
        assert_eq!(p.prepsat, vec![(0, b("3", 3.0))]);
        assert_eq!(p.preskoceno, 1);
    }

    #[test]
    fn precislovat_od_volneho_bez_kolizi() {
        let p = naplanuj(&soubor(), &vykres(), Politika::Precislovat(9));
        // 9 je volné, 10 je ve výkrese → 11
        assert_eq!(p.precislovano, vec![("2".into(), "9".into()), ("3".into(), "11".into())]);
        assert_eq!(p.pridat.len(), 6);
        let cisla: Vec<&str> = p.pridat.iter().map(|b| b.cislo.as_str()).collect();
        assert_eq!(cisla, vec!["1", "2", "9", "11", "", ""]);
    }

    #[test]
    fn ponechat_je_dosavadni_chovani() {
        let p = naplanuj(&soubor(), &vykres(), Politika::Ponechat);
        assert_eq!(p.pridat, soubor());
        assert_eq!(p.preskoceno, 0);
    }

    #[test]
    fn aktualizace_opakovaneho_importu() {
        // druhý import téhož souboru: změněný bod se přepíše, nový přidá
        let stary = vec![b("1", 1.0), b("2", 2.0)];
        let novy = vec![b("1", 1.0), b("2", 2.5), b("3", 3.0)];
        let p = naplanuj(&novy, &stary, Politika::Prepsat);
        assert_eq!(p.prepsat, vec![(0, b("1", 1.0)), (1, b("2", 2.5))]);
        assert_eq!(p.pridat, vec![b("3", 3.0)]);
    }

    #[test]
    fn klice_politiky() {
        for p in [Politika::Preskocit, Politika::Prepsat, Politika::Precislovat(5), Politika::Ponechat] {
            assert_eq!(Politika::z_klice(p.klic(), 5), p);
        }
    }
}
