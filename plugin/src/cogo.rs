//! COGO body a skupiny bodů ve výkrese.
//!
//! Bod = POINT s XDATA `TEREN_BOD` (číslo, popis, soubor importu, vlastní
//! styl), na libovolné hladině (starší výkresy: POINT na hladině `TER_BODY`).
//! Grafika bodu podle stylu – značka (hladina `TER_BODY_ZNACKA`) a popisy
//! (`TER_BODY_CISLO`, `TER_BODY_VYSKA`, `TER_BODY_POPIS`) – nese v XDATA odkaz
//! na polohu bodu.
//! Skupina bodů = pomocný POINT na vypnuté hladině `TER_SKUPINY` s definicí
//! v XDATA `TEREN_POVRCH` (první řetězec `SKUPINA`).

use teren::preklad::tr;
use teren::trf;
use std::collections::HashMap;

use ocs_plugin_api::host::codec::entities::{Circle, Insert, Line, LwPolyline, Point, Text};
use ocs_plugin_api::host::codec::types::{Color, Vector2, Vector3};
use ocs_plugin_api::host::{Document as CadDocument, EntityType, ExtendedDataRecord, Handle, LayerConfig, XDataValue};

use teren::body::Bod;
use teren::skupiny::{self, SkupinaBodu, VSECHNY};

use crate::povrchy::{APP_BOD, APP_POVRCH, H_BODY};

/// Hladina skrytých bodů (vypnutá).
pub const H_SKRYTE: &str = "TER_BODY_SKRYTE";
use crate::styly::{self, Rozlisovac, StylBodu, Znacka};
use crate::vykres::Vykres;

pub const H_CISLO: &str = "TER_BODY_CISLO";
pub const H_VYSKA: &str = "TER_BODY_VYSKA";
pub const H_POPIS: &str = "TER_BODY_POPIS";
pub const H_ZNACKA: &str = "TER_BODY_ZNACKA";
pub const H_SKUPINY: &str = "TER_SKUPINY";
const POPISEK: &str = "#POPISEK";
const SKUPINA: &str = "SKUPINA";

#[derive(Clone, Debug)]
pub struct BodVykresu {
    pub handle: Handle,
    pub bod: Bod,
    /// Soubor, ze kterého byl bod importován („(ručně)“ = vytvořen ručně, „“ = neznámo).
    pub zdroj: String,
    /// Vlastní styl bodu („“ = podle skupiny).
    pub styl: String,
}

fn retezce(r: &ExtendedDataRecord) -> Vec<&str> {
    r.values.iter().filter_map(|v| if let XDataValue::String(s) = v { Some(s.as_str()) } else { None }).collect()
}

pub fn xdata_bodu(cislo: &str, popis: &str, zdroj: &str, styl: &str) -> ExtendedDataRecord {
    let mut r = ExtendedDataRecord::new(APP_BOD);
    r.add_value(XDataValue::String(cislo.into()));
    r.add_value(XDataValue::String(popis.into()));
    r.add_value(XDataValue::String(zdroj.into()));
    r.add_value(XDataValue::String(styl.into()));
    r.add_value(XDataValue::String(format!("{}BOD", crate::povrchy::TRIDA)));
    r
}

/// Všechny COGO body výkresu.
pub fn body(doc: &CadDocument) -> Vec<BodVykresu> {
    let mut out = Vec::new();
    for e in doc.model_space_entities() {
        let EntityType::Point(p) = e else { continue };
        let zaznam = p.common.extended_data.get_record(APP_BOD);
        if zaznam.is_none() && !p.common.layer.eq_ignore_ascii_case(H_BODY) && !p.common.layer.eq_ignore_ascii_case(H_SKRYTE) {
            continue;
        }
        let t = zaznam.map(retezce).unwrap_or_default();
        let s = |i: usize| t.get(i).map(|x| x.to_string()).unwrap_or_default();
        out.push(BodVykresu {
            handle: p.common.handle,
            bod: Bod { cislo: s(0), x: p.location.x, y: p.location.y, z: p.location.z, popis: s(1) },
            zdroj: s(2),
            styl: s(3),
        });
    }
    out
}

// ── Grafika bodů (značka a popisy podle stylu) ───────────────────────────────

/// Klíč polohy bodu (milimetry).
pub fn klic(x: f64, y: f64) -> (i64, i64) {
    ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Druh {
    Cislo,
    Vyska,
    Popis,
    Znacka,
}

impl Druh {
    fn hladina(self) -> &'static str {
        match self {
            Druh::Cislo => H_CISLO,
            Druh::Vyska => H_VYSKA,
            Druh::Popis => H_POPIS,
            Druh::Znacka => H_ZNACKA,
        }
    }
    fn z_hladiny(h: &str) -> Option<Self> {
        [Druh::Cislo, Druh::Vyska, Druh::Popis, Druh::Znacka].into_iter().find(|d| d.hladina().eq_ignore_ascii_case(h))
    }
    /// Rozmístění popisů ze starších verzí (bez odkazu na bod): posun v
    /// násobcích výšky popisu a poměr výšky textu.
    fn stare_rozmisteni(self) -> (f64, f64, f64) {
        match self {
            Druh::Cislo => (0.4, 0.3, 1.0),
            Druh::Vyska => (0.4, -1.1, 1.0),
            _ => (0.4, -2.5, 0.8),
        }
    }
}

fn odkaz(b: &Bod, druh: Druh) -> ExtendedDataRecord {
    let mut r = ExtendedDataRecord::new(APP_BOD);
    r.add_value(XDataValue::String(POPISEK.into()));
    r.add_value(XDataValue::Real(b.x));
    r.add_value(XDataValue::Real(b.y));
    let trida = match druh {
        Druh::Znacka => "ZNACKA",
        Druh::Cislo => "CISLO",
        Druh::Vyska => "VYSKA",
        Druh::Popis => "POPIS",
    };
    r.add_value(XDataValue::String(format!("{}{trida}", crate::povrchy::TRIDA)));
    r
}

fn hotovo(mut e: EntityType, b: &Bod, druh: Druh, barva: styly::Barva) -> EntityType {
    let c = e.common_mut();
    c.layer = druh.hladina().into();
    c.color = styly::barva(barva);
    c.extended_data.add_record(odkaz(b, druh));
    e
}

