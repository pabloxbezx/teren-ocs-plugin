//! TIN povrch – omezená Delaunayova triangulace.
//!
//! Povrch vzniká z *definice*:
//! body, povinné spojnice (zlomové hrany), vnější hranice, díry a seznam
//! editací, které se při každém přepočtu aplikují znovu v uvedeném pořadí.
//!
//! Výpočet probíhá v lokálních souřadnicích posunutých o `pocatek`, aby se
//! s velkými souřadnicemi S-JTSK (~10^6 m) nepracovalo na hranici přesnosti.

use crate::preklad::tr;
use crate::trf;
use crate::body::Bod;
use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy)]
pub struct Vrchol {
    /// Lokální souřadnice (vůči `Povrch::pocatek`).
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Index bodu ve vstupním seznamu (`None` = vrchol ze spojnice nebo editace).
    pub bod: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct NastaveniPovrchu {
    /// Maximální délka hrany trojúhelníku (2D). `None` = bez omezení.
    pub max_hrana: Option<f64>,
    /// Tolerance pro shodné body v půdorysu [m].
    pub tolerance_duplicit: f64,
}

impl Default for NastaveniPovrchu {
    fn default() -> Self {
        Self { max_hrana: None, tolerance_duplicit: 0.001 }
    }
}

/// Úprava povrchu uložená v definici. Souřadnice jsou globální.
#[derive(Debug, Clone, PartialEq)]
pub enum Editace {
    /// Přidá vrchol povrchu (bod se nepřidává mezi COGO body).
    PridatBod([f64; 3]),
    /// Vyřadí z povrchu vrchol na této poloze.
    OdebratBod([f64; 2]),
    /// Otočí hranu mezi dvěma vrcholy na druhou úhlopříčku.
    OtocitHranu([f64; 2], [f64; 2]),
    /// Odstraní trojúhelník, který obsahuje tento bod.
    SmazatTrojuhelnik([f64; 2]),
    /// Vynutí hranu mezi dvěma vrcholy.
    PridatHranu([f64; 2], [f64; 2]),
    /// Odstraní oba trojúhelníky u hrany mezi dvěma vrcholy.
    SmazatHranu([f64; 2], [f64; 2]),
}

impl Editace {
    /// Textový zápis pro uložení do výkresu.
    pub fn zapis(&self) -> String {
        match self {
            Editace::PridatBod([x, y, z]) => format!("PB;{x};{y};{z}"),
            Editace::OdebratBod([x, y]) => format!("OB;{x};{y}"),
            Editace::OtocitHranu([x1, y1], [x2, y2]) => format!("OH;{x1};{y1};{x2};{y2}"),
            Editace::SmazatTrojuhelnik([x, y]) => format!("ST;{x};{y}"),
            Editace::PridatHranu([x1, y1], [x2, y2]) => format!("PH;{x1};{y1};{x2};{y2}"),
            Editace::SmazatHranu([x1, y1], [x2, y2]) => format!("SH;{x1};{y1};{x2};{y2}"),
        }
    }

    pub fn precti(s: &str) -> Option<Self> {
        let mut it = s.split(';');
        let typ = it.next()?;
        let c: Vec<f64> = it.map(|v| v.parse().ok()).collect::<Option<_>>()?;
        Some(match (typ, c.as_slice()) {
            ("PB", [x, y, z]) => Editace::PridatBod([*x, *y, *z]),
            ("OB", [x, y]) => Editace::OdebratBod([*x, *y]),
            ("OH", [a, b, c, d]) => Editace::OtocitHranu([*a, *b], [*c, *d]),
            ("ST", [x, y]) => Editace::SmazatTrojuhelnik([*x, *y]),
            ("PH", [a, b, c, d]) => Editace::PridatHranu([*a, *b], [*c, *d]),
            ("SH", [a, b, c, d]) => Editace::SmazatHranu([*a, *b], [*c, *d]),
            _ => return None,
        })
    }

    pub fn popis(&self) -> &'static str {
        match self {
            Editace::PridatBod(_) => tr("přidání bodu"),
            Editace::OdebratBod(_) => tr("odebrání bodu"),
            Editace::OtocitHranu(..) => tr("otočení hrany"),
            Editace::SmazatTrojuhelnik(_) => tr("smazání trojúhelníku"),
            Editace::PridatHranu(..) => tr("přidání hrany"),
            Editace::SmazatHranu(..) => tr("smazání hrany"),
        }
    }

