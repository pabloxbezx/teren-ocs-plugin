//! Pohled podélného profilu jako objekt výkresu.
//!
//! Blok `TRA_POHLED_<ID>` (rám, mřížka, popisy os, srovnávací rovina,
//! titulek, terénní profil) vložený v levém dolním rohu pohledu. Definice
//! je v XDATA `TRASY_POHLED` na vložení. Terénní profil se počítá při
//! kreslení z trasy a z trojúhelníků povrchu Terénu.

use ocs_plugin_api::host::codec::entities::text::{TextHorizontalAlignment, TextVerticalAlignment};
use ocs_plugin_api::host::codec::entities::{BoundaryEdge, BoundaryPath, Hatch, Insert, LwPolyline, PolylineEdge, Text};
use ocs_plugin_api::host::codec::types::{Color, LineWeight, Transparency, Vector2};
use ocs_plugin_api::host::{Document as CadDocument, EntityType, ExtendedDataRecord, Handle, TableOperation, TextStyleConfig, XDataValue};

use trasy::geometrie::{DruhBodu, Geometrie, Prvek};
use trasy::pasy::{mista_bez_kolizi, vyska_v, DruhPasu, Pas, Poloha, Zdroj};
use trasy::pohled::{nastaveni_s, DefinicePohledu, Osy};
use trasy::styl::Hodnoty;
use trasy::vzhled::Vzhled;
use crate::lang::tr;
use trasy::profil::{profil_terenu, rozsah_vysek, Povrch};
use trasy::stanice::{cislo, staniceni, FormatStanice};
use trasy::trf;

use crate::tra::objekt::{self, cara, na, retezce, v3};
use crate::tra::povrchy;
use crate::vykres::Vykres;

pub const APP: &str = "TRASY_POHLED";

/// Ve výkrese jsou pohledy – pracovní vlákno je po změně výkresu hlídá.
pub static MA_POHLEDY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn ma_pohledy() -> bool {
    MA_POHLEDY.load(std::sync::atomic::Ordering::SeqCst)
}

/// Otisk zdrojů pohledu: vložení trasy a povrchu (handle se mění s každým
/// přestavěním, poloha a otočení s přesunem).
pub fn otisk(doc: &CadDocument, def: &DefinicePohledu) -> String {
    otisk_s(doc, def, None)
}

/// Najde trasu pohledu: nově uloženou (`nova`), jinak ve výkrese.
fn trasa_pohledu(doc: &CadDocument, def: &DefinicePohledu, nova: Option<&objekt::ObjektTrasy>) -> Option<objekt::ObjektTrasy> {
    match nova {
        Some(t) if t.def.id == def.trasa => Some(t.clone()),
        _ => objekt::trasy(doc).into_iter().find(|t| t.def.id == def.trasa),
    }
}

/// Otisk s nově uloženou trasou (výkres může být ještě ten před uložením).
pub fn otisk_s(doc: &CadDocument, def: &DefinicePohledu, nova: Option<&objekt::ObjektTrasy>) -> String {
    let t = trasa_pohledu(doc, def, nova);
    let p = povrchy::povrchy(doc).into_iter().find(|p| p.id == def.povrch);
    let t = t.map_or("-".to_string(), |t| {
        // nivelety se mění bez přestavění bloku trasy → do otisku jejich součet
        let niv: String = t.def.nivelety.iter().flat_map(|n| n.zapis()).collect::<Vec<_>>().join("|");
        let h = niv.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        format!("{:X}@{:.3},{:.3},{:.6}#{:X}", t.insert.value(), t.vlozeni[0], t.vlozeni[1], t.otoceni, h)
    });
    let p = p.map_or("-".to_string(), |p| format!("{:X}@{:.3},{:.3}", p.insert.value(), p.vlozeni[0], p.vlozeni[1]));
    // styl profilu: jeho změna překreslí všechny pohledy, které ho používají
    let s = crate::styly::styly_profilu(doc).into_iter().find(|s| s.id == def.styl).map_or("-".to_string(), |s| s.otisk());
    // další povrchy, ze kterých pásy čtou (upravený terén…) – jen když nějaké jsou,
    // ať se otisk starších pohledů nemění
    let vse = povrchy::povrchy(doc);
    let mut dalsi: Vec<String> = Vec::new();
    for pas in &def.pasy {
        for z in [&pas.zdroj, &pas.zdroj2] {
            let id = match z {
                Zdroj::Povrch(id) => Some(id.clone()),
                Zdroj::Upraveny => upraveny_povrch(&vse, def),
                _ => None,
            };
            if let Some(x) = id.and_then(|id| vse.iter().find(|p| p.id == id)) {
                let o = format!("{:X}", x.insert.value());
                if !dalsi.contains(&o) {
                    dalsi.push(o);
                }
            }
        }
    }
    if dalsi.is_empty() {
        format!("{t};{p};{s}")
    } else {
        format!("{t};{p};{s};{}", dalsi.join(","))
    }
}

/// Překreslí pohledy, jejichž trasa nebo povrch se od posledního kreslení
/// změnily. Vrací počet překreslených.
///
/// `doc` musí být stažený PŘED otevřením kroku Zpět – stažení výkresu uvnitř
/// kroku ho v OCS uzavře. Nově uloženou trasu (ještě není v `doc`) předá `nova`.
pub fn prekresli_zastarale(v: &mut dyn Vykres, doc: &CadDocument, jen_trasa: Option<&str>, nova: Option<&objekt::ObjektTrasy>) -> usize {
    prekresli_zastarale_h(v, doc, jen_trasa, nova).len()
}

/// Jako [`prekresli_zastarale`], vrací dvojice (staré vložení, nové vložení).
pub fn prekresli_zastarale_h(
    v: &mut dyn Vykres,
    doc: &CadDocument,
    jen_trasa: Option<&str>,
    nova: Option<&objekt::ObjektTrasy>,
) -> Vec<(Handle, Handle)> {
    let vse = pohledy(doc);
    MA_POHLEDY.store(!vse.is_empty(), std::sync::atomic::Ordering::SeqCst);
    let zastarale: Vec<ObjektPohledu> =
        vse.into_iter().filter(|p| !jen_trasa.is_some_and(|id| id != p.def.trasa) && otisk_s(doc, &p.def, nova) != p.def.otisk).collect();
    // režim „po dokončení úprav“ / pozastaveno: jen poznamenat, přepočítá se později
    if !zastarale.is_empty() && !crate::prepocet::smi() {
        crate::prepocet::odloz();
        return Vec::new();
    }
    let mut out = Vec::new();
    for p in zastarale {
        match uloz_s(v, doc, Some(&p), &p.def, p.vlozeni, nova) {
            Ok((h, _)) => out.push((p.insert, h)),
            Err(e) => v.chyba(&trf!("Trasy: pohled {}: {}", "Alignments: view {}: {}", p.def.id, e)),
        }
    }
    out
}

pub const H_POHLED: &str = "TER-PRF";
pub const H_RAM: &str = "TER-PRF-FRAME";
pub const H_MRIZKA: &str = "TER-PRF-GRID";
pub const H_POPIS: &str = "TER-PRF-LABEL";
pub const H_TEREN: &str = "TER-PRF-GROUND";
pub const H_PASY: &str = "TER-PRF-BANDS";
pub const H_NIVELETA: &str = "TER-PRF-GRADE";
pub const H_NIV_POLYGON: &str = "TER-PRF-GRADE-TANGENTS";

#[derive(Clone, Debug)]
pub struct ObjektPohledu {
    pub insert: Handle,
    pub blok: String,
    pub def: DefinicePohledu,
    pub vlozeni: [f64; 2],
}

