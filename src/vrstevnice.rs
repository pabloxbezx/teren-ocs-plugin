//! Vrstevnice z TIN povrchu.
//!
//! Každý trojúhelník se protne vodorovnou rovinou na úrovni vrstevnice,
//! úsečky se pak pospojují přes společné hrany trojúhelníků do lomených čar.
//! Výška vrcholu přesně na úrovni vrstevnice se bere o nepatrnou hodnotu
//! výše, aby vrstevnice nikdy neprocházela vrcholem.

use crate::povrch::Povrch;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct NastaveniVrstevnic {
    /// Interval vedlejších vrstevnic [m].
    pub interval: f64,
    /// Interval hlavních vrstevnic [m]; měl by být násobkem `interval`.
    pub hlavni: f64,
}

#[derive(Debug, Clone)]
pub struct Vrstevnice {
    pub vyska: f64,
    pub hlavni: bool,
    pub uzavrena: bool,
    /// Globální souřadnice vrcholů (x = E, y = N).
    pub body: Vec<(f64, f64)>,
}

const EPS_Z: f64 = 1e-7;

/// Klíč bodu vrstevnice = hrana TIN (seřazené indexy vrcholů).
type Hrana = (usize, usize);

pub fn vrstevnice(povrch: &Povrch, nastaveni: &NastaveniVrstevnic) -> Vec<Vrstevnice> {
    assert!(nastaveni.interval > 0.0, "interval vrstevnic musí být kladný");
    let stat = povrch.statistika();
    if stat.pocet_trojuhelniku == 0 {
        return Vec::new();
    }
    let k_min = (stat.z_min / nastaveni.interval).ceil() as i64;
    let k_max = (stat.z_max / nastaveni.interval).floor() as i64;

    // úsečky po úrovních: úroveň -> seznam (hrana_a, hrana_b)
    let mut usecky: HashMap<i64, Vec<(Hrana, Hrana)>> = HashMap::new();
    let z_eff = |z: f64, uroven: f64| if (z - uroven).abs() < EPS_Z { uroven + EPS_Z } else { z };

    for t in &povrch.trojuhelniky {
        let zt = t.map(|i| povrch.vrcholy[i].z);
        let lo = ((zt[0].min(zt[1]).min(zt[2])) / nastaveni.interval).ceil() as i64 - 1;
        let hi = ((zt[0].max(zt[1]).max(zt[2])) / nastaveni.interval).floor() as i64 + 1;
        for k in lo.max(k_min)..=hi.min(k_max) {
            let uroven = k as f64 * nastaveni.interval;
            let z = zt.map(|z| z_eff(z, uroven));
            let mut prusecik = Vec::with_capacity(2);
            for (p, q) in [(0, 1), (1, 2), (2, 0)] {
                if (z[p] - uroven) * (z[q] - uroven) < 0.0 {
                    let (a, b) = (t[p].min(t[q]), t[p].max(t[q]));
                    prusecik.push((a, b));
                }
            }
            if prusecik.len() == 2 {
                usecky.entry(k).or_default().push((prusecik[0], prusecik[1]));
            }
        }
    }

    let mut vysledek = Vec::new();
    let mut urovne: Vec<i64> = usecky.keys().copied().collect();
    urovne.sort_unstable();
    for k in urovne {
        let uroven = k as f64 * nastaveni.interval;
        let hlavni = {
            let r = uroven / nastaveni.hlavni;
            (r - r.round()).abs() < 1e-6
        };
        let bod = |h: Hrana| -> (f64, f64) {
            // Vždy se počítá od vrcholu s nižším indexem → shodný bod z obou trojúhelníků.
            let (a, b) = (povrch.vrcholy[h.0], povrch.vrcholy[h.1]);
            let (za, zb) = (z_eff(a.z, uroven), z_eff(b.z, uroven));
            let t = (uroven - za) / (zb - za);
            (a.x + t * (b.x - a.x) + povrch.pocatek.0, a.y + t * (b.y - a.y) + povrch.pocatek.1)
        };
        for (retez, uzavrena) in spoj(&usecky[&k]) {
            vysledek.push(Vrstevnice {
                vyska: uroven,
                hlavni,
                uzavrena,
                body: retez.into_iter().map(bod).collect(),
            });
        }
    }
    vysledek
}

