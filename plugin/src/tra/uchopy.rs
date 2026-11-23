//! Úprava trasy úchopy (API v11).
//!
//! Úchopy se ukážou na trase, kterou uživatel vybere kliknutím (nebo na všech
//! trasách v režimu „Úchopy zap“). Čtvereček na každém vrcholu tečnového
//! polygonu (posun vrcholu), kolečko uprostřed každého oblouku (změna
//! poloměru – tah po ose úhlu), trojúhelník uprostřed každého úseku polygonu
//! (tažením vznikne nový vrchol s poloměrem z editoru). Během tažení
//! se kreslí živý náhled osy; když se oblouky nevejdou, náhled je červený
//! a po puštění se nic nezmění. Úchopy jsou jen zobrazení – do výkresu se
//! neukládají; po zapnutí režimu a po každé změně výkresu se ohlásí znovu.

use std::collections::HashMap;

use ocs_plugin_api::host::Handle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use ocs_plugin_api::host::{
    CustomGripEvent, CustomGripPhase, CustomGripResponse, Document as CadDocument, HostApi, PluginGrip, PluginGripShape,
    PluginRequestSender, PreviewWire,
};
use trasy::geometrie::{geometrie, Geometrie, Prvek, Vrchol};
use crate::lang::tr;
use trasy::trf;

use crate::tra::objekt::{self, ObjektTrasy};
use crate::tra::pohled::ObjektPohledu;
use trasy::niveleta::{Niveleta, VrcholNivelety};
use crate::vykres::{PresHostPrimo, PresKanal, Vykres};

/// Režim zapnutý.
pub static ZAPNUTO: AtomicBool = AtomicBool::new(false);

const VRCHOL: u64 = 0;
const POLOMER: u64 = 1;
/// Vložení vrcholu do úseku polygonu za vrcholem.
const VLOZIT: u64 = 2;

/// Vybrané objekty ve výkrese (z upozornění na změnu výběru).
static VYBER: Mutex<Vec<Handle>> = Mutex::new(Vec::new());
/// Ve výběru je trasa (úchopy se mají obnovovat i bez režimu).
static VYBRANA_TRASA: AtomicBool = AtomicBool::new(false);

/// Značka úchopů Tras (úchopy Terénu mají id = handle bodu).
pub const ZNACKA: u64 = 1 << 63;

fn id_trasy(t: &ObjektTrasy, vrchol: usize, druh: u64) -> u64 {
    id(t, vrchol, druh)
}

fn id(t: &ObjektTrasy, vrchol: usize, druh: u64) -> u64 {
    ZNACKA | (t.insert.value() << 20) | ((vrchol as u64) << 2) | druh
}

fn rozloz(grip: u64) -> (u64, usize, u64) {
    let grip = grip & !ZNACKA;
    (grip >> 20, ((grip >> 2) & 0x3_FFFF) as usize, grip & 3)
}

/// Úchopy tras: všech v režimu „Úchopy zap“, jinak jen vybraných.
pub fn uchopy(doc: &CadDocument) -> Vec<PluginGrip> {
    let vse = ZAPNUTO.load(Ordering::SeqCst);
    let vyber = VYBER.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut out = Vec::new();
    let trasy = objekt::trasy(doc);
    for t in trasy.iter().filter(|t| vse || vyber.contains(&t.insert)) {
        out.extend(uchopy_trasy(t));
    }
    let pohledy = crate::tra::pohled::pohledy(doc);
    for p in pohledy.iter().filter(|p| vyber.contains(&p.insert)) {
        if let Some(t) = trasy.iter().find(|t| t.def.id == p.def.trasa) {
            out.extend(uchopy_pohledu(p, t));
        }
    }
    // vrchol vybraný v Editoru trasy: úchop vrcholu i u trasy, která není
    // vybraná (vybraná má úchopy všech vrcholů)
    if let Some((id, k)) = crate::tra::editor::zvyrazneny_vrchol() {
        if let Some(t) = trasy.iter().find(|t| t.def.id == id && !(vse || vyber.contains(&t.insert))) {
            if let Some(x) = t.vrcholy().get(k) {
                out.push(PluginGrip {
                    xy_plane: false,
                    grip_id: id_trasy(t, k, VRCHOL),
                    anchor: Some(t.insert),
                    point: [x.x, x.y, 0.0],
                    axis: None,
                    shape: PluginGripShape::Square,
                });
            }
        }
    }
    // vrchol vybraný v Editoru nivelety: úchop na niveletě (jen v pohledech,
    // které nejsou vybrané – tam jsou úchopy všech vrcholů)
    if let Some((id, k)) = crate::tra::editor_nivelety::zvyrazneny_vrchol() {
        if let Some(t) = trasy.iter().find(|t| t.def.id == id) {
            if let Some(v) = t.def.nivelety.first().and_then(|n| n.vrcholy.get(k)) {
                for p in pohledy.iter().filter(|p| p.def.trasa == id && !vyber.contains(&p.insert)) {
                    if let Some(point) = p.na_vykres(v.s, v.z) {
                        out.push(PluginGrip {
                            grip_id: id_niv(p, NIV_VRCHOL, k),
                            anchor: Some(t.insert),
                            point,
                            axis: None,
                            shape: PluginGripShape::Square,
                            xy_plane: false,
                        });
                    }
                }
            }
        }
    }
    out
}