pub fn pohledy(doc: &CadDocument) -> Vec<ObjektPohledu> {
    doc.model_space_entities().filter_map(z_entity).collect()
}

/// Pohled z jedné entity (vložení s definicí pohledu).
pub fn z_entity(e: &EntityType) -> Option<ObjektPohledu> {
    let EntityType::Insert(i) = e else { return None };
    let def = DefinicePohledu::cti(&retezce(i.common.extended_data.get_record(APP)?))?;
    Some(ObjektPohledu { insert: i.common.handle, blok: i.block_name.clone(), def, vlozeni: [i.insert_point.x, i.insert_point.y] })
}

impl ObjektPohledu {
    /// (staničení, výška) → bod výkresu (podle os z posledního kreslení).
    pub fn na_vykres(&self, s: f64, z: f64) -> Option<[f64; 3]> {
        let [od, _, sr, _, prev] = self.def.osy?;
        Some([self.vlozeni[0] + s - od, self.vlozeni[1] + (z - sr) * prev, 0.0])
    }
    /// Bod výkresu → (staničení, výška).
    pub fn na_profil(&self, p: [f64; 3]) -> Option<(f64, f64)> {
        let [od, _, sr, _, prev] = self.def.osy?;
        Some((od + p[0] - self.vlozeni[0], sr + (p[1] - self.vlozeni[1]) / prev))
    }
    /// Leží bod v rámu pohledu?
    pub fn obsahuje(&self, p: [f64; 2]) -> bool {
        let Some([od, do_, sr, horni, prev]) = self.def.osy else { return false };
        let (x, y) = (p[0] - self.vlozeni[0], p[1] - self.vlozeni[1]);
        x >= -1e-6 && x <= do_ - od + 1e-6 && y >= -1e-6 && y <= (horni - sr) * prev + 1e-6
    }
}

/// Data pro kreslení pohledu (z výkresu; kreslení samo už výkres nepotřebuje).
#[derive(Clone, Debug)]
pub struct Podklady {
    pub trasa: String,
    pub povrch: String,
    pub g: Geometrie,
    /// Terénní profil povrchu pohledu.
    pub useky: Vec<Vec<[f64; 2]>>,
    /// Profily dalších povrchů (id → úseky).
    pub dalsi: Vec<(String, Vec<Vec<[f64; 2]>>)>,
    /// Jména povrchů výkresu (id, jméno) – pro legendu a editor.
    pub povrchy: Vec<(String, String)>,
    pub niveleta: Option<trasy::niveleta::Niveleta>,
    /// Id povrchu „upravený terén“ (pásy se zdrojem `Zdroj::Upraveny`).
    pub upraveny: Option<String>,
    pub od: f64,
    pub do_: f64,
    pub zmin: f64,
    pub zmax: f64,
    pub trojuhelniku: usize,
}

/// Velká písmena kromě jednotek v hranatých závorkách („Staničení [m]“ →
/// „STANIČENÍ [m]“, „Sklon [‰]“ → „SKLON [‰]“).
pub fn velka_mimo_jednotky(t: &str) -> String {
    let mut v = String::with_capacity(t.len());
    let mut v_zavorce = false;
    for c in t.chars() {
        match c {
            '[' => v_zavorce = true,
            ']' => v_zavorce = false,
            _ => {}
        }
        if v_zavorce {
            v.push(c);
        } else {
            v.extend(c.to_uppercase());
        }
    }
    v
}

/// Povrch „upravený terén“ pohledu: nastavený (`povrch_upraveny`), jinak
/// první další povrch, přednostně se jménem „upravený“ / „návrh“.
fn upraveny_povrch(vse: &[povrchy::PovrchTerenu], def: &DefinicePohledu) -> Option<String> {
    if let Some(id) = def.vyjimky.get("povrch_upraveny").filter(|id| !id.trim().is_empty()) {
        if vse.iter().any(|p| p.id == *id) {
            return Some(id.clone());
        }
    }
    let jine: Vec<_> = vse.iter().filter(|p| p.id != def.povrch).collect();
    let jmeno = |p: &&&povrchy::PovrchTerenu| {
        let j = p.jmeno.to_lowercase();
        ["uprav", "návrh", "navrh", "design"].iter().any(|x| j.contains(x)) // i18n:ne
    };
    jine.iter().find(jmeno).or(jine.first()).map(|p| p.id.clone())
}

/// Načte z výkresu, co pohled potřebuje. `vsechny` = profily všech povrchů
/// (pro editor, kde si pás může vybrat jakýkoli), jinak jen těch, ze kterých pásy čtou.
pub fn podklady(doc: &CadDocument, def: &DefinicePohledu, nova: Option<&objekt::ObjektTrasy>, vsechny: bool) -> Result<Podklady, String> {
    let trasa = trasa_pohledu(doc, def, nova).ok_or_else(|| trf!("trasa {} už ve výkrese není", "alignment {} is no longer in the drawing", def.trasa))?;
    let g = trasa.geometrie()?;
    let vse = povrchy::povrchy(doc);
    let povrch = vse.iter().find(|p| p.id == def.povrch).ok_or_else(|| trf!("povrch {} už ve výkrese není", "surface {} is no longer in the drawing", def.povrch))?;
    let troj = povrchy::trojuhelniky(doc, povrch);
    if troj.is_empty() {
        return Err(trf!(
            "povrch „{}“ nemá ve výkrese trojúhelníky – v Terénu mu nastavte styl se zobrazeným TIN a zkuste to znovu", "surface \"{}\" has no triangles in the drawing – give it a style showing the TIN in Terén and try again",
            povrch.jmeno
        ));
    }
    let pocet = troj.len();
    let index = Povrch::new(troj);
    let od = def.od.unwrap_or(g.pocatek).max(g.pocatek);
    let do_ = def.do_.unwrap_or(g.konec()).min(g.konec());
    let useky = profil_terenu(&g, &index, od, do_);
    let (mut zmin, mut zmax) = rozsah_vysek(&useky).ok_or_else(|| trf!("trasa {} nepřechází přes povrch „{}“", "alignment {} does not cross surface \"{}\"", trasa.def.jmeno, povrch.jmeno))?;
    let niveleta = trasa.def.nivelety.first().filter(|n| n.kontrola().is_ok()).cloned();
    if let Some(n) = &niveleta {
        for v in &n.vrcholy {
            if v.s >= od - 1e-6 && v.s <= do_ + 1e-6 {
                zmin = zmin.min(v.z);
                zmax = zmax.max(v.z);
            }
        }
    }
    let upraveny = upraveny_povrch(&vse, def);
    let mut chci: Vec<String> = Vec::new();
    for p in &def.pasy {
        for z in [&p.zdroj, &p.zdroj2] {
            match z {
                Zdroj::Povrch(id) => chci.push(id.clone()),
                Zdroj::Upraveny => chci.extend(upraveny.clone()),
                _ => {}
            }
        }
    }
    let mut dalsi = Vec::new();
    for x in &vse {
        if x.id == def.povrch || !(vsechny || chci.contains(&x.id)) {
            continue;
        }
        let t = povrchy::trojuhelniky(doc, x);
        if t.is_empty() {
            continue;
        }
        dalsi.push((x.id.clone(), profil_terenu(&g, &Povrch::new(t), od, do_)));
    }
    Ok(Podklady {
        trasa: trasa.def.jmeno.clone(),
        povrch: povrch.jmeno.clone(),
        g,
        useky,
        dalsi,
        povrchy: vse.iter().map(|p| (p.id.clone(), p.jmeno.clone())).collect(),
        upraveny,
        niveleta,
        od,
        do_,
        zmin,
        zmax,
        trojuhelniku: pocet,
    })
}