/// Spojí úsečky do řetězců. Vrací (seznam hran, uzavřená).
fn spoj(usecky: &[(Hrana, Hrana)]) -> Vec<(Vec<Hrana>, bool)> {
    let mut sousede: HashMap<Hrana, Vec<usize>> = HashMap::new();
    for (i, (a, b)) in usecky.iter().enumerate() {
        sousede.entry(*a).or_default().push(i);
        sousede.entry(*b).or_default().push(i);
    }
    let mut pouzita = vec![false; usecky.len()];
    let mut retezy = Vec::new();

    let prochazej = |start_usecka: usize, start: Hrana, pouzita: &mut Vec<bool>| -> (Vec<Hrana>, bool) {
        let mut retez = vec![start];
        let mut aktualni = start;
        let mut usecka = Some(start_usecka);
        while let Some(u) = usecka {
            pouzita[u] = true;
            let (a, b) = usecky[u];
            let dalsi = if a == aktualni { b } else { a };
            retez.push(dalsi);
            aktualni = dalsi;
            usecka = sousede[&aktualni].iter().copied().find(|&j| !pouzita[j]);
        }
        let uzavrena = retez.len() > 2 && retez.first() == retez.last();
        if uzavrena {
            retez.pop();
        }
        (retez, uzavrena)
    };

    // Nejdřív otevřené vrstevnice (začínají na okraji povrchu – hrana s jedinou úsečkou).
    let mut okraje: Vec<Hrana> = sousede.iter().filter(|(_, v)| v.len() == 1).map(|(h, _)| *h).collect();
    okraje.sort_unstable();
    for h in okraje {
        let u = sousede[&h][0];
        if !pouzita[u] {
            retezy.push(prochazej(u, h, &mut pouzita));
        }
    }
    // Zbytek jsou uzavřené smyčky.
    for u in 0..usecky.len() {
        if !pouzita[u] {
            let start = usecky[u].0;
            retezy.push(prochazej(u, start, &mut pouzita));
        }
    }
    retezy
}

#[cfg(test)]
mod testy {
    use super::*;
    use crate::body::Bod;
    use crate::povrch::{NastaveniPovrchu, Povrch};

    fn povrch(f: impl Fn(f64, f64) -> f64, n: usize, krok: f64) -> Povrch {
        let mut body = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (i as f64 * krok, j as f64 * krok);
                body.push(Bod { cislo: format!("{i}_{j}"), x: x - 823000.0, y: y - 1070000.0, z: f(x, y), popis: "T".into() });
            }
        }
        let vyber: Vec<usize> = (0..body.len()).collect();
        Povrch::z_bodu(&body, &vyber, &NastaveniPovrchu::default()).unwrap()
    }

    #[test]
    fn naklonena_rovina() {
        // z = 300 + 0.1 x, x ∈ <0, 50> → z ∈ <300, 305>
        let p = povrch(|x, _| 300.0 + 0.1 * x, 11, 5.0);
        let v = vrstevnice(&p, &NastaveniVrstevnic { interval: 0.5, hlavni: 2.5 });
        // úrovně 300.5 … 305.0 = 10, každá jedna otevřená čára
        // (300.0 leží přesně na okrajové hraně povrchu, ta se nekreslí)
        assert_eq!(v.len(), 10);
        for c in &v {
            assert!(!c.uzavrena);
            let x_ocek = (c.vyska - 300.0) / 0.1 - 823000.0;
            for &(x, _) in &c.body {
                assert!((x - x_ocek).abs() < 1e-5, "vrstevnice {} x={x} očekáváno {x_ocek}", c.vyska);
            }
            let ys: Vec<f64> = c.body.iter().map(|b| b.1).collect();
            let rozsah = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - ys.iter().cloned().fold(f64::INFINITY, f64::min);
            assert!((rozsah - 50.0).abs() < 1e-6, "vrstevnice musí jít přes celý povrch");
        }
        let hlavni: Vec<f64> = v.iter().filter(|c| c.hlavni).map(|c| c.vyska).collect();
        assert_eq!(hlavni, vec![302.5, 305.0]);
    }

    #[test]
    fn kopec_uzavrene() {
        // kužel s vrcholem uprostřed
        let p = povrch(|x, y| 310.0 - ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt() * 0.1, 21, 5.0);
        let v = vrstevnice(&p, &NastaveniVrstevnic { interval: 1.0, hlavni: 5.0 });
        let uzavrene: Vec<&Vrstevnice> = v.iter().filter(|c| c.uzavrena).collect();
        assert!(!uzavrene.is_empty());
        for c in uzavrene {
            // poloměr ~ (310 - h) / 0.1
            let r_ocek = (310.0 - c.vyska) / 0.1;
            for &(x, y) in &c.body {
                let r = ((x + 823000.0 - 50.0).powi(2) + (y + 1070000.0 - 50.0).powi(2)).sqrt();
                assert!((r - r_ocek).abs() < 1.0, "h={} r={r} ocek={r_ocek}", c.vyska);
            }
        }
    }

    #[test]
    fn vrchol_presne_na_urovni() {
        // všechny výšky jsou celé metry → vrstevnice prochází vrcholy, nesmí se rozpadnout
        let p = povrch(|x, _| (300.0 + x / 5.0).round(), 6, 5.0);
        let v = vrstevnice(&p, &NastaveniVrstevnic { interval: 1.0, hlavni: 5.0 });
        for c in &v {
            assert!(c.body.len() >= 2);
        }
        let mut urovne: Vec<f64> = v.iter().map(|c| c.vyska).collect();
        urovne.dedup();
        assert_eq!(urovne.len(), v.len(), "každá úroveň má být jedna souvislá čára");
    }
}
