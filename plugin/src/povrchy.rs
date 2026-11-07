//! Pojmenované povrchy uložené ve výkrese.
//!
//! Každý povrch je jeden blok `TER_POVRCH_<ID>` vložený v počátku. Na vložení
//! bloku (INSERT) je v XDATA `TEREN_POVRCH` uložená definice: jméno, styl,
//! intervaly, max. hrana, vyloučené kódy, úpravy, otisk zdrojů a statistika.
//! Povinné spojnice, hranice a díry jsou obyčejné čáry ve výkrese na hladinách
//! povrchu, takže je jde upravovat běžnými nástroji.

use teren::preklad::tr;
use teren::trf;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ocs_plugin_api::host::codec::entities::text::{TextHorizontalAlignment, TextVerticalAlignment};
use ocs_plugin_api::host::codec::entities::{Face3D, Insert, Line, LwPolyline, Point, Text};
use ocs_plugin_api::host::codec::types::{Color, Vector2, Vector3};
use ocs_plugin_api::host::{
    Document as CadDocument, EntityType, ExtendedDataRecord, Handle, LayerConfig, TableOperation, XDataValue,
};

use teren::body::Bod;
use teren::kontrola::otisk_definice;
use teren::skupiny::VSECHNY;
use teren::povrch::{Definice, Editace, Hlaseni, Povrch};
use teren::vrstevnice::{vrstevnice, NastaveniVrstevnic, Vrstevnice};

use crate::cogo;
use crate::nastaveni;
use crate::styly;
use crate::vykres::Vykres;

pub const APP_POVRCH: &str = "TEREN_POVRCH";
pub const APP_BOD: &str = "TEREN_BOD";
/// Předpona třídy objektu v XDATA (název objektu ve Vlastnostech, API v10).
pub const TRIDA: &str = "TEREN-TRIDA:";

/// Záznam s třídou objektu (pod aplikací TEREN_BOD, kterou deklaruje plugin).
pub fn zaznam_tridy(trida: &str) -> ExtendedDataRecord {
    let mut r = ExtendedDataRecord::new(APP_BOD);
    r.add_value(XDataValue::String(format!("{TRIDA}{trida}")));
    r
}
pub const H_BODY: &str = "TER_BODY";
/// Vypnutá hladina pro povrchy se stylem Nezobrazovat.
pub const H_SKRYTY_POVRCH: &str = "TER_POVRCH_SKRYTY";
const VERZE1: &str = "TEREN1";
const VERZE2: &str = "TEREN2";
/// Tolerance přichycení vrcholu 2D spojnice na bod zaměření [m].
const TOL_BOD: f64 = 0.01;

/// Intervaly vrstevnic (vedlejší, hlavní) nabízené v nabídkách.
pub const INTERVALY: [(f64, f64); 6] = [(0.1, 0.5), (0.2, 1.0), (0.25, 1.0), (0.5, 2.5), (1.0, 5.0), (2.0, 10.0)];
/// Maximální délky hran nabízené v nabídkách (0 = bez omezení).
pub const HRANY: [f64; 5] = [0.0, 20.0, 40.0, 80.0, 150.0];

// ── Záznam povrchu ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct Uprava {
    pub e: Editace,
    pub zapnuto: bool,
}

/// Skupina bodů v definici povrchu.
#[derive(Clone, Debug, PartialEq)]
pub struct SkupinaVPovrchu {
    pub id: String,
    pub zapnuto: bool,
}

#[derive(Clone, Debug, Default)]
pub struct StatUlozena {
    pub trojuhelniku: usize,
    pub z_min: f64,
    pub z_max: f64,
    pub plocha_2d: f64,
    pub plocha_3d: f64,
    pub bodu: usize,
}

#[derive(Clone, Debug)]
pub struct ZaznamPovrchu {
    /// Handle vložení bloku (`NULL` u povrchu, který ještě není ve výkrese).
    pub insert: Handle,
    /// Identifikátor bez diakritiky – část názvu bloku a hladin.
    pub id: String,
    /// Jméno pro zobrazení.
    pub jmeno: String,
    /// Id stylu povrchu (vestavěný nebo uložený ve výkrese).
    pub styl: String,
    pub interval: f64,
    pub hlavni: f64,
    pub max_hrana: Option<f64>,
    /// Skupiny bodů, ze kterých se povrch skládá.
    pub skupiny: Vec<SkupinaVPovrchu>,
    /// Vypnuté položky definice (čáry, objekty výkresu) – zůstávají, ale nepoužijí se.
    pub vypnute: Vec<Handle>,
    /// Kódy bodů, které se do povrchu neberou.
    pub vyloucene_kody: Vec<String>,
    /// Automatický přepočet při změně zdrojů.
    pub auto: bool,
    pub upravy: Vec<Uprava>,
    /// Otisk zdrojů při posledním přepočtu.
    pub otisk: String,
    pub stat: StatUlozena,
    /// Blok, na který vložení skutečně odkazuje.
    pub blok_vlozeni: String,
}

pub struct Hladiny {
    pub povrch: String,
    pub tin: String,
    pub hlavni: String,
    pub vedlejsi: String,
    pub obrys: String,
    pub popis: String,
    pub spojnice: String,
    pub hranice: String,
    pub diry: String,
    pub upravy: String,
    pub spad: String,
}

impl ZaznamPovrchu {
    pub fn novy(id: String, jmeno: String, body_zamereni: bool) -> Self {
        let n = nastaveni::akt();
        let skupiny =
            if body_zamereni { vec![SkupinaVPovrchu { id: VSECHNY.into(), zapnuto: true }] } else { Vec::new() };
        Self {
            insert: Handle::NULL,
            id,
            jmeno,
            styl: "VRSTP".into(),
            interval: 0.5,
            hlavni: 2.5,
            max_hrana: None,
            skupiny,
            vypnute: Vec::new(),
            vyloucene_kody: if body_zamereni { n.vyloucene_kody } else { Vec::new() },
            auto: false,
            upravy: Vec::new(),
            otisk: String::new(),
            stat: StatUlozena::default(),
            blok_vlozeni: String::new(),
        }
    }

    /// Jméno bloku ve výkrese (skutečné, jinak odvozené z id).
    pub fn blok_ve_vykrese(&self) -> String {
        if self.blok_vlozeni.is_empty() { self.blok() } else { self.blok_vlozeni.clone() }
    }

    pub fn blok(&self) -> String {
        format!("TER_POVRCH_{}", self.id)
    }