/// Výsledek sestavení pohledu (objekty v souřadnicích pohledu a souhrn).
pub struct Sestaveny {
    pub objekty: Vec<EntityType>,
    /// Textové styly, které objekty používají: (jméno, soubor písma).
    pub styly_textu: Vec<(String, String)>,
    pub trojuhelniku: usize,
    pub useku: usize,
    pub osy: Osy,
}

/// Kde co v nakresleném pohledu leží (souřadnice pohledu) – pro klikání v editoru.
#[derive(Clone, Debug, Default)]
pub struct Rozvrzeni {
    /// Pásy: index v `def.pasy`, horní a dolní okraj.
    pub pasy: Vec<(usize, f64, f64)>,
    /// Graf: x0, y0, x1, y1.
    pub graf: [f64; 4],
    pub nadpis: Option<[f64; 4]>,
    pub legenda: Option<[f64; 4]>,
    /// Levý okraj (záhlaví).
    pub vlevo: f64,
}

/// Spočte terénní profil a objekty pohledu (souřadnice vůči levému dolnímu rohu).
pub fn sestav(doc: &CadDocument, def: &DefinicePohledu, nova: Option<&objekt::ObjektTrasy>) -> Result<Sestaveny, String> {
    // pásy ze stylu výkresu (nebo vlastní pásy pohledu), nastavení přes styly výkresu
    let styly = crate::styly::styly_profilu(doc);
    let efektivni = trasy::pohled::efektivni(def, &styly);
    let p = podklady(doc, &efektivni, nova, false)?;
    let nast = nastaveni_s(&efektivni, &styly);
    let (objekty, _, osy) = kresli(&p, &efektivni, &nast);
    let mut styly_textu: Vec<(String, String)> = Vec::new();
    for (_, pis) in &Vzhled::z(&nast).pisma {
        let jm = pis.styl_textu();
        if !styly_textu.iter().any(|x| x.0 == jm) {
            styly_textu.push((jm, trasy::vzhled::soubor_fontu(&pis.font, pis.tucne)));
        }
    }
    Ok(Sestaveny { objekty, styly_textu, trojuhelniku: p.trojuhelniku, useku: p.useky.len(), osy })
}

/// Psaní textů a čar podle vzhledu (výšky a tloušťky v mm na papíře).
struct Pero<'a> {
    vz: &'a Vzhled,
    /// Model na mm papíru (měřítko / 1000).
    m: f64,
}

fn barva_ent(b: Option<[u8; 3]>) -> Color {
    b.map_or(Color::ByLayer, |b| Color::Rgb { r: b[0], g: b[1], b: b[2] })
}

impl Pero<'_> {
    fn vyska(&self, role: &str) -> f64 {
        self.vz.pismo(role).vyska * self.m
    }

    /// Text role písma; `uhel` v radiánech.
    fn text(&self, role: &str, t: String, p: [f64; 2], hz: TextHorizontalAlignment, vt: TextVerticalAlignment, uhel: f64, vrstva: &str) -> EntityType {
        let pis = self.vz.pismo(role);
        let mut x = Text::with_value(t, v3(p)).with_height(pis.vyska * self.m);
        x.horizontal_alignment = hz;
        x.vertical_alignment = vt;
        x.alignment_point = Some(v3(p));
        x.rotation = uhel;
        x.width_factor = pis.sirka;
        x.oblique_angle = if pis.kurziva { 15f64.to_radians() } else { 0.0 };
        x.style = pis.styl_textu();
        let mut e = na(EntityType::Text(x), vrstva);
        e.common_mut().color = barva_ent(pis.barva.or(self.vz.barva_textu));
        e
    }

    fn obarvi(&self, mut e: EntityType, cara: &str) -> EntityType {
        let c = self.vz.cara(cara);
        let tl = self.vz.tloustka(&c.tloustka);
        let k = e.common_mut();
        k.color = barva_ent(c.barva);
        k.line_weight = LineWeight::Value((tl * 100.0).round().clamp(0.0, 211.0) as i16);
        e
    }

    fn cara(&self, a: [f64; 2], b: [f64; 2], cara_k: &str, vrstva: &str) -> EntityType {
        self.obarvi(cara(a, b, vrstva), cara_k)
    }

    fn polylinie(&self, body: &[[f64; 2]], cara_k: &str, vrstva: &str) -> EntityType {
        let b: Vec<Vector2> = body.iter().map(|p| Vector2::new(p[0], p[1])).collect();
        self.obarvi(na(EntityType::LwPolyline(LwPolyline::from_points(b)), vrstva), cara_k)
    }

    /// Plná výplň mnohoúhelníku s průhledností (0–1).
    fn vypln(&self, body: &[[f64; 2]], barva: Option<[u8; 3]>, pruhlednost: f64, vrstva: &str) -> Option<EntityType> {
        if body.len() < 3 {
            return None;
        }
        let mut h = Hatch::solid();
        let mut bp = BoundaryPath::external();
        bp.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(body.iter().map(|p| Vector2::new(p[0], p[1])).collect(), true)));
        h.paths.push(bp);
        let mut e = na(EntityType::Hatch(Box::new(h)), vrstva);
        let k = e.common_mut();
        k.color = barva_ent(barva.or(Some([128, 128, 128])));
        k.transparency = Transparency::from_percent(pruhlednost);
        Some(e)
    }

    /// Obdélník výplně.
    fn obdelnik(&self, x0: f64, y0: f64, x1: f64, y1: f64, barva: Option<[u8; 3]>, pruhlednost: f64, vrstva: &str) -> Option<EntityType> {
        if (x1 - x0).abs() < 1e-9 || (y1 - y0).abs() < 1e-9 {
            return None;
        }
        self.vypln(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], barva, pruhlednost, vrstva)
    }

    /// Přibližná šířka textu role (pro štítky a legendu).
    fn sirka_textu(&self, role: &str, t: &str) -> f64 {
        let p = self.vz.pismo(role);
        t.chars().count() as f64 * p.vyska * self.m * if p.tucne { 0.66 } else { 0.58 } * p.sirka
    }
}

