//! Panel „Vlastnosti bodu“ v doku Open CAD Studia (API v7): vlastnosti vybraných
//! bodů – číslo, výška, kód, styl –, přepínač posunu bodů úchopem a aktivní
//! povrch s jeho stavem (aby nemusel být otevřený Prospektor).
//!
//! Panel je deklarativní (pole, výběr, tlačítka); hodnoty plní pracovní
//! vlákno podle výběru, změny polí chodí jako upozornění `PanelAction`.

use std::collections::HashMap;
use std::sync::Mutex;

use ocs_plugin_api::dialog::{DialogControl, DialogValue, PanelDef};
use ocs_plugin_api::host::{Document, Handle};
use teren::preklad::tr;
use teren::trf;

use crate::cogo::BodVykresu;
use crate::spravce::{self, Akce};

pub const ID: &str = "teren_bod";
pub const PRIKAZ: &str = "TER_PANEL";

#[derive(Default)]
struct Stav {
    /// Body, které panel ukazuje.
    body: Vec<Handle>,
    /// Hodnoty, jak je panel naposledy dostal (číslo, z, kód).
    ukazane: HashMap<String, String>,
    /// Hodnoty, jak je uživatel změnil.
    aktualni: HashMap<String, String>,
    /// Vybraný styl (index v nabídce; 0 = beze změny).
    styl: usize,
    /// Id stylů v pořadí nabídky (od indexu 2).
    styly: Vec<String>,
}

static STAV: Mutex<Option<Stav>> = Mutex::new(None);

/// Panel byl v této relaci otevřen (má smysl ho průběžně obnovovat).
static OTEVREN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn otevren() -> bool {
    OTEVREN.load(std::sync::atomic::Ordering::Relaxed)
}

fn stav<R>(f: impl FnOnce(&mut Stav) -> R) -> R {
    let mut g = STAV.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Stav::default))
}

/// Definice panelu se seznamem stylů (id, jméno).
pub fn definice(styly: &[(String, String)]) -> PanelDef {
    OTEVREN.store(true, std::sync::atomic::Ordering::Relaxed);
    stav(|s| {
        s.styly = styly.iter().map(|(id, _)| id.clone()).collect();
        s.styl = 0;
    });
    let mut volby = vec![tr("(beze změny)").to_string(), tr("podle skupiny / kódu").to_string()];
    volby.extend(styly.iter().map(|(_, j)| j.clone()));
    let text = |id: &str, label: &str| DialogControl::Text { id: id.into(), label: label.into(), default: String::new() };
    let tlacitko = |id: &str, label: &str, action: Option<&str>| DialogControl::Button {
        id: id.into(),
        label: label.into(),
        action: action.map(Into::into),
        align_h: None,
        width: None,
    };
    PanelDef::new(ID, tr("Vlastnosti bodu"))
        .with_command(PRIKAZ)
        .with_width(260.0)
        .with_control(text("vyber", tr("Vybráno")))
        .with_control(text("cislo", tr("Číslo")))
        .with_control(text("z", tr("Výška Z")))
        .with_control(text("kod", tr("Kód")))
        .with_control(DialogControl::Choice { id: "styl".into(), label: tr("Styl").into(), options: volby, default_index: 0 })
        .with_control(DialogControl::Row {
            controls: vec![tlacitko("ulozit", tr("Uložit"), None), tlacitko("ukazat", tr("Ukázat"), None)],
            align_h: None,
            align_v: None,
        })
        .with_control(DialogControl::Checkbox { id: "uchopy".into(), label: tr("Posun bodů úchopem").into(), default: false })
        .with_control(DialogControl::InfoText {
            text: tr("Vyberte bod ve výkrese (klik na značku nebo popis). Více bodům najednou se uloží jen změněná pole; číslo jen u jednoho bodu.").into(),
            align_h: None,
        })
        // aktivní povrch – jen prvky, které panel už používá (bez skupiny Fields)
        .with_control(text("povrch", tr("Aktivní povrch")))
        .with_control(DialogControl::Row {
            controls: vec![
                tlacitko("prepocitat", tr("Přepočítat"), Some("TER_PREPOCITAT")),
                tlacitko("pohled3d", tr("Prohlédnout ve 3D"), Some("TER_3D")),
            ],
            align_h: None,
            align_v: None,
        })
        .with_control(tlacitko("prospektor", tr("Otevřít Prospektor"), Some("TER_PROSPEKTOR")))
}