    /// Místo úpravy (pro ukázání ve výkrese).
    pub fn poloha(&self) -> [f64; 2] {
        match self {
            Editace::PridatBod([x, y, _]) => [*x, *y],
            Editace::OdebratBod(p) | Editace::SmazatTrojuhelnik(p) => *p,
            Editace::OtocitHranu(a, b) | Editace::PridatHranu(a, b) | Editace::SmazatHranu(a, b) => {
                [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
            }
        }
    }
}

/// Vše, z čeho se povrch počítá. Souřadnice jsou globální.
#[derive(Debug, Clone, Default)]
pub struct Definice {
    pub body: Vec<Bod>,
    /// Povinné spojnice – 3D lomené čáry.
    pub spojnice: Vec<Vec<[f64; 3]>>,
    /// Vnější hranice – uzavřené polygony (povrch se ořízne na jejich vnitřek).
    pub hranice: Vec<Vec<[f64; 2]>>,
    /// Díry – uzavřené polygony, uvnitř se povrch vynechá.
    pub diry: Vec<Vec<[f64; 2]>>,
    pub editace: Vec<Editace>,
    pub max_hrana: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Duplicita {
    /// Index bodu, který byl vynechán.
    pub vynechany: usize,
    /// Index bodu, který zůstal v povrchu.
    pub ponechany: usize,
    pub rozdil_z: f64,
}

/// Co se při výpočtu nepovedlo nebo na co je dobré upozornit.
#[derive(Debug, Clone, Default)]
pub struct Hlaseni {
    /// Úseky spojnic, které nešlo vložit, protože kříží jinou spojnici
    /// (globální souřadnice začátku a konce).
    pub krizeni_spojnic: Vec<([f64; 2], [f64; 2])>,
    /// Editace, které už nemají na co působit (např. hrana neexistuje).
    pub nepouzite_editace: Vec<usize>,
    /// Vrchol spojnice ležící na bodě, ale s jinou výškou (bere se výška bodu).
    pub rozdilne_vysky: usize,
}

#[derive(Debug, Clone)]
pub struct Povrch {
    pub pocatek: (f64, f64),
    pub vrcholy: Vec<Vrchol>,
    /// Trojúhelníky jako indexy do `vrcholy`, orientace proti směru hodin.
    pub trojuhelniky: Vec<[usize; 3]>,
    /// Hrany povinných spojnic (seřazené indexy vrcholů).
    pub povinne_hrany: HashSet<(usize, usize)>,
    pub duplicity: Vec<Duplicita>,
    /// Počet trojúhelníků odstraněných kvůli maximální délce hrany.
    pub odstraneno_dlouhych: usize,
    pub hlaseni: Hlaseni,
}

#[derive(Debug, Clone, Copy)]
struct V {
    p: Point2<f64>,
    idx: usize,
}

impl HasPosition for V {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.p
    }
}

#[derive(Debug)]
pub struct ChybaPovrchu(pub String);

impl std::fmt::Display for ChybaPovrchu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Vyhledávání vrcholů podle polohy s tolerancí.
struct Mrizka {
    tol: f64,
    bunky: HashMap<(i64, i64), Vec<usize>>,
}

impl Mrizka {
    fn new(tol: f64) -> Self {
        Self { tol: tol.max(1e-6), bunky: HashMap::new() }
    }
    fn bunka(&self, x: f64, y: f64) -> (i64, i64) {
        ((x / self.tol).floor() as i64, (y / self.tol).floor() as i64)
    }
    fn najdi(&self, vrcholy: &[Vrchol], x: f64, y: f64) -> Option<usize> {
        let (cx, cy) = self.bunka(x, y);
        let mut nejlepsi: Option<(usize, f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &i in self.bunky.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    let d = (vrcholy[i].x - x).hypot(vrcholy[i].y - y);
                    if d <= self.tol && nejlepsi.is_none_or(|(_, n)| d < n) {
                        nejlepsi = Some((i, d));
                    }
                }
            }
        }
        nejlepsi.map(|(i, _)| i)
    }
    fn vloz(&mut self, i: usize, x: f64, y: f64) {
        let b = self.bunka(x, y);
        self.bunky.entry(b).or_default().push(i);
    }
}

fn orient(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Bod uvnitř polygonu (paprsková metoda).
pub fn uvnitr(polygon: &[[f64; 2]], x: f64, y: f64) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut v = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (polygon[i][0], polygon[i][1]);
        let (xj, yj) = (polygon[j][0], polygon[j][1]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            v = !v;
        }
        j = i;
    }
    v
}