// ── Úchopy nivelety v pohledu ───────────────────────────────────────────────

const NIV: u64 = 3;
const NIV_VRCHOL: usize = 0;
const NIV_POLOMER: usize = 1;
const NIV_VLOZIT: usize = 2;

fn id_niv(p: &ObjektPohledu, sub: usize, i: usize) -> u64 {
    ZNACKA | (p.insert.value() << 20) | ((((sub << 16) | i) as u64) << 2) | NIV
}

/// Úchopy nivelety ve vybraném pohledu: vrcholy, poloměry, vložení vrcholu.
pub fn uchopy_pohledu(p: &ObjektPohledu, t: &ObjektTrasy) -> Vec<PluginGrip> {
    let Some(n) = t.def.nivelety.first() else { return Vec::new() };
    let mut out = Vec::new();
    let mut grip = |id: u64, bod: Option<[f64; 3]>, osa: Option<[f64; 3]>, tvar: PluginGripShape| {
        if let Some(point) = bod {
            // Vlastník úchopu = trasa, ne pohled: OCS při tahu vyřadí z uchopování
            // vlastníka a s pohledem by vyřadil i terén – nešlo by se ho chytit.
            // (Pohled se pozná z čísla úchopu.)
            out.push(PluginGrip { grip_id: id, anchor: Some(t.insert), point, axis: osa, shape: tvar, xy_plane: false });
        }
    };
    for (i, v) in n.vrcholy.iter().enumerate() {
        grip(id_niv(p, NIV_VRCHOL, i), p.na_vykres(v.s, v.z), None, PluginGripShape::Square);
    }
    for (i, w) in n.vrcholy.windows(2).enumerate() {
        grip(id_niv(p, NIV_VLOZIT, i), p.na_vykres((w[0].s + w[1].s) / 2.0, (w[0].z + w[1].z) / 2.0), None, PluginGripShape::Triangle);
    }
    for o in n.oblouky() {
        let s = n.vrcholy[o.vrchol].s;
        grip(id_niv(p, NIV_POLOMER, o.vrchol), n.vyska(s).and_then(|z| p.na_vykres(s, z)), Some([0.0, 1.0, 0.0]), PluginGripShape::Circle);
    }
    out
}

struct TahNivelety {
    doc: CadDocument,
    pohled: ObjektPohledu,
    trasa: ObjektTrasy,
    vrcholy: Vec<VrcholNivelety>,
    sub: usize,
    i: usize,
    start: [f64; 3],
}

static TAH_NIV: Mutex<Option<TahNivelety>> = Mutex::new(None);