/// Nakreslí pohled z podkladů: objekty, rozvržení (pro editor) a osy.
pub fn kresli(pk: &Podklady, def: &DefinicePohledu, nast: &Hodnoty) -> (Vec<EntityType>, Rozvrzeni, Osy) {
    // pásy s „upraveným terénem“ čtou z konkrétního povrchu
    let def_upr;
    let def = if def.pasy.iter().any(|p| p.zdroj == Zdroj::Upraveny || p.zdroj2 == Zdroj::Upraveny) {
        let mut d = def.clone();
        for p in &mut d.pasy {
            for z in [&mut p.zdroj, &mut p.zdroj2] {
                if *z == Zdroj::Upraveny {
                    if let Some(id) = &pk.upraveny {
                        *z = Zdroj::Povrch(id.clone());
                    }
                }
            }
        }
        def_upr = d;
        &def_upr
    } else {
        def
    };
    let osy = Osy::rozvrhni(pk.od, pk.do_, pk.zmin, pk.zmax, def);
    let vz = Vzhled::z(nast);
    let m = def.meritko / 1000.0;
    let pero = Pero { vz: &vz, m };
    // výkresový režim: záhlaví bez rámečků, hodnoty u svislic (jinak jako „jen svislice“)
    let vykres = nast.text("cary_pasu") == "vykres";
    let svislice = nast.text("cary_pasu") == "svislice" || vykres;
    let fmt = FormatStanice::z_kodu(nast.text("staniceni"));
    let sablona = nast.text("sablona").to_string();
    let hg = pero.vyska("graf");
    let (w, v) = (osy.sirka(), osy.vyska());
    let sirka_zahlavi = vz.sirka_zahlavi * m;
    let mut ent = Vec::new();
    let pasy_vid: Vec<&Pas> = def.pasy.iter().filter(|p| !p.skryty).collect();
    // plocha pod terénem (pod vším ostatním)
    if vz.vypln_terenu == "plna" {
        for u in &pk.useky {
            let mut body: Vec<[f64; 2]> = u.iter().map(|p| osy.bod(p[0], p[1])).collect();
            if let (Some(a), Some(b)) = (body.first().copied(), body.last().copied()) {
                body.push([b[0], 0.0]);
                body.push([a[0], 0.0]);
                ent.extend(pero.vypln(&body, vz.cara("teren").barva, 0.85, H_TEREN));
            }
        }
    }
    // mřížka
    let ma_pas_staniceni = pasy_vid.iter().any(|p| p.druh == DruhPasu::Staniceni);
    let mrizka = nast.ano("mrizka");
    for s in osy.staniceni_mrizky() {
        let x = s - osy.od;
        if mrizka {
            ent.push(pero.cara([x, 0.0], [x, v], "mrizka", H_MRIZKA));
        }
        if ma_pas_staniceni || !mrizka {
            continue;
        }
        ent.push(pero.text("graf", staniceni(s, fmt, 0, true), [x, -1.2 * hg], TextHorizontalAlignment::Center, TextVerticalAlignment::Top, 0.0, H_POPIS));
    }
    let stupnice = nast.text("stupnice").to_string();
    for z in osy.vysky_mrizky() {
        let y = (z - osy.srovnavaci) * osy.prevyseni;
        if mrizka {
            ent.push(pero.cara([0.0, y], [w, y], "mrizka", H_MRIZKA));
        }
        // u srovnávací roviny je s pásy popis „S.R. …“ (viz níže)
        if !pasy_vid.is_empty() && (z - osy.srovnavaci).abs() < 1e-6 {
            continue;
        }
        if stupnice != "skryt" {
            ent.push(pero.cara([0.0, y], [-0.6 * hg, y], "pasy", H_RAM));
            ent.push(pero.text("graf", cislo(z, 0, true), [-1.0 * hg, y], TextHorizontalAlignment::Right, TextVerticalAlignment::Middle, 0.0, H_POPIS));
        }
        if stupnice == "obe" {
            ent.push(pero.cara([w, y], [w + 0.6 * hg, y], "pasy", H_RAM));
            ent.push(pero.text("graf", cislo(z, 0, true), [w + 1.0 * hg, y], TextHorizontalAlignment::Left, TextVerticalAlignment::Middle, 0.0, H_POPIS));
        }
    }
    if stupnice != "skryt" {
        ent.push(pero.cara([0.0, 0.0], [0.0, v], "pasy", H_RAM));
    }
    if stupnice == "obe" {
        ent.push(pero.cara([w, 0.0], [w, v], "pasy", H_RAM));
    }
    // rám grafu (bez rámu aspoň srovnávací rovina)
    if nast.ano("ram") {
        for (a, b) in [([0.0, 0.0], [w, 0.0]), ([w, 0.0], [w, v]), ([w, v], [0.0, v]), ([0.0, v], [0.0, 0.0])] {
            ent.push(pero.cara(a, b, "pasy", H_RAM));
        }
    } else {
        ent.push(pero.cara([0.0, 0.0], [w, 0.0], "pasy", H_RAM));
    }
    // srovnávací rovina
    let sr = trf!("S.R. {} m n. m.", "Datum {} m a.s.l.", cislo(osy.srovnavaci, 2, true));
    if vykres && !pasy_vid.is_empty() {
        // záhlaví „SROVNÁVACÍ ROVINA“ a hodnota nad osou na začátku grafu
        ent.push(pero.text("zahlavi", tr("Srovnávací rovina", "Datum").to_uppercase(), [-sirka_zahlavi + 2.0 * m, 0.8 * m], TextHorizontalAlignment::Left, TextVerticalAlignment::Bottom, 0.0, H_PASY));
        ent.push(pero.text("hodnota", trf!("{} m n. m.", "{} m a.s.l.", cislo(osy.srovnavaci, 2, true)), [0.8 * m, 0.8 * m], TextHorizontalAlignment::Left, TextVerticalAlignment::Bottom, 0.0, H_PASY));
    } else if pasy_vid.is_empty() {
        ent.push(pero.text("legenda", sr, [0.0, -2.0 * hg], TextHorizontalAlignment::Left, TextVerticalAlignment::Top, 0.0, H_POPIS));
    } else {
        ent.push(pero.text("legenda", sr, [-sirka_zahlavi + 0.8 * hg, 0.6 * hg], TextHorizontalAlignment::Left, TextVerticalAlignment::Bottom, 0.0, H_POPIS));
    }
    // lomové body (svislice v grafu i místa hodnot v pásech)
    let lomy_niv: Vec<f64> = pk.niveleta.as_ref().map_or(Vec::new(), |n| n.vrcholy.iter().map(|v| v.s).collect());
    let lomy_z_nivelety = matches!(sablona.as_str(), "kanalizace" | "tok");
    let lomy_spolecne: Vec<f64> =
        if lomy_z_nivelety && !lomy_niv.is_empty() { lomy_niv.clone() } else { lomy_profilu(&pk.useky).into_iter().chain(lomy_niv.iter().copied()).collect() };
    // profily dalších povrchů, ze kterých pásy čtou
    for (id, u) in &pk.dalsi {
        if !pasy_vid.iter().any(|p| p.zdroj == Zdroj::Povrch(id.clone()) || p.zdroj2 == Zdroj::Povrch(id.clone())) {
            continue;
        }
        for x in u {
            let b: Vec<[f64; 2]> = x.iter().map(|p| osy.bod(p[0], p[1])).collect();
            ent.push(pero.polylinie(&b, "povrchy", H_TEREN));
        }
    }
    // terén
    for u in &pk.useky {
        let b: Vec<[f64; 2]> = u.iter().map(|p| osy.bod(p[0], p[1])).collect();
        ent.push(pero.polylinie(&b, "teren", H_TEREN));
    }
    // niveleta: tečnový polygon (je-li zakružení) a niveleta s oblouky
    if let Some(n) = &pk.niveleta {
        // tečnový polygon jen v místě oblouků (jinak by šedá čára překryla niveletu)
        for t in n.tecny_oblouku() {
            let b: Vec<[f64; 2]> = t.iter().map(|p| osy.bod(p[0], p[1])).collect();
            ent.push(pero.polylinie(&b, "polygon", H_NIV_POLYGON));
        }
        let b: Vec<[f64; 2]> = n.body().into_iter().filter(|p| p[0] >= osy.od - 1e-6 && p[0] <= osy.do_ + 1e-6).map(|p| osy.bod(p[0], p[1])).collect();
        if b.len() >= 2 {
            ent.push(pero.polylinie(&b, "niveleta", H_NIVELETA));
        }
        // popis zakružovacích oblouků u vrcholu: nad vypuklým, pod údolnicovým
        for o in n.oblouky().into_iter().filter(|_| nast.ano("popis_oblouku")) {
            let v = n.vrcholy[o.vrchol];
            if v.s < osy.od - 1e-6 || v.s > osy.do_ + 1e-6 {
                continue;
            }
            let p = osy.bod(v.s, v.z);
            let t = match o.typ {
                trasy::niveleta::TypOblouku::Nesoumerny { .. } => {
                    format!("L1 {} · L2 {} · y {}", cislo(o.t1, 2, true), cislo(o.t2, 2, true), cislo(o.vzepeti, 3, true))
                }
                trasy::niveleta::TypOblouku::Kruh => trf!(
                    "kruh R {} · L {} · K {}", "circle R {} · L {} · K {}",
                    cislo(o.r, 0, true),
                    cislo(o.delka, 2, true),
                    cislo(o.k, if o.k < 10.0 { 2 } else { 1 }, true)
                ),
                trasy::niveleta::TypOblouku::Parabola => {
                    format!("R {} · L {} · K {}", cislo(o.r, 0, true), cislo(o.delka, 2, true), cislo(o.k, if o.k < 10.0 { 2 } else { 1 }, true))
                }
            };
            let (y, va) = if o.vypukly { (p[1] + 0.8 * hg, TextVerticalAlignment::Bottom) } else { (p[1] - 0.8 * hg, TextVerticalAlignment::Top) };
            ent.push(pero.text("graf", t, [p[0], y], TextHorizontalAlignment::Center, va, 0.0, H_POPIS));
        }
    }
    let kontext = KontextPasu {
        def,
        osy: &osy,
        g: &pk.g,
        useky: &pk.useky,
        dalsi: &pk.dalsi,
        niveleta: pk.niveleta.as_ref(),
        pero: &pero,
        sirka_zahlavi,
        svislice,
        vykres,
        svislice_pasu: nast.ano("svislice_pasu"),
        svislice_vykresu: Default::default(),
        fmt,
        procenta: nast.text("sklon") == "procento",
        lomy_z_nivelety,
        lomy_spolecne: &lomy_spolecne,
    };
    let (pasy_ent, vyska_nad, mut rozvrzeni) = pasy(&kontext);
    // svislice v grafu: v místech, kde pásy zapsaly hodnoty (bez pásů v lomech)
    if vz.svislice && !pasy_vid.is_empty() {
        let mut mista: Vec<f64> = kontext.svislice_vykresu.borrow().iter().map(|x| x + osy.od).collect();
        if mista.is_empty() {
            mista = lomy_spolecne.clone();
        }
        mista.sort_by(|a, b| a.total_cmp(b));
        mista.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        for s in mista {
            if s < osy.od - 1e-6 || s > osy.do_ + 1e-6 {
                continue;
            }
            let z = [vyska_v(&pk.useky, s), pk.niveleta.as_ref().and_then(|n| n.vyska(s))].into_iter().flatten().fold(f64::NAN, f64::max);
            if z.is_finite() {
                let y = (z - osy.srovnavaci) * osy.prevyseni;
                ent.push(pero.cara([s - osy.od, 0.0], [s - osy.od, y], "mrizka", H_MRIZKA));
            }
        }
    }
    ent.extend(pasy_ent);
    rozvrzeni.graf = [0.0, 0.0, w, v];
    rozvrzeni.vlevo = -sirka_zahlavi;
    let dole = rozvrzeni.pasy.iter().map(|p| p.2).fold(0.0_f64, f64::min);
    // nadpis nad pásy nad grafem, za ním měřítko (ve štítku)
    let mv = def.meritko / def.prevyseni.max(0.1);
    let meritko_txt = format!("M 1:{} / 1:{}", cislo(def.meritko, 0, true), cislo(mv, 0, true));
    let nadpis = trasy::vzhled::dosad_nadpis(&vz.nadpis, &pk.trasa, &pk.povrch, &meritko_txt);
    let hn = pero.vyska("nadpis");
    let yn = v + vyska_nad + 1.6 * hn;
    let xn = -sirka_zahlavi;
    if !nadpis.trim().is_empty() {
        ent.push(pero.text("nadpis", nadpis.clone(), [xn, yn], TextHorizontalAlignment::Left, TextVerticalAlignment::Baseline, 0.0, H_POPIS));
    }
    let sn = if nadpis.trim().is_empty() { 0.0 } else { pero.sirka_textu("nadpis", &nadpis) };
    let hl = pero.vyska("legenda");
    let xm = xn + sn + if sn > 0.0 { 1.2 * hn } else { 0.0 };
    let sm = pero.sirka_textu("legenda", &meritko_txt);
    if vz.stitek {
        let (pad, vy) = (0.9 * hl, yn + 0.5 * hl);
        let (x0, x1, y0, y1) = (xm, xm + sm + 2.0 * pad, vy - 0.5 * hl - pad, vy + 0.5 * hl + pad);
        let r = (y1 - y0) / 2.0;
        let mut pl = LwPolyline::from_points(vec![Vector2::new(x0 + r, y0), Vector2::new(x1 - r, y0), Vector2::new(x1 - r, y1), Vector2::new(x0 + r, y1)]);
        pl.is_closed = true;
        if pl.vertices.len() == 4 {
            pl.vertices[1].bulge = 1.0;
            pl.vertices[3].bulge = 1.0;
        }
        ent.push(pero.obarvi(na(EntityType::LwPolyline(pl), H_POPIS), "pasy"));
        ent.push(pero.text("legenda", meritko_txt, [x0 + pad, vy], TextHorizontalAlignment::Left, TextVerticalAlignment::Middle, 0.0, H_POPIS));
        rozvrzeni.nadpis = Some([xn, yn - 0.4 * hn, x1, yn + hn]);
    } else {
        ent.push(pero.text("legenda", meritko_txt, [xm, yn], TextHorizontalAlignment::Left, TextVerticalAlignment::Baseline, 0.0, H_POPIS));
        rozvrzeni.nadpis = Some([xn, yn - 0.4 * hn, xm + sm, yn + hn]);
    }
    // legenda čar pod pásy
    if vz.legenda {
        let yl = dole - 3.0 * hl;
        let mut x = 0.0;
        let mut polozky: Vec<(String, &str)> = vec![(trf!("Terén – {}", "Terrain – {}", pk.povrch), "teren")];
        if pk.niveleta.is_some() {
            let nazev = nast.text("nazev_nivelety");
            polozky.push((if nazev.trim().is_empty() { tr("Niveleta", "Design profile").to_string() } else { crate::lang::tr_dyn(nazev) }, "niveleta"));
        }
        for (id, _) in &pk.dalsi {
            if pasy_vid.iter().any(|p| p.zdroj == Zdroj::Povrch(id.clone()) || p.zdroj2 == Zdroj::Povrch(id.clone())) {
                let j = pk.povrchy.iter().find(|p| p.0 == *id).map_or(id.clone(), |p| p.1.clone());
                polozky.push((j, "povrchy"));
            }
        }
        for (t, k) in polozky {
            ent.push(pero.cara([x, yl + 0.4 * hl], [x + 8.0 * m, yl + 0.4 * hl], k, H_POPIS));
            let s = pero.sirka_textu("legenda", &t);
            ent.push(pero.text("legenda", t, [x + 9.5 * m, yl], TextHorizontalAlignment::Left, TextVerticalAlignment::Baseline, 0.0, H_POPIS));
            x += 9.5 * m + s + 6.0 * m;
        }
        rozvrzeni.legenda = Some([-1.0 * m, yl - 0.6 * hl, x, yl + 1.4 * hl]);
    }
    (ent, rozvrzeni, osy)
}