fn klic(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

impl Povrch {
    /// Vytvoří povrch jen z vybraných bodů (`vyber` = indexy do `body`).
    pub fn z_bodu(body: &[Bod], vyber: &[usize], nastaveni: &NastaveniPovrchu) -> Result<Self, ChybaPovrchu> {
        let def = Definice {
            body: vyber.iter().map(|&i| body[i].clone()).collect(),
            max_hrana: nastaveni.max_hrana,
            ..Default::default()
        };
        let mut p = Self::z_definice(&def, nastaveni.tolerance_duplicit)?;
        // indexy bodů zpět na indexy ve vstupním seznamu
        for v in &mut p.vrcholy {
            v.bod = v.bod.map(|i| vyber[i]);
        }
        for d in &mut p.duplicity {
            d.vynechany = vyber[d.vynechany];
            d.ponechany = vyber[d.ponechany];
        }
        Ok(p)
    }

    /// Vytvoří povrch z definice.
    ///
    /// Hranice a díry se do povrchu vkládají jako povinné hrany: nejdřív se
    /// spočte povrch bez nich, hranice se zhustí v průsečících s jeho
    /// hranami a výšky se z něj interpolují. Ořez tak sedí přesně na čáru.
    pub fn z_definice(def: &Definice, tolerance_duplicit: f64) -> Result<Self, ChybaPovrchu> {
        if def.hranice.is_empty() && def.diry.is_empty() {
            return Self::z_definice_vnitrni(def, tolerance_duplicit);
        }
        let bez_hranic = Definice { hranice: Vec::new(), diry: Vec::new(), ..def.clone() };
        let prvni = Self::z_definice_vnitrni(&bez_hranic, tolerance_duplicit)?;
        let mut def2 = def.clone();
        for polygon in def.hranice.iter().chain(def.diry.iter()) {
            def2.spojnice.extend(prvni.zhusti_na_povrchu(polygon).into_iter().filter(|c| c.len() >= 2));
        }
        let mut p = Self::z_definice_vnitrni(&def2, tolerance_duplicit)?;
        // křížení hranic s vlastními úseky se nehlásí jako chyba spojnic
        p.hlaseni.krizeni_spojnic.retain(|(a, b)| {
            !def.hranice.iter().chain(def.diry.iter()).any(|poly| {
                poly.iter().any(|v| (v[0] - a[0]).hypot(v[1] - a[1]) < 1e-6 || (v[0] - b[0]).hypot(v[1] - b[1]) < 1e-6)
            })
        });
        Ok(p)
    }

    /// Uzavřený polygon jako 3D čáry po povrchu: vrcholy polygonu
    /// a průsečíky s hranami trojúhelníků, výšky z povrchu. Kde polygon
    /// opouští povrch, čára se přeruší.
    fn zhusti_na_povrchu(&self, polygon: &[[f64; 2]]) -> Vec<Vec<[f64; 3]>> {
        let mut hrany: HashSet<(usize, usize)> = HashSet::new();
        for t in &self.trojuhelniky {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                hrany.insert(klic(a, b));
            }
        }
        let mut retezy: Vec<Vec<[f64; 3]>> = vec![Vec::new()];
        let n = polygon.len();
        for i in 0..n {
            let (p, q) = (polygon[i], polygon[(i + 1) % n]);
            let (px, py) = (p[0] - self.pocatek.0, p[1] - self.pocatek.1);
            let (qx, qy) = (q[0] - self.pocatek.0, q[1] - self.pocatek.1);
            let mut body: Vec<(f64, f64, f64)> = vec![(0.0, px, py)];
            for &(a, b) in &hrany {
                let (va, vb) = (self.vrcholy[a], self.vrcholy[b]);
                let (rx, ry) = (qx - px, qy - py);
                let (sx, sy) = (vb.x - va.x, vb.y - va.y);
                let den = rx * sy - ry * sx;
                if den.abs() < 1e-12 {
                    continue;
                }
                let t = ((va.x - px) * sy - (va.y - py) * sx) / den;
                let u = ((va.x - px) * ry - (va.y - py) * rx) / den;
                if t > 1e-9 && t < 1.0 - 1e-9 && (-1e-9..=1.0 + 1e-9).contains(&u) {
                    body.push((t, px + t * rx, py + t * ry));
                }
            }
            body.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, x, y) in body {
                let (gx, gy) = (x + self.pocatek.0, y + self.pocatek.1);
                let akt = retezy.last_mut().unwrap();
                match self.vyska(gx, gy) {
                    Some(z) => {
                        if akt.last().is_none_or(|l: &[f64; 3]| (l[0] - gx).hypot(l[1] - gy) > 1e-6) {
                            akt.push([gx, gy, z]);
                        }
                    }
                    None if !akt.is_empty() => retezy.push(Vec::new()),
                    None => {}
                }
            }
        }
        // uzavření: celý polygon na povrchu → spojit konec se začátkem,
        // jinak navázat poslední řetěz na první
        if retezy.len() == 1 {
            let c = &mut retezy[0];
            if let Some(f) = c.first().copied() {
                if c.last().is_some_and(|l| (f[0] - l[0]).hypot(f[1] - l[1]) > 1e-6) {
                    c.push(f);
                }
            }
        } else if retezy.len() > 1 && !retezy[0].is_empty() {
            let prvni = retezy.remove(0);
            retezy.last_mut().unwrap().extend(prvni);
        }
        retezy
    }

    fn z_definice_vnitrni(def: &Definice, tolerance_duplicit: f64) -> Result<Self, ChybaPovrchu> {
        const TOL_EDITACE: f64 = 0.001;
        let odebrane: Vec<[f64; 2]> = def
            .editace
            .iter()
            .filter_map(|e| if let Editace::OdebratBod(p) = e { Some(*p) } else { None })
            .collect();
        // přidaný bod, který se později odebral, se nepřidá (rozhoduje pořadí úprav)
        let pridane: Vec<[f64; 3]> = def
            .editace
            .iter()
            .enumerate()
            .filter_map(|(i, e)| if let Editace::PridatBod(p) = e { Some((i, *p)) } else { None })
            .filter(|(i, p)| {
                !def.editace[i + 1..].iter().any(|e| {
                    matches!(e, Editace::OdebratBod(o) if (o[0] - p[0]).hypot(o[1] - p[1]) <= TOL_EDITACE)
                })
            })
            .map(|(_, p)| p)
            .collect();

        let je_odebrany = |x: f64, y: f64| odebrane.iter().any(|o| (o[0] - x).hypot(o[1] - y) <= TOL_EDITACE);

        // počátek
        let (mut mx, mut my) = (f64::INFINITY, f64::INFINITY);
        let mut rozsah = |x: f64, y: f64| {
            mx = mx.min(x);
            my = my.min(y);
        };
        def.body.iter().for_each(|b| rozsah(b.x, b.y));
        def.spojnice.iter().flatten().for_each(|p| rozsah(p[0], p[1]));
        pridane.iter().for_each(|p| rozsah(p[0], p[1]));
        if !mx.is_finite() {
            return Err(ChybaPovrchu(
                tr("povrch nemá žádné body – jeho skupiny bodů jsou prázdné nebo vypnuté; přidejte skupinu v Prospektoru (Z čeho se povrch počítá › Skupiny bodů)").into(),
            ));
        }
        let pocatek = (mx.floor(), my.floor());
        let lok = |x: f64, y: f64| (x - pocatek.0, y - pocatek.1);

        let mut mrizka = Mrizka::new(tolerance_duplicit);
        let mut vrcholy: Vec<Vrchol> = Vec::new();
        let mut duplicity = Vec::new();
        let mut hlaseni = Hlaseni::default();

        for (i, b) in def.body.iter().enumerate() {
            if je_odebrany(b.x, b.y) {
                continue;
            }
            let (x, y) = lok(b.x, b.y);
            if let Some(j) = mrizka.najdi(&vrcholy, x, y) {
                if let Some(pon) = vrcholy[j].bod {
                    duplicity.push(Duplicita { vynechany: i, ponechany: pon, rozdil_z: b.z - vrcholy[j].z });
                }
                continue;
            }
            mrizka.vloz(vrcholy.len(), x, y);
            vrcholy.push(Vrchol { x, y, z: b.z, bod: Some(i) });
        }
        for p in &pridane {
            let (x, y) = lok(p[0], p[1]);
            match mrizka.najdi(&vrcholy, x, y) {
                Some(j) => vrcholy[j].z = p[2],
                None => {
                    mrizka.vloz(vrcholy.len(), x, y);
                    vrcholy.push(Vrchol { x, y, z: p[2], bod: None });
                }
            }
        }

        // vrcholy spojnic
        let mut spojnice_idx: Vec<Vec<usize>> = Vec::new();
        for s in &def.spojnice {
            let mut retez = Vec::with_capacity(s.len());
            for p in s {
                let (x, y) = lok(p[0], p[1]);
                let i = match mrizka.najdi(&vrcholy, x, y) {
                    Some(j) => {
                        if (vrcholy[j].z - p[2]).abs() > 0.005 {
                            hlaseni.rozdilne_vysky += 1;
                        }
                        j
                    }
                    None => {
                        mrizka.vloz(vrcholy.len(), x, y);
                        vrcholy.push(Vrchol { x, y, z: p[2], bod: None });
                        vrcholy.len() - 1
                    }
                };
                if retez.last() != Some(&i) {
                    retez.push(i);
                }
            }
            if retez.len() >= 2 {
                spojnice_idx.push(retez);
            }
        }

        if vrcholy.len() < 3 {
            return Err(ChybaPovrchu(trf!("pro povrch jsou potřeba aspoň 3 body, ve skupinách je {} – přidejte body nebo další skupinu", vrcholy.len())));
        }

        let mut tin: ConstrainedDelaunayTriangulation<V> = ConstrainedDelaunayTriangulation::new();
        let mut handly: Vec<FixedVertexHandle> = Vec::with_capacity(vrcholy.len());
        for (idx, v) in vrcholy.iter().enumerate() {
            let h = tin
                .insert(V { p: Point2::new(v.x, v.y), idx })
                .map_err(|e| ChybaPovrchu(format!("chyba triangulace: {e:?}")))?;
            handly.push(h);
        }
        for retez in &spojnice_idx {
            for w in retez.windows(2) {
                let (a, b) = (handly[w[0]], handly[w[1]]);
                if a == b {
                    continue;
                }
                if tin.try_add_constraint(a, b).is_empty() && !tin.exists_constraint(a, b) {
                    let g = |i: usize| [vrcholy[i].x + pocatek.0, vrcholy[i].y + pocatek.1];
                    hlaseni.krizeni_spojnic.push((g(w[0]), g(w[1])));
                }
            }
        }
        // vynucené hrany z úprav
        let mut nepouzite_pred: Vec<usize> = Vec::new();
        for (ei, e) in def.editace.iter().enumerate() {
            if let Editace::PridatHranu(a, b) = e {
                let ia = mrizka.najdi(&vrcholy, a[0] - pocatek.0, a[1] - pocatek.1);
                let ib = mrizka.najdi(&vrcholy, b[0] - pocatek.0, b[1] - pocatek.1);
                match (ia, ib) {
                    (Some(ia), Some(ib)) if ia != ib => {
                        let (ha, hb) = (handly[ia], handly[ib]);
                        if tin.try_add_constraint(ha, hb).is_empty() && !tin.exists_constraint(ha, hb) {
                            nepouzite_pred.push(ei);
                        }
                    }
                    _ => nepouzite_pred.push(ei),
                }
            }
        }
        if tin.num_inner_faces() == 0 {
            return Err(ChybaPovrchu(tr("body leží na jedné přímce, trojúhelníky z nich nevzniknou – přidejte body mimo tuto přímku").into()));
        }

        let povinne_hrany: HashSet<(usize, usize)> = tin
            .undirected_edges()
            .filter(|e| e.is_constraint_edge())
            .map(|e| {
                let [a, b] = e.vertices().map(|v| v.data().idx);
                klic(a, b)
            })
            .collect();

        let hranice_lok: Vec<Vec<[f64; 2]>> =
            def.hranice.iter().map(|p| p.iter().map(|q| [q[0] - pocatek.0, q[1] - pocatek.1]).collect()).collect();
        let diry_lok: Vec<Vec<[f64; 2]>> =
            def.diry.iter().map(|p| p.iter().map(|q| [q[0] - pocatek.0, q[1] - pocatek.1]).collect()).collect();

        let mut trojuhelniky = Vec::with_capacity(tin.num_inner_faces());
        let mut odstraneno_dlouhych = 0;
        for f in tin.inner_faces() {
            let t = f.vertices().map(|h| h.data().idx);
            let [a, b, c] = t.map(|i| vrcholy[i]);
            if let Some(max) = def.max_hrana {
                let d = |p: Vrchol, q: Vrchol| (p.x - q.x).hypot(p.y - q.y);
                // povinné hrany se nezkracují – trojúhelník na spojnici zůstává
                let dlouha = |p: usize, q: usize| d(vrcholy[p], vrcholy[q]) > max && !povinne_hrany.contains(&klic(p, q));
                if dlouha(t[0], t[1]) || dlouha(t[1], t[2]) || dlouha(t[2], t[0]) {
                    odstraneno_dlouhych += 1;
                    continue;
                }
            }
            let (cx, cy) = ((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0);
            if !hranice_lok.is_empty() && !hranice_lok.iter().any(|h| uvnitr(h, cx, cy)) {
                continue;
            }
            if diry_lok.iter().any(|h| uvnitr(h, cx, cy)) {
                continue;
            }
            trojuhelniky.push(t);
        }

        let mut povrch = Self {
            pocatek,
            vrcholy,
            trojuhelniky,
            povinne_hrany,
            duplicity,
            odstraneno_dlouhych,
            hlaseni,
        };

        povrch.hlaseni.nepouzite_editace.extend(nepouzite_pred);
        // topologické editace v pořadí zadání
        for (i, e) in def.editace.iter().enumerate() {
            let ok = match e {
                Editace::OtocitHranu(a, b) => povrch.otoc_hranu_global(*a, *b).is_ok(),
                Editace::SmazatTrojuhelnik(p) => povrch.smaz_trojuhelnik_global(p[0], p[1]),
                Editace::SmazatHranu(a, b) => povrch.smaz_hranu_global(*a, *b),
                Editace::PridatBod(_) | Editace::OdebratBod(_) | Editace::PridatHranu(..) => true,
            };
            if !ok {
                povrch.hlaseni.nepouzite_editace.push(i);
            }
        }
        Ok(povrch)
    }

    /// Globální souřadnice vrcholu.
    pub fn globalni(&self, i: usize) -> (f64, f64, f64) {
        let v = &self.vrcholy[i];
        (v.x + self.pocatek.0, v.y + self.pocatek.1, v.z)
    }

    fn najdi_vrchol_lok(&self, x: f64, y: f64, tol: f64) -> Option<usize> {
        let mut nejlepsi: Option<(usize, f64)> = None;
        for (i, v) in self.vrcholy.iter().enumerate() {
            let d = (v.x - x).hypot(v.y - y);
            if d <= tol && nejlepsi.is_none_or(|(_, n)| d < n) {
                nejlepsi = Some((i, d));
            }
        }
        nejlepsi.map(|(i, _)| i)
    }

    /// Vrcholy použité v trojúhelnících.
    pub fn pouzite_vrcholy(&self) -> Vec<bool> {
        let mut p = vec![false; self.vrcholy.len()];
        for t in &self.trojuhelniky {
            for &i in t {
                p[i] = true;
            }
        }
        p
    }

    /// Nejbližší vrchol povrchu ke globálnímu bodu (jen vrcholy v trojúhelnících).
    pub fn nejblizsi_vrchol(&self, x: f64, y: f64, max_vzdalenost: f64) -> Option<usize> {
        let (lx, ly) = (x - self.pocatek.0, y - self.pocatek.1);
        let pouzite = self.pouzite_vrcholy();
        self.vrcholy
            .iter()
            .enumerate()
            .filter(|(i, _)| pouzite[*i])
            .map(|(i, v)| (i, (v.x - lx).hypot(v.y - ly)))
            .filter(|(_, d)| *d <= max_vzdalenost)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Nejbližší hrana trojúhelníků ke globálnímu bodu: indexy vrcholů a vzdálenost.
    pub fn nejblizsi_hrana(&self, x: f64, y: f64) -> Option<((usize, usize), f64)> {
        let (px, py) = (x - self.pocatek.0, y - self.pocatek.1);
        let mut nejlepsi: Option<((usize, usize), f64)> = None;
        for t in &self.trojuhelniky {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (va, vb) = (self.vrcholy[a], self.vrcholy[b]);
                let (dx, dy) = (vb.x - va.x, vb.y - va.y);
                let l2 = dx * dx + dy * dy;
                let s = if l2 > 0.0 { (((px - va.x) * dx + (py - va.y) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let d = (va.x + s * dx - px).hypot(va.y + s * dy - py);
                if nejlepsi.is_none_or(|(_, n)| d < n) {
                    nejlepsi = Some((klic(a, b), d));
                }
            }
        }
        nejlepsi
    }

    /// Otočí hranu mezi vrcholy `a`, `b` (indexy). Vrací chybu s důvodem.
    pub fn otoc_hranu(&mut self, a: usize, b: usize) -> Result<(), &'static str> {
        if self.povinne_hrany.contains(&klic(a, b)) {
            return Err(tr("drží ji spojnice (lomová hrana); spojnici můžete v Prospektoru vypnout"));
        }
        let sdilene: Vec<usize> = self
            .trojuhelniky
            .iter()
            .enumerate()
            .filter(|(_, t)| t.contains(&a) && t.contains(&b))
            .map(|(i, _)| i)
            .collect();
        if sdilene.len() != 2 {
            return Err(tr("leží na okraji povrchu, není ji s čím prohodit"));
        }
        let protejsi = |t: [usize; 3]| *t.iter().find(|&&v| v != a && v != b).unwrap();
        let (t1, t2) = (sdilene[0], sdilene[1]);
        let (p, q) = (protejsi(self.trojuhelniky[t1]), protejsi(self.trojuhelniky[t2]));
        let pt = |i: usize| (self.vrcholy[i].x, self.vrcholy[i].y);
        // nová hrana p–q musí protínat a–b (čtyřúhelník konvexní)
        let o1 = orient(pt(p), pt(q), pt(a));
        let o2 = orient(pt(p), pt(q), pt(b));
        let o3 = orient(pt(a), pt(b), pt(p));
        let o4 = orient(pt(a), pt(b), pt(q));
        if !(o1 * o2 < 0.0 && o3 * o4 < 0.0) {
            return Err(tr("trojúhelníky kolem ní netvoří vypuklý čtyřúhelník – zkuste nejdřív otočit sousední hranu"));
        }
        let ccw = |x: usize, y: usize, z: usize| if orient(pt(x), pt(y), pt(z)) > 0.0 { [x, y, z] } else { [x, z, y] };
        self.trojuhelniky[t1] = ccw(p, q, a);
        self.trojuhelniky[t2] = ccw(q, p, b);
        Ok(())
    }

    fn otoc_hranu_global(&mut self, a: [f64; 2], b: [f64; 2]) -> Result<(), &'static str> {
        let tol = 0.001;
        let ia = self.najdi_vrchol_lok(a[0] - self.pocatek.0, a[1] - self.pocatek.1, tol).ok_or("vrchol neexistuje")?;
        let ib = self.najdi_vrchol_lok(b[0] - self.pocatek.0, b[1] - self.pocatek.1, tol).ok_or("vrchol neexistuje")?;
        self.otoc_hranu(ia, ib)
    }

    /// Odstraní trojúhelníky sdílející hranu `a`–`b` (indexy). Vrací počet odstraněných.
    pub fn smaz_hranu(&mut self, a: usize, b: usize) -> usize {
        let pred = self.trojuhelniky.len();
        self.trojuhelniky.retain(|t| !(t.contains(&a) && t.contains(&b)));
        pred - self.trojuhelniky.len()
    }

    fn smaz_hranu_global(&mut self, a: [f64; 2], b: [f64; 2]) -> bool {
        let tol = 0.001;
        let ia = self.najdi_vrchol_lok(a[0] - self.pocatek.0, a[1] - self.pocatek.1, tol);
        let ib = self.najdi_vrchol_lok(b[0] - self.pocatek.0, b[1] - self.pocatek.1, tol);
        match (ia, ib) {
            (Some(ia), Some(ib)) => self.smaz_hranu(ia, ib) > 0,
            _ => false,
        }
    }

    /// Index trojúhelníku obsahujícího globální bod.
    pub fn trojuhelnik_v_bode(&self, x: f64, y: f64) -> Option<usize> {
        let (px, py) = (x - self.pocatek.0, y - self.pocatek.1);
        self.trojuhelniky.iter().position(|t| {
            let [a, b, c] = t.map(|i| (self.vrcholy[i].x, self.vrcholy[i].y));
            let (d1, d2, d3) = (orient(a, b, (px, py)), orient(b, c, (px, py)), orient(c, a, (px, py)));
            let neg = d1 < -1e-12 || d2 < -1e-12 || d3 < -1e-12;
            let pos = d1 > 1e-12 || d2 > 1e-12 || d3 > 1e-12;
            !(neg && pos)
        })
    }

    fn smaz_trojuhelnik_global(&mut self, x: f64, y: f64) -> bool {
        match self.trojuhelnik_v_bode(x, y) {
            Some(i) => {
                self.trojuhelniky.remove(i);
                true
            }
            None => false,
        }
    }

    /// Výška povrchu v bodě (globální souřadnice). `None` mimo povrch.
    pub fn vyska(&self, x: f64, y: f64) -> Option<f64> {
        let (px, py) = (x - self.pocatek.0, y - self.pocatek.1);
        let t = self.trojuhelnik_v_bode(x, y)?;
        let [a, b, c] = self.trojuhelniky[t].map(|i| self.vrcholy[i]);
        let det = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
        if det.abs() < 1e-12 {
            return Some((a.z + b.z + c.z) / 3.0);
        }
        let l1 = ((b.y - c.y) * (px - c.x) + (c.x - b.x) * (py - c.y)) / det;
        let l2 = ((c.y - a.y) * (px - c.x) + (a.x - c.x) * (py - c.y)) / det;
        Some(l1 * a.z + l2 * b.z + (1.0 - l1 - l2) * c.z)
    }

    /// Obrys povrchu – hrany trojúhelníků, které nemají souseda,
    /// pospojované do uzavřených polygonů (globální souřadnice).
    pub fn obrys(&self) -> Vec<Vec<(f64, f64, f64)>> {
        let mut pocet: HashMap<(usize, usize), u32> = HashMap::new();
        for t in &self.trojuhelniky {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *pocet.entry(klic(a, b)).or_default() += 1;
            }
        }
        // orientované okrajové hrany (z trojúhelníků CCW → obrys CCW)
        let mut dalsi: HashMap<usize, Vec<usize>> = HashMap::new();
        for t in &self.trojuhelniky {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                if pocet[&klic(a, b)] == 1 {
                    dalsi.entry(a).or_default().push(b);
                }
            }
        }
        let mut vysledek = Vec::new();
        let mut starty: Vec<usize> = dalsi.keys().copied().collect();
        starty.sort_unstable();
        for s in starty {
            while let Some(n) = dalsi.get_mut(&s).and_then(|v| v.pop()) {
                let mut smycka = vec![s];
                let mut akt = n;
                while akt != s {
                    smycka.push(akt);
                    match dalsi.get_mut(&akt).and_then(|v| v.pop()) {
                        Some(x) => akt = x,
                        None => break,
                    }
                    if smycka.len() > self.vrcholy.len() + 1 {
                        break;
                    }
                }
                vysledek.push(smycka.into_iter().map(|i| self.globalni(i)).collect());
            }
        }
        vysledek
    }

    pub fn statistika(&self) -> Statistika {
        let mut s = Statistika {
            pocet_vrcholu: 0,
            pocet_trojuhelniku: self.trojuhelniky.len(),
            z_min: f64::INFINITY,
            z_max: f64::NEG_INFINITY,
            plocha_2d: 0.0,
            plocha_3d: 0.0,
        };
        let mut pouzite = vec![false; self.vrcholy.len()];
        for t in &self.trojuhelniky {
            for &i in t {
                if !pouzite[i] {
                    pouzite[i] = true;
                    s.pocet_vrcholu += 1;
                    s.z_min = s.z_min.min(self.vrcholy[i].z);
                    s.z_max = s.z_max.max(self.vrcholy[i].z);
                }
            }
            let [a, b, c] = t.map(|i| self.vrcholy[i]);
            let (ux, uy, uz) = (b.x - a.x, b.y - a.y, b.z - a.z);
            let (vx, vy, vz) = (c.x - a.x, c.y - a.y, c.z - a.z);
            let nz = ux * vy - uy * vx;
            s.plocha_2d += 0.5 * nz.abs();
            let nx = uy * vz - uz * vy;
            let ny = uz * vx - ux * vz;
            s.plocha_3d += 0.5 * (nx * nx + ny * ny + nz * nz).sqrt();
        }
        s
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Statistika {
    pub pocet_vrcholu: usize,
    pub pocet_trojuhelniku: usize,
    pub z_min: f64,
    pub z_max: f64,
    pub plocha_2d: f64,
    pub plocha_3d: f64,
}

#[cfg(test)]
mod testy {
    use super::*;

    fn bod(c: &str, x: f64, y: f64, z: f64) -> Bod {
        Bod { cislo: c.into(), x, y, z, popis: "T".into() }
    }

    fn mrizka(n: usize, krok: f64, x0: f64, y0: f64) -> Vec<Bod> {
        let mut v = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (x0 + i as f64 * krok, y0 + j as f64 * krok);
                v.push(bod(&format!("{i}_{j}"), x, y, 300.0 + 0.1 * (i as f64) * krok));
            }
        }
        v
    }

    fn vse(b: &[Bod]) -> Vec<usize> {
        (0..b.len()).collect()
    }

    #[test]
    fn ctverec_dva_trojuhelniky() {
        let b = vec![bod("1", 0., 0., 1.), bod("2", 10., 0., 2.), bod("3", 10., 10., 3.), bod("4", 0., 10., 4.)];
        let p = Povrch::z_bodu(&b, &[0, 1, 2, 3], &NastaveniPovrchu::default()).unwrap();
        assert_eq!(p.trojuhelniky.len(), 2);
        let s = p.statistika();
        assert!((s.plocha_2d - 100.0).abs() < 1e-9);
        assert_eq!((s.z_min, s.z_max), (1.0, 4.0));
        assert_eq!(p.obrys().len(), 1);
        assert_eq!(p.obrys()[0].len(), 4);
    }

    #[test]
    fn mrizka_sjtsk_souradnice() {
        let b = mrizka(11, 5.0, -823000.0, -1070000.0);
        let p = Povrch::z_bodu(&b, &vse(&b), &NastaveniPovrchu::default()).unwrap();
        assert_eq!(p.trojuhelniky.len(), 2 * 10 * 10);
        let s = p.statistika();
        assert!((s.plocha_2d - 2500.0).abs() < 1e-6, "{}", s.plocha_2d);
        assert!((s.plocha_3d - 2500.0 * (1.01f64).sqrt()).abs() < 1e-6);
        let (gx, gy, _) = p.globalni(0);
        assert_eq!((gx, gy), (-823000.0, -1070000.0));
    }

    #[test]
    fn vyska_v_bode() {
        let b = mrizka(11, 5.0, -823000.0, -1070000.0);
        let p = Povrch::z_bodu(&b, &vse(&b), &NastaveniPovrchu::default()).unwrap();
        let z = p.vyska(-823000.0 + 12.3, -1070000.0 + 7.7).unwrap();
        assert!((z - 301.23).abs() < 1e-9, "{z}");
        assert!(p.vyska(-823000.0 - 1.0, -1070000.0 + 5.0).is_none());
    }

    #[test]
    fn duplicity() {
        let b = vec![bod("1", 0., 0., 1.), bod("2", 10., 0., 2.), bod("3", 0., 10., 3.), bod("1b", 0.0004, 0., 1.5)];
        let p = Povrch::z_bodu(&b, &[0, 1, 2, 3], &NastaveniPovrchu::default()).unwrap();
        assert_eq!(p.duplicity.len(), 1);
        assert_eq!(p.duplicity[0].vynechany, 3);
        assert_eq!(p.duplicity[0].ponechany, 0);
        assert!((p.duplicity[0].rozdil_z - 0.5).abs() < 1e-12);
    }

    #[test]
    fn max_hrana() {
        let mut b = mrizka(3, 1.0, 0.0, 0.0);
        b.push(bod("daleko", 100.0, 1.0, 300.0));
        let n = NastaveniPovrchu { max_hrana: Some(5.0), ..Default::default() };
        let p = Povrch::z_bodu(&b, &vse(&b), &n).unwrap();
        assert_eq!(p.trojuhelniky.len(), 8);
        assert!(p.odstraneno_dlouhych > 0);
    }

    #[test]
    fn primka_je_chyba() {
        let b = vec![bod("1", 0., 0., 1.), bod("2", 1., 1., 1.), bod("3", 2., 2., 1.)];
        assert!(Povrch::z_bodu(&b, &[0, 1, 2], &NastaveniPovrchu::default()).is_err());
    }

    /// Čtverec, kde Delaunay zvolí úhlopříčku 0–2; spojnice 1–3 ji musí přebít.
    fn ctverec() -> Vec<Bod> {
        vec![bod("0", 0., 0., 0.), bod("1", 10., 0., 5.), bod("2", 10., 10., 0.), bod("3", 0., 10.2, 5.)]
    }

    fn ma_hranu(p: &Povrch, a: usize, b: usize) -> bool {
        p.trojuhelniky.iter().any(|t| t.contains(&a) && t.contains(&b))
    }

    #[test]
    fn spojnice_vynuti_hranu() {
        let def = Definice { body: ctverec(), ..Default::default() };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        let (a, b) = if ma_hranu(&p, 0, 2) { (1, 3) } else { (0, 2) };
        let def = Definice {
            body: ctverec(),
            spojnice: vec![vec![
                [p.globalni(a).0, p.globalni(a).1, p.globalni(a).2],
                [p.globalni(b).0, p.globalni(b).1, p.globalni(b).2],
            ]],
            ..Default::default()
        };
        let p2 = Povrch::z_definice(&def, 0.001).unwrap();
        assert!(ma_hranu(&p2, a, b));
        assert!(p2.povinne_hrany.contains(&klic(a, b)));
        assert!(p2.hlaseni.krizeni_spojnic.is_empty());
    }

    #[test]
    fn spojnice_s_novym_vrcholem_a_krizeni() {
        let b = mrizka(5, 10.0, 0.0, 0.0);
        let def = Definice {
            body: b,
            spojnice: vec![
                vec![[5.0, 0.0, 305.0], [5.0, 40.0, 305.0]],
                vec![[0.0, 15.0, 300.0], [40.0, 15.0, 304.0]],
            ],
            ..Default::default()
        };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        assert_eq!(p.hlaseni.krizeni_spojnic.len(), 1);
        // výška na spojnici x = 5 je dána spojnicí
        assert!((p.vyska(5.0, 30.0).unwrap() - 305.0).abs() < 1e-9);
    }

    #[test]
    fn hranice_a_dira() {
        let b = mrizka(11, 1.0, 0.0, 0.0);
        let def = Definice {
            body: b.clone(),
            hranice: vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]]],
            ..Default::default()
        };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        assert!((p.statistika().plocha_2d - 50.0).abs() < 1e-9);
        let def = Definice {
            body: b,
            diry: vec![vec![[2.0, 2.0], [4.0, 2.0], [4.0, 4.0], [2.0, 4.0]]],
            ..Default::default()
        };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        assert!((p.statistika().plocha_2d - 96.0).abs() < 1e-9);
        assert!(p.vyska(3.0, 3.0).is_none());
        assert_eq!(p.obrys().len(), 2);
    }

    #[test]
    fn hranice_presne_mimo_body() {
        // rovina z = 300 + 0.1 x; hranice a díra neleží na bodech
        let b = mrizka(11, 1.0, -823000.0, -1070000.0);
        let (x0, y0) = (-823000.0, -1070000.0);
        let r = |a: f64, b: f64, c: f64, d: f64| vec![[x0 + a, y0 + b], [x0 + c, y0 + b], [x0 + c, y0 + d], [x0 + a, y0 + d]];
        let def = Definice {
            body: b,
            hranice: vec![r(0.5, 0.3, 9.5, 7.7)],
            diry: vec![r(2.2, 2.1, 4.6, 3.9)],
            ..Default::default()
        };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        let s = p.statistika();
        assert!((s.plocha_2d - (9.0 * 7.4 - 2.4 * 1.8)).abs() < 1e-6, "{}", s.plocha_2d);
        // výška na hranici odpovídá rovině
        let z = p.vyska(x0 + 0.5, y0 + 5.0).unwrap();
        assert!((z - 300.05).abs() < 1e-9, "{z}");
        assert!(p.hlaseni.krizeni_spojnic.is_empty());
    }

    #[test]
    fn otoceni_hrany_a_editace() {
        let def = Definice { body: ctverec(), ..Default::default() };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        let (a, b) = if ma_hranu(&p, 0, 2) { (0, 2) } else { (1, 3) };
        let g = |i: usize| [p.globalni(i).0, p.globalni(i).1];
        let e = Editace::OtocitHranu(g(a), g(b));
        assert_eq!(Editace::precti(&e.zapis()), Some(e.clone()));
        let def = Definice { body: ctverec(), editace: vec![e], ..Default::default() };
        let p2 = Povrch::z_definice(&def, 0.001).unwrap();
        assert!(!ma_hranu(&p2, a, b));
        assert_eq!(p2.trojuhelniky.len(), 2);
        assert!(p2.hlaseni.nepouzite_editace.is_empty());
        // plocha se otočením nemění
        assert!((p2.statistika().plocha_2d - p.statistika().plocha_2d).abs() < 1e-9);
        // okrajovou hranu otočit nejde
        let mut p3 = p2.clone();
        assert!(p3.otoc_hranu(0, 1).is_err());
    }

    #[test]
    fn pridat_a_smazat_hranu() {
        let def = Definice { body: ctverec(), ..Default::default() };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        let (a, b) = if ma_hranu(&p, 0, 2) { (1, 3) } else { (0, 2) };
        let g = |i: usize| [p.globalni(i).0, p.globalni(i).1];
        let e = Editace::PridatHranu(g(a), g(b));
        assert_eq!(Editace::precti(&e.zapis()), Some(e.clone()));
        let p2 = Povrch::z_definice(&Definice { body: ctverec(), editace: vec![e.clone()], ..Default::default() }, 0.001).unwrap();
        assert!(ma_hranu(&p2, a, b));
        assert!(p2.hlaseni.nepouzite_editace.is_empty());
        // smazání vynucené hrany odstraní oba trojúhelníky
        let p3 = Povrch::z_definice(
            &Definice { body: ctverec(), editace: vec![e, Editace::SmazatHranu(g(a), g(b))], ..Default::default() },
            0.001,
        )
        .unwrap();
        assert!(p3.trojuhelniky.is_empty());
        // okrajová hrana – jen jeden trojúhelník
        let p4 = Povrch::z_definice(
            &Definice { body: ctverec(), editace: vec![Editace::SmazatHranu(g(0), g(1))], ..Default::default() },
            0.001,
        )
        .unwrap();
        assert_eq!(p4.trojuhelniky.len(), 1);
    }

    #[test]
    fn odebrat_pridat_bod_smazat_trojuhelnik() {
        let b = mrizka(3, 10.0, 0.0, 0.0);
        let def = Definice {
            body: b.clone(),
            editace: vec![Editace::OdebratBod([10.0, 10.0]), Editace::PridatBod([10.0, 10.0, 500.0])],
            ..Default::default()
        };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        assert!((p.vyska(10.0, 10.0).unwrap() - 500.0).abs() < 1e-9);
        // přidaný a pak odebraný bod v povrchu nezůstane
        let def = Definice {
            body: b.clone(),
            editace: vec![Editace::PridatBod([5.0, 5.0, 900.0]), Editace::OdebratBod([5.0, 5.0])],
            ..Default::default()
        };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        assert!(p.vyska(5.0, 5.0).unwrap() < 400.0);
        let def = Definice { body: b, editace: vec![Editace::SmazatTrojuhelnik([1.0, 0.5])], ..Default::default() };
        let p = Povrch::z_definice(&def, 0.001).unwrap();
        assert_eq!(p.trojuhelniky.len(), 7);
        assert!(p.vyska(1.0, 0.5).is_none());
    }
}