fn nove_vrcholy_nivelety(t: &TahNivelety, pt: [f64; 3], pusteni: bool) -> Vec<VrcholNivelety> {
    let mut v = t.vrcholy.clone();
    let i = t.i;
    let Some((s, z)) = t.pohled.na_profil(pt) else { return v };
    match t.sub {
        NIV_VRCHOL => {
            let lo = if i > 0 { v[i - 1].s + 0.01 } else { f64::MIN };
            let hi = if i + 1 < v.len() { v[i + 1].s - 0.01 } else { f64::MAX };
            v[i].s = s.clamp(lo, hi);
            v[i].z = z;
        }
        NIV_VLOZIT if i + 1 < v.len() => {
            let s = s.clamp(v[i].s + 0.01, v[i + 1].s - 0.01);
            v.insert(i + 1, VrcholNivelety::new(s, z, crate::tra::prikazy::R_NIVELETY));
            if Niveleta::nova("N", "", v.clone()).kontrola().is_err() {
                v[i + 1].r = 0.0;
            }
        }
        NIV_POLOMER => {
            if let Some(r) = pusteni.then(|| napsana_vzdalenost(t.start, pt)).flatten() {
                // napsané číslo platí v zadání oblouku (R, délka L nebo K)
                use trasy::niveleta::Zadani;
                match v[i].zadani {
                    Zadani::L(_) => v[i].zadani = Zadani::L(r),
                    Zadani::K => v[i].r = r * 100.0,
                    Zadani::R => v[i].r = r,
                }
            } else {
                let n = Niveleta::nova("N", "", t.vrcholy.clone());
                let skl = n.sklony();
                let di = (skl[i] - skl[i - 1]).abs();
                let y = (v[i].z - z).abs();
                if di > 1e-9 {
                    trasy::niveleta::nastav_polomer(&mut v, i, (8.0 * y / (di * di)).max(1.0));
                }
            }
        }
        _ => {}
    }
    v
}

fn nahled_nivelety(t: &TahNivelety, pt: [f64; 3]) -> CustomGripResponse {
    let v = nove_vrcholy_nivelety(t, pt, false);
    let n = Niveleta::nova("N", "", v);
    let body = |b: Vec<[f64; 3]>| b;
    let polygon: Vec<[f64; 3]> = n.vrcholy.iter().filter_map(|x| t.pohled.na_vykres(x.s, x.z)).collect();
    let mut wires = Vec::new();
    if n.kontrola().is_ok() {
        wires.push(PreviewWire::with_color(polygon, [0.6, 0.6, 0.6, 1.0]));
        wires.push(PreviewWire::with_color(body(n.body().into_iter().filter_map(|b| t.pohled.na_vykres(b[0], b[1])).collect()), [1.0, 0.8, 0.2, 1.0]));
    } else {
        wires.push(PreviewWire::with_color(polygon, [1.0, 0.2, 0.2, 1.0]));
    }
    CustomGripResponse { preview: wires, ..Default::default() }
}

fn zacatek_nivelety(host: &mut dyn HostApi, h: u64, idx: usize, pt: [f64; 3]) -> CustomGripResponse {
    let (sub, i) = (idx >> 16, idx & 0xFFFF);
    // výkres jednou na začátku: trasa s niveletou a povrch pro překreslení po puštění
    let doc = host.document().clone();
    let Some(pohled) = crate::tra::pohled::pohledy(&doc).into_iter().find(|p| p.insert.value() == h) else { return CustomGripResponse::default() };
    let Some(trasa) = objekt::trasy(&doc).into_iter().find(|t| t.def.id == pohled.def.trasa) else { return CustomGripResponse::default() };
    let Some(n) = trasa.def.nivelety.first() else { return CustomGripResponse::default() };
    if i >= n.vrcholy.len() {
        return CustomGripResponse::default();
    }
    let start = match sub {
        NIV_POLOMER => n.vyska(n.vrcholy[i].s).and_then(|z| pohled.na_vykres(n.vrcholy[i].s, z)).unwrap_or(pt),
        NIV_VRCHOL => pohled.na_vykres(n.vrcholy[i].s, n.vrcholy[i].z).unwrap_or(pt),
        _ => pt,
    };
    let vrcholy = n.vrcholy.clone();
    if sub != NIV_VLOZIT {
        crate::tra::editor_nivelety::nastav_aktivni(&trasa.def.id);
        crate::tra::editor_nivelety::vyber_a_obnov(i);
    }
    let t = TahNivelety { doc, pohled, trasa, vrcholy, sub, i, start };
    let r = nahled_nivelety(&t, pt);
    *TAH_NIV.lock().unwrap_or_else(|e| e.into_inner()) = Some(t);
    r
}