/// Značka a popisy bodu podle stylu; `m` = měřítko (500 = 1:500).
pub fn grafika(b: &Bod, s: &StylBodu, m: f64) -> Vec<EntityType> {
    grafika_posun(b, s, m, (0.0, 0.0))
}

/// Řádky popisu bodu: (text, druh, posun x, posun y, výška písma) vůči bodu.
fn radky_popisu(b: &Bod, s: &StylBodu, m: f64) -> Vec<(String, Druh, f64, f64, f64)> {
    let h = s.pismo * m / 1000.0;
    let r = s.velikost * m / 1000.0 / 2.0;
    let dx = if s.znacka == Znacka::Bod { 0.4 * h } else { r + 0.3 * h };
    let cislo = (!b.cislo.is_empty() && s.popisek.cislo()).then(|| b.cislo.clone());
    let vyska = s.popisek.vyska().then(|| s.text_vysky(b.z));
    let popis = (s.popis && !b.popis.is_empty()).then(|| b.popis.clone());
    use styly::Popisek as P;
    let mut out = Vec::new();
    let mut radek_popisu = -0.5 * h;
    match s.popisek {
        P::CisloVyska => {
            match (&cislo, &vyska) {
                (Some(c), Some(v)) => {
                    out.push((c.clone(), Druh::Cislo, dx, 0.3 * h, h));
                    out.push((v.clone(), Druh::Vyska, dx, -1.1 * h, h));
                }
                (None, Some(v)) => out.push((v.clone(), Druh::Vyska, dx, -0.5 * h, h)),
                _ => {}
            }
            radek_popisu = -2.5 * h;
        }
        P::JedenRadek => {
            let mut px = dx;
            if let Some(c) = &cislo {
                out.push((c.clone(), Druh::Cislo, px, -0.5 * h, h));
                px += (c.chars().count() as f64 + 1.0) * 0.75 * h;
            }
            if let Some(v) = &vyska {
                out.push((v.clone(), Druh::Vyska, px, -0.5 * h, h));
            }
            radek_popisu = -1.9 * h;
        }
        P::JenVyska => {
            if let Some(v) = &vyska {
                out.push((v.clone(), Druh::Vyska, dx, -0.5 * h, h));
                radek_popisu = -1.9 * h;
            }
        }
        P::JenCislo => {
            if let Some(c) = &cislo {
                out.push((c.clone(), Druh::Cislo, dx, -0.5 * h, h));
                radek_popisu = -1.9 * h;
            }
        }
        P::Nic => {}
    }
    if let Some(p) = popis {
        out.push((p, Druh::Popis, dx, radek_popisu, 0.8 * h));
    }
    out
}