    pub fn hladiny(&self) -> Hladiny {
        let p = format!("TER_{}", self.id);
        Hladiny {
            tin: format!("{p}_TIN"),
            hlavni: format!("{p}_VRST_HLAVNI"),
            vedlejsi: format!("{p}_VRST_VEDLEJSI"),
            obrys: format!("{p}_OBRYS"),
            popis: format!("{p}_POPIS"),
            spojnice: format!("{p}_SPOJNICE"),
            hranice: format!("{p}_HRANICE"),
            diry: format!("{p}_DIRY"),
            upravy: format!("{p}_UPRAVY"),
            spad: format!("{p}_SPAD"),
            povrch: p,
        }
    }

    fn xdata(&self) -> ExtendedDataRecord {
        let mut r = ExtendedDataRecord::new(APP_POVRCH);
        let mut s = |t: String| r.add_value(XDataValue::String(t));
        s(VERZE2.into());
        s(format!("id={}", self.id));
        s(format!("jmeno={}", self.jmeno));
        s(format!("styl={}", self.styl));
        s(format!("interval={}", self.interval));
        s(format!("hlavni={}", self.hlavni));
        s(format!("hrana={}", self.max_hrana.unwrap_or(0.0)));
        for g in &self.skupiny {
            s(format!("sk={}|{}", g.zapnuto as u8, g.id));
        }
        for h in &self.vypnute {
            s(format!("vyp={:X}", h.value()));
        }
        s(format!("kody={}", self.vyloucene_kody.join(",")));
        s(format!("auto={}", self.auto as u8));
        s(format!("otisk={}", self.otisk));
        let st = &self.stat;
        s(format!(
            "stat={};{};{};{};{};{}",
            st.trojuhelniku, st.z_min, st.z_max, st.plocha_2d, st.plocha_3d, st.bodu
        ));
        for u in &self.upravy {
            s(format!("u={}|{}", u.zapnuto as u8, u.e.zapis()));
        }
        r
    }

    fn z_xdata(insert: Handle, r: &ExtendedDataRecord) -> Option<Self> {
        let t: Vec<String> = r
            .values
            .iter()
            .filter_map(|v| if let XDataValue::String(s) = v { Some(s.clone()) } else { None })
            .collect();
        match t.first().map(String::as_str) {
            Some(VERZE2) => {}
            Some(VERZE1) => return Self::z_xdata_v1(insert, r),
            _ => return None,
        }
        let mut z = Self::novy(String::new(), String::new(), true);
        z.insert = insert;
        z.vyloucene_kody.clear();
        z.skupiny.clear();
        let mut body_v03 = None;
        for p in &t[1..] {
            let Some((k, v)) = p.split_once('=') else { continue };
            match k {
                "id" => z.id = v.into(),
                "jmeno" => z.jmeno = v.into(),
                "styl" => z.styl = v.into(),
                "interval" => z.interval = v.parse().unwrap_or(0.5),
                "hlavni" => z.hlavni = v.parse().unwrap_or(2.5),
                "hrana" => z.max_hrana = v.parse::<f64>().ok().filter(|h| *h > 0.0),
                "body" => body_v03 = Some(v != "0"),
                "sk" => {
                    if let Some((zap, id)) = v.split_once('|') {
                        z.skupiny.push(SkupinaVPovrchu { id: id.into(), zapnuto: zap != "0" });
                    }
                }
                "vyp" => {
                    if let Ok(h) = u64::from_str_radix(v, 16) {
                        z.vypnute.push(Handle::new(h));
                    }
                }
                "kody" => z.vyloucene_kody = v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect(),
                "auto" => z.auto = v == "1",
                "otisk" => z.otisk = v.into(),
                "stat" => {
                    let c: Vec<f64> = v.split(';').filter_map(|x| x.parse().ok()).collect();
                    if c.len() == 6 {
                        z.stat = StatUlozena {
                            trojuhelniku: c[0] as usize,
                            z_min: c[1],
                            z_max: c[2],
                            plocha_2d: c[3],
                            plocha_3d: c[4],
                            bodu: c[5] as usize,
                        };
                    }
                }
                "u" => {
                    if let Some((zap, e)) = v.split_once('|') {
                        if let Some(e) = Editace::precti(e) {
                            z.upravy.push(Uprava { e, zapnuto: zap != "0" });
                        }
                    }
                }
                _ => {}
            }
        }
        // povrch z verze 0.3 bral body z hladiny TER_BODY = skupina Všechny body
        if body_v03 == Some(true) && z.skupiny.is_empty() {
            z.skupiny.push(SkupinaVPovrchu { id: VSECHNY.into(), zapnuto: true });
        }
        (!z.id.is_empty()).then_some(z)
    }

    pub fn je_vypnuto(&self, h: Handle) -> bool {
        self.vypnute.contains(&h)
    }

    /// Čtení povrchů uložených verzí 0.2.
    fn z_xdata_v1(insert: Handle, r: &ExtendedDataRecord) -> Option<Self> {
        let mut retezce = Vec::new();
        let mut cisla = Vec::new();
        for v in &r.values {
            match v {
                XDataValue::String(t) => retezce.push(t.clone()),
                XDataValue::Real(c) => cisla.push(*c),
                _ => {}
            }
        }
        if retezce.len() < 5 || cisla.len() < 3 {
            return None;
        }
        let mut z = Self::novy(retezce[1].clone(), retezce[2].clone(), retezce[4] != "BEZBODU");
        z.insert = insert;
        z.vyloucene_kody.clear();
        z.styl = retezce[3].clone();
        z.interval = cisla[0];
        z.hlavni = cisla[1];
        z.max_hrana = (cisla[2] > 0.0).then_some(cisla[2]);
        z.upravy = retezce[5..].iter().filter_map(|t| Editace::precti(t)).map(|e| Uprava { e, zapnuto: true }).collect();
        Some(z)
    }
}

/// Všechny povrchy ve výkrese v pořadí, v jakém jsou uložené.
pub fn povrchy(doc: &CadDocument) -> Vec<ZaznamPovrchu> {
    doc.model_space_entities()
        .filter_map(|e| match e {
            EntityType::Insert(i) => i
                .common
                .extended_data
                .get_record(APP_POVRCH)
                .and_then(|r| ZaznamPovrchu::z_xdata(i.common.handle, r))
                .map(|mut z| {
                    z.blok_vlozeni = i.block_name.clone();
                    z
                }),
            _ => None,
        })
        .collect()
}