fn uloz_niveletu(host: &mut dyn HostApi, t: &TahNivelety, v: Vec<VrcholNivelety>) {
    if let Err(e) = Niveleta::nova("N", "", v.clone()).kontrola() {
        let (txt, a) = e.popis();
        host.push_error(&trf!(
            "Trasy: změna se nepřijala – {}", "Alignments: change rejected – {}",
            crate::lang::formatuj(crate::lang::tr_tab(txt), &a.iter().map(|x| x as &dyn std::fmt::Display).collect::<Vec<_>>())
        ));
        return;
    }
    VLASTNI_UPRAVA.store(true, Ordering::SeqCst);
    host.begin_undo_group(tr("Úprava nivelety", "Edit design profile"));
    let mut def = t.trasa.def.clone();
    def.nivelety[0].vrcholy = v;
    let nova = ObjektTrasy { def: def.clone(), ..t.trasa.clone() };
    let pary = {
        let mut w = PresHostPrimo(host);
        objekt::uloz_definici(&mut w, t.trasa.insert, &def);
        // niveleta je vidět jen v pohledu – překreslit hned i při odloženém přepočtu
        let _hned = crate::prepocet::Vynuceni::zacni();
        let pary = crate::tra::pohled::prekresli_zastarale_h(&mut w, &t.doc, Some(&def.id), Some(&nova));
        w.hotovo();
        pary
    };
    host.end_undo_group();
    // výběr a úchopy na překreslených pohledech (vložení mají nové handly)
    let mut vyber = VYBER.lock().unwrap_or_else(|e| e.into_inner());
    for (stary, novy) in &pary {
        if let Some(x) = vyber.iter_mut().find(|x| **x == *stary) {
            *x = *novy;
        }
    }
    let vybrane = vyber.clone();
    drop(vyber);
    let _ = host.set_selection(&vybrane);
    let nove: Vec<Handle> = pary.iter().map(|p| p.1).filter(|h| vybrane.contains(h)).collect();
    let mut g = Vec::new();
    if let Ok(e) = host.entities_batch(&nove) {
        for p in e.into_iter().flatten().filter_map(|e| crate::tra::pohled::z_entity(&e)) {
            g.extend(uchopy_pohledu(&p, &nova));
        }
    }
    let _ = host.set_plugin_grips(crate::MANIFEST.id, crate::uchopy::spoj_trasy(g));
    crate::tra::editor::nastav_aktivni(&def.id);
}

/// Úchopy jedné trasy.
pub fn uchopy_trasy(t: &ObjektTrasy) -> Vec<PluginGrip> {
    let mut out = Vec::new();
    {
        let v = t.vrcholy();
        for (i, x) in v.iter().enumerate() {
            out.push(PluginGrip {
                xy_plane: false,
                grip_id: id(&t, i, VRCHOL),
                anchor: Some(t.insert),
                point: [x.x, x.y, 0.0],
                axis: None,
                shape: PluginGripShape::Square,
            });
        }
        for i in 0..v.len().saturating_sub(1) {
            out.push(PluginGrip {
                xy_plane: false,
                grip_id: id(&t, i, VLOZIT),
                anchor: Some(t.insert),
                point: [(v[i].x + v[i + 1].x) / 2.0, (v[i].y + v[i + 1].y) / 2.0, 0.0],
                axis: None,
                shape: PluginGripShape::Triangle,
            });
        }
        if let Ok(g) = geometrie(&v, t.def.pocatek) {
            for (i, u) in g.vrcholy.iter().enumerate() {
                if u.tecna > 0.0 {
                    let p = [v[i].x + u.vzepeti * u.osa[0], v[i].y + u.vzepeti * u.osa[1], 0.0];
                    out.push(PluginGrip {
                        xy_plane: false,
                        grip_id: id(&t, i, POLOMER),
                        anchor: Some(t.insert),
                        point: p,
                        axis: Some([u.osa[0], u.osa[1], 0.0]),
                        shape: PluginGripShape::Circle,
                    });
                }
            }
        }
    }
    out
}

struct Tah {
    trasa: ObjektTrasy,
    vrchol: usize,
    druh: u64,
    vrcholy: Vec<Vrchol>,
    /// Kde úchop na začátku tažení byl.
    start: [f64; 3],
}

/// Napsané číslo při tažení posune úchop přesně o tu vzdálenost od začátku
/// (OCS jinak neumí). Tažení myší skoro nikdy neskončí na vzdálenosti, která
/// má nejvýš 3 desetinná místa – podle toho se pozná, že uživatel psal.
pub(crate) fn napsana_vzdalenost(start: [f64; 3], p: [f64; 3]) -> Option<f64> {
    let d = (p[0] - start[0]).hypot(p[1] - start[1]);
    let tisiciny = d * 1000.0;
    (d > 1e-6 && (tisiciny - tisiciny.round()).abs() < 1e-5).then_some((tisiciny.round()) / 1000.0)
}