/// Obdélník popisu vůči bodu (x0, y0, x1, y1); `None` = bez popisu.
fn obdelnik_popisu(radky: &[(String, Druh, f64, f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let mut o: Option<(f64, f64, f64, f64)> = None;
    for (t, _, px, py, v) in radky {
        let w = t.chars().count() as f64 * 0.72 * v;
        let r = (*px, *py, px + w, py + v);
        o = Some(match o {
            None => r,
            Some(a) => (a.0.min(r.0), a.1.min(r.1), a.2.max(r.2), a.3.max(r.3)),
        });
    }
    o
}

/// Grafika bodů s odsunutím překrývajících se popisů (podle nastavení).
/// `prekazky` = polohy ostatních bodů ve výkrese (jejich značky).
pub fn grafika_rozmistena(polozky: &[(Bod, StylBodu)], prekazky: &[[f64; 2]], m: f64) -> Vec<EntityType> {
    if !crate::nastaveni::akt().odsazeni_popisku {
        return polozky.iter().flat_map(|(b, s)| grafika(b, s, m)).collect();
    }
    let posuny = rozmisti_popisy(polozky, prekazky, m);
    polozky.iter().zip(posuny).flat_map(|((b, s), p)| grafika_posun(b, s, m, p)).collect()
}

/// Najde pro každý popis posun, aby se nepřekrýval s jinými popisy ani značkami.
fn rozmisti_popisy(polozky: &[(Bod, StylBodu)], prekazky: &[[f64; 2]], m: f64) -> Vec<(f64, f64)> {
    type Obd = (f64, f64, f64, f64);
    let prekryv = |a: &Obd, b: &Obd| a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3;
    // mřížka obsazených obdélníků
    let bunka = polozky.iter().map(|(_, s)| s.pismo * m / 1000.0).fold(0.0_f64, f64::max).max(0.01) * 4.0;
    let mut mrizka: HashMap<(i64, i64), Vec<Obd>> = HashMap::new();
    let bunky = |o: &Obd| {
        let (a, b) = ((o.0 / bunka).floor() as i64, (o.1 / bunka).floor() as i64);
        let (c, d) = ((o.2 / bunka).floor() as i64, (o.3 / bunka).floor() as i64);
        let mut v = Vec::new();
        for i in a..=c {
            for j in b..=d {
                v.push((i, j));
            }
        }
        v
    };
    let vloz = |mrizka: &mut HashMap<(i64, i64), Vec<Obd>>, o: Obd| {
        for k in bunky(&o) {
            mrizka.entry(k).or_default().push(o);
        }
    };
    // značky všech bodů jsou překážky
    for (b, s) in polozky {
        let r = (s.velikost * m / 1000.0 / 2.0).max(s.pismo * m / 1000.0 * 0.2);
        vloz(&mut mrizka, (b.x - r, b.y - r, b.x + r, b.y + r));
    }
    for p in prekazky {
        let r = 0.6 * m / 1000.0;
        vloz(&mut mrizka, (p[0] - r, p[1] - r, p[0] + r, p[1] + r));
    }
    let mut out = Vec::with_capacity(polozky.len());
    for (b, s) in polozky {
        let radky = if s.skryt { Vec::new() } else { radky_popisu(b, s, m) };
        let Some(o) = obdelnik_popisu(&radky) else {
            out.push((0.0, 0.0));
            continue;
        };
        let (w, v) = (o.2 - o.0, o.3 - o.1);
        // vpravo (výchozí), vlevo, nahoru, dolů, šikmo a dál
        let vlevo = -(w + 2.0 * o.0);
        let kandidati = [
            (0.0, 0.0),
            (vlevo, 0.0),
            (0.0, v * 0.75),
            (0.0, -v * 0.75),
            (vlevo, v * 0.75),
            (vlevo, -v * 0.75),
            (0.0, v * 1.5),
            (0.0, -v * 1.5),
            (w * 0.6, v),
            (w * 0.6, -v),
            (vlevo - w * 0.6, v),
            (vlevo - w * 0.6, -v),
        ];
        let mut vybrany = (0.0, 0.0);
        for (ox, oy) in kandidati {
            let c = (b.x + o.0 + ox, b.y + o.1 + oy, b.x + o.2 + ox, b.y + o.3 + oy);
            let volno = bunky(&c)
                .iter()
                .filter_map(|k| mrizka.get(k))
                .flatten()
                .all(|x| !prekryv(x, &c));
            if volno {
                vybrany = (ox, oy);
                break;
            }
        }
        let c = (b.x + o.0 + vybrany.0, b.y + o.1 + vybrany.1, b.x + o.2 + vybrany.0, b.y + o.3 + vybrany.1);
        vloz(&mut mrizka, c);
        out.push(vybrany);
    }
    out
}

/// Předpona bloků vestavěných značek bodů.
pub const BLOK_ZNACKY: &str = "TER_ZN_";

/// Velikost (větší rozměr) vlastních bloků značek – kvůli měřítku vložení.
fn velikosti_bloku() -> &'static std::sync::Mutex<HashMap<String, f64>> {
    static V: std::sync::OnceLock<std::sync::Mutex<HashMap<String, f64>>> = std::sync::OnceLock::new();
    V.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn blok_vestavene(z: Znacka) -> String {
    format!("{BLOK_ZNACKY}{}", z.kod())
}

/// Geometrie tvaru značky s poloměrem `r` kolem `stred`; `barva` = None → podle bloku.
fn tvary_znacky(znacka: Znacka, r: f64, stred: (f64, f64, f64), barva_bloku: Option<()>) -> Vec<EntityType> {
    let (x, y, z) = stred;
    let mut out: Vec<EntityType> = znacka
        .tvary()
        .into_iter()
        .map(|t| match t {
            styly::Tvar::Cara(a, c) => EntityType::Line(Line::from_points(
                Vector3::new(x + a.0 * r, y + a.1 * r, z),
                Vector3::new(x + c.0 * r, y + c.1 * r, z),
            )),
            styly::Tvar::Kruh(c, rr) => {
                EntityType::Circle(Circle::from_center_radius(Vector3::new(x + c.0 * r, y + c.1 * r, z), rr * r))
            }
            styly::Tvar::Lomena(v, zavrena) => {
                let mut p = LwPolyline::from_points(v.iter().map(|(a, c)| Vector2::new(x + a * r, y + c * r)).collect());
                p.is_closed = zavrena;
                p.elevation = z;
                EntityType::LwPolyline(p)
            }
        })
        .collect();
    if barva_bloku.is_some() {
        for e in &mut out {
            e.common_mut().color = Color::ByBlock;
            e.common_mut().layer = "0".into();
        }
    }
    out
}

/// Vložení bloku značky bodu (`None` = bez značky nebo blok chybí).
fn vlozeni_znacky(s: &StylBodu, m: f64, x: f64, y: f64, z: f64) -> Option<Insert> {
    let velikost = s.velikost * m / 1000.0;
    let (jmeno, meritko) = if !s.blok.is_empty() {
        let v = *velikosti_bloku().lock().unwrap_or_else(|e| e.into_inner()).get(&s.blok.to_uppercase())?;
        (s.blok.clone(), velikost / v.max(1e-9))
    } else {
        if s.znacka.tvary().is_empty() {
            return None;
        }
        let jm = blok_vestavene(s.znacka);
        if !velikosti_bloku().lock().unwrap_or_else(|e| e.into_inner()).contains_key(&jm) {
            return None;
        }
        (jm, velikost / 2.0)
    };
    Some(Insert::new(jmeno, Vector3::new(x, y, z)).with_uniform_scale(meritko))
}

/// Větší rozměr obsahu bloku.
fn rozmer_bloku(doc: &CadDocument, jmeno: &str) -> Option<f64> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for e in doc.entities_in_block(jmeno) {
        let o = match &e {
            EntityType::Arc(a) => Some((a.center.x - a.radius, a.center.y - a.radius, a.center.x + a.radius, a.center.y + a.radius)),
            EntityType::Text(t) => Some((t.insertion_point.x, t.insertion_point.y, t.insertion_point.x, t.insertion_point.y)),
            EntityType::Insert(i) => Some((i.insert_point.x, i.insert_point.y, i.insert_point.x, i.insert_point.y)),
            _ => obalka(e),
        };
        if let Some((a, b, c, d)) = o {
            x0 = x0.min(a);
            y0 = y0.min(b);
            x1 = x1.max(c);
            y1 = y1.max(d);
        }
    }
    let r = (x1 - x0).max(y1 - y0);
    (r.is_finite() && r > 1e-9).then_some(r)
}

/// Bloky značek bodů: vestavěné tvary se vytvoří, když ve výkrese chybí; u vlastních
/// bloků se zjistí velikost (značka se vkládá v měřítku podle velikosti ve stylu).
pub fn zajisti_bloky_znacek(v: &mut dyn Vykres, doc: &CadDocument) -> Result<(), String> {
    let mut velikosti = HashMap::new();
    let ma_blok = |n: &str| doc.block_records.iter().any(|b| b.name.eq_ignore_ascii_case(n));
    let mut vestavene: Vec<Znacka> = Vec::new();
    for s in styly::styly_bodu(doc) {
        if s.skryt {
            continue;
        }
        if !s.blok.is_empty() {
            if let Some(r) = rozmer_bloku(doc, &s.blok) {
                velikosti.insert(s.blok.to_uppercase(), r);
            }
        } else if !s.znacka.tvary().is_empty() && !vestavene.contains(&s.znacka) {
            vestavene.push(s.znacka);
        }
    }
    for zn in vestavene {
        let jm = blok_vestavene(zn);
        if !ma_blok(&jm) {
            let h = v.pridej(tvary_znacky(zn, 1.0, (0.0, 0.0, 0.0), Some(())))?;
            let vysledek = v.tabulka(ocs_plugin_api::host::TableOperation::BlockCreate {
                name: jm.clone(),
                entities: h.clone(),
                base_point: [0.0, 0.0, 0.0],
                erase_originals: true,
                description: Some(trf!("Značka bodu „{}“ (plugin Terén)", zn.nazev())),
            });
            if let Err(e) = vysledek {
                // tvary značky nesmí zůstat ve výkrese v počátku souřadnic
                for x in h {
                    v.odeber(x);
                }
                // blok mezitím vytvořil někdo jiný (souběh) – v pořádku
                if !v.dokument().is_ok_and(|d| d.block_records.iter().any(|b| b.name.eq_ignore_ascii_case(&jm))) {
                    return Err(trf!("blok značky {} nejde vytvořit: {}", jm, e));
                }
            }
        }
        velikosti.insert(jm, 2.0);
    }
    *velikosti_bloku().lock().unwrap_or_else(|e| e.into_inner()) = velikosti;
    Ok(())
}

/// Bloky výkresu, které jdou použít jako vlastní značka bodu.
pub fn bloky_pro_znacky(doc: &CadDocument) -> Vec<String> {
    let mut v: Vec<String> = doc
        .block_records
        .iter()
        .map(|b| b.name.clone())
        .filter(|n| !n.starts_with('*') && !n.to_uppercase().starts_with("TER_") && !n.eq_ignore_ascii_case("_ArchTick"))
        .filter(|n| doc.entities_in_block(n).next().is_some())
        .collect();
    v.sort_by_key(|n| n.to_lowercase());
    v
}

/// Značka a popisy bodu, popisy posunuté o `posun` (s vodicí čarou).
pub fn grafika_posun(b: &Bod, s: &StylBodu, m: f64, posun: (f64, f64)) -> Vec<EntityType> {
    if s.skryt {
        return Vec::new();
    }
    let h = s.pismo * m / 1000.0;
    let r = s.velikost * m / 1000.0 / 2.0;
    let (x, y, z) = (b.x, b.y, b.z);
    let mut out = Vec::new();
    // značka = vložení bloku (vestavěný tvar nebo vlastní blok uživatele) – jeden objekt, jeden úchop
    if let Some(ins) = vlozeni_znacky(s, m, x, y, z) {
        out.push(hotovo(EntityType::Insert(ins), b, Druh::Znacka, s.barva_znacky));
    } else {
        out.extend(tvary_znacky(s.znacka, r, (x, y, z), None).into_iter().map(|e| hotovo(e, b, Druh::Znacka, s.barva_znacky)));
    }

    let radky = radky_popisu(b, s, m);
    let (ox, oy) = posun;
    if ox.abs() + oy.abs() > 1e-9 {
        // vodicí čára od značky k nejbližšímu bodu posunutého popisu
        if let Some(o) = obdelnik_popisu(&radky) {
            let (x0, y0, x1, y1) = (x + o.0 + ox, y + o.1 + oy, x + o.2 + ox, y + o.3 + oy);
            let cil = (x.clamp(x0, x1), y.clamp(y0, y1));
            let d = (cil.0 - x).hypot(cil.1 - y);
            let r0 = r.max(0.2 * h);
            if d > r0 * 1.2 {
                let start = (x + (cil.0 - x) / d * r0, y + (cil.1 - y) / d * r0);
                let l = Line::from_points(Vector3::new(start.0, start.1, z), Vector3::new(cil.0, cil.1, z));
                out.push(hotovo(EntityType::Line(l), b, Druh::Znacka, s.barva_textu));
            }
        }
    }
    for (t, druh, px, py, vyska) in radky {
        let t = Text::with_value(t, Vector3::new(x + px + ox, y + py + oy, z)).with_height(vyska);
        out.push(hotovo(EntityType::Text(t), b, druh, s.barva_textu));
    }
    out
}

/// Objekty nového bodu: POINT s XDATA + značka a popisy podle stylu.
pub fn objekty_bodu(b: &Bod, zdroj: &str, styl: &StylBodu, m: f64) -> Vec<EntityType> {
    let mut out = vec![bod_entita(b, zdroj, styl)];
    out.extend(grafika(b, styl, m));
    out
}

/// Samotný POINT bodu s XDATA (bez značky a popisů).
pub fn bod_entita(b: &Bod, zdroj: &str, styl: &StylBodu) -> EntityType {
    let mut p = Point::from_coords(b.x, b.y, b.z);
    p.common.layer = hladina_bodu(styl).into();
    p.common.extended_data.add_record(xdata_bodu(&b.cislo, &b.popis, zdroj, ""));
    EntityType::Point(p)
}

/// Grafika bodů ve výkrese podle polohy bodu. U popisů ze starších verzí
/// (bez odkazu) se poloha bodu odvodí z rozmístění textu.
/// Obálka objektu značky (x0, y0, x1, y1).
fn obalka(e: &EntityType) -> Option<(f64, f64, f64, f64)> {
    let body: Vec<(f64, f64)> = match e {
        EntityType::Line(l) => vec![(l.start.x, l.start.y), (l.end.x, l.end.y)],
        EntityType::Circle(c) => vec![(c.center.x - c.radius, c.center.y - c.radius), (c.center.x + c.radius, c.center.y + c.radius)],
        EntityType::LwPolyline(p) => p.vertices.iter().map(|v| (v.location.x, v.location.y)).collect(),
        EntityType::Point(p) => vec![(p.location.x, p.location.y)],
        EntityType::Insert(i) => vec![(i.insert_point.x, i.insert_point.y)],
        _ => return None,
    };
    let x0 = body.iter().map(|b| b.0).fold(f64::MAX, f64::min);
    let x1 = body.iter().map(|b| b.0).fold(f64::MIN, f64::max);
    let y0 = body.iter().map(|b| b.1).fold(f64::MAX, f64::min);
    let y1 = body.iter().map(|b| b.1).fold(f64::MIN, f64::max);
    (x0 <= x1).then_some((x0, y0, x1, y1))
}

/// Grafika bodů podle polohy bodu + značky bez odkazu, které k žádnému bodu
/// nepatří (zbytky ze starších verzí).
fn index_grafiky_vse(doc: &CadDocument) -> (HashMap<(i64, i64), Vec<(Druh, Handle)>>, Vec<Handle>) {
    let mut idx: HashMap<(i64, i64), Vec<(Druh, Handle)>> = HashMap::new();
    let mut sirotci = Vec::new();
    let mut polohy: Option<Vec<(f64, f64)>> = None;
    for e in doc.model_space_entities() {
        let Some(druh) = Druh::z_hladiny(&e.common().layer) else { continue };
        let odkaz = e.common().extended_data.get_record(APP_BOD).and_then(|r| {
            let s = r.values.first().map(|v| matches!(v, XDataValue::String(s) if s == POPISEK)).unwrap_or(false);
            let c: Vec<f64> = r.values.iter().filter_map(|v| if let XDataValue::Real(c) = v { Some(*c) } else { None }).collect();
            (s && c.len() >= 2).then(|| klic(c[0], c[1]))
        });
        let k = match (odkaz, e) {
            (Some(k), _) => k,
            (None, EntityType::Text(t)) if druh != Druh::Znacka => {
                let (dx, dy, h) = druh.stare_rozmisteni();
                let vt = t.height / h;
                klic(t.insertion_point.x - dx * vt, t.insertion_point.y - dy * vt)
            }
            // značka bez odkazu (starší verze): patří bodu, který leží v její obálce
            (None, _) if druh == Druh::Znacka => {
                let Some((x0, y0, x1, y1)) = obalka(e) else { continue };
                let polohy = polohy.get_or_insert_with(|| body(doc).iter().map(|b| (b.bod.x, b.bod.y)).collect());
                let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let t = 0.001;
                let nejblizsi = polohy
                    .iter()
                    .filter(|(x, y)| *x >= x0 - t && *x <= x1 + t && *y >= y0 - t && *y <= y1 + t)
                    .min_by(|a, b| (a.0 - cx).hypot(a.1 - cy).total_cmp(&(b.0 - cx).hypot(b.1 - cy)));
                match nejblizsi {
                    Some(&(x, y)) => klic(x, y),
                    None => {
                        sirotci.push(e.common().handle);
                        continue;
                    }
                }
            }
            _ => continue,
        };
        idx.entry(k).or_default().push((druh, e.common().handle));
    }
    (idx, sirotci)
}

fn index_grafiky(doc: &CadDocument) -> HashMap<(i64, i64), Vec<(Druh, Handle)>> {
    index_grafiky_vse(doc).0
}

/// Grafika, která patří k daným polohám bodů. Z každé polohy se bere nejvýš
/// jeden popis každého druhu – dva body na stejném místě si tak popisy
/// nesmažou navzájem.
pub fn grafika_bodu(doc: &CadDocument, polohy: &[[f64; 2]]) -> Vec<Handle> {
    let mut idx = index_grafiky(doc);
    let mut out = Vec::new();
    for p in polohy {
        let Some(v) = idx.get_mut(&klic(p[0], p[1])) else { continue };
        for druh in [Druh::Cislo, Druh::Vyska, Druh::Popis] {
            if let Some(i) = v.iter().position(|(d, _)| *d == druh) {
                out.push(v.remove(i).1);
            }
        }
        // značka (víc objektů) – celá
        let (zn, zbytek): (Vec<_>, Vec<_>) = std::mem::take(v).into_iter().partition(|(d, _)| *d == Druh::Znacka);
        out.extend(zn.into_iter().map(|(_, h)| h));
        *v = zbytek;
    }
    out
}

/// Nesoulad grafiky s body po přesunu, smazání či kopírování bodů:
/// grafika bez bodu (nebo daleko od něj) a body bez grafiky.
pub struct Nesoulad {
    pub navic: Vec<Handle>,
    pub chybi: Vec<BodVykresu>,
}

impl Nesoulad {
    pub fn je_prazdny(&self) -> bool {
        self.navic.is_empty() && self.chybi.is_empty()
    }
}

/// Najde nesoulad grafiky a bodů. `m` = měřítko; grafika dál než 50 mm
/// (na papíře) od svého bodu se považuje za kopii a odstraní se.
pub fn nesoulad(doc: &CadDocument, m: f64) -> Nesoulad {
    let body = body(doc);
    let ro = Rozlisovac::new(doc);
    let mut poloh: HashMap<(i64, i64), usize> = HashMap::new();
    for b in &body {
        *poloh.entry(klic(b.bod.x, b.bod.y)).or_default() += 1;
    }
    let mez = 0.05 * m;
    // včetně popisů ze starších verzí bez odkazu
    let (idx, sirotci) = index_grafiky_vse(doc);
    let mut s_grafikou: std::collections::HashSet<(i64, i64)> = idx.into_keys().collect();
    let mut navic = sirotci;
    for e in doc.model_space_entities() {
        if Druh::z_hladiny(&e.common().layer).is_none() {
            continue;
        }
        let Some(r) = e.common().extended_data.get_record(APP_BOD) else { continue };
        let c: Vec<f64> = r.values.iter().filter_map(|v| if let XDataValue::Real(c) = v { Some(*c) } else { None }).collect();
        if c.len() < 2 {
            continue;
        }
        let k = klic(c[0], c[1]);
        let misto = match e {
            EntityType::Text(t) => Some((t.insertion_point.x, t.insertion_point.y)),
            EntityType::Circle(t) => Some((t.center.x, t.center.y)),
            EntityType::Line(t) => Some((t.start.x, t.start.y)),
            EntityType::LwPolyline(t) => t.vertices.first().map(|v| (v.location.x, v.location.y)),
            _ => None,
        };
        let daleko = misto.is_some_and(|(x, y)| (x - c[0]).hypot(y - c[1]) > mez);
        if !poloh.contains_key(&k) || daleko {
            navic.push(e.common().handle);
        } else {
            s_grafikou.insert(k);
        }
    }
    let chybi = body
        .into_iter()
        .filter(|b| !s_grafikou.contains(&klic(b.bod.x, b.bod.y)))
        .filter(|b| !grafika(&b.bod, &ro.styl_bodu(b), m).is_empty())
        .collect();
    Nesoulad { navic, chybi }
}

/// Odstraní přebytečnou grafiku a doplní chybějící.
pub fn oprav_nesoulad(v: &mut dyn Vykres, doc: &CadDocument, n: &Nesoulad) -> Result<(), String> {
    let m = crate::nastaveni::meritko(doc);
    let ro = Rozlisovac::new(doc);
    smaz_objekty(v, n.navic.clone());
    let polozky: Vec<(Bod, StylBodu)> = n.chybi.iter().map(|b| (b.bod.clone(), ro.styl_bodu(b))).collect();
    let ostatni: Vec<[f64; 2]> = body(doc).iter().map(|b| [b.bod.x, b.bod.y]).collect();
    zajisti_bloky_znacek(v, doc)?;
    let nove = grafika_rozmistena(&polozky, &ostatni, m);
    if !nove.is_empty() {
        priprav_vykres(v)?;
        v.pridej(nove)?;
    }
    Ok(())
}

/// Body vybrané ve výkrese – přímo, nebo přes jejich značku či popis.
pub fn body_z_vyberu(doc: &CadDocument, vyber: &[Handle]) -> Vec<Handle> {
    if vyber.is_empty() {
        return Vec::new();
    }
    let vsechny = body(doc);
    let mut out: Vec<Handle> = vsechny.iter().map(|b| b.handle).filter(|h| vyber.contains(h)).collect();
    let idx = index_grafiky(doc);
    let klice: std::collections::HashSet<(i64, i64)> =
        idx.iter().filter(|(_, v)| v.iter().any(|(_, h)| vyber.contains(h))).map(|(k, _)| *k).collect();
    if !klice.is_empty() {
        for b in &vsechny {
            if klice.contains(&klic(b.bod.x, b.bod.y)) && !out.contains(&b.handle) {
                out.push(b.handle);
            }
        }
    }
    out
}

/// Všechna grafika bodů (značky a popisy).
pub fn vsechna_grafika(doc: &CadDocument) -> Vec<Handle> {
    doc.model_space_entities()
        .filter(|e| Druh::z_hladiny(&e.common().layer).is_some())
        .map(|e| e.common().handle)
        .collect()
}

/// Hladina bodu podle stylu (skryté body jsou na vypnuté hladině).
/// Hladina bodu (POINT). Skryté body jsou na vypnuté hladině. Značku bodu
/// kreslí vždy styl – samotný POINT je neviditelný (PDMODE = 1).
pub fn hladina_bodu(s: &StylBodu) -> &'static str {
    if s.skryt {
        H_SKRYTE
    } else {
        H_BODY
    }
}