// ── Značky úprav a přiřazené čáry ───────────────────────────────────────────

const ZNACKA: &str = "UPRAVA";
const PUVODNI: &str = "PUVODNI";

/// XDATA značky úpravy povrchu (objekt na hladině úprav).
pub fn xdata_upravy(id: &str, e: &Editace) -> ExtendedDataRecord {
    let mut r = ExtendedDataRecord::new(APP_POVRCH);
    r.add_value(XDataValue::String(ZNACKA.into()));
    r.add_value(XDataValue::String(id.into()));
    r.add_value(XDataValue::String(e.zapis()));
    r
}

fn retezce(r: &ExtendedDataRecord) -> Vec<&str> {
    r.values.iter().filter_map(|v| if let XDataValue::String(s) = v { Some(s.as_str()) } else { None }).collect()
}

/// Úpravy povrchu uložené jako značky ve výkrese (dosud nepřevzaté do definice).
pub fn znacky_uprav(doc: &CadDocument, id: &str) -> Vec<(Handle, Editace)> {
    doc.model_space_entities()
        .filter_map(|e| {
            let r = e.common().extended_data.get_record(APP_POVRCH)?;
            let t = retezce(r);
            if t.len() == 3 && t[0] == ZNACKA && t[1].eq_ignore_ascii_case(id) {
                Editace::precti(t[2]).map(|ed| (e.common().handle, ed))
            } else {
                None
            }
        })
        .collect()
}

/// Přesune čáru na hladinu povrchu a zapamatuje si původní hladinu.
pub fn prirad_caru(mut e: EntityType, cil: &str) -> EntityType {
    let puvodni = e.common().layer.clone();
    let c = e.common_mut();
    if c.extended_data.get_record(APP_POVRCH).is_none() {
        let mut r = ExtendedDataRecord::new(APP_POVRCH);
        r.add_value(XDataValue::String(PUVODNI.into()));
        r.add_value(XDataValue::String(puvodni));
        c.extended_data.add_record(r);
    }
    c.layer = cil.to_string();
    e
}

/// Vrátí čáru z povrchu na původní hladinu (jinak na hladinu 0).
pub fn uvolni_caru(mut e: EntityType) -> EntityType {
    let c = e.common_mut();
    let puvodni = c
        .extended_data
        .get_record(APP_POVRCH)
        .and_then(|r| {
            let t = retezce(r);
            (t.len() == 2 && t[0] == PUVODNI).then(|| t[1].to_string())
        })
        .unwrap_or_else(|| "0".into());
    c.extended_data.remove_record(APP_POVRCH);
    c.layer = puvodni;
    e
}

// ── Aktivní povrch ───────────────────────────────────────────────────────────

fn aktivni_id() -> &'static Mutex<Option<String>> {
    static A: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(None))
}

pub fn nastav_aktivni(id: &str) {
    *aktivni_id().lock().unwrap_or_else(|e| e.into_inner()) = Some(id.to_string());
}

pub fn aktivni_id_hodnota() -> Option<String> {
    aktivni_id().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Aktivní povrch: naposledy vybraný, jinak poslední ve výkrese.
pub fn aktivni(doc: &CadDocument) -> Result<ZaznamPovrchu, String> {
    let vse = povrchy(doc);
    if vse.is_empty() {
        return Err(tr("ve výkrese zatím není povrch – vytvořte ho tlačítkem Nový povrch (záložka Terén) nebo v Prospektoru pravým tlačítkem na Povrchy").into());
    }
    if let Some(id) = aktivni_id_hodnota() {
        if let Some(z) = vse.iter().find(|z| z.id == id) {
            return Ok(z.clone());
        }
    }
    let z = vse.last().unwrap().clone();
    nastav_aktivni(&z.id);
    Ok(z)
}

/// Volný identifikátor povrchu odvozený od `zaklad`.
pub fn volne_id(doc: &CadDocument, zaklad: &str) -> String {
    let vse = povrchy(doc);
    let obsazeno = |id: &str| vse.iter().any(|z| z.id.eq_ignore_ascii_case(id));
    if !obsazeno(zaklad) {
        return zaklad.to_string();
    }
    (2..).map(|n| format!("{zaklad}{n}")).find(|id| !obsazeno(id)).unwrap()
}

// ── Data z výkresu ────────────────────────────────────────────────────────────

/// Body zaměření (všechny COGO body výkresu).
pub fn body_z_vykresu(doc: &CadDocument) -> Vec<Bod> {
    cogo::body(doc).into_iter().map(|b| b.bod).collect()
}

/// Výška bodu zaměření na dané poloze (do tolerance).
fn vyska_bodu(body: &[Bod], x: f64, y: f64, tol: f64) -> Option<f64> {
    body.iter()
        .map(|b| (b, (b.x - x).hypot(b.y - y)))
        .filter(|(_, d)| *d <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(b, _)| b.z)
}

/// Vrcholy čáry (3D). U 2D křivky se výška bere z bodu zaměření ve vrcholu,
/// jinak z výšky křivky.
fn vrcholy_cary(e: &EntityType, body: &[Bod]) -> Option<(Vec<[f64; 3]>, bool)> {
    Some(match e {
        EntityType::Polyline3D(p) => (
            p.vertices.iter().map(|v| [v.position.x, v.position.y, v.position.z]).collect(),
            p.is_closed(),
        ),
        EntityType::LwPolyline(p) => (
            p.vertices
                .iter()
                .map(|v| {
                    let (x, y) = (v.location.x, v.location.y);
                    [x, y, vyska_bodu(body, x, y, TOL_BOD).unwrap_or(p.elevation)]
                })
                .collect(),
            p.is_closed,
        ),
        EntityType::Polyline2D(p) => (
            p.vertices
                .iter()
                .map(|v| {
                    let (x, y) = (v.location.x, v.location.y);
                    [x, y, vyska_bodu(body, x, y, TOL_BOD).unwrap_or(p.elevation)]
                })
                .collect(),
            p.is_closed(),
        ),
        EntityType::Line(l) => (vec![[l.start.x, l.start.y, l.start.z], [l.end.x, l.end.y, l.end.z]], false),
        _ => return None,
    })
}

// ── Objekty výkresu v definici ───────────────────────────────────────────────

const OBJEKT: &str = "OBJEKT";

/// Typ objektů výkresu, ze kterých povrch bere body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypObjektu {
    Body,
    Bloky,
    Cary,
    Plochy,
}