/// Vrcholy po puštění úchopu: u kolečka napsané číslo = nový poloměr.
fn vrcholy_po_pusteni(t: &Tah, p: [f64; 3]) -> Vec<Vrchol> {
    if t.druh == POLOMER {
        if let Some(r) = napsana_vzdalenost(t.start, p) {
            let mut v = t.vrcholy.clone();
            v[t.vrchol].r = r;
            return v;
        }
    }
    nove_vrcholy(t, p)
}

static TAH: Mutex<Option<Tah>> = Mutex::new(None);

/// Vrcholy po tažení do bodu `p`.
fn nove_vrcholy(t: &Tah, p: [f64; 3]) -> Vec<Vrchol> {
    let mut v = t.vrcholy.clone();
    let i = t.vrchol;
    if t.druh == VLOZIT {
        v.insert(i + 1, Vrchol::new(p[0], p[1], crate::tra::editor::polomer()));
        // když se oblouk s poloměrem z editoru nevejde, vrchol vznikne jako lom
        if geometrie(&v, 0.0).is_err() {
            v[i + 1].r = 0.0;
        }
    } else if t.druh == VRCHOL {
        v[i].x = p[0];
        v[i].y = p[1];
    } else if let Ok(g) = geometrie(&t.vrcholy, 0.0) {
        let u = g.vrcholy[i];
        let e = (p[0] - v[i].x) * u.osa[0] + (p[1] - v[i].y) * u.osa[1];
        if v[i].ma_prechodnice() {
            // s přechodnicemi: poloměr i přechodnice úměrně vnější vzdálenosti
            if u.vzepeti > 1e-9 && e > 1e-6 {
                let k = e / u.vzepeti;
                v[i].r *= k;
                v[i].l1 *= k;
                v[i].l2 *= k;
            }
        } else {
            let k = 1.0 / (u.odbocka.abs() / 2.0).cos() - 1.0;
            if k > 1e-12 {
                v[i].r = (e / k).max(0.01);
            }
        }
    }
    v
}

fn cary_osy(g: &Geometrie, barva: [f32; 4]) -> Vec<PreviewWire> {
    g.prvky
        .iter()
        .map(|(_, p)| match *p {
            Prvek::Primka { a, b } => PreviewWire::line([a[0], a[1], 0.0], [b[0], b[1], 0.0], Some(barva)),
            Prvek::Oblouk { stred, r, uhel_a, sweep, .. } => oblouk_nahledu(stred, r, uhel_a, sweep, Some(barva)),
            Prvek::Prechodnice { .. } => prechodnice_nahledu(p, Some(barva)),
        })
        .collect()
}

/// Přechodnice v náhledu jako lomená čára (krok 1 m).
pub fn prechodnice_nahledu(p: &Prvek, barva: Option<[f32; 4]>) -> PreviewWire {
    PreviewWire::Polyline { points: p.body(1.0).iter().map(|b| [b[0], b[1], 0.0]).collect(), color: barva }
}

/// Analytický oblouk náhledu (OCS ho kreslí hladce, přenáší se jen pár čísel).
pub fn oblouk_nahledu(stred: [f64; 2], r: f64, uhel_a: f64, sweep: f64, barva: Option<[f32; 4]>) -> PreviewWire {
    let (z, k) = if sweep >= 0.0 { (uhel_a, uhel_a + sweep) } else { (uhel_a + sweep, uhel_a) };
    PreviewWire::arc([stred[0], stred[1], 0.0], r, z, k, barva)
}

fn nahled(t: &Tah, p: [f64; 3]) -> CustomGripResponse {
    let v = nove_vrcholy(t, p);
    let polygon = PreviewWire::with_color(v.iter().map(|x| [x.x, x.y, 0.0]).collect(), [0.6, 0.6, 0.6, 1.0]);
    let mut wires = vec![polygon];
    match geometrie(&v, t.trasa.def.pocatek) {
        Ok(g) => wires.extend(cary_osy(&g, [1.0, 0.8, 0.2, 1.0])),
        Err(_) => wires[0] = PreviewWire::with_color(v.iter().map(|x| [x.x, x.y, 0.0]).collect(), [1.0, 0.2, 0.2, 1.0]),
    }
    CustomGripResponse { preview: wires, ..Default::default() }
}