/// Upravený bod (POINT) se zachovaným handlem.
pub fn upraveny_bod(doc: &CadDocument, bv: &BodVykresu, novy: &Bod, styl: &str) -> Option<EntityType> {
    let e = doc.model_space_entities().find(|e| e.common().handle == bv.handle)?;
    let EntityType::Point(p) = e else { return None };
    let mut p = p.clone();
    let ro = Rozlisovac::new(doc);
    p.common.layer = hladina_bodu(&ro.styl(novy, &bv.zdroj, styl)).into();
    p.location = Vector3::new(novy.x, novy.y, novy.z);
    p.common.extended_data.remove_record(APP_BOD);
    p.common.extended_data.add_record(xdata_bodu(&novy.cislo, &novy.popis, &bv.zdroj, styl));
    Some(EntityType::Point(p))
}

/// Smaže objekty – po jednom, větší počet najednou (přes dočasný blok,
/// aby to byl jeden požadavek).
pub fn smaz_objekty(v: &mut dyn Vykres, handly: Vec<Handle>) -> usize {
    if handly.is_empty() {
        return 0;
    }
    if handly.len() <= 4 {
        return handly.into_iter().filter(|h| v.odeber(*h)).count();
    }
    let n = handly.len();
    let jmeno = format!("TER_KOS_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_micros()));
    let blok = v.tabulka(ocs_plugin_api::host::TableOperation::BlockCreate {
        name: jmeno.clone(),
        entities: handly.clone(),
        base_point: [0.0, 0.0, 0.0],
        erase_originals: true,
        description: None,
    });
    match blok {
        Ok(_) => {
            let _ = v.tabulka(ocs_plugin_api::host::TableOperation::BlockDelete { name: jmeno });
            n
        }
        Err(_) => handly.into_iter().filter(|h| v.odeber(*h)).count(),
    }
}

/// Změna bodu: (bod ve výkrese, nový bod, nový vlastní styl).
pub type ZmenaBodu = (BodVykresu, Bod, String);

/// Upraví body (poloha, číslo, popis, styl) a překreslí jejich grafiku.
pub fn uprav_body(v: &mut dyn Vykres, doc: &CadDocument, zmeny: &[ZmenaBodu], popis: &str) -> Result<usize, String> {
    let m = crate::nastaveni::meritko(doc);
    let ro = Rozlisovac::new(doc);
    let ent: Vec<EntityType> = zmeny.iter().filter_map(|(bv, b, st)| upraveny_bod(doc, bv, b, st)).collect();
    let n = ent.len();
    v.transakce(popis, ent)?;
    let polohy: Vec<[f64; 2]> = zmeny.iter().map(|(bv, ..)| [bv.bod.x, bv.bod.y]).collect();
    smaz_objekty(v, grafika_bodu(doc, &polohy));
    let polozky: Vec<(Bod, StylBodu)> = zmeny.iter().map(|(bv, b, st)| (b.clone(), ro.styl(b, &bv.zdroj, st))).collect();
    let ostatni: Vec<[f64; 2]> = body(doc)
        .iter()
        .filter(|b| !zmeny.iter().any(|(bv, ..)| bv.handle == b.handle))
        .map(|b| [b.bod.x, b.bod.y])
        .collect();
    zajisti_bloky_znacek(v, doc)?;
    let nove = grafika_rozmistena(&polozky, &ostatni, m);
    if !nove.is_empty() {
        priprav_vykres(v)?;
        v.pridej(nove)?;
    }
    Ok(n)
}

/// Smaže body i s grafikou.
pub fn smaz_body(v: &mut dyn Vykres, doc: &CadDocument, body: &[BodVykresu]) -> usize {
    let polohy: Vec<[f64; 2]> = body.iter().map(|b| [b.bod.x, b.bod.y]).collect();
    let mut h: Vec<Handle> = body.iter().map(|b| b.handle).collect();
    h.extend(grafika_bodu(doc, &polohy));
    smaz_objekty(v, h);
    body.len()
}

/// Smaže grafiku všech bodů a vytvoří ji znovu podle stylů a měřítka.
pub fn obnov_grafiku(v: &mut dyn Vykres, doc: &CadDocument) -> Result<usize, String> {
    let m = crate::nastaveni::meritko(doc);
    let ro = Rozlisovac::new(doc);
    let body = body(doc);
    let mapa_bodu: HashMap<Handle, &Point> = doc
        .model_space_entities()
        .filter_map(|e| if let EntityType::Point(p) = e { Some((p.common.handle, p)) } else { None })
        .collect();
    // body, kterým se změnila viditelnost (styl Nezobrazovat)
    let presun: Vec<EntityType> = body
        .iter()
        .filter_map(|b| {
            let h = hladina_bodu(&ro.styl_bodu(b));
            let p = *mapa_bodu.get(&b.handle)?;
            if p.common.layer.eq_ignore_ascii_case(h) {
                return None;
            }
            let mut p = p.clone();
            p.common.layer = h.into();
            Some(EntityType::Point(p))
        })
        .collect();
    // body ze starších verzí bez třídy (název ve Vlastnostech) – doplnit
    let presun_h: std::collections::HashSet<Handle> = presun.iter().map(|e| e.common().handle).collect();
    let mut presun = presun;
    let bez_tridy: HashMap<Handle, &Point> = doc
        .model_space_entities()
        .filter_map(|e| if let EntityType::Point(p) = e { Some(p) } else { None })
        .filter(|p| {
            !p.common.extended_data.get_record(APP_BOD).is_some_and(|r| {
                r.values.iter().any(|v| matches!(v, XDataValue::String(s) if s.starts_with(crate::povrchy::TRIDA)))
            })
        })
        .map(|p| (p.common.handle, p))
        .collect();
    for b in &body {
        if presun_h.contains(&b.handle) {
            continue;
        }
        if let Some(p) = bez_tridy.get(&b.handle) {
            let mut p = (*p).clone();
            p.common.extended_data.remove_record(APP_BOD);
            p.common.extended_data.add_record(xdata_bodu(&b.bod.cislo, &b.bod.popis, &b.zdroj, &b.styl));
            presun.push(EntityType::Point(p));
        }
    }
    if !presun.is_empty() {
        priprav_vykres(v)?;
        v.transakce(tr("Body"), presun)?;
    }
    smaz_objekty(v, vsechna_grafika(doc));
    let polozky: Vec<(Bod, StylBodu)> = body.iter().map(|b| (b.bod.clone(), ro.styl_bodu(b))).collect();
    zajisti_bloky_znacek(v, doc)?;
    let nove = grafika_rozmistena(&polozky, &[], m);
    if !nove.is_empty() {
        priprav_vykres(v)?;
        v.pridej(nove)?;
    }
    Ok(body.len())
}

// ── Příprava výkresu ─────────────────────────────────────────────────────────

/// Hladiny bodů a registrace XDATA aplikací (aby šly objekty s XDATA
/// vkládat i z interaktivního příkazu).
pub fn priprav_vykres(v: &mut dyn Vykres) -> Result<(), String> {
    let doc = v.dokument()?;
    // značku bodu kreslí styl – vlastní značka bodu
    // Open CADu (PDMODE) by se kreslila přes ni
    if doc.header.point_display_mode != 1 {
        v.prikaz("PDMODE 1");
    }
    for (nazev, barva, vypnuto) in
        [(H_BODY, 30, false), (H_SKRYTE, 30, true), (H_ZNACKA, 30, false), (H_CISLO, 7, false), (H_VYSKA, 7, false), (H_POPIS, 8, false), (H_SKUPINY, 8, true)]
    {
        if !doc.layers.iter().any(|l| l.name.eq_ignore_ascii_case(nazev)) {
            v.hladina(LayerConfig {
                name: nazev.into(),
                color: Some(Color::from_index(barva)),
                off: vypnuto.then_some(true),
                plottable: vypnuto.then_some(false),
                description: vypnuto.then(|| {
                    if nazev == H_SKRYTE { tr("Skryté body (styl Nezobrazovat)") } else { tr("Definice skupin bodů (plugin Terén) – nemazat") }.into()
                }),
                ..Default::default()
            });
        }
    }
    zajisti_bloky_znacek(v, &doc)?;
    for app in [APP_BOD, APP_POVRCH] {
        if !doc.app_ids.iter().any(|a| a.name.eq_ignore_ascii_case(app)) {
            // XDATA aplikace se v DWG registruje zápisem záznamu – na pomocný bod
            let h = v.pridej_jednu(EntityType::Point(Point::from_coords(0.0, 0.0, 0.0)))?;
            v.zapis_xdata(h, ExtendedDataRecord::new(app));
            v.odeber(h);
        }
    }
    Ok(())
}

// ── Skupiny bodů ─────────────────────────────────────────────────────────────

pub fn xdata_skupiny(s: &SkupinaBodu) -> ExtendedDataRecord {
    let mut r = ExtendedDataRecord::new(APP_POVRCH);
    r.add_value(XDataValue::String(SKUPINA.into()));
    for t in s.zapis() {
        r.add_value(XDataValue::String(t));
    }
    r
}

/// Pomocný objekt nesoucí definici skupiny.
pub fn nosic_skupiny(s: &SkupinaBodu, poloha: [f64; 3]) -> EntityType {
    let mut p = Point::from_coords(poloha[0], poloha[1], poloha[2]);
    p.common.layer = H_SKUPINY.into();
    p.common.extended_data.add_record(xdata_skupiny(s));
    EntityType::Point(p)
}

/// Skupiny uložené ve výkrese (bez „Všechny body“), s handlem nosiče.
pub fn ulozene_skupiny(doc: &CadDocument) -> Vec<(Handle, SkupinaBodu)> {
    doc.model_space_entities()
        .filter_map(|e| {
            let r = e.common().extended_data.get_record(APP_POVRCH)?;
            let t = retezce(r);
            if t.first() != Some(&SKUPINA) {
                return None;
            }
            SkupinaBodu::precti(&t[1..]).map(|s| (e.common().handle, s))
        })
        .collect()
}

/// Všechny skupiny: „Všechny body“ a uložené.
pub fn skupiny(doc: &CadDocument) -> Vec<SkupinaBodu> {
    let mut v = vec![SkupinaBodu::vsechny()];
    v.extend(ulozene_skupiny(doc).into_iter().map(|(_, s)| s));
    v
}

pub fn najdi_skupinu(doc: &CadDocument, id: &str) -> Option<SkupinaBodu> {
    if id == VSECHNY {
        return Some(SkupinaBodu::vsechny());
    }
    ulozene_skupiny(doc).into_iter().map(|(_, s)| s).find(|s| s.id.eq_ignore_ascii_case(id))
}

/// Body, které patří aspoň do jedné ze skupin (sjednocení), v pořadí výkresu.
pub fn body_skupin<'a>(body: &'a [BodVykresu], skupiny: &[SkupinaBodu]) -> Vec<&'a BodVykresu> {
    let filtry: Vec<_> = skupiny.iter().map(|s| s.filtr()).collect();
    body.iter().filter(|b| filtry.iter().any(|f| f.obsahuje(&b.bod, &b.zdroj))).collect()
}