pub const TYPY_OBJEKTU: [TypObjektu; 4] = [TypObjektu::Body, TypObjektu::Bloky, TypObjektu::Cary, TypObjektu::Plochy];

impl TypObjektu {
    pub fn kod(self) -> &'static str {
        match self {
            TypObjektu::Body => "P",
            TypObjektu::Bloky => "B",
            TypObjektu::Cary => "C",
            TypObjektu::Plochy => "F",
        }
    }
    pub fn z_kodu(k: &str) -> Option<Self> {
        TYPY_OBJEKTU.into_iter().find(|t| t.kod() == k)
    }
    pub fn nazev(self) -> &'static str {
        match self {
            TypObjektu::Body => tr("Body (POINT)"),
            TypObjektu::Bloky => tr("Bloky (výška z vložení)"),
            TypObjektu::Cary => tr("Čáry a křivky (vrcholy)"),
            TypObjektu::Plochy => tr("3D plochy"),
        }
    }
    pub fn mnozne(self) -> &'static str {
        match self {
            TypObjektu::Body => tr("body"),
            TypObjektu::Bloky => tr("bloky"),
            TypObjektu::Cary => tr("čáry"),
            TypObjektu::Plochy => tr("3D plochy"),
        }
    }
    /// Má smysl volba „zachovat hrany“?
    pub fn ma_hrany(self) -> bool {
        matches!(self, TypObjektu::Cary | TypObjektu::Plochy)
    }
    pub fn odpovida(self, e: &EntityType) -> bool {
        match self {
            TypObjektu::Body => matches!(e, EntityType::Point(_)),
            TypObjektu::Bloky => matches!(e, EntityType::Insert(i) if !i.block_name.to_uppercase().starts_with("TER_POVRCH_")),
            TypObjektu::Cary => matches!(
                e,
                EntityType::Line(_) | EntityType::LwPolyline(_) | EntityType::Polyline2D(_) | EntityType::Polyline3D(_)
            ),
            TypObjektu::Plochy => matches!(e, EntityType::Face3D(_)),
        }
    }
}

/// Příslušnost objektu k povrchům: (id povrchu, typ, zachovat hrany).
fn prislusnost(e: &EntityType) -> Option<Vec<(String, TypObjektu, bool)>> {
    let r = e.common().extended_data.get_record(APP_POVRCH)?;
    let t = retezce(r);
    if t.first() != Some(&OBJEKT) {
        return None;
    }
    Some(
        t[1..]
            .iter()
            .filter_map(|x| {
                let mut c = x.split('|');
                let id = c.next()?.to_string();
                let typ = TypObjektu::z_kodu(c.next()?)?;
                let hrany = c.next() == Some("1");
                Some((id, typ, hrany))
            })
            .collect(),
    )
}

fn zapis_prislusnost(e: &mut EntityType, p: &[(String, TypObjektu, bool)]) {
    let c = e.common_mut();
    c.extended_data.remove_record(APP_POVRCH);
    if p.is_empty() {
        return;
    }
    let mut r = ExtendedDataRecord::new(APP_POVRCH);
    r.add_value(XDataValue::String(OBJEKT.into()));
    for (id, typ, hrany) in p {
        r.add_value(XDataValue::String(format!("{id}|{}|{}", typ.kod(), *hrany as u8)));
    }
    c.extended_data.add_record(r);
}

pub struct PridaniObjektu {
    /// Upravené objekty (zapsat transakcí).
    pub upravit: Vec<EntityType>,
    pub pridano: usize,
    pub uz_bylo: usize,
    pub jineho_typu: usize,
    /// Objekty, které už plugin používá jinak (spojnice, popisy, povrchy…).
    pub obsazene: usize,
}

/// Připraví přidání vybraných objektů do definice povrchu.
pub fn pridej_objekty(doc: &CadDocument, vyber: &[Handle], id: &str, typ: TypObjektu, hrany: bool) -> PridaniObjektu {
    let mut v = PridaniObjektu { upravit: Vec::new(), pridano: 0, uz_bylo: 0, jineho_typu: 0, obsazene: 0 };
    for e in doc.model_space_entities().filter(|e| vyber.contains(&e.common().handle)) {
        if !typ.odpovida(e) {
            v.jineho_typu += 1;
            continue;
        }
        let mut p = match prislusnost(e) {
            Some(p) => p,
            None if e.common().extended_data.get_record(APP_POVRCH).is_some() => {
                v.obsazene += 1;
                continue;
            }
            None => Vec::new(),
        };
        if p.iter().any(|(x, _, _)| x.eq_ignore_ascii_case(id)) {
            v.uz_bylo += 1;
            continue;
        }
        p.push((id.to_string(), typ, hrany && typ.ma_hrany()));
        let mut e = e.clone();
        zapis_prislusnost(&mut e, &p);
        v.upravit.push(e);
        v.pridano += 1;
    }
    v
}

/// Objekt bez příslušnosti k povrchu `id` (pro transakci), `None` = nepatřil.
pub fn odeber_objekt(doc: &CadDocument, h: Handle, id: &str) -> Option<EntityType> {
    let e = doc.model_space_entities().find(|e| e.common().handle == h)?;
    let mut p = prislusnost(e)?;
    let pred = p.len();
    p.retain(|(x, _, _)| !x.eq_ignore_ascii_case(id));
    if p.len() == pred {
        return None;
    }
    let mut e = e.clone();
    zapis_prislusnost(&mut e, &p);
    Some(e)
}