/// Tažení úchopu (volá Open CAD Studio).
pub fn udalost(host: &mut dyn HostApi, ev: CustomGripEvent) -> CustomGripResponse {
    match ev.phase {
        CustomGripPhase::Begin => {
            crate::uchopy::TAHNE.store(true, Ordering::SeqCst);
            let (h, vrchol, druh) = rozloz(ev.grip_id);
            if druh == NIV {
                return zacatek_nivelety(host, h, vrchol, ev.point);
            }
            let Some(trasa) = nacti_trasu(host, Handle::new(h)) else {
                crate::uchopy::TAHNE.store(false, Ordering::SeqCst);
                return CustomGripResponse::default();
            };
            let vrcholy = trasa.vrcholy();
            if vrchol >= vrcholy.len() {
                crate::uchopy::TAHNE.store(false, Ordering::SeqCst);
                return CustomGripResponse::default();
            }
            // výchozí bod = poloha úchopu, jak byl ohlášený (od ní OCS měří napsanou vzdálenost)
            let start = match geometrie(&vrcholy, trasa.def.pocatek) {
                Ok(g) if druh == POLOMER => {
                    let u = g.vrcholy[vrchol];
                    [vrcholy[vrchol].x + u.vzepeti * u.osa[0], vrcholy[vrchol].y + u.vzepeti * u.osa[1], 0.0]
                }
                Ok(_) if druh == VRCHOL => [vrcholy[vrchol].x, vrcholy[vrchol].y, 0.0],
                _ => ev.point,
            };
            // klik na vrchol nebo poloměr vybere vrchol v Editoru trasy
            if druh != VLOZIT {
                crate::tra::editor::nastav_aktivni(&trasa.def.id);
                crate::tra::editor::vyber_a_obnov(vrchol);
            }
            let t = Tah { trasa, vrchol, druh, vrcholy, start };
            let r = nahled(&t, ev.point);
            *TAH.lock().unwrap_or_else(|e| e.into_inner()) = Some(t);
            r
        }
        CustomGripPhase::Move if TAH_NIV.lock().unwrap_or_else(|e| e.into_inner()).is_some() => {
            match TAH_NIV.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                Some(t) => nahled_nivelety(t, ev.point),
                None => CustomGripResponse::default(),
            }
        }
        CustomGripPhase::Move => match TAH.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(t) => nahled(t, ev.point),
            None => CustomGripResponse::default(),
        },
        CustomGripPhase::Cancel => {
            *TAH_NIV.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *TAH.lock().unwrap_or_else(|e| e.into_inner()) = None;
            crate::uchopy::TAHNE.store(false, Ordering::SeqCst);
            CustomGripResponse::default()
        }
        CustomGripPhase::Settle => {
            if let Some(t) = TAH_NIV.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let v = nove_vrcholy_nivelety(&t, ev.point, true);
                if v != t.vrcholy {
                    uloz_niveletu(host, &t, v);
                }
                crate::uchopy::TAHNE.store(false, Ordering::SeqCst);
                return CustomGripResponse::default();
            }
            let tah = TAH.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(t) = tah {
                let v = vrcholy_po_pusteni(&t, ev.point);
                if v != t.vrcholy {
                    uloz(host, &t, v);
                }
            }
            crate::uchopy::TAHNE.store(false, Ordering::SeqCst);
            CustomGripResponse::default()
        }
    }
}

fn uloz(host: &mut dyn HostApi, t: &Tah, v: Vec<Vrchol>) {
    if let Err(e) = geometrie(&v, t.trasa.def.pocatek) {
        host.push_error(&trf!("Trasy: změna se nepřijala – {}", "Alignments: change rejected – {}", objekt::chyba_geometrie(e)));
        return;
    }
    // obnovu po vlastní úpravě dělá tato funkce sama (bez kopie výkresu)
    VLASTNI_UPRAVA.store(true, Ordering::SeqCst);
    // Výkres stáhnout před krokem Zpět: s pohledy je potřeba (povrch) a bez
    // nich umožní přepsat blok trasy na místě (snímek + transakce místo ~11
    // požadavků). U velkého výkresu bez pohledů radši postaru bez snímku.
    let doc = (crate::tra::pohled::ma_pohledy() || !crate::vykres::velky_vykres()).then(|| host.document().clone());
    host.begin_undo_group(tr("Úprava trasy", "Edit alignment"));
    let mut nova = None;
    {
        let mut w = PresHostPrimo(host);
        let mut def = t.trasa.def.clone();
        def.vrcholy = v;
        match objekt::uloz(&mut w, doc.as_ref(), Some(&t.trasa), def) {
            Ok(h) => {
                let mut vyber = VYBER.lock().unwrap_or_else(|e| e.into_inner());
                if vyber.contains(&t.trasa.insert) {
                    vyber.retain(|x| *x != t.trasa.insert);
                    vyber.push(h);
                    drop(vyber);
                    w.vyber(vec![h]);
                }
                nova = Some(h);
            }
            Err(e) => w.chyba(&trf!("Trasy: {}", "Alignments: {}", e)),
        }
    }
    host.end_undo_group();
    crate::tra::editor::nastav_aktivni(&t.trasa.def.id);
    let nova = nova.and_then(|h| nacti_trasu(host, h));
    match nova {
        Some(n) if !ZAPNUTO.load(Ordering::SeqCst) => {
            let _ = host.set_plugin_grips(crate::MANIFEST.id, crate::uchopy::spoj_trasy(uchopy_trasy(&n)));
            if crate::tra::editor::otevren() {
                host.update_panel(crate::tra::editor::ID, crate::tra::editor::hodnoty_trasy(Some(&n)));
            }
        }
        // režim všech úchopů nebo chyba: obnovu udělá pracovní vlákno
        _ => VLASTNI_UPRAVA.store(false, Ordering::SeqCst),
    }
}