/// Identifikátor z jména: velká písmena bez diakritiky, mezery → `_`.
pub fn bez_diakritiky(jmeno: &str) -> String {
    let zaklad: String = jmeno
        .chars()
        .map(|c| match c {
            'á' | 'Á' => 'A',
            'č' | 'Č' => 'C',
            'ď' | 'Ď' => 'D',
            'é' | 'É' | 'ě' | 'Ě' => 'E',
            'í' | 'Í' => 'I',
            'ň' | 'Ň' => 'N',
            'ó' | 'Ó' => 'O',
            'ř' | 'Ř' => 'R',
            'š' | 'Š' => 'S',
            'ť' | 'Ť' => 'T',
            'ú' | 'Ú' | 'ů' | 'Ů' => 'U',
            'ý' | 'Ý' => 'Y',
            'ž' | 'Ž' => 'Z',
            c if c.is_ascii_alphanumeric() => c.to_ascii_uppercase(),
            _ => '_',
        })
        .collect();
    zaklad.trim_matches('_').chars().take(24).collect()
}

/// Identifikátor skupiny bez diakritiky a mezer, volný ve výkrese.
pub fn volne_id_skupiny(doc: &CadDocument, jmeno: &str) -> String {
    let mut zaklad = bez_diakritiky(jmeno);
    if zaklad.is_empty() {
        zaklad = "SKUPINA".into();
    }
    let obsazena: Vec<String> = ulozene_skupiny(doc).into_iter().map(|(_, s)| s.id.to_uppercase()).collect();
    if !obsazena.contains(&zaklad) {
        return zaklad;
    }
    (2..).map(|n| format!("{zaklad}_{n}")).find(|id| !obsazena.contains(id)).unwrap()
}