fn zajisti_hladiny(v: &mut dyn Vykres, doc: &CadDocument, styly_textu: &[(String, String)]) {
    for (jmeno, soubor) in styly_textu {
        if !doc.text_styles.contains(jmeno) {
            let _ = v.tabulka(TableOperation::TextStyleCreate {
                config: TextStyleConfig { name: jmeno.clone(), height: Some(0.0), width_factor: Some(1.0), font_file: Some(soubor.clone()), ..Default::default() },
            });
        }
    }
    for (nazev, barva) in [(H_POHLED, 7), (H_RAM, 7), (H_MRIZKA, 8), (H_POPIS, 7), (H_TEREN, 32), (H_PASY, 7), (H_NIVELETA, 1), (H_NIV_POLYGON, 8)] {
        if !doc.layers.iter().any(|l| l.name.eq_ignore_ascii_case(nazev)) {
            v.hladina(crate::hladiny::konfigurace(nazev, barva));
        }
    }
}

/// Nakreslí pohled (nový nebo místo starého) s levým dolním rohem ve `vlozeni`.
pub fn uloz(
    v: &mut dyn Vykres,
    doc: &CadDocument,
    stary: Option<&ObjektPohledu>,
    def: &DefinicePohledu,
    vlozeni: [f64; 2],
) -> Result<(Handle, Sestaveny), String> {
    uloz_s(v, doc, stary, def, vlozeni, None)
}