/// Přečte jednu trasu podle vložení – jen tu entitu, ne celý výkres.
fn nacti_trasu(host: &mut dyn HostApi, h: Handle) -> Option<ObjektTrasy> {
    match host.entities_batch(&[h]) {
        Ok(v) => v.into_iter().flatten().find_map(|e| objekt::z_entity(&e)),
        Err(_) => objekt::trasy(host.document()).into_iter().find(|t| t.insert == h),
    }
}

/// Právě proběhla úprava úchopem a úchopy i panel jsou obnovené – následná
/// změna výkresu se přeskočí (ušetří se stažení celého výkresu).
static VLASTNI_UPRAVA: AtomicBool = AtomicBool::new(false);

// ── Obnova úchopů po změně výkresu ──────────────────────────────────────────

fn kanaly() -> &'static Mutex<HashMap<u64, Box<dyn PluginRequestSender>>> {
    static K: std::sync::OnceLock<Mutex<HashMap<u64, Box<dyn PluginRequestSender>>>> = std::sync::OnceLock::new();
    K.get_or_init(Default::default)
}

pub fn registruj_kanal(tab: u64, s: Box<dyn PluginRequestSender>) {
    kanaly().lock().unwrap_or_else(|e| e.into_inner()).insert(tab, s);
}

static BEZI: AtomicBool = AtomicBool::new(false);
static ZNOVU: AtomicBool = AtomicBool::new(false);

/// Výkres se změnil: při zapnutém režimu znovu ohlásit úchopy (v pracovním
/// vlákně – kanál čeká na konec příkazů a tažení).
pub fn zmena_vykresu(tab: u64) {
    if VLASTNI_UPRAVA.swap(false, Ordering::SeqCst) {
        return;
    }
    if !ZAPNUTO.load(Ordering::SeqCst)
        && !crate::tra::editor::otevren()
        && !VYBRANA_TRASA.load(Ordering::SeqCst)
        && !crate::tra::pohled::ma_pohledy()
    {
        return;
    }
    spust_obnovu(tab);
}

/// Otevřený výkres: zjistit, jestli má pohledy (a případně je přepočítat).
pub fn vykres_otevren(tab: u64) {
    spust_obnovu(tab);
}

/// Výběr ve výkrese se změnil: úchopy na vybraných trasách, trasa do editoru.
pub fn vyber_zmenen(tab: u64, handles: Vec<Handle>) {
    let byla = VYBRANA_TRASA.load(Ordering::SeqCst);
    let prazdny = handles.is_empty();
    *VYBER.lock().unwrap_or_else(|e| e.into_inner()) = handles;
    if prazdny && !byla {
        return;
    }
    spust_obnovu(tab);
}