/// Jméno skupiny, které ve výkrese ještě není.
pub fn volne_jmeno_skupiny(doc: &CadDocument, zaklad: &str) -> String {
    let jmena: Vec<String> = ulozene_skupiny(doc).into_iter().map(|(_, s)| s.jmeno.to_lowercase()).collect();
    if !jmena.contains(&zaklad.to_lowercase()) {
        return zaklad.to_string();
    }
    (2..).map(|n| format!("{zaklad} {n}")).find(|j| !jmena.contains(&j.to_lowercase())).unwrap()
}

/// Uloží skupinu (novou vloží, existující přepíše).
pub fn uloz_skupinu(v: &mut dyn Vykres, doc: &CadDocument, s: &SkupinaBodu) -> Result<(), String> {
    if s.id == VSECHNY {
        return Err(tr("skupinu Všechny body nelze měnit").into());
    }
    if let Some((h, _)) = ulozene_skupiny(doc).into_iter().find(|(_, x)| x.id.eq_ignore_ascii_case(&s.id)) {
        if !v.zapis_xdata(h, xdata_skupiny(s)) {
            return Err(tr("skupinu se nepodařilo uložit").into());
        }
        return Ok(());
    }
    priprav_vykres(v)?;
    // nosič se položí do těžiště bodů skupiny (aby neovlivnil rozsah výkresu)
    let vse = body(doc);
    let vybrane = body_skupin(&vse, std::slice::from_ref(s));
    let zdroj: Vec<&BodVykresu> = if vybrane.is_empty() { vse.iter().collect() } else { vybrane };
    let poloha = if zdroj.is_empty() {
        [0.0, 0.0, 0.0]
    } else {
        let n = zdroj.len() as f64;
        [zdroj.iter().map(|b| b.bod.x).sum::<f64>() / n, zdroj.iter().map(|b| b.bod.y).sum::<f64>() / n, 0.0]
    };
    v.pridej_jednu(nosic_skupiny(s, poloha))?;
    Ok(())
}