pub fn uloz_s(
    v: &mut dyn Vykres,
    doc: &CadDocument,
    stary: Option<&ObjektPohledu>,
    def: &DefinicePohledu,
    vlozeni: [f64; 2],
    nova: Option<&objekt::ObjektTrasy>,
) -> Result<(Handle, Sestaveny), String> {
    let s = sestav(doc, def, nova)?;
    zajisti_hladiny(v, doc, &s.styly_textu);
    let mut def_ulozena = def.clone();
    def_ulozena.otisk = otisk_s(doc, &def_ulozena, nova);
    def_ulozena.osy = Some([s.osy.od, s.osy.do_, s.osy.srovnavaci, s.osy.horni, s.osy.prevyseni]);
    let zaznam_pohledu = || {
        let mut r = ExtendedDataRecord::new(APP);
        for x in def_ulozena.zapis() {
            r.add_value(XDataValue::String(x));
        }
        r
    };
    // existující pohled: obsah bloku i vložení jednou transakcí
    if let Some(st) = stary {
        if let Some(EntityType::Insert(staré)) = doc.get_entity(st.insert) {
            let mut ins = EntityType::Insert(staré.clone());
            if let EntityType::Insert(i) = &mut ins {
                i.insert_point = v3(vlozeni);
            }
            crate::vykres::nastav_zaznam(&mut ins, zaznam_pohledu());
            crate::vykres::nastav_zaznam(&mut ins, crate::povrchy::zaznam_tridy("POHLED"));
            if crate::vykres::prepis_blok(v, doc, &st.blok, s.objekty.clone(), ins, tr("Podélný profil", "Longitudinal profile"))? {
                MA_POHLEDY.store(true, std::sync::atomic::Ordering::SeqCst);
                v.zmeneno();
                return Ok((st.insert, s));
            }
        }
    }
    let blok = format!("TRA_POHLED_{}", def.id);
    let docasny = format!("{blok}_NOVY");
    if doc.block_records.iter().any(|b| b.name.eq_ignore_ascii_case(&docasny)) {
        let _ = v.tabulka(TableOperation::BlockDelete { name: docasny.clone() });
    }
    let handly = v.pridej(s.objekty.clone())?;
    v.tabulka(TableOperation::BlockCreate {
        name: docasny.clone(),
        entities: handly,
        base_point: [0.0, 0.0, 0.0],
        erase_originals: true,
        description: Some(trf!("Pohled podélného profilu {} (plugin Trasy)", "Longitudinal profile view {} (Alignments plugin)", def.id)),
    })
    .map_err(|e| trf!("blok pohledu nejde vytvořit: {}", "the view block cannot be created: {}", e))?;
    if let Some(st) = stary {
        v.odeber(st.insert);
        if !st.blok.eq_ignore_ascii_case(&docasny) && doc.block_records.iter().any(|b| b.name.eq_ignore_ascii_case(&st.blok)) {
            let _ = v.tabulka(TableOperation::BlockDelete { name: st.blok.clone() });
        }
    }
    let jmeno = match v.tabulka(TableOperation::BlockRename { from: docasny.clone(), to: blok.clone() }) {
        Ok(_) => blok,
        Err(_) => docasny,
    };
    let mut ins = Insert::new(jmeno, v3(vlozeni));
    ins.common.layer = H_POHLED.into();
    let insert = v.pridej_jednu(EntityType::Insert(ins))?;
    MA_POHLEDY.store(true, std::sync::atomic::Ordering::SeqCst);
    if !v.zapis_xdata(insert, zaznam_pohledu()) {
        return Err(tr("nepodařilo se uložit definici pohledu", "the view definition could not be saved").into());
    }
    v.zapis_xdata(insert, crate::povrchy::zaznam_tridy("POHLED"));
    v.hotovo();
    Ok((insert, s))
}

/// Co potřebují pásy z pohledu.
struct KontextPasu<'a> {
    def: &'a DefinicePohledu,
    osy: &'a Osy,
    g: &'a Geometrie,
    /// Terénní profil povrchu pohledu.
    useky: &'a [Vec<[f64; 2]>],
    /// Profily dalších povrchů (id → úseky).
    dalsi: &'a [(String, Vec<Vec<[f64; 2]>>)],
    niveleta: Option<&'a trasy::niveleta::Niveleta>,
    pero: &'a Pero<'a>,
    sirka_zahlavi: f64,
    /// Jen svislice (bez rámečků a vodorovných čar u hodnotových pásů).
    svislice: bool,
    /// Výkresový režim: záhlaví bez čar, velkými písmeny.
    vykres: bool,
    /// Svislice v pásech s hodnotami; bez nich jsou čísla v ose staničení.
    svislice_pasu: bool,
    /// Místa zapsaných hodnot (svislice v grafu; ve výkresovém režimu i souvislé
    /// svislice přes pásy).
    svislice_vykresu: std::cell::RefCell<Vec<f64>>,
    fmt: FormatStanice,
    procenta: bool,
    /// Lomové body = vrcholy nivelety (šachty, profily toku).
    lomy_z_nivelety: bool,
    lomy_spolecne: &'a [f64],
}

/// Lomové body terénního profilu: místní maxima a minima a konce úseků.
fn lomy_profilu(useky: &[Vec<[f64; 2]>]) -> Vec<f64> {
    let mut out = Vec::new();
    for u in useky {
        if let (Some(a), Some(b)) = (u.first(), u.last()) {
            out.push(a[0]);
            out.push(b[0]);
        }
        for w in u.windows(3) {
            let (d1, d2) = (w[1][1] - w[0][1], w[2][1] - w[1][1]);
            if d1 * d2 < 0.0 && (d1.abs() + d2.abs()) > 0.02 {
                out.push(w[1][0]);
            }
        }
    }
    out
}

