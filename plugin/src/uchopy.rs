//! Režim „Posun bodů“: vlastní úchop Terénu na každém bodě (API v11).
//!
//! Úchop vykreslí a hlídá Open CAD Studio, tažení posílá pluginu. Během
//! tažení se značka i popisy bodu hýbou živě, po puštění se bod přesune
//! (výška zůstane) jedním krokem Zpět. Úchopy jsou jen zobrazení – do výkresu
//! se neukládají a po zapnutí režimu se ohlásí znovu.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use ocs_plugin_api::host::codec::types::Vector3;
use ocs_plugin_api::host::{
    CustomGripEvent, CustomGripPhase, CustomGripResponse, Document as CadDocument, EntityType, Handle, HostApi, PluginGrip,
    PluginGripShape, PreviewWire, MAX_PLUGIN_LIVE_ENTITIES,
};
use teren::preklad::tr;
use teren::trf;

use crate::cogo;
use crate::vykres::{PresHostPrimo, Vykres};

/// Režim zapnutý.
pub static ZAPNUTO: AtomicBool = AtomicBool::new(false);
/// Právě se táhne úchop – pracovní vlákno počká (viz `vykres`).
pub static TAHNE: AtomicBool = AtomicBool::new(false);

/// Nejvíc úchopů na plugin (limit Open CAD Studia).
pub const MAX_UCHOPU: usize = 4096;

struct Tah {
    handle: Handle,
    start: [f64; 3],
    /// Bod a jeho grafika v poloze před tažením.
    objekty: Vec<EntityType>,
}

static TAH: Mutex<Option<Tah>> = Mutex::new(None);

pub fn zapnuto() -> bool {
    ZAPNUTO.load(Ordering::SeqCst)
}

/// Úchopy pro všechny body výkresu (prázdné, když je režim vypnutý).
/// Druhá hodnota: kolik bodů se nevešlo.
pub fn uchopy(doc: &CadDocument) -> (Vec<PluginGrip>, usize) {
    if !zapnuto() {
        return (Vec::new(), 0);
    }
    let body = cogo::body(doc);
    let navic = body.len().saturating_sub(MAX_UCHOPU);
    let g = body
        .iter()
        .take(MAX_UCHOPU)
        .map(|b| PluginGrip {
            grip_id: b.handle.value(),
            anchor: Some(b.handle),
            point: [b.bod.x, b.bod.y, b.bod.z],
            axis: None,
            shape: PluginGripShape::Circle,
        })
        .collect();
    (g, navic)
}

fn posunuty(e: &EntityType, dx: f64, dy: f64) -> EntityType {
    let mut e = e.clone();
    e.as_entity_mut().translate(Vector3::new(dx, dy, 0.0));
    e
}

fn zivy_nahled(t: &Tah, p: [f64; 3]) -> CustomGripResponse {
    let (dx, dy) = (p[0] - t.start[0], p[1] - t.start[1]);
    let zive: Vec<(Handle, EntityType)> =
        t.objekty.iter().take(MAX_PLUGIN_LIVE_ENTITIES).map(|e| (e.common().handle, posunuty(e, dx, dy))).collect();
    let cara = PreviewWire::line(t.start, [p[0], p[1], t.start[2]], Some([1.0, 0.8, 0.2, 1.0]));
    CustomGripResponse { preview: vec![cara], live_entities: zive, ..Default::default() }
}

/// Tažení úchopu bodu (volá Open CAD Studio).
pub fn udalost(host: &mut dyn HostApi, ev: CustomGripEvent) -> CustomGripResponse {
    match ev.phase {
        CustomGripPhase::Begin => {
            TAHNE.store(true, Ordering::SeqCst);
            let doc = host.document().clone();
            let Some(bv) = cogo::body(&doc).into_iter().find(|b| b.handle.value() == ev.grip_id) else {
                TAHNE.store(false, Ordering::SeqCst);
                return CustomGripResponse::default();
            };
            let mut objekty: Vec<EntityType> = Vec::new();
            let mut handly = vec![bv.handle];
            handly.extend(cogo::grafika_bodu(&doc, &[[bv.bod.x, bv.bod.y]]));
            for e in doc.model_space_entities() {
                if handly.contains(&e.common().handle) {
                    objekty.push(e.clone());
                }
            }
            let t = Tah { handle: bv.handle, start: [bv.bod.x, bv.bod.y, bv.bod.z], objekty };
            let r = zivy_nahled(&t, ev.point);
            *TAH.lock().unwrap_or_else(|e| e.into_inner()) = Some(t);
            r
        }
        CustomGripPhase::Move => match TAH.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(t) => zivy_nahled(t, ev.point),
            None => CustomGripResponse::default(),
        },
        CustomGripPhase::Cancel => {
            *TAH.lock().unwrap_or_else(|e| e.into_inner()) = None;
            TAHNE.store(false, Ordering::SeqCst);
            CustomGripResponse::default()
        }
        CustomGripPhase::Settle => {
            let tah = TAH.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(t) = tah {
                let (dx, dy) = (ev.point[0] - t.start[0], ev.point[1] - t.start[1]);
                if dx.hypot(dy) > 1e-6 {
                    posun(host, t.handle, dx, dy);
                }
            }
            TAHNE.store(false, Ordering::SeqCst);
            CustomGripResponse::default()
        }
    }
}

fn posun(host: &mut dyn HostApi, h: Handle, dx: f64, dy: f64) {
    // celý posun (bod, smazání a vložení grafiky) = jeden krok Zpět
    host.begin_undo_group(tr("Posun bodu"));
    posun_bez_skupiny(host, h, dx, dy);
    host.end_undo_group();
}

fn posun_bez_skupiny(host: &mut dyn HostApi, h: Handle, dx: f64, dy: f64) {
    let mut v = PresHostPrimo(host);
    let Ok(doc) = v.dokument() else { return };
    let Some(bv) = cogo::body(&doc).into_iter().find(|b| b.handle == h) else { return };
    let novy = teren::body::Bod { x: bv.bod.x + dx, y: bv.bod.y + dy, ..bv.bod.clone() };
    let styl = bv.styl.clone();
    match cogo::uprav_body(&mut v, &doc, &[(bv, novy, styl)], tr("Posun bodu")) {
        Ok(_) => v.hotovo(),
        Err(e) => v.chyba(&trf!("Terén: posun bodu: {}", e)),
    }
    // úchopy znovu – bod je jinde
    if let Ok(doc) = v.dokument() {
        let (g, _) = uchopy(&doc);
        let _ = v.0.set_plugin_grips(crate::MANIFEST.id, g);
    }
}
