//! Kontrola zaměření a otisk zdrojů povrchu.

use crate::body::Bod;
use crate::povrch::Definice;
use spade::{DelaunayTriangulation, HasPosition, Point2, Triangulation};

#[derive(Debug, Clone)]
pub struct Podezrely {
    /// Index bodu ve vstupním seznamu.
    pub index: usize,
    /// Odchylka výšky od mediánu okolních bodů [m].
    pub dz: f64,
    /// Výška, kterou by bod měl podle okolí (rovina sousedů).
    pub okoli: f64,
}

struct V {
    p: Point2<f64>,
    i: usize,
}
impl HasPosition for V {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.p
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { 0.5 * (v[n / 2 - 1] + v[n / 2]) }
}

/// Rovina z = a + b·x + c·y proložená body (nejmenší čtverce). `None` u degenerovaného okolí.
fn rovina(b: &[(f64, f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = b.len() as f64;
    let (mx, my, mz) = b.iter().fold((0.0, 0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n, a.2 + p.2 / n));
    let (mut sxx, mut sxy, mut syy, mut sxz, mut syz) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(x, y, z) in b {
        let (dx, dy, dz) = (x - mx, y - my, z - mz);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
        sxz += dx * dz;
        syz += dy * dz;
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() < 1e-9 * (sxx * syy).max(1e-12) {
        return None;
    }
    let bb = (sxz * syy - syz * sxy) / det;
    let cc = (syz * sxx - sxz * sxy) / det;
    Some((mz - bb * mx - cc * my, bb, cc))
}

/// Body, jejichž výška výrazně vybočuje z okolí.
///
/// Okolní body (sousedé v triangulaci) se proloží rovinou, takže se bere
/// v úvahu sklon terénu. Bod je podezřelý, když se od roviny liší víc než
/// o `prah` a zároveň víc než trojnásobek odchylek okolních bodů od téže
/// roviny (robustní odhad MAD) – na členitém terénu se tak práh zvyšuje.
/// Výsledek je seřazený od největší odchylky.
pub fn podezrele_body(body: &[Bod], prah: f64) -> Vec<Podezrely> {
    if body.len() < 4 {
        return Vec::new();
    }
    let (x0, y0) = body.iter().fold((f64::MAX, f64::MAX), |a, b| (a.0.min(b.x), a.1.min(b.y)));
    let mut tin: DelaunayTriangulation<V> = DelaunayTriangulation::new();
    for (i, b) in body.iter().enumerate() {
        let _ = tin.insert(V { p: Point2::new(b.x - x0, b.y - y0), i });
    }
    let mut out = Vec::new();
    for v in tin.vertices() {
        let i = v.data().i;
        let okoli: Vec<(f64, f64, f64)> = v
            .out_edges()
            .map(|e| {
                let b = &body[e.to().data().i];
                (b.x - x0, b.y - y0, b.z)
            })
            .collect();
        if okoli.len() < 4 {
            continue;
        }
        let Some((a, bx, cy)) = rovina(&okoli) else { continue };
        let vyska_roviny = |x: f64, y: f64| a + bx * x + cy * y;
        let mut zbytky: Vec<f64> = okoli.iter().map(|p| (p.2 - vyska_roviny(p.0, p.1)).abs()).collect();
        let rozptyl = median(&mut zbytky) * 1.4826;
        let (px, py) = (body[i].x - x0, body[i].y - y0);
        let ocekavana = vyska_roviny(px, py);
        let dz = body[i].z - ocekavana;
        if dz.abs() > prah && dz.abs() > 3.0 * rozptyl.max(0.05) {
            out.push(Podezrely { index: i, dz, okoli: ocekavana });
        }
    }
    out.sort_by(|a, b| b.dz.abs().total_cmp(&a.dz.abs()));
    out
}

/// Stabilní 64bitový otisk (FNV-1a) – nezávislý na verzi překladače.
#[derive(Clone, Copy)]
pub struct Otisk(u64);

impl Default for Otisk {
    fn default() -> Self {
        Otisk(0xcbf2_9ce4_8422_2325)
    }
}

impl Otisk {
    pub fn bajty(&mut self, b: &[u8]) {
        for &x in b {
            self.0 ^= x as u64;
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    pub fn cislo(&mut self, v: f64) {
        // zaokrouhlení na 0,1 mm, aby šum v posledních bitech nic neměnil
        self.bajty(&((v * 10_000.0).round() as i64).to_le_bytes());
    }
    pub fn text(&mut self, t: &str) {
        self.bajty(t.as_bytes());
        self.bajty(&[0]);
    }
    pub fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

/// Otisk všeho, z čeho povrch vzniká. Shodný otisk = povrch je aktuální.
pub fn otisk_definice(def: &Definice, nastaveni: &str) -> String {
    let mut o = Otisk::default();
    o.text(nastaveni);
    o.text("body");
    for b in &def.body {
        o.cislo(b.x);
        o.cislo(b.y);
        o.cislo(b.z);
    }
    for (nazev, seznam) in [("spojnice", &def.spojnice)] {
        o.text(nazev);
        for c in seznam {
            o.text("|");
            for p in c {
                o.cislo(p[0]);
                o.cislo(p[1]);
                o.cislo(p[2]);
            }
        }
    }
    for (nazev, seznam) in [("hranice", &def.hranice), ("diry", &def.diry)] {
        o.text(nazev);
        for c in seznam {
            o.text("|");
            for p in c {
                o.cislo(p[0]);
                o.cislo(p[1]);
            }
        }
    }
    o.text("editace");
    for e in &def.editace {
        o.text(&e.zapis());
    }
    o.cislo(def.max_hrana.unwrap_or(0.0));
    o.hex()
}

#[cfg(test)]
mod testy {
    use super::*;

    fn mrizka() -> Vec<Bod> {
        let mut v = Vec::new();
        for i in 0..10 {
            for j in 0..10 {
                let (x, y) = (i as f64 * 5.0, j as f64 * 5.0);
                v.push(Bod { cislo: format!("{}", i * 10 + j), x, y, z: 300.0 + 0.05 * x + 0.02 * y, popis: "T".into() });
            }
        }
        v
    }

    #[test]
    fn najde_spicku_a_jamu() {
        let mut b = mrizka();
        b[44].z += 3.0;
        b[77].z -= 1.5;
        let p = podezrele_body(&b, 0.5);
        assert_eq!(p.len(), 2, "{p:?}");
        assert_eq!(p[0].index, 44);
        assert!((p[0].dz - 3.0).abs() < 0.2);
        assert_eq!(p[1].index, 77);
    }

    #[test]
    fn svah_bez_falesnych_poplachu() {
        // strmý svah 30 % a mírné údolí – nic nemá být podezřelé
        let mut v = Vec::new();
        for i in 0..15 {
            for j in 0..15 {
                let (x, y) = (i as f64 * 5.0, j as f64 * 5.0);
                let udoli = 0.02 * (y - 35.0).powi(2);
                v.push(Bod { cislo: format!("{i}_{j}"), x, y, z: 300.0 + 0.3 * x + udoli, popis: "T".into() });
            }
        }
        assert!(podezrele_body(&v, 0.5).is_empty());
        v[100].z += 1.2;
        let p = podezrele_body(&v, 0.5);
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].index, 100);
    }

    #[test]
    fn hladky_teren_bez_podezrelych() {
        assert!(podezrele_body(&mrizka(), 0.3).is_empty());
    }

    #[test]
    fn otisk_reaguje_na_zmenu() {
        let def = Definice { body: mrizka(), ..Default::default() };
        let a = otisk_definice(&def, "VRSTP");
        assert_eq!(a, otisk_definice(&def, "VRSTP"));
        let mut d2 = def.clone();
        d2.body[3].z += 0.001;
        assert_ne!(a, otisk_definice(&d2, "VRSTP"));
        assert_ne!(a, otisk_definice(&def, "TIN"));
    }
}