fn spust_obnovu(tab: u64) {
    ZNOVU.store(true, Ordering::SeqCst);
    if BEZI.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        while ZNOVU.swap(false, Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let s = kanaly().lock().unwrap_or_else(|e| e.into_inner()).remove(&tab);
            let Some(s) = s else { break };
            let mut k = PresKanal(s);
            // Prospektor právě mění výkres – počkat, až skončí
            let start = std::time::Instant::now();
            while crate::tra::prospektor::PRACUJE.load(Ordering::SeqCst) && start.elapsed() < std::time::Duration::from_secs(30) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            if let Ok(mut doc) = k.dokument() {
                // pohledy, jejichž trasa nebo povrch se změnily (přepočet povrchu
                // v Terénu, přesun trasy běžným příkazem, Zpět…)
                if crate::tra::pohled::ma_pohledy() || !crate::tra::pohled::pohledy(&doc).is_empty() {
                    let zastarale = crate::tra::pohled::pohledy(&doc).iter().any(|p| crate::tra::pohled::otisk(&doc, &p.def) != p.def.otisk);
                    if zastarale && !crate::prepocet::smi() {
                        // odložený přepočet – provede ho správce po chvíli klidu
                        crate::prepocet::odloz();
                    } else if zastarale {
                        k.zacatek_kroku(tr("Přepočet podélného profilu", "Recompute longitudinal profile"));
                        let n = crate::tra::pohled::prekresli_zastarale(&mut k, &doc, None, None);
                        k.konec_kroku();
                        if n > 0 {
                            k.info(&trf!("Trasy: přepočítáno pohledů: {}.", "Alignments: views recomputed: {}.", n));
                        }
                        if let Ok(d) = k.dokument() {
                            doc = d;
                        }
                    } else {
                        crate::tra::pohled::MA_POHLEDY.store(!crate::tra::pohled::pohledy(&doc).is_empty(), Ordering::SeqCst);
                    }
                }
                let vyber = VYBER.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let vybrane: Vec<ObjektTrasy> = objekt::trasy(&doc).into_iter().filter(|t| vyber.contains(&t.insert)).collect();
                let vybrany_pohled = crate::tra::pohled::pohledy(&doc).into_iter().find(|p| vyber.contains(&p.insert));
                if let Some(p) = &vybrany_pohled {
                    crate::tra::editor::nastav_aktivni(&p.def.trasa);
                }
                let byla = VYBRANA_TRASA.swap(!vybrane.is_empty() || vybrany_pohled.is_some(), Ordering::SeqCst);
                if let Some(t) = vybrane.first() {
                    crate::tra::editor::nastav_aktivni(&t.def.id);
                }
                if ZAPNUTO.load(Ordering::SeqCst)
                    || byla
                    || !vybrane.is_empty()
                    || vybrany_pohled.is_some()
                    || crate::tra::editor_nivelety::zvyrazneny_vrchol().is_some()
                {
                    let g = uchopy(&doc);
                    k.uchopy(crate::uchopy::spoj_trasy(g));
                }
                if crate::tra::editor::otevren() {
                    k.panel_id(crate::tra::editor::ID, crate::tra::editor::hodnoty(&doc));
                }
                // klik na pohled s niveletou: panel Editor nivelety s jeho niveletou
                let s_niveletou = vybrany_pohled.as_ref().filter(|p| {
                    objekt::trasy(&doc).iter().any(|t| t.def.id == p.def.trasa && !t.def.nivelety.is_empty())
                });
                if let Some(p) = s_niveletou {
                    crate::tra::editor_nivelety::nastav_aktivni(&p.def.trasa);
                }
                if crate::tra::editor_nivelety::otevrit_pro(s_niveletou.map(|p| p.insert.value())) {
                    let def = (!crate::tra::editor_nivelety::otevren()).then(crate::tra::editor_nivelety::definice);
                    k.panel_ukaz(crate::tra::editor_nivelety::ID, def);
                }
                if crate::tra::editor_nivelety::otevren() {
                    k.panel_id(crate::tra::editor_nivelety::ID, crate::tra::editor_nivelety::hodnoty(&doc));
                }
            }
            registruj_kanal(tab, k.0);
        }
        BEZI.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn pozna_napsane_cislo() {
        let z = [100.123456, 200.654321, 0.0];
        // napsáno 40 ve směru osy (0,6; 0,8)
        let p = [z[0] + 0.6 * 40.0, z[1] + 0.8 * 40.0, 0.0];
        assert_eq!(napsana_vzdalenost(z, p), Some(40.0));
        let p = [z[0] + 0.6 * 12.5, z[1] + 0.8 * 12.5, 0.0];
        assert_eq!(napsana_vzdalenost(z, p), Some(12.5));
        // tažení myší
        let p = [z[0] + 7.318_274_9, z[1] - 3.910_028_3, 0.0];
        assert_eq!(napsana_vzdalenost(z, p), None);
    }
}