/// Aktivní povrch a jeho stav pro panel („Stávající terén · ● aktuální“).
pub fn text_povrchu(doc: &Document) -> String {
    match crate::povrchy::aktivni(doc) {
        Ok(z) if crate::povrchy::zastaraly(doc, &z) => trf!("{} · ⚠ zastaralý", z.jmeno),
        Ok(z) => trf!("{} · ● aktuální", z.jmeno),
        Err(_) => tr("žádný – vytvořte Nový povrch").to_string(),
    }
}

/// Jen řádek aktivního povrchu (po změně výkresu; pole bodů zůstanou).
pub fn jen_povrch(doc: &Document) -> HashMap<String, DialogValue> {
    HashMap::from([("povrch".to_string(), DialogValue::Text(text_povrchu(doc)))])
}

/// Hodnoty panelu pro vybrané body (a zapamatuje si je) a text aktivního povrchu.
pub fn hodnoty(body: &[&BodVykresu], povrch: String) -> HashMap<String, DialogValue> {
    let mut u: HashMap<String, String> = HashMap::new();
    let vyber = match body {
        [] => tr("nic – vyberte bod").to_string(),
        [b] => trf!("bod {}", b.bod.cislo),
        _ => trf!("bodů: {}", body.len()),
    };
    let spolecne = |f: &dyn Fn(&BodVykresu) -> String| -> String {
        let prvni = body.first().map(|b| f(b)).unwrap_or_default();
        if body.iter().all(|b| f(b) == prvni) { prvni } else { String::new() }
    };
    u.insert("cislo".into(), if body.len() == 1 { body[0].bod.cislo.clone() } else { String::new() });
    u.insert("z".into(), spolecne(&|b| format!("{:.3}", b.bod.z)));
    u.insert("kod".into(), spolecne(&|b| b.bod.popis.clone()));
    stav(|s| {
        s.body = body.iter().map(|b| b.handle).collect();
        s.ukazane = u.clone();
        s.aktualni = u.clone();
        s.styl = 0;
    });
    let mut out: HashMap<String, DialogValue> = u.into_iter().map(|(k, v)| (k, DialogValue::Text(v))).collect();
    out.insert("vyber".into(), DialogValue::Text(vyber));
    out.insert("styl".into(), DialogValue::Choice { index: 0, text: tr("(beze změny)").into() });
    out.insert("uchopy".into(), DialogValue::Bool(crate::uchopy::zapnuto()));
    out.insert("povrch".into(), DialogValue::Text(povrch));
    out
}

/// Změna nebo klik v panelu (upozornění od Open CAD Studia).
pub fn akce(control_id: &str, action: &str, value: Option<DialogValue>) {
    match (control_id, value) {
        ("cislo" | "z" | "kod", Some(DialogValue::Text(t))) => stav(|s| {
            s.aktualni.insert(control_id.into(), t);
        }),
        ("styl", Some(DialogValue::Choice { index, .. })) => stav(|s| s.styl = index),
        ("uchopy", Some(DialogValue::Bool(b))) => spravce::posli(Akce::Uchopy(Some(b))),
        ("ulozit", _) if action == "click" => ulozit(),
        ("ukazat", _) if action == "click" => {
            let h = stav(|s| s.body.clone());
            if !h.is_empty() {
                spravce::posli(Akce::UkazatBody(h));
            }
        }
        _ => {}
    }
}

fn ulozit() {
    let a = stav(|s| {
        if s.body.is_empty() {
            return None;
        }
        let zmena = |k: &str| {
            let a = s.aktualni.get(k).map(|t| t.trim().to_string()).unwrap_or_default();
            (Some(&a) != s.ukazane.get(k).map(|t| t.trim().to_string()).as_ref()).then_some(a)
        };
        let cislo = if s.body.len() == 1 { zmena("cislo").filter(|c| !c.is_empty()) } else { None };
        let z = match zmena("z") {
            Some(t) if !t.is_empty() => match t.replace(',', ".").parse::<f64>() {
                Ok(z) if z.is_finite() => Some(z),
                _ => return Some(Err(trf!("neplatná výška: {}", t))),
            },
            _ => None,
        };
        let kod = zmena("kod");
        let styl = match s.styl {
            0 => None,
            1 => Some(String::new()),
            i => s.styly.get(i - 2).cloned(),
        };
        Some(Ok(Akce::BodyPanel(s.body.clone(), cislo, z, kod, styl)))
    });
    match a {
        Some(Ok(a)) => spravce::posli(a),
        Some(Err(e)) => spravce::posli(Akce::Hlaseni(trf!("Terén: {}", e))),
        None => {}
    }
}