pub fn smaz_skupinu(v: &mut dyn Vykres, doc: &CadDocument, id: &str) -> Result<(), String> {
    let (h, _) = ulozene_skupiny(doc)
        .into_iter()
        .find(|(_, x)| x.id.eq_ignore_ascii_case(id))
        .ok_or(tr("skupina už ve výkrese není"))?;
    if v.odeber(h) { Ok(()) } else { Err(tr("skupinu se nepodařilo smazat").into()) }
}

/// Identifikátor skupiny bodů zaměření.
pub const ZAMERENI: &str = "ZAMERENI";

/// Skupina „Zaměření“: všechny body kromě ručně vytvořených.
pub fn skupina_zamereni() -> SkupinaBodu {
    SkupinaBodu {
        id: ZAMERENI.into(),
        jmeno: tr("Zaměření").into(),
        popis: tr("všechny body kromě ručně vytvořených").into(),
        vse: true,
        bez_rucnich: true,
        ..Default::default()
    }
}

/// Další volné číslo bodu ve výkrese.
pub fn dalsi_cislo(doc: &CadDocument) -> u64 {
    let b = body(doc);
    skupiny::dalsi_cislo(b.iter().map(|b| b.bod.cislo.as_str()))
}

/// Pomocné body Terénu (definice stylů a skupin) uložené v počátku 0,0 –
/// vznikly v prázdném výkresu – přesune k bodům zaměření, aby „Zoom –
/// vše“ neukazoval i počátek souřadnic. Vrací, zda něco přesunul.
pub fn presun_nosice(v: &mut dyn Vykres, doc: &CadDocument) -> bool {
    let nosice: Vec<EntityType> = doc
        .model_space_entities()
        .filter(|e| {
            let l = &e.common().layer;
            l.eq_ignore_ascii_case(H_SKUPINY) || l.eq_ignore_ascii_case(crate::styly::H_STYLY)
        })
        .filter(|e| matches!(e, EntityType::Point(p) if p.location.x == 0.0 && p.location.y == 0.0))
        .cloned()
        .collect();
    if nosice.is_empty() {
        return false;
    }
    let b = body(doc);
    if b.is_empty() {
        return false;
    }
    let n = b.len() as f64;
    let (x, y) = (b.iter().map(|b| b.bod.x).sum::<f64>() / n, b.iter().map(|b| b.bod.y).sum::<f64>() / n);
    v.zpet(tr("Terén"));
    let mut zmena = false;
    for mut e in nosice {
        if let EntityType::Point(p) = &mut e {
            p.location.x = x;
            p.location.y = y;
        }
        zmena |= v.uprav(e);
    }
    zmena
}