/// Body (a případně hrany) z objektu výkresu.
fn data_objektu(e: &EntityType, typ: TypObjektu, vsechny_body: &[Bod]) -> (Vec<[f64; 3]>, Vec<Vec<[f64; 3]>>) {
    let v3 = |v: &Vector3| [v.x, v.y, v.z];
    match (typ, e) {
        (TypObjektu::Body, EntityType::Point(p)) => (vec![v3(&p.location)], Vec::new()),
        (TypObjektu::Bloky, EntityType::Insert(i)) => (vec![v3(&i.insert_point)], Vec::new()),
        (TypObjektu::Cary, e) => match vrcholy_cary(e, vsechny_body) {
            Some((mut v, uzavrena)) => {
                let body = v.clone();
                if uzavrena && v.len() > 2 {
                    v.push(v[0]);
                }
                (body, vec![v])
            }
            None => (Vec::new(), Vec::new()),
        },
        (TypObjektu::Plochy, EntityType::Face3D(f)) => {
            let mut rohy = vec![v3(&f.first_corner), v3(&f.second_corner), v3(&f.third_corner)];
            let ctvrty = v3(&f.fourth_corner);
            if (ctvrty[0] - rohy[2][0]).hypot(ctvrty[1] - rohy[2][1]) > 1e-9 {
                rohy.push(ctvrty);
            }
            let mut obvod = rohy.clone();
            obvod.push(rohy[0]);
            (rohy, vec![obvod])
        }
        _ => (Vec::new(), Vec::new()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DruhPolozky {
    Spojnice,
    Hranice,
    Dira,
    Objekt(TypObjektu),
}

/// Položka definice povrchu (pro strom v Prospektoru).
#[derive(Clone, Debug)]
pub struct Polozka {
    pub handle: Handle,
    pub druh: DruhPolozky,
    pub vrcholu: usize,
    pub delka: f64,
    pub stred: [f64; 2],
    /// Poloměr oblasti pro přiblížení.
    pub rozmer: f64,
    pub zapnuto: bool,
    /// U objektů výkresu: zachovat hrany.
    pub hrany: bool,
}

pub struct Zdroje {
    /// Nastavení a styl, které mění výsledek – součást otisku.
    pub nastaveni: String,
    pub definice: Definice,
    pub pocet_bodu: usize,
    pub vylouceno_bodu: usize,
    /// Počet bodů v jednotlivých skupinách povrchu (id → počet).
    pub body_skupin: Vec<(String, usize)>,
    pub polozky: Vec<Polozka>,
    /// Úpravy včetně dosud nepřevzatých značek (zapnuté i vypnuté).
    pub upravy: Vec<Uprava>,
}

fn rozsah(v: &[[f64; 3]]) -> ([f64; 2], f64) {
    if v.is_empty() {
        return ([0.0, 0.0], 0.0);
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in v {
        x0 = x0.min(p[0]);
        x1 = x1.max(p[0]);
        y0 = y0.min(p[1]);
        y1 = y1.max(p[1]);
    }
    ([(x0 + x1) / 2.0, (y0 + y1) / 2.0], ((x1 - x0).max(y1 - y0) / 2.0).max(1.0))
}

pub fn definice(doc: &CadDocument, z: &ZaznamPovrchu) -> Zdroje {
    let h = z.hladiny();
    let body_vykresu = cogo::body(doc);
    let vsechny_body: Vec<Bod> = body_vykresu.iter().map(|b| b.bod.clone()).collect();
    let mut upravy = z.upravy.clone();
    upravy.extend(znacky_uprav(doc, &z.id).into_iter().map(|(_, e)| Uprava { e, zapnuto: true }));
    let editace = upravy.iter().filter(|u| u.zapnuto).map(|u| u.e.clone()).collect();
    let mut def = Definice { max_hrana: z.max_hrana, editace, ..Default::default() };
    let mut polozky = Vec::new();
    for e in doc.model_space_entities() {
        let handle = e.common().handle;
        let zapnuto = !z.je_vypnuto(handle);
        // objekty výkresu
        if let Some(p) = prislusnost(e) {
            for (id, typ, hrany) in p.into_iter().filter(|(id, _, _)| id.eq_ignore_ascii_case(&z.id)) {
                let _ = id;
                let (body, cary) = data_objektu(e, typ, &vsechny_body);
                if body.is_empty() {
                    continue;
                }
                let (stred, rozmer) = rozsah(&body);
                let delka: f64 = cary.first().map_or(0.0, |c| c.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum());
                polozky.push(Polozka { handle, druh: DruhPolozky::Objekt(typ), vrcholu: body.len(), delka, stred, rozmer, zapnuto, hrany });
                if zapnuto {
                    def.body.extend(body.iter().map(|p| Bod { cislo: String::new(), x: p[0], y: p[1], z: p[2], popis: String::new() }));
                    if hrany {
                        def.spojnice.extend(cary);
                    }
                }
            }
            continue;
        }
        let vrstva = &e.common().layer;
        let druh = if vrstva.eq_ignore_ascii_case(&h.spojnice) {
            DruhPolozky::Spojnice
        } else if vrstva.eq_ignore_ascii_case(&h.hranice) {
            DruhPolozky::Hranice
        } else if vrstva.eq_ignore_ascii_case(&h.diry) {
            DruhPolozky::Dira
        } else {
            continue;
        };
        let Some((mut v, uzavrena)) = vrcholy_cary(e, &vsechny_body) else { continue };
        let delka: f64 = v.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
        let (stred, rozmer) = rozsah(&v);
        polozky.push(Polozka { handle, druh, vrcholu: v.len(), delka, stred, rozmer, zapnuto, hrany: false });
        if !zapnuto {
            continue;
        }
        match druh {
            DruhPolozky::Spojnice => {
                if uzavrena && v.len() > 2 {
                    v.push(v[0]);
                }
                def.spojnice.push(v);
            }
            _ => {
                let polygon: Vec<[f64; 2]> = v.iter().map(|p| [p[0], p[1]]).collect();
                if polygon.len() >= 3 {
                    if druh == DruhPolozky::Hranice {
                        def.hranice.push(polygon);
                    } else {
                        def.diry.push(polygon);
                    }
                }
            }
        }
    }
    // body ze skupin
    let skupiny = cogo::skupiny(doc);
    let mut body_skupin = Vec::new();
    let mut aktivni = Vec::new();
    for g in &z.skupiny {
        let Some(s) = skupiny.iter().find(|s| s.id.eq_ignore_ascii_case(&g.id)) else {
            body_skupin.push((g.id.clone(), 0));
            continue;
        };
        body_skupin.push((g.id.clone(), cogo::body_skupin(&body_vykresu, std::slice::from_ref(s)).len()));
        if g.zapnuto {
            aktivni.push(s.clone());
        }
    }
    let (mut pocet_bodu, mut vylouceno_bodu) = (0, 0);
    for b in cogo::body_skupin(&body_vykresu, &aktivni) {
        if z.vyloucene_kody.iter().any(|k| k.eq_ignore_ascii_case(b.bod.kod())) {
            vylouceno_bodu += 1;
        } else {
            pocet_bodu += 1;
            def.body.push(b.bod.clone());
        }
    }
    let st = styly::styl_povrchu(doc, &z.styl);
    // měřítko mění jen popisy a šipky
    let zavisi_na_meritku = st.sipky || (st.vrstevnice && st.popis != styly::PopisVrstevnic::Nic);
    let nastaveni = format!(
        "{}|{}|{}|{}|{}",
        st.zapis().join(";"),
        z.interval,
        z.hlavni,
        if zavisi_na_meritku { nastaveni::meritko(doc) } else { 0.0 },
        z.vyloucene_kody.join(",")
    );
    Zdroje { nastaveni, definice: def, pocet_bodu, vylouceno_bodu, body_skupin, polozky, upravy }
}

/// Otisk aktuálních zdrojů povrchu.
pub fn otisk(z: &ZaznamPovrchu, zdroje: &Zdroje) -> String {
    let _ = z;
    otisk_definice(&zdroje.definice, &zdroje.nastaveni)
}

/// Povrch je zastaralý, pokud se jeho zdroje od posledního přepočtu změnily.
pub fn zastaraly(doc: &CadDocument, z: &ZaznamPovrchu) -> bool {
    let zdroje = definice(doc, z);
    !znacky_uprav(doc, &z.id).is_empty() || otisk(z, &zdroje) != z.otisk
}

// ── Přestavění bloku povrchu ─────────────────────────────────────────────────

/// Šipky směru spádu – v těžišti trojúhelníků, které jsou na šipku dost velké.
fn sipky_spadu(p: &Povrch, delka: f64) -> Vec<EntityType> {
    let mut out = Vec::new();
    if delka <= 0.0 {
        return out;
    }
    let min_plocha = delka * delka * 0.35;
    for t in &p.trojuhelniky {
        let [a, b, c] = t.map(|i| p.globalni(i));
        let plocha = ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)) / 2.0;
        if plocha.abs() < min_plocha {
            continue;
        }
        // rovina z = gx·x + gy·y + k
        let d = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
        let gx = ((b.2 - a.2) * (c.1 - a.1) - (c.2 - a.2) * (b.1 - a.1)) / d;
        let gy = ((c.2 - a.2) * (b.0 - a.0) - (b.2 - a.2) * (c.0 - a.0)) / d;
        let g = gx.hypot(gy);
        if g < 1e-4 {
            continue;
        }
        let (ux, uy) = (-gx / g, -gy / g);
        let (sx, sy, sz) = ((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0, (a.2 + b.2 + c.2) / 3.0);
        let (x0, y0) = (sx - ux * delka / 2.0, sy - uy * delka / 2.0);
        let (x1, y1) = (sx + ux * delka / 2.0, sy + uy * delka / 2.0);
        let hrot = delka * 0.3;
        let (cs, sn) = (0.42_f64.cos(), 0.42_f64.sin());
        let l = |a: (f64, f64), b: (f64, f64)| {
            EntityType::Line(Line::from_points(Vector3::new(a.0, a.1, sz), Vector3::new(b.0, b.1, sz)))
        };
        out.push(l((x0, y0), (x1, y1)));
        for s in [sn, -sn] {
            let (rx, ry) = (-ux * cs - -uy * s, -ux * s + -uy * cs);
            out.push(l((x1, y1), (x1 + rx * hrot, y1 + ry * hrot)));
        }
    }
    out
}

/// Posune objekt povrchu (3D plocha, křivka, text) v půdorysu.
fn posun(mut e: EntityType, dx: f64, dy: f64) -> EntityType {
    let p = |v: &mut Vector3| {
        v.x += dx;
        v.y += dy;
    };
    match &mut e {
        EntityType::Face3D(f) => {
            p(&mut f.first_corner);
            p(&mut f.second_corner);
            p(&mut f.third_corner);
            p(&mut f.fourth_corner);
        }
        EntityType::LwPolyline(l) => {
            for v in &mut l.vertices {
                v.location.x += dx;
                v.location.y += dy;
            }
        }
        EntityType::Text(t) => {
            p(&mut t.insertion_point);
            if let Some(a) = t.alignment_point.as_mut() {
                p(a);
            }
        }
        EntityType::Line(l) => {
            p(&mut l.start);
            p(&mut l.end);
        }
        _ => {}
    }
    e
}

fn na_hladine(mut e: EntityType, h: &str) -> EntityType {
    e.common_mut().layer = h.to_string();
    e
}

/// Popisy vrstevnic – text ve směru čáry, čitelný zleva/zdola.
fn popisy_vrstevnic(
    v: &[Vrstevnice],
    vyska: f64,
    rozestup: f64,
    hladina: &str,
    jen_hlavni: bool,
    text_vysky: impl Fn(f64) -> String,
) -> Vec<EntityType> {
    let mut out = Vec::new();
    for c in v.iter().filter(|c| (c.hlavni || !jen_hlavni) && c.body.len() >= 2) {
        let delky: Vec<f64> = c.body.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).collect();
        let celkem: f64 = delky.iter().sum();
        if celkem < vyska * 6.0 {
            continue;
        }
        let pocet = ((celkem / rozestup).floor() as usize).max(1);
        for k in 0..pocet {
            let cil = celkem * (k as f64 + 0.5) / pocet as f64;
            let mut s = 0.0;
            for (i, d) in delky.iter().enumerate() {
                if s + d >= cil && *d > 0.0 {
                    let t = (cil - s) / d;
                    let (a, b) = (c.body[i], c.body[i + 1]);
                    let (x, y) = (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
                    let mut uhel = (b.1 - a.1).atan2(b.0 - a.0);
                    if uhel > std::f64::consts::FRAC_PI_2 {
                        uhel -= std::f64::consts::PI;
                    } else if uhel < -std::f64::consts::FRAC_PI_2 {
                        uhel += std::f64::consts::PI;
                    }
                    let text = text_vysky(c.vyska);
                    let mut t = Text::with_value(text, Vector3::new(x, y, c.vyska)).with_height(vyska).with_rotation(uhel);
                    t.horizontal_alignment = TextHorizontalAlignment::Center;
                    t.vertical_alignment = TextVerticalAlignment::Middle;
                    t.alignment_point = Some(Vector3::new(x, y, c.vyska));
                    out.push(na_hladine(EntityType::Text(t), hladina));
                    break;
                }
                s += d;
            }
        }
    }
    out
}

pub fn zajisti_hladinu(v: &mut dyn Vykres, doc: &CadDocument, nazev: &str, barva: i16) {
    if !doc.layers.iter().any(|l| l.name.eq_ignore_ascii_case(nazev)) {
        v.hladina(LayerConfig { name: nazev.to_string(), color: Some(Color::from_index(barva)), ..Default::default() });
    }
}

fn zajisti_hladiny(v: &mut dyn Vykres, doc: &CadDocument, z: &ZaznamPovrchu) {
    let h = z.hladiny();
    for (nazev, barva) in [
        (&h.povrch, 7),
        (&h.tin, 8),
        (&h.hlavni, 22),
        (&h.vedlejsi, 32),
        (&h.obrys, 140),
        (&h.popis, 22),
        (&h.spojnice, 1),
        (&h.hranice, 5),
        (&h.diry, 5),
        (&h.upravy, 6),
        (&h.spad, 150),
    ] {
        zajisti_hladinu(v, doc, nazev, barva);
    }
}

#[allow(dead_code)]
pub struct Vysledek {
    pub povrch: Povrch,
    pub pocet_bodu: usize,
    pub vylouceno_bodu: usize,
    pub vrstevnic: usize,
    pub spojnic: usize,
    pub hranic: usize,
    pub der: usize,
}

/// Hlášení z posledního přepočtu každého povrchu (pro Prospektor).
pub fn posledni_hlaseni() -> &'static Mutex<HashMap<String, Hlaseni>> {
    static H: OnceLock<Mutex<HashMap<String, Hlaseni>>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Vypočte povrch a nahradí jeho blok ve výkrese. Aktualizuje `z` (vložení, úpravy, otisk, statistika).
pub fn prestav(v: &mut dyn Vykres, z: &mut ZaznamPovrchu) -> Result<Vysledek, String> {
    prestav_s(v, z, None)
}

/// Jako [`prestav`], jen s už načteným snímkem výkresu (o požadavek méně).
pub fn prestav_s(v: &mut dyn Vykres, z: &mut ZaznamPovrchu, doc: Option<CadDocument>) -> Result<Vysledek, String> {
    let doc = match doc {
        Some(d) => d,
        None => v.dokument()?,
    };
    let zdroje = definice(&doc, z);
    let def = &zdroje.definice;
    let povrch = Povrch::z_definice(def, 0.001).map_err(|e| e.to_string())?;
    let h = z.hladiny();

    let st = styly::styl_povrchu(&doc, &z.styl);
    let m = nastaveni::meritko(&doc);
    let barvene = |e: EntityType, hl: &str, barva: styly::Barva, tl: i16| {
        let mut e = na_hladine(e, hl);
        e.common_mut().color = styly::barva(barva);
        e.common_mut().line_weight = styly::tloustka(tl);
        e
    };
    let mut ent: Vec<EntityType> = Vec::new();
    if st.tin {
        for t in &povrch.trojuhelniky {
            let [a, b, c] = t.map(|i| {
                let (x, y, zz) = povrch.globalni(i);
                Vector3::new(x, y, zz)
            });
            ent.push(barvene(EntityType::Face3D(Face3D::triangle(a, b, c)), &h.tin, st.barva_tin, -1));
        }
    }
    let (interval, hlavni) = st.interval.unwrap_or((z.interval, z.hlavni));
    let vrst = if st.vrstevnice {
        vrstevnice(&povrch, &NastaveniVrstevnic { interval, hlavni })
    } else {
        Vec::new()
    };
    for c in &vrst {
        let mut pl = LwPolyline::from_points(c.body.iter().map(|&(x, y)| Vector2::new(x, y)).collect());
        pl.elevation = c.vyska;
        pl.is_closed = c.uzavrena;
        ent.push(if c.hlavni {
            barvene(EntityType::LwPolyline(pl), &h.hlavni, st.barva_hlavni, st.tloustka_hlavni)
        } else {
            barvene(EntityType::LwPolyline(pl), &h.vedlejsi, st.barva_vedlejsi, st.tloustka_vedlejsi)
        });
    }
    if st.popis != styly::PopisVrstevnic::Nic && !vrst.is_empty() {
        let jen_hlavni = st.popis == styly::PopisVrstevnic::Hlavni;
        for e in popisy_vrstevnic(&vrst, st.pismo * m / 1000.0, st.rozestup * m / 1000.0, &h.popis, jen_hlavni, |z| st.text_vysky(z)) {
            ent.push(barvene(e, &h.popis, st.barva_popisu, -1));
        }
    }
    if st.sipky {
        for e in sipky_spadu(&povrch, st.delka_sipky * m / 1000.0) {
            ent.push(barvene(e, &h.spad, st.barva_sipek, -1));
        }
    }
    if st.obrys {
        for o in povrch.obrys() {
            let mut pl = LwPolyline::from_points(o.iter().map(|&(x, y, _)| Vector2::new(x, y)).collect());
            pl.is_closed = true;
            ent.push(barvene(EntityType::LwPolyline(pl), &h.obrys, st.barva_obrys, -1));
        }
    }
    if povrch.trojuhelniky.is_empty() {
        return Err(tr("povrch nemá žádné trojúhelníky – zkontrolujte hranici (neořízla vše?), díry a skupiny bodů v Prospektoru").into());
    }
    if ent.is_empty() {
        // styl Nezobrazovat – blok potřebuje aspoň jeden objekt: bod na vypnuté hladině
        if !doc.layers.iter().any(|l| l.name.eq_ignore_ascii_case(H_SKRYTY_POVRCH)) {
            v.hladina(LayerConfig {
                name: H_SKRYTY_POVRCH.into(),
                off: Some(true),
                plottable: Some(false),
                description: Some(tr("Skryté povrchy (styl Nezobrazovat)").into()),
                ..Default::default()
            });
        }
        let (x, y) = povrch.pocatek;
        ent.push(na_hladine(EntityType::Point(Point::from_coords(x, y, 0.0)), H_SKRYTY_POVRCH));
    }

    zajisti_hladiny(v, &doc, z);
    // základní bod bloku v povrchu (ne v počátku – jinak by „zoom vše“ zahrnul i počátek)
    // Vložení bloku leží v povrchu, ne v počátku: Open CAD Studio počítá
    // „zoom vše“ z geometrie bloku bez transformace a z bodu vložení. Objekty
    // se proto před vytvořením bloku posunou o základní bod – v bloku pak mají
    // skutečné souřadnice a vložení v základním bodě je vykreslí na místě.
    let zaklad = [povrch.pocatek.0, povrch.pocatek.1, 0.0];
    let ent: Vec<EntityType> = ent.into_iter().map(|e| posun(e, zaklad[0], zaklad[1])).collect();
    // Nový blok se nejdřív postaví pod dočasným jménem a vloží; starý povrch
    // se odstraní až potom. Když cokoli selže, původní povrch zůstane.
    let docasny = format!("{}_NOVY", z.blok());
    if doc.block_records.iter().any(|b| b.name.eq_ignore_ascii_case(&docasny)) {
        let _ = v.tabulka(TableOperation::BlockDelete { name: docasny.clone() });
    }
    let handly = v.pridej(ent)?;
    v.tabulka(TableOperation::BlockCreate {
        name: docasny.clone(),
        entities: handly,
        base_point: zaklad,
        erase_originals: true,
        description: Some(trf!("Povrch {} (plugin Terén)", z.jmeno)),
    })
    .map_err(|e| trf!("blok povrchu nejde vytvořit: {}", e))?;
    // starý povrch pryč
    if z.insert != Handle::NULL {
        v.odeber(z.insert);
    }
    let stary = z.blok_ve_vykrese();
    if !stary.eq_ignore_ascii_case(&docasny) && doc.block_records.iter().any(|b| b.name.eq_ignore_ascii_case(&stary)) {
        if let Err(e) = v.tabulka(TableOperation::BlockDelete { name: stary }) {
            v.chyba(&trf!("Terén: starý blok povrchu nejde odstranit: {}", e));
        }
    }
    let jmeno_bloku = match v.tabulka(TableOperation::BlockRename { from: docasny.clone(), to: z.blok() }) {
        Ok(_) => z.blok(),
        Err(_) => docasny,
    };
    z.blok_vlozeni = jmeno_bloku.clone();
    let mut ins = Insert::new(jmeno_bloku, Vector3::new(zaklad[0], zaklad[1], zaklad[2]));
    ins.common.layer = h.povrch.clone();
    // třída objektu rovnou do vložení, když už je aplikace ve výkrese registrovaná
    let trida_hned = doc.app_ids.iter().any(|a| a.name.eq_ignore_ascii_case(APP_BOD));
    if trida_hned {
        ins.common.extended_data.add_record(zaznam_tridy("POVRCH"));
    }
    let insert = v.pridej_jednu(EntityType::Insert(ins))?;
    z.insert = insert;
    // značky úprav se převezmou do definice a z výkresu zmizí
    let znacky = znacky_uprav(&doc, &z.id);
    for (hn, e) in &znacky {
        z.upravy.push(Uprava { e: e.clone(), zapnuto: true });
        v.odeber(*hn);
    }

    let s = povrch.statistika();
    z.stat = StatUlozena {
        trojuhelniku: s.pocet_trojuhelniku,
        z_min: s.z_min,
        z_max: s.z_max,
        plocha_2d: s.plocha_2d,
        plocha_3d: s.plocha_3d,
        bodu: zdroje.pocet_bodu,
    };
    // otisk se počítá ze zdrojů bez značek (ty jsou teď součástí z.upravy)
    z.otisk = otisk(z, &zdroje);
    if !v.zapis_xdata(insert, z.xdata()) {
        return Err(tr("nepodařilo se uložit definici povrchu").into());
    }
    if !trida_hned {
        v.zapis_xdata(insert, zaznam_tridy("POVRCH"));
    }
    if nastaveni::akt().povrch_nevybirat {
        v.jen_zobrazeni(insert, true);
    }
    posledni_hlaseni().lock().unwrap_or_else(|e| e.into_inner()).insert(z.id.clone(), povrch.hlaseni.clone());
    nastav_aktivni(&z.id);
    Ok(Vysledek {
        vrstevnic: vrst.len(),
        spojnic: def.spojnice.len(),
        hranic: def.hranice.len(),
        der: def.diry.len(),
        pocet_bodu: zdroje.pocet_bodu,
        vylouceno_bodu: zdroje.vylouceno_bodu,
        povrch,
    })
}

/// Uloží jen definici (bez přepočtu) – pro změny, které nemění výsledek hned.
pub fn uloz_zaznam(v: &mut dyn Vykres, z: &ZaznamPovrchu) -> bool {
    z.insert != Handle::NULL && v.zapis_xdata(z.insert, z.xdata())
}

pub fn cislo_cz(v: f64) -> String {
    format!("{v}").replace('.', ",")
}

/// Krátký výpis do příkazové řádky – podrobnosti jsou v Prospektoru.
pub fn vypis(v: &mut dyn Vykres, z: &ZaznamPovrchu, r: &Vysledek) {
    let s = r.povrch.statistika();
    let hl = &r.povrch.hlaseni;
    let mut upozorneni = Vec::new();
    if !hl.krizeni_spojnic.is_empty() {
        upozorneni.push(trf!("{} křížení spojnic", hl.krizeni_spojnic.len()));
    }
    if !hl.nepouzite_editace.is_empty() {
        upozorneni.push(trf!("{} úprav bez účinku", hl.nepouzite_editace.len()));
    }
    v.info(&trf!("Terén: „{}“ – {} trojúhelníků, výška {:.2}–{:.2}", z.jmeno, s.pocet_trojuhelniku, s.z_min, s.z_max));
    if !upozorneni.is_empty() {
        v.chyba(&trf!("⚠ Terén: „{}“ – {} → Prospektor", z.jmeno, upozorneni.join(", ")));
    }
}

/// Smaže povrch (vložení i blok). Čáry spojnic a hranic zůstanou.
pub fn smaz(v: &mut dyn Vykres, doc: &CadDocument, z: &ZaznamPovrchu) -> Result<(), String> {
    v.odeber(z.insert);
    let blok = z.blok_ve_vykrese();
    if doc.block_records.iter().any(|b| b.name.eq_ignore_ascii_case(&blok)) {
        v.tabulka(TableOperation::BlockDelete { name: blok })?;
    }
    Ok(())
}

/// Povrchy vybrané ve výkrese (blok povrchu nebo objekt na jeho hladinách).
pub fn povrchy_z_vyberu(doc: &CadDocument, vyber: &[Handle]) -> Vec<String> {
    if vyber.is_empty() {
        return Vec::new();
    }
    let vybrane: Vec<&EntityType> = doc.model_space_entities().filter(|e| vyber.contains(&e.common().handle)).collect();
    povrchy(doc)
        .into_iter()
        .filter(|z| {
            let pref = format!("{}_", z.hladiny().povrch.to_uppercase());
            vyber.contains(&z.insert) || vybrane.iter().any(|e| e.common().layer.to_uppercase().starts_with(&pref))
        })
        .map(|z| z.id)
        .collect()
}