/// Pásy pod rámem a nad rámem pohledu, záhlaví vlevo. Vrací objekty, výšku
/// bloku pásů nad grafem (kvůli nadpisu) a polohu pásů.
fn pasy(k: &KontextPasu) -> (Vec<EntityType>, f64, Rozvrzeni) {
    let mut ent = Vec::new();
    let mut rozvrzeni = Rozvrzeni::default();
    let (osy, pero) = (k.osy, k.pero);
    let vz = pero.vz;
    let w = osy.sirka();
    let m = pero.m;
    let hh = pero.vyska("hodnota");
    let body: Vec<f64> = k.g.hlavni_body.iter().filter(|b| b.druh != DruhBodu::V).map(|b| b.staniceni).collect();
    let profil = |z: &Zdroj| -> Option<&[Vec<[f64; 2]>]> {
        match z {
            Zdroj::Teren => Some(k.useky),
            Zdroj::Povrch(id) if *id == k.def.povrch => Some(k.useky),
            Zdroj::Povrch(id) => k.dalsi.iter().find(|x| x.0 == *id).map(|x| &x.1[..]),
            Zdroj::Niveleta | Zdroj::Upraveny | Zdroj::Potrubi => None,
        }
    };
    let vyska = |z: &Zdroj, s: f64| -> Option<f64> {
        match z {
            Zdroj::Niveleta => k.niveleta.and_then(|n| n.vyska(s)),
            _ => profil(z).and_then(|u| vyska_v(u, s)),
        }
    };
    let x_od = |s: f64| (s.max(osy.od).min(osy.do_)) - osy.od;
    let cara = |ent: &mut Vec<EntityType>, a: [f64; 2], b: [f64; 2]| ent.push(pero.cara(a, b, "pasy", H_PASY));
    // rámeček textového řádku: vodorovné čáry, okraje a hranice úseků (sdílené
    // hrany sousedních řádků splynou – žádné zdvojení)
    let ramecek = |ent: &mut Vec<EntityType>, y0: f64, y1: f64, hranice: &[f64]| {
        cara(ent, [0.0, y0], [w, y0]);
        cara(ent, [0.0, y1], [w, y1]);
        for x in [0.0, w].iter().chain(hranice.iter()) {
            if *x >= -1e-9 && *x <= w + 1e-9 {
                cara(ent, [*x, y0], [*x, y1]);
            }
        }
    };
    let sklon_txt = |i: f64, des: usize| {
        if k.procenta { format!("{} %", cislo(i * 100.0, des + 1, true)) } else { format!("{} ‰", cislo(i * 1000.0, des, true)) }
    };

    let kresli = |ent: &mut Vec<EntityType>, pas: &Pas, y0: f64, y1: f64| {
        let ym = (y0 + y1) / 2.0;
        let role = pas.role_pisma().to_string();
        let ht = pero.vyska(&role);
        // podbarvení záhlaví a pásu
        if vz.podbarvit_zahlavi {
            ent.extend(pero.obdelnik(-k.sirka_zahlavi, y1, 0.0, y0, Some([128, 128, 128]), 0.82, H_PASY));
        }
        if pas.podbarvit {
            ent.extend(pero.obdelnik(0.0, y1, w, y0, Some([128, 128, 128]), 0.9, H_PASY));
        }
        // záhlaví a rámeček řádku (jen v režimu s rámečky)
        if k.vykres {
            // výkresový režim: záhlaví bez čar
        } else if !k.svislice {
            for (a, b) in [
                ([-k.sirka_zahlavi, y0], [w, y0]),
                ([-k.sirka_zahlavi, y1], [w, y1]),
                ([-k.sirka_zahlavi, y0], [-k.sirka_zahlavi, y1]),
                ([0.0, y0], [0.0, y1]),
                ([w, y0], [w, y1]),
            ] {
                cara(ent, a, b);
            }
        } else {
            // jen svislice: dělicí čára v záhlaví a hrana záhlaví
            cara(ent, [-k.sirka_zahlavi, y1], [0.0, y1]);
            cara(ent, [0.0, y0], [0.0, y1]);
        }
        let zahlavi = if pas.nazev.trim().is_empty() { crate::lang::tr_tab(pas.druh.nazev()).to_string() } else { pas.nazev.clone() };
        let zahlavi = if k.vykres { velka_mimo_jednotky(&zahlavi) } else { zahlavi };
        // výkresový režim: osa staničení
        if k.vykres && pas.druh == DruhPasu::Staniceni {
            cara(ent, [0.0, y0], [w, y0]);
        }
        ent.push(pero.text("zahlavi", zahlavi, [-k.sirka_zahlavi + 2.0 * m, ym], TextHorizontalAlignment::Left, TextVerticalAlignment::Middle, 0.0, H_PASY));
        let text_useku = |ent: &mut Vec<EntityType>, a: f64, b: f64, t: String, stred: bool| {
            let (x, z) = if stred { ((a + b) / 2.0, TextHorizontalAlignment::Center) } else { (a + 1.5 * m, TextHorizontalAlignment::Left) };
            ent.push(pero.text(&role, t, [x, ym], z, TextVerticalAlignment::Middle, 0.0, H_PASY));
        };
        match pas.druh {
            DruhPasu::Smer => {
                let d = 0.28 * (y0 - y1);
                let mut x_pred = 0.0;
                for (s0, p) in &k.g.prvky {
                    let Prvek::Oblouk { r, sweep, .. } = *p else { continue };
                    let (a, b) = ((s0.max(osy.od) - osy.od), ((s0 + p.delka()).min(osy.do_) - osy.od));
                    if b <= a {
                        continue;
                    }
                    let dy = if sweep > 0.0 { d } else { -d };
                    cara(ent, [x_pred, ym], [a, ym]);
                    cara(ent, [a, ym], [a, ym + dy]);
                    cara(ent, [a, ym + dy], [b, ym + dy]);
                    cara(ent, [b, ym + dy], [b, ym]);
                    let (ty, zarovnani) = if dy > 0.0 { (ym - 0.3 * ht, TextVerticalAlignment::Top) } else { (ym + 0.3 * ht, TextVerticalAlignment::Bottom) };
                    ent.push(pero.text(&role, format!("R = {}", cislo(r, 2, true)), [(a + b) / 2.0, ty], TextHorizontalAlignment::Center, zarovnani, 0.0, H_PASY));
                    x_pred = b;
                }
                cara(ent, [x_pred, ym], [w, ym]);
            }
            DruhPasu::Sklony => {
                let Some(n) = k.niveleta else { return };
                let useky: Vec<_> = n.useky().into_iter().filter(|u| u.do_ > osy.od && u.od < osy.do_).collect();
                // nízký řádek: jen text „sklon – délka“ v rámečku
                if pas.vyska < 8.0 {
                    let hranice: Vec<f64> = useky.iter().flat_map(|u| [x_od(u.od), x_od(u.do_)]).collect();
                    ramecek(ent, y0, y1, &hranice);
                    for u in &useky {
                        text_useku(ent, x_od(u.od), x_od(u.do_), format!("{} – {}", sklon_txt(u.sklon, pas.desetin), cislo(u.do_ - u.od, 2, true)), pas.na_stred);
                    }
                    return;
                }
                if k.svislice {
                    ramecek(ent, y0, y1, &[]);
                }
                for u in &useky {
                    let (a, b) = (x_od(u.od), x_od(u.do_));
                    if b <= a {
                        continue;
                    }
                    cara(ent, [a, y0], [a, y1]);
                    cara(ent, [b, y0], [b, y1]);
                    let (ya, yb) = if u.sklon >= 0.0 { (y1, y0) } else { (y0, y1) };
                    cara(ent, [a, ya], [b, yb]);
                    ent.push(pero.text(&role, sklon_txt(u.sklon, pas.desetin), [(a + b) / 2.0, ym + 0.15 * ht], TextHorizontalAlignment::Center, TextVerticalAlignment::Bottom, 0.0, H_PASY));
                    ent.push(pero.text(&role, format!("{} m", cislo(u.do_ - u.od, 2, true)), [(a + b) / 2.0, ym - 0.15 * ht], TextHorizontalAlignment::Center, TextVerticalAlignment::Top, 0.0, H_PASY));
                }
            }
            DruhPasu::Vzdalenosti => {
                let mista = mista_bez_kolizi(pas, osy.od, osy.do_, k.lomy_spolecne, &body, 1.4 * hh);
                let hranice: Vec<f64> = mista.iter().map(|s| s - osy.od).collect();
                ramecek(ent, y0, y1, &hranice);
                for w2 in mista.windows(2) {
                    text_useku(ent, w2[0] - osy.od, w2[1] - osy.od, cislo(w2[1] - w2[0], 2, true), true);
                }
            }
            DruhPasu::Text => {
                ramecek(ent, y0, y1, &[]);
                text_useku(ent, 0.0, w, pas.text.clone(), pas.na_stred);
            }
            DruhPasu::Useky => {
                let useky: Vec<_> = pas.useky.iter().filter(|u| u.do_ > osy.od && u.od < osy.do_).collect();
                let hranice: Vec<f64> = useky.iter().flat_map(|u| [x_od(u.od), x_od(u.do_)]).collect();
                ramecek(ent, y0, y1, &hranice);
                for u in useky {
                    text_useku(ent, x_od(u.od), x_od(u.do_), u.text.clone(), pas.na_stred);
                }
            }
            druh => {
                let lomy: Vec<f64> = match (&pas.druh, &pas.zdroj) {
                    (DruhPasu::Kota, z @ Zdroj::Povrch(_)) if !k.lomy_z_nivelety => {
                        profil(z).map(lomy_profilu).unwrap_or_default().into_iter().chain(k.lomy_spolecne.iter().copied()).collect()
                    }
                    _ => k.lomy_spolecne.to_vec(),
                };
                // rozestup ≈ výška svislého textu, ať se hodnoty nepřekrývají
                let mut mista = mista_bez_kolizi(pas, osy.od, osy.do_, &lomy, &body, 1.4 * ht);
                {
                    // z míst těsně u sebe jen první (svislé texty se nepřepisují,
                    // žádné zdvojené svislice)
                    let mut posledni = f64::NEG_INFINITY;
                    mista.retain(|&s| {
                        let ok = s - posledni >= 1.4 * ht;
                        if ok {
                            posledni = s;
                        }
                        ok
                    });
                }
                for s in mista {
                    let x = s - osy.od;
                    let hodnota = match druh {
                        DruhPasu::Staniceni => {
                            let cele = pas.krok > 0.0 && ((s / pas.krok).round() * pas.krok - s).abs() < 1e-6;
                            Some(staniceni(s, k.fmt, if cele { 0 } else { pas.desetin }, true))
                        }
                        DruhPasu::Teren => vyska(&Zdroj::Teren, s).map(|z| cislo(z, pas.desetin, true)),
                        DruhPasu::Niveleta => vyska(&Zdroj::Niveleta, s).map(|z| cislo(z, pas.desetin, true)),
                        DruhPasu::Kota => vyska(&pas.zdroj, s).map(|z| cislo(z, pas.desetin, true)),
                        DruhPasu::Rozdil => match (vyska(&pas.zdroj, s), vyska(&pas.zdroj2, s)) {
                            (Some(a), Some(b)) => {
                                let d = a - b;
                                // „+“ jen u výkopu/násypu (niveleta − terén), ne u hloubek
                                let plus = d > 0.0 && pas.zdroj == Zdroj::Niveleta;
                                Some(format!("{}{}", if plus { "+" } else { "" }, cislo(d, pas.desetin, true)))
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    let Some(t) = hodnota else { continue };
                    k.svislice_vykresu.borrow_mut().push(x);
                    if !k.svislice_pasu {
                        // bez svislic: číslo přesně v ose staničení (u staničení
                        // ve výkresovém režimu visí od osy dolů)
                        if k.vykres && druh == DruhPasu::Staniceni {
                            ent.push(pero.text(&role, t, [x, y0 - 1.0 * m], TextHorizontalAlignment::Right, TextVerticalAlignment::Middle, std::f64::consts::FRAC_PI_2, H_PASY));
                        } else {
                            ent.push(pero.text(&role, t, [x, ym], TextHorizontalAlignment::Center, TextVerticalAlignment::Middle, std::f64::consts::FRAC_PI_2, H_PASY));
                        }
                        continue;
                    }
                    if k.vykres {
                        // výkresový režim: svislice souvisle přes všechny pásy (viz níže),
                        // číslo na střed pásu
                        let dx = if x < 1.5 * ht { 1.25 * ht } else { -0.3 * ht };
                        if druh == DruhPasu::Staniceni {
                            ent.push(pero.text(&role, t, [x + dx, y0 - 1.0 * m], TextHorizontalAlignment::Right, TextVerticalAlignment::Bottom, std::f64::consts::FRAC_PI_2, H_PASY));
                        } else {
                            ent.push(pero.text(&role, t, [x + dx, ym], TextHorizontalAlignment::Center, TextVerticalAlignment::Bottom, std::f64::consts::FRAC_PI_2, H_PASY));
                        }
                        continue;
                    }
                    cara(ent, [x, y0], [x, y1]);
                    // svislý text vlevo od čáry (u začátku napravo – vlevo je záhlaví)
                    let dx = if x < 1.5 * ht { 1.25 * ht } else { -0.3 * ht };
                    ent.push(pero.text(&role, t, [x + dx, y1 + 1.2 * m], TextHorizontalAlignment::Left, TextVerticalAlignment::Bottom, std::f64::consts::FRAC_PI_2, H_PASY));
                }
            }
        }
    };

    let viditelne: Vec<(usize, &Pas)> = k.def.pasy.iter().enumerate().filter(|(_, p)| !p.skryty).collect();
    let vyska_pasu = |p: &Pas| p.vyska * vz.hustota * m;
    let nad: Vec<(usize, &Pas)> = viditelne.iter().copied().filter(|(_, p)| p.poloha == Poloha::Nad).collect();
    let pod: Vec<(usize, &Pas)> = viditelne.iter().copied().filter(|(_, p)| p.poloha != Poloha::Nad).collect();
    let mezera = 4.0 * m;
    let vyska_nad = if nad.is_empty() { 0.0 } else { nad.iter().map(|(_, p)| vyska_pasu(p)).sum::<f64>() + mezera };
    let mut y0 = osy.vyska() + vyska_nad;
    if !nad.is_empty() && !k.vykres {
        cara(&mut ent, [-k.sirka_zahlavi, y0], [w, y0]);
    }
    for (i, p) in nad {
        let y1 = y0 - vyska_pasu(p);
        kresli(&mut ent, p, y0, y1);
        rozvrzeni.pasy.push((i, y0, y1));
        y0 = y1;
    }
    let mut y0 = 0.0;
    if !pod.is_empty() && !k.vykres {
        cara(&mut ent, [-k.sirka_zahlavi, 0.0], [0.0, 0.0]);
    }
    // výkresový režim: konec souvislých svislic = spodek posledního hodnotového
    // pásu, který navazuje na graf (staničení včetně)
    let mut konec_svislic = 0.0;
    let mut navazuje = true;
    for (i, p) in pod {
        let y1 = y0 - vyska_pasu(p);
        kresli(&mut ent, p, y0, y1);
        rozvrzeni.pasy.push((i, y0, y1));
        if navazuje && p.druh.hodnotovy() {
            konec_svislic = y1;
        } else {
            navazuje = false;
        }
        y0 = y1;
    }
    if k.vykres && k.svislice_pasu {
        let mut xs = k.svislice_vykresu.borrow().clone();
        xs.sort_by(|a, b| a.total_cmp(b));
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        for x in xs {
            cara(&mut ent, [x, 0.0], [x, konec_svislic]);
        }
    }
    if y0 < 0.0 && k.svislice && !k.vykres {
        cara(&mut ent, [0.0, y0], [w, y0]);
    }
    (ent, vyska_nad, rozvrzeni)
}

#[cfg(test)]
mod testy {
    #[test]
    fn velka_pismena_bez_jednotek() {
        assert_eq!(super::velka_mimo_jednotky("Staničení [m]"), "STANIČENÍ [m]");
        assert_eq!(super::velka_mimo_jednotky("Profil [mm] – materiál – délka [m]"), "PROFIL [mm] – MATERIÁL – DÉLKA [m]");
        assert_eq!(super::velka_mimo_jednotky("Kóta terénu"), "KÓTA TERÉNU");
    }
}
