//! Panel „Editor trasy“ v doku Open CAD Studia.
//!
//! Hlavička (trasa, délka, počet vrcholů), tabulka vrcholů (X, Y, typ, R,
//! přechodnice L1 / L2, parametr A, úhel, přímá za), lišta vybraného vrcholu
//! (◀ / Ukázat ve výkrese / ▶, zvýraznění, popisy hlavních bodů), detail
//! vrcholu, nástroje (vrcholy, oblouky, přechodnice), nastavení oblouku při
//! vložení a kontrola.
//!
//! Řádek tabulky v Open CAD Studiu vybrat nejde – vrchol se přepíná lištou
//! nebo kliknutím na úchop vrcholu (vybraný řádek má v čísle hvězdičku).
//!
//! Změny polí chodí jako upozornění `PanelAction` při každém stisku klávesy
//! a bez přístupu k výkresu. Panel si je proto jen zapamatuje a správce
//! (pracovní vlákno s kanálem do výkresu) je po krátké pauze v psaní
//! zapíše do výkresu a panel obnoví.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ocs_plugin_api::dialog::{DialogControl, DialogValue, PanelDef, TextStyle};
use ocs_plugin_api::host::{Document as CadDocument, HostApi};
use trasy::geometrie::{geometrie, parametry_prechodnice, tecny, vzd, Chyba, Geometrie, Vrchol};
use crate::lang::tr;
use trasy::stanice::{cislo, staniceni, FormatStanice};
use trasy::trf;

use crate::tra::objekt::{self, ObjektTrasy};
use crate::vykres::{PresKanal, Vykres};

pub const ID: &str = "trasy_editor";
pub const PRIKAZ: &str = "TRA_EDITOR";

/// Pauza v psaní, po které se změna z panelu zapíše do výkresu.
const PAUZA: Duration = Duration::from_millis(900);

#[derive(Clone, Debug)]
enum Zmena {
    Tabulka(Vec<Vec<String>>),
    /// Pole detailu vybraného vrcholu (id, text).
    Pole(String, String),
    /// Typ oblouku vybraného vrcholu (0 bez oblouku, 1 kružnice, 2 s přechodnicemi).
    Typ(usize),
}

struct Stav {
    /// Id trasy, kterou panel ukazuje.
    aktivni: Option<String>,
    vybrany: usize,
    zmena: Option<(Zmena, Instant)>,
    obnovit: bool,
    /// Nastavení oblouku při vložení: R, typ (0 kružnice, 1 s přechodnicemi),
    /// zadání přechodnice (0 délka L, 1 parametr A), hodnota, poměr L1 : L2.
    n_r: f64,
    n_typ: usize,
    n_zad: usize,
    n_hod: f64,
    n_pom: (f64, f64),
    /// Když se oblouk nevejde, zmenšit ho (jinak nevložit).
    zmensit: bool,
    /// Kontrola: nejmenší poloměr a nejkratší mezipřímka.
    k_rmin: f64,
    k_mez: f64,
    zvyraznit: bool,
    ukazat: bool,
    popisy: Option<bool>,
}

static STAV: Mutex<Stav> = Mutex::new(Stav {
    aktivni: None,
    vybrany: 0,
    zmena: None,
    obnovit: false,
    n_r: 20.0,
    n_typ: 0,
    n_zad: 0,
    n_hod: 20.0,
    n_pom: (1.0, 1.0),
    zmensit: true,
    k_rmin: 20.0,
    k_mez: 10.0,
    zvyraznit: true,
    ukazat: false,
    popisy: None,
});
static OTEVREN: AtomicBool = AtomicBool::new(false);
/// Požadavek „vrátit poslední vrchol“ pro běžící kreslení.
static VRATIT: AtomicBool = AtomicBool::new(false);

fn stav<R>(f: impl FnOnce(&mut Stav) -> R) -> R {
    f(&mut STAV.lock().unwrap_or_else(|e| e.into_inner()))
}

pub fn otevren() -> bool {
    OTEVREN.load(Ordering::SeqCst)
}
/// Poloměr nového oblouku (kreslení, vložení vrcholu, prodloužení).
pub fn polomer() -> f64 {
    stav(|s| s.n_r)
}
pub fn aktivni() -> Option<String> {
    stav(|s| s.aktivni.clone())
}
pub fn nastav_aktivni(id: &str) {
    stav(|s| {
        if s.aktivni.as_deref() != Some(id) {
            s.zmena = None;
            s.vybrany = 0;
        }
        s.aktivni = Some(id.to_string());
    });
}
pub fn vybrany() -> usize {
    stav(|s| s.vybrany)
}
/// Vybere vrchol a požádá o obnovu panelu (klik na úchop vrcholu).
pub fn vyber_a_obnov(i: usize) {
    stav(|s| {
        s.vybrany = i;
        s.obnovit = true;
    });
}
/// Starý panel měl tabulky s tlačítkem Použít změny – nový zapisuje hned.
pub fn vezmi_tabulku() -> Option<Vec<Vec<String>>> {
    None
}
pub fn vezmi_prvky() -> Option<Vec<Vec<String>>> {
    None
}
/// Má kreslení vrátit poslední vrchol? (jednou)
pub fn vezmi_vratit() -> bool {
    VRATIT.swap(false, Ordering::SeqCst)
}

/// Vrchol trasy, který se má zvýraznit: (id trasy, index) – jen při otevřeném
/// panelu se zapnutým zvýrazněním.
pub fn zvyrazneny_vrchol() -> Option<(String, usize)> {
    if !otevren() {
        return None;
    }
    stav(|s| if s.zvyraznit { s.aktivni.clone().map(|t| (t, s.vybrany)) } else { None })
}

/// Číslo z buňky nebo pole (desetinná čárka, mezery, hvězdička vybraného řádku).
pub fn cti(t: &str) -> Option<f64> {
    let t: String = t.chars().filter(|c| !matches!(c, '*' | ' ' | '\u{a0}')).collect();
    t.replace(',', ".").parse().ok().filter(|x: &f64| x.is_finite())
}

/// Poměr „1 : 1“ (nebo „40:60“).
fn cti_pomer(t: &str) -> Option<(f64, f64)> {
    let (a, b) = t.split_once(':')?;
    match (cti(a), cti(b)) {
        (Some(a), Some(b)) if a > 0.0 && b > 0.0 => Some((a, b)),
        _ => None,
    }
}

// ── Vkládání oblouku podle nastavení ────────────────────────────────────────

/// Výsledek vložení oblouku podle nastavení.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vlozeni {
    Ano,
    /// Oblouk se vešel až zmenšený.
    Zmenseno,
    /// Nevešel se (vrchol zůstal beze změny).
    Nevejde,
}

/// Délky přechodnic L1, L2 k poloměru `r` podle nastavení panelu.
fn prechodnice_z_nastaveni(r: f64) -> (f64, f64) {
    let (zad, h, (a, b)) = stav(|s| (s.n_zad, s.n_hod, s.n_pom));
    let l1 = if zad == 1 { h * h / r.max(0.01) } else { h };
    (l1, l1 * b / a)
}

/// Vloží do vrcholu `k` oblouk podle nastavení panelu (`jen_prechodnice` =
/// ponechat poloměr vrcholu a jen přidat přechodnice). Když se nevejde
/// a zmenšení je zapnuté, zkrátí se nejdřív přechodnice (u malého úhlu
/// vrcholu se jinak nevejdou při žádném poloměru), pak i poloměr.
pub fn dej_oblouk(v: &mut [Vrchol], k: usize, jen_prechodnice: bool) -> Vlozeni {
    if k == 0 || k + 1 >= v.len() {
        return Vlozeni::Nevejde;
    }
    let (n_r, n_typ, zmensit) = stav(|s| (s.n_r, s.n_typ, s.zmensit));
    let puvodni = v[k];
    let r0 = if jen_prechodnice && v[k].r > 0.0 { v[k].r } else { n_r };
    let s_prech = jen_prechodnice || n_typ == 1;
    let (l1, l2) = if s_prech { prechodnice_z_nastaveni(r0) } else { (0.0, 0.0) };
    // poloměr se u „jen přechodnice“ nemění
    let kroky = |od: f64| std::iter::successors(Some(1.0), |f| Some(f * 0.85)).take_while(move |f| *f > od);
    let fr_kroky: Vec<f64> = if zmensit && !jen_prechodnice { kroky(0.1).collect() } else { vec![1.0] };
    let fl_kroky: Vec<f64> = if zmensit && s_prech { kroky(0.02).collect() } else { vec![1.0] };
    for &fr in &fr_kroky {
        for &fl in &fl_kroky {
            v[k].r = (r0 * fr * 100.0).round() / 100.0;
            v[k].l1 = (l1 * fl * 100.0).round() / 100.0;
            v[k].l2 = (l2 * fl * 100.0).round() / 100.0;
            if geometrie(v, 0.0).is_ok() {
                return if fr < 1.0 || fl < 1.0 { Vlozeni::Zmenseno } else { Vlozeni::Ano };
            }
        }
    }
    v[k] = puvodni;
    Vlozeni::Nevejde
}

// ── Definice panelu ─────────────────────────────────────────────────────────

fn tlacitko(id: &str, label: &str, prikaz: Option<&str>) -> DialogControl {
    DialogControl::Button { id: id.into(), label: label.into(), action: prikaz.map(Into::into), align_h: None, width: None }
}

fn radek(c: Vec<DialogControl>) -> DialogControl {
    DialogControl::Row { controls: c, align_h: None, align_v: None }
}

fn radek_nahore(c: Vec<DialogControl>) -> DialogControl {
    DialogControl::Row { controls: c, align_h: None, align_v: Some(ocs_plugin_api::dialog::VerticalAlign::Top) }
}

fn nadpis(t: &str) -> DialogControl {
    DialogControl::Column {
        controls: vec![
            DialogControl::StyledText { text: " ".into(), style: TextStyle::Normal, align_h: None },
            DialogControl::StyledText { text: t.into(), style: TextStyle::Heading, align_h: None },
        ],
        align_h: None,
        align_v: None,
    }
}

fn sloupec(nazev: &str, c: Vec<DialogControl>) -> DialogControl {
    let mut v = vec![DialogControl::StyledText { text: nazev.into(), style: TextStyle::Muted, align_h: None }];
    v.extend(c);
    DialogControl::Column { controls: v, align_h: None, align_v: None }
}

fn text(id: &str, label: &str, default: String) -> DialogControl {
    DialogControl::Text { id: id.into(), label: label.into(), default }
}

const TYPY: [&str; 3] = ["bez oblouku", "kružnice", "s přechodnicemi"];

pub fn definice() -> PanelDef {
    OTEVREN.store(true, Ordering::SeqCst);
    let (n_r, n_typ, n_zad, n_hod, n_pom, zmensit, k_rmin, k_mez, zvyraznit) =
        stav(|s| (s.n_r, s.n_typ, s.n_zad, s.n_hod, s.n_pom, s.zmensit, s.k_rmin, s.k_mez, s.zvyraznit));
    PanelDef::new(ID, tr("Editor trasy", "Alignment editor"))
        .with_command(PRIKAZ)
        .with_width(580.0)
        .with_control(DialogControl::Label { id: "info".into(), text: String::new(), align_h: None })
        .with_control(nadpis(tr("Vrcholy", "Vertices")))
        .with_control(DialogControl::Table {
            id: "vrcholy".into(),
            label: None,
            headers: vec![
                "#".into(),
                "X".into(),
                "Y".into(),
                tr("Typ", "Type").into(),
                "R".into(),
                "L1".into(),
                "L2".into(),
                "A1 / A2".into(),
                tr("Úhel [g]", "Angle [gon]").into(),
                tr("Přímá za", "Tangent after").into(),
            ],
            rows: Vec::new(),
            allow_add: false,
            allow_delete: false,
        })
        .with_control(DialogControl::StyledText {
            text: tr("* = vybraný vrchol. Typ: b = bez oblouku, k = kružnice, p = s přechodnicemi. Přepsané X, Y, R, L1, L2 se zapíší po chvíli; A = √(R·L) se dopočítá.", "* = selected vertex. Type: b = no curve, k = circular arc, p = with transitions. Edited X, Y, R, L1, L2 are applied after a moment; A = √(R·L) is computed.").into(),
            style: TextStyle::Muted,
            align_h: None,
        })
        .with_control(nadpis(tr("Vybraný vrchol", "Selected vertex")))
        .with_control(DialogControl::Choice { id: "vrchol".into(), label: tr("Vrchol", "Vertex").into(), options: vec!["1".into()], default_index: 0 })
        .with_control(radek(vec![
            tlacitko("pred", tr("◀ Předchozí", "◀ Previous"), None),
            tlacitko("ukazat", tr("Ukázat ve výkrese", "Show in drawing"), None),
            tlacitko("dalsi", tr("Další ▶", "Next ▶"), None),
        ]))
        // zaškrtávátka samostatně – v řádku s tlačítkem jim OCS neobnoví stav
        .with_control(DialogControl::Checkbox { id: "zvyraznit".into(), label: tr("Zvýraznit v situaci", "Highlight in plan").into(), default: zvyraznit })
        .with_control(DialogControl::Checkbox { id: "popisy".into(), label: tr("Popisy hlavních bodů", "Key point labels").into(), default: true })
        .with_control(DialogControl::Fields {
            id: "detail".into(),
            title: None,
            controls: vec![
                text("d_x", "X [m]", String::new()),
                text("d_y", "Y [m]", String::new()),
                DialogControl::Choice {
                    id: "d_typ".into(),
                    label: tr("Typ oblouku", "Curve type").into(),
                    options: TYPY.iter().map(|t| crate::lang::tr_tab(t).to_string()).collect(),
                    default_index: 0,
                },
                text("d_r", tr("Poloměr R [m]", "Radius R [m]"), String::new()),
                text("d_l1", tr("Přechodnice L1 před vrcholem [m]", "Transition L1 before vertex [m]"), String::new()),
                text("d_l2", tr("Přechodnice L2 za vrcholem [m]", "Transition L2 after vertex [m]"), String::new()),
                DialogControl::Label { id: "d_info".into(), text: String::new(), align_h: None },
            ],
            collapsed: false,
        })
        .with_control(nadpis(tr("Nástroje", "Tools")))
        .with_control(radek_nahore(vec![
            sloupec(tr("Vrcholy", "Vertices"), vec![
                tlacitko("pridat", tr("+ Přidat vrchol", "+ Add vertex"), Some("TRA_VLOZIT_VRCHOL")),
                tlacitko("smazat", tr("✕ Smazat vrchol", "✕ Delete vertex"), Some("TRA_SMAZAT_VRCHOL")),
                tlacitko("posun", tr("✥ Posunout přesně", "✥ Move exactly"), Some("TRA_POSUN_VRCHOL")),
                tlacitko("prodlouzit", tr("→ Prodloužit", "→ Extend"), Some("TRA_PRODLOUZIT")),
            ]),
            sloupec(tr("Oblouky", "Curves"), vec![
                tlacitko("vlozit", tr("⌒ Vložit oblouk", "⌒ Insert curve"), Some("TRA_VLOZIT_OBLOUK")),
                tlacitko("zmenit_r", tr("R Změnit poloměr", "R Change radius"), Some("TRA_OBLOUK")),
                tlacitko("odstranit", tr("✕ Odstranit oblouk", "✕ Remove curve"), Some("TRA_BEZ_OBLOUKU")),
                tlacitko("vsechny", tr("⌒⌒ Do všech vrcholů", "⌒⌒ At all vertices"), Some("TRA_OBLOUKY_VSE")),
            ]),
            sloupec(tr("Přechodnice", "Transitions"), vec![
                tlacitko("prech", tr("∫ Přidat přechodnice", "∫ Add transitions"), Some("TRA_PRECH")),
                tlacitko("bez_prech", tr("✕ Odebrat přechodnice", "✕ Remove transitions"), Some("TRA_BEZ_PRECH")),
                tlacitko("prech_vse", tr("∫∫ Všem obloukům", "∫∫ To all curves"), Some("TRA_PRECH_VSE")),
            ]),
        ]))
        .with_control(DialogControl::Checkbox { id: "zmensit".into(), label: tr("Zmenšit, když se nevejde", "Shrink if it does not fit").into(), default: zmensit })
        .with_control(nadpis(tr("Nastavení oblouku při vložení", "Curve settings for inserting")))
        .with_control(DialogControl::Fields {
            id: "nastaveni".into(),
            title: None,
            controls: vec![
                text("n_r", tr("Poloměr R [m]", "Radius R [m]"), cislo(n_r, 2, false)),
                DialogControl::Choice {
                    id: "n_typ".into(),
                    label: tr("Typ oblouku", "Curve type").into(),
                    options: vec![tr("kružnice", "circle").into(), tr("s přechodnicemi", "with transitions").into()],
                    default_index: n_typ,
                },
                DialogControl::Choice {
                    id: "n_zad".into(),
                    label: tr("Přechodnice zadána", "Transition given by").into(),
                    options: vec![tr("délkou L", "length L").into(), tr("parametrem A", "parameter A").into()],
                    default_index: n_zad,
                },
                text("n_hod", tr("Hodnota (L nebo A)", "Value (L or A)"), cislo(n_hod, 2, false)),
                text("n_pom", tr("L1 : L2 (1 : 1 = souměrné)", "L1 : L2 (1 : 1 = symmetric)"), format!("{} : {}", cislo(n_pom.0, 0, false), cislo(n_pom.1, 0, false))),
            ],
            collapsed: false,
        })
        .with_control(nadpis(tr("Prvky trasy", "Alignment elements")))
        .with_control(DialogControl::Table {
            id: "prvky".into(),
            label: None,
            headers: vec![tr("Prvek", "Element").into(), tr("Od", "From").into(), tr("Do", "To").into(), tr("Délka", "Length").into(), "R / A".into()],
            rows: Vec::new(),
            allow_add: false,
            allow_delete: false,
        })
        .with_control(nadpis(tr("Kontrola", "Check")))
        .with_control(DialogControl::Fields {
            id: "kontrola_nast".into(),
            title: None,
            controls: vec![
                text("k_rmin", tr("Nejmenší R [m]", "Minimum R [m]"), cislo(k_rmin, 0, false)),
                text("k_mez", tr("Min. mezipřímka [m]", "Min. tangent [m]"), cislo(k_mez, 0, false)),
            ],
            collapsed: false,
        })
        .with_control(DialogControl::Label { id: "kontrola".into(), text: String::new(), align_h: None })
        .with_control(nadpis(tr("Trasa", "Alignment")))
        .with_control(radek(vec![
            tlacitko("nova", tr("Nová trasa", "New alignment"), Some("TRA_KRESLIT")),
            tlacitko("zpet", tr("Vrátit vrchol (při kreslení)", "Undo vertex (while drawing)"), None),
            tlacitko("vybrat", tr("Jiná trasa", "Other alignment"), Some("TRA_EDITOR_VYBRAT")),
            tlacitko("vlastnosti", tr("Vlastnosti", "Properties"), Some("TRA_VLASTNOSTI")),
        ]))
}

// ── Hodnoty panelu ──────────────────────────────────────────────────────────

/// Trasa, kterou editor ukazuje: aktivní, jinak první ve výkrese.
pub fn trasa_editoru(doc: &CadDocument) -> Option<ObjektTrasy> {
    let vse = objekt::trasy(doc);
    let id = aktivni();
    vse.iter().find(|t| Some(&t.def.id) == id.as_ref()).cloned().or_else(|| vse.into_iter().next())
}

/// Typ oblouku vrcholu: 0 bez oblouku, 1 kružnice, 2 s přechodnicemi.
fn typ_vrcholu(v: &[Vrchol], k: usize) -> usize {
    if k == 0 || k + 1 >= v.len() || v[k].r <= 0.0 {
        0
    } else if v[k].ma_prechodnice() {
        2
    } else {
        1
    }
}

/// Úhel odbočení ve vrcholu se znaménkem (+ vlevo) [rad].
fn odbocka(v: &[Vrchol], k: usize) -> Option<f64> {
    if k == 0 || k + 1 >= v.len() {
        return None;
    }
    let a1 = (v[k].y - v[k - 1].y).atan2(v[k].x - v[k - 1].x);
    let a2 = (v[k + 1].y - v[k].y).atan2(v[k + 1].x - v[k].x);
    let mut d = a2 - a1;
    while d > std::f64::consts::PI {
        d -= 2.0 * std::f64::consts::PI;
    }
    while d < -std::f64::consts::PI {
        d += 2.0 * std::f64::consts::PI;
    }
    Some(d)
}

fn gon(r: f64) -> f64 {
    r * 200.0 / std::f64::consts::PI
}

/// Mezipřímka za vrcholem `k` (k dalšímu vrcholu, mezi tečnami oblouků).
fn prima_za(v: &[Vrchol], k: usize) -> Option<f64> {
    (k + 1 < v.len()).then(|| vzd(v[k].bod(), v[k + 1].bod()) - tecny(v, k).1 - tecny(v, k + 1).0)
}

/// Řádky tabulky vrcholů.
pub fn radky(v: &[Vrchol], vybrany: usize) -> Vec<Vec<String>> {
    (0..v.len())
        .map(|k| {
            let x = v[k];
            let typ = typ_vrcholu(v, k);
            let vnitrni = k > 0 && k + 1 < v.len();
            let pismeno = if !vnitrni { String::new() } else { ["b", "k", "p"][typ].to_string() };
            let nebo_prazdne = |b: bool, t: String| if b { t } else { String::new() };
            let a = |l: f64| if l > 0.0 { cislo((x.r * l).sqrt(), 1, false) } else { "–".into() };
            vec![
                if k == vybrany { format!("* {}", k + 1) } else { (k + 1).to_string() },
                cislo(x.x, 3, false),
                cislo(x.y, 3, false),
                pismeno,
                nebo_prazdne(vnitrni && typ > 0, cislo(x.r, 2, false)),
                nebo_prazdne(vnitrni && typ == 2, cislo(x.l1, 2, false)),
                nebo_prazdne(vnitrni && typ == 2, cislo(x.l2, 2, false)),
                nebo_prazdne(vnitrni && typ == 2, format!("{} / {}", a(x.l1), a(x.l2))),
                odbocka(v, k).map_or(String::new(), |d| format!("{} {}", cislo(gon(d.abs()), 3, false), if d > 0.0 { "L" } else { "P" })),
                prima_za(v, k).map_or(String::new(), |m| cislo(m, 2, false)),
            ]
        })
        .collect()
}

/// Řádky tabulky prvků (přímá, přechodnice, oblouk se staničením).
fn radky_prvku(g: &Geometrie, f: FormatStanice) -> Vec<Vec<String>> {
    trasy::upravy::tabulka_prvku(g)
        .iter()
        .map(|r| {
            let strana = |x: f64| if x > 0.0 { tr("L", "L") } else { tr("P", "R") };
            vec![
                if r.oblouk {
                    tr("Oblouk", "Curve")
                } else if r.prechodnice {
                    tr("Přechodnice", "Transitions")
                } else {
                    tr("Přímá", "Tangent")
                }
                .to_string(),
                staniceni(r.od, f, 2, true),
                staniceni(r.do_, f, 2, true),
                cislo(r.delka, 2, true),
                if r.oblouk {
                    format!("R {} {}", cislo(r.polomer.abs(), 2, true), strana(r.polomer))
                } else if r.prechodnice {
                    format!("A {}", cislo(r.polomer.abs(), 2, true))
                } else {
                    String::new()
                },
            ]
        })
        .collect()
}

/// Řádky kontroly (✕ chyba, ! upozornění, ✓ v pořádku).
pub fn kontrola(v: &[Vrchol], pocatek: f64) -> Vec<String> {
    let (rmin, mmin) = stav(|s| (s.k_rmin, s.k_mez));
    let mut out = Vec::new();
    if let Err(e) = geometrie(v, pocatek) {
        out.push(format!("✕ {}", objekt::chyba_geometrie(e)));
        // u překryvu dopsat, o kolik
        for k in 0..v.len().saturating_sub(1) {
            if let Some(m) = prima_za(v, k).filter(|m| *m < -1e-6) {
                out.push(trf!("✕ Oblouky ve vrcholech {} a {} se překrývají o {} m.", "✕ Curves at vertices {} and {} overlap by {} m.", k + 1, k + 2, cislo(-m, 2, true)));
            }
        }
        return out;
    }
    for k in 1..v.len().saturating_sub(1) {
        let x = v[k];
        if x.r <= 0.0 || odbocka(v, k).is_none_or(|d| d.abs() < 1e-6) {
            continue;
        }
        if x.r < rmin {
            out.push(trf!("! Vrchol {}: R = {} m je menší než {} m.", "! Vertex {}: R = {} m is less than {} m.", k + 1, cislo(x.r, 2, true), cislo(rmin, 0, true)));
        }
        for (l, kde) in [(x.l1, tr("před", "before")), (x.l2, tr("za", "after"))] {
            if l > 0.0 {
                let a = (x.r * l).sqrt();
                if a < x.r / 3.0 || a > x.r {
                    out.push(trf!(
                        "! Vrchol {}: přechodnice {} vrcholem má A = {} – doporučeno R/3 až R ({} až {}).", "! Vertex {}: transition {} the vertex has A = {} – recommended R/3 to R ({} to {}).",
                        k + 1,
                        kde,
                        cislo(a, 1, true),
                        cislo(x.r / 3.0, 1, true),
                        cislo(x.r, 1, true)
                    ));
                }
            }
        }
        // mezipřímka k dalšímu oblouku
        let d = v[k + 1];
        if k + 2 < v.len() && d.r > 0.0 {
            let m = prima_za(v, k).unwrap_or(0.0);
            let protismer = odbocka(v, k).unwrap_or(0.0).signum() != odbocka(v, k + 1).unwrap_or(0.0).signum();
            if m < 0.01 && protismer && x.l2 > 0.0 && d.l1 > 0.0 {
                out.push(trf!("✓ Vrcholy {} a {}: protisměrné oblouky s inflexním bodem přechodnic (v pořádku).", "✓ Vertices {} and {}: reverse curves with transition inflection point (OK).", k + 1, k + 2));
            } else if m < 0.01 && !protismer {
                out.push(trf!("! Vrcholy {} a {}: stejnosměrné oblouky bez mezipřímky – zvažte jeden oblouk.", "! Vertices {} and {}: same-direction curves without a tangent – consider a single curve.", k + 1, k + 2));
            } else if m < mmin {
                out.push(trf!(
                    "! Mezipřímka mezi oblouky {} a {} je jen {} m (méně než {} m).", "! Tangent between curves {} and {} is only {} m (less than {} m).",
                    k + 1,
                    k + 2,
                    cislo(m, 2, true),
                    cislo(mmin, 0, true)
                ));
            }
        }
    }
    if !out.iter().any(|t| t.starts_with('!')) {
        out.push(tr("✓ V pořádku – oblouky se vejdou, poloměry, přechodnice i mezipřímky vyhovují.", "✓ OK – curves fit, radii, transitions and tangents are fine.").into());
    }
    out
}

/// Hodnoty panelu (bez pole poloměru – to patří uživateli).
pub fn hodnoty(doc: &CadDocument) -> HashMap<String, DialogValue> {
    hodnoty_trasy(trasa_editoru(doc).as_ref())
}

/// Hodnoty panelu pro danou trasu (bez stahování výkresu).
pub fn hodnoty_trasy(t: Option<&ObjektTrasy>) -> HashMap<String, DialogValue> {
    let mut m = HashMap::new();
    let Some(t) = t else {
        m.insert("info".into(), DialogValue::Text(tr("Žádná trasa – nakreslete Novou trasu, nebo klikněte na trasu ve výkrese.", "No alignment – draw a New alignment or click an alignment in the drawing.").into()));
        m.insert("vrcholy".into(), DialogValue::Table(Vec::new()));
        m.insert("prvky".into(), DialogValue::Table(Vec::new()));
        m.insert("kontrola".into(), DialogValue::Text(String::new()));
        return m;
    };
    nastav_aktivni(&t.def.id);
    let v = t.vrcholy();
    let k = vybrany().min(v.len().saturating_sub(1));
    stav(|s| s.vybrany = k);
    let h = objekt::hodnoty(&t.def);
    let f = FormatStanice::z_kodu(h.text("format"));
    let g = geometrie(&v, t.def.pocatek).ok();
    m.insert(
        "info".into(),
        DialogValue::Text(trf!(
            "Trasa {} – {} · délka {} m · vrcholů {} · začátek {}", "Alignment {} – {} · length {} m · vertices {} · start {}",
            t.def.id,
            t.def.jmeno,
            g.as_ref().map_or("–".to_string(), |g| cislo(g.delka(), 2, true)),
            v.len(),
            staniceni(t.def.pocatek, f, 2, true)
        )),
    );
    m.insert("vrcholy".into(), DialogValue::Table(radky(&v, k)));
    m.insert("prvky".into(), DialogValue::Table(g.as_ref().map_or(Vec::new(), |g| radky_prvku(g, f))));
    let seznam: Vec<String> = (0..v.len())
        .map(|i| {
            let typ = if i == 0 || i + 1 == v.len() { tr("konec", "end") } else { crate::lang::tr_tab(TYPY[typ_vrcholu(&v, i)]) };
            format!("{} ({})", i + 1, typ)
        })
        .collect();
    m.insert("vrchol".into(), DialogValue::ChoiceOptions { options: seznam, selected_index: k });
    let x = v[k];
    let vnitrni = k > 0 && k + 1 < v.len();
    let typ = typ_vrcholu(&v, k);
    m.insert("d_x".into(), DialogValue::Text(cislo(x.x, 3, false)));
    m.insert("d_y".into(), DialogValue::Text(cislo(x.y, 3, false)));
    m.insert("d_typ".into(), DialogValue::Choice { index: typ, text: crate::lang::tr_tab(TYPY[typ]).into() });
    m.insert("d_r".into(), DialogValue::Text(if typ > 0 { cislo(x.r, 2, false) } else { String::new() }));
    m.insert("d_l1".into(), DialogValue::Text(if typ == 2 { cislo(x.l1, 2, false) } else { String::new() }));
    m.insert("d_l2".into(), DialogValue::Text(if typ == 2 { cislo(x.l2, 2, false) } else { String::new() }));
    let info = if !vnitrni {
        tr("Krajní vrchol – bez oblouku.", "End vertex – no curve.").to_string()
    } else if let (Some(g), true) = (&g, typ > 0) {
        let u = g.vrcholy[k];
        let od = g.hlavni_body.iter().find(|b| b.vrchol == k && matches!(b.druh, trasy::geometrie::DruhBodu::Tk | trasy::geometrie::DruhBodu::Tp));
        let do_ = g.hlavni_body.iter().find(|b| b.vrchol == k && matches!(b.druh, trasy::geometrie::DruhBodu::Kt | trasy::geometrie::DruhBodu::Pt));
        let mut t = trf!(
            "Úhel {} g {} · T1 {} m · T2 {} m · kruhový oblouk {} m · celkem {} m", "Angle {} gon {} · T1 {} m · T2 {} m · circular arc {} m · total {} m",
            cislo(gon(u.odbocka.abs()), 4, true),
            if u.odbocka > 0.0 { tr("vlevo", "left") } else { tr("vpravo", "right") },
            cislo(u.tecna, 2, true),
            cislo(u.tecna2, 2, true),
            cislo(u.delka_oblouku, 2, true),
            cislo(u.delka_celkem, 2, true)
        );
        if typ == 2 {
            let (_, p1, _, a1) = parametry_prechodnice(x.l1, x.r);
            let (_, p2, _, a2) = parametry_prechodnice(x.l2, x.r);
            t.push_str(&trf!(" · A1 {} · A2 {} · p1 {} m · p2 {} m", " · A1 {} · A2 {} · p1 {} m · p2 {} m", cislo(a1, 2, true), cislo(a2, 2, true), cislo(p1, 3, true), cislo(p2, 3, true)));
        }
        if let (Some(a), Some(b)) = (od, do_) {
            t.push_str(&trf!(" · staničení {} – {}", " · chainage {} – {}", staniceni(a.staniceni, f, 2, true), staniceni(b.staniceni, f, 2, true)));
        }
        t
    } else if typ > 0 {
        tr("Oblouk se nevejde – viz Kontrola.", "The curve does not fit – see Check.").to_string()
    } else {
        tr("Bez oblouku (lom).", "No curve (angle point).").to_string()
    };
    m.insert("d_info".into(), DialogValue::Text(info));
    for (id, vid) in [("d_typ", vnitrni), ("d_r", vnitrni && typ > 0), ("d_l1", vnitrni && typ == 2), ("d_l2", vnitrni && typ == 2)] {
        m.insert(format!("{id}:visible"), DialogValue::Visible(vid));
    }
    let n_prech = stav(|s| s.n_typ == 1);
    for id in ["n_zad", "n_hod", "n_pom"] {
        m.insert(format!("{id}:visible"), DialogValue::Visible(n_prech));
    }
    m.insert("kontrola".into(), DialogValue::Text(kontrola(&v, t.def.pocatek).join("\n")));
    // zaškrtávátka (výchozí hodnotu z definice OCS v panelu neukáže)
    let (zv, zm) = stav(|s| (s.zvyraznit, s.zmensit));
    m.insert("zvyraznit".into(), DialogValue::Bool(zv));
    m.insert("zmensit".into(), DialogValue::Bool(zm));
    m.insert("popisy".into(), DialogValue::Bool(h.ano("popis_body")));
    m
}

/// Obnoví panel (když je otevřený) – z příkazu, který má výkres.
pub fn obnov(host: &mut dyn HostApi, doc: &CadDocument) {
    if otevren() {
        host.update_panel(ID, hodnoty(doc));
    }
}

/// Změna pole v panelu (upozornění bez přístupu k výkresu).
pub fn akce(control_id: &str, _action: &str, value: Option<DialogValue>) {
    let ted = Instant::now();
    let cislo_z = |v: &DialogValue| match v {
        DialogValue::Number(x) => Some(*x),
        DialogValue::Text(t) => cti(t),
        _ => None,
    };
    match (control_id, value) {
        ("vrcholy", Some(DialogValue::Table(t))) => stav(|s| s.zmena = Some((Zmena::Tabulka(t), ted))),
        ("pred", _) => stav(|s| {
            s.vybrany = s.vybrany.saturating_sub(1);
            s.obnovit = true;
        }),
        ("dalsi", _) => stav(|s| {
            s.vybrany += 1;
            s.obnovit = true;
        }),
        ("vrchol", Some(v)) => {
            let i = match v {
                DialogValue::Choice { index, .. } => Some(index),
                ref x => cislo_z(x).filter(|x| *x >= 1.0).map(|x| x.round() as usize - 1),
            };
            if let Some(i) = i {
                stav(|s| {
                    s.vybrany = i;
                    s.obnovit = true;
                });
            }
        }
        ("ukazat", _) => stav(|s| {
            s.ukazat = true;
            s.obnovit = true;
        }),
        ("zvyraznit", Some(DialogValue::Bool(b))) => stav(|s| {
            s.zvyraznit = b;
            s.obnovit = true;
        }),
        ("popisy", Some(DialogValue::Bool(b))) => stav(|s| {
            s.popisy = Some(b);
            s.obnovit = true;
        }),
        ("zmensit", Some(DialogValue::Bool(b))) => stav(|s| s.zmensit = b),
        // volba v nabídce platí hned
        ("d_typ", Some(DialogValue::Choice { index, .. })) => stav(|s| s.zmena = Some((Zmena::Typ(index.min(2)), ted - PAUZA))),
        (id @ ("d_x" | "d_y" | "d_r" | "d_l1" | "d_l2"), Some(DialogValue::Text(t))) => {
            stav(|s| s.zmena = Some((Zmena::Pole(id.to_string(), t), ted)))
        }
        // nastavení – jen zapamatovat
        ("n_r", Some(v)) => {
            if let Some(x) = cislo_z(&v).filter(|x| *x >= 0.0) {
                stav(|s| s.n_r = x);
            }
        }
        ("n_typ", Some(DialogValue::Choice { index, .. })) => stav(|s| {
            s.n_typ = index.min(1);
            s.obnovit = true;
        }),
        ("n_zad", Some(DialogValue::Choice { index, .. })) => stav(|s| s.n_zad = index.min(1)),
        ("n_hod", Some(v)) => {
            if let Some(x) = cislo_z(&v).filter(|x| *x > 0.0) {
                stav(|s| s.n_hod = x);
            }
        }
        ("n_pom", Some(DialogValue::Text(t))) => {
            if let Some(p) = cti_pomer(&t) {
                stav(|s| s.n_pom = p);
            }
        }
        ("k_rmin", Some(v)) => {
            if let Some(x) = cislo_z(&v).filter(|x| *x >= 0.0) {
                stav(|s| {
                    s.k_rmin = x;
                    s.obnovit = true;
                });
            }
        }
        ("k_mez", Some(v)) => {
            if let Some(x) = cislo_z(&v).filter(|x| *x >= 0.0) {
                stav(|s| {
                    s.k_mez = x;
                    s.obnovit = true;
                });
            }
        }
        ("zpet", _) => VRATIT.store(true, Ordering::SeqCst),
        _ => {}
    }
}

/// Je co zapsat nebo obnovit? (pauza v psaní uplynula)
pub fn je_cas() -> bool {
    otevren() && stav(|s| s.obnovit || s.zmena.as_ref().is_some_and(|(_, t)| t.elapsed() >= PAUZA))
}

/// Typ podle textu buňky: b / – / prázdné = bez oblouku, k (c) = kružnice,
/// p (s, t) = s přechodnicemi.
fn typ_z_textu(t: &str) -> Option<usize> {
    let Some(c) = t.trim().chars().next() else { return Some(0) };
    match c.to_lowercase().next()? {
        'b' | '–' | '-' | '0' | 'n' => Some(0),
        'k' | 'c' => Some(1),
        'p' | 's' | 't' => Some(2),
        _ => None,
    }
}

/// Vrchol `k` s typem oblouku `typ` (0 bez, 1 kružnice, 2 s přechodnicemi).
/// Chybějící poloměr a přechodnice se doplní z nastavení panelu.
fn s_typem(v: &[Vrchol], k: usize, typ: usize) -> Vrchol {
    let mut x = v[k];
    match typ {
        0 => {
            x.r = 0.0;
            x.l1 = 0.0;
            x.l2 = 0.0;
        }
        1 => {
            if x.r <= 0.0 {
                x.r = polomer();
            }
            x.l1 = 0.0;
            x.l2 = 0.0;
        }
        _ => {
            if x.r <= 0.0 {
                x.r = polomer();
            }
            if x.l1 <= 0.0 && x.l2 <= 0.0 {
                (x.l1, x.l2) = prechodnice_z_nastaveni(x.r);
            }
        }
    }
    x
}

/// Nové vrcholy podle změny z panelu (`None` = nic se nemění).
fn nove_vrcholy(v0: &[Vrchol], k: usize, z: &Zmena) -> Result<Option<Vec<Vrchol>>, String> {
    let mut v = v0.to_vec();
    let vnitrni = k > 0 && k + 1 < v.len();
    match z {
        Zmena::Tabulka(rows) => {
            if rows.len() != v.len() {
                return Ok(None);
            }
            let puvodni = radky(v0, k);
            for (i, r) in rows.iter().enumerate() {
                let zmeneno = |c: usize| r.get(c).map(|x| x.trim()) != puvodni[i].get(c).map(|x| x.trim());
                let hodnota = |c: usize| r.get(c).and_then(|x| cti(x));
                // nepřepsaná buňka drží přesnou hodnotu (tabulka ukazuje jen 3 desetinná místa)
                if zmeneno(1) {
                    v[i].x = hodnota(1).ok_or_else(|| trf!("řádek {} nemá platné X", "row {} has no valid X", i + 1))?;
                }
                if zmeneno(2) {
                    v[i].y = hodnota(2).ok_or_else(|| trf!("řádek {} nemá platné Y", "row {} has no valid Y", i + 1))?;
                }
                if i == 0 || i + 1 >= v.len() {
                    continue;
                }
                if zmeneno(3) {
                    let t = r.get(3).and_then(|c| typ_z_textu(c)).ok_or_else(|| trf!("řádek {}: typ b, k nebo p", "row {}: type b, k or p", i + 1))?;
                    v[i] = s_typem(&v, i, t);
                }
                if zmeneno(4) {
                    let r = hodnota(4).unwrap_or(0.0).max(0.0);
                    v[i].r = r;
                    if r <= 0.0 {
                        v[i].l1 = 0.0;
                        v[i].l2 = 0.0;
                    }
                }
                if zmeneno(5) {
                    v[i].l1 = hodnota(5).unwrap_or(0.0).max(0.0);
                }
                if zmeneno(6) {
                    v[i].l2 = hodnota(6).unwrap_or(0.0).max(0.0);
                }
            }
        }
        Zmena::Pole(id, t) => {
            let Some(x) = cti(t) else { return Ok(None) };
            match id.as_str() {
                "d_x" => v[k].x = x,
                "d_y" => v[k].y = x,
                "d_r" if vnitrni => {
                    v[k].r = x.max(0.0);
                    if x <= 0.0 {
                        v[k].l1 = 0.0;
                        v[k].l2 = 0.0;
                    }
                }
                "d_l1" if vnitrni => v[k].l1 = x.max(0.0),
                "d_l2" if vnitrni => v[k].l2 = x.max(0.0),
                _ => return Ok(None),
            }
        }
        Zmena::Typ(t) => {
            if !vnitrni {
                return Ok(None);
            }
            v[k] = s_typem(&v, k, *t);
        }
    }
    if v == v0 {
        return Ok(None);
    }
    Ok(Some(v))
}

/// Zapíše rozepsanou změnu z panelu do výkresu a panel obnoví (správce po pauze v psaní).
pub fn proved(k: &mut PresKanal) {
    let zmena = stav(|s| {
        s.obnovit = false;
        s.zmena.take()
    });
    let Ok(doc) = k.dokument() else { return };
    let Some(t) = trasa_editoru(&doc) else {
        k.panel_id(ID, hodnoty_trasy(None));
        return;
    };
    let vrcholy = t.vrcholy();
    if let Some((z, _)) = zmena {
        // úprava v tabulce vybere řádek, ve kterém se psalo
        if let Zmena::Tabulka(rows) = &z {
            let puvodni = radky(&vrcholy, vybrany());
            if let Some(r) = rows.iter().zip(&puvodni).position(|(a, b)| a.iter().skip(1).ne(b.iter().skip(1))) {
                stav(|s| s.vybrany = r);
            }
        }
        let i = vybrany().min(vrcholy.len().saturating_sub(1));
        match nove_vrcholy(&vrcholy, i, &z) {
            Ok(Some(v)) => match geometrie(&v, t.def.pocatek) {
                Ok(_) => {
                    let mut def = t.def.clone();
                    def.vrcholy = v;
                    k.zpet(tr("Úprava trasy v editoru", "Alignment edit in editor"));
                    let r = objekt::uloz(k, Some(&doc), Some(&t), def);
                    k.konec_kroku();
                    match r {
                        Ok(_) => k.info(&trf!("Trasy: trasa {} upravena (vrchol {}).", "Alignments: alignment {} changed (vertex {}).", t.def.jmeno, i + 1)),
                        Err(e) => k.chyba(&trf!("Trasy: {}", "Alignments: {}", e)),
                    }
                }
                Err(e) => {
                    let mut text = objekt::chyba_geometrie(e.clone());
                    if let Chyba::Prekryv { a, .. } = e {
                        if let Some(m) = prima_za(&v, a).filter(|m| *m < 0.0) {
                            text.push_str(&trf!(" (překryv {} m)", " (overlap {} m)", cislo(-m, 2, true)));
                        }
                    }
                    k.chyba(&trf!("Trasy: změna se nepřijala – {}", "Alignments: change rejected – {}", text));
                }
            },
            Ok(None) => {}
            Err(e) => k.chyba(&trf!("Trasy: změna se nepřijala – {}", "Alignments: change rejected – {}", e)),
        }
    }
    if let Some(b) = stav(|s| s.popisy.take()) {
        if let Ok(doc) = k.dokument() {
            if let Some(t) = trasa_editoru(&doc) {
                let mut def = t.def.clone();
                def.vyjimky.insert("popis_body".into(), if b { "1" } else { "0" }.into());
                k.zpet(tr("Popisy hlavních bodů", "Key point labels"));
                let _ = objekt::uloz(k, Some(&doc), Some(&t), def);
                k.konec_kroku();
                k.info(&if b { tr("Trasy: popisy hlavních bodů zapnuty.", "Alignments: key point labels on.") } else { tr("Trasy: popisy hlavních bodů vypnuty.", "Alignments: key point labels off.") });
            }
        }
    }
    let doc = k.dokument().unwrap_or(doc);
    let t = trasa_editoru(&doc);
    k.panel_id(ID, hodnoty_trasy(t.as_ref()));
    // Ukázat ve výkrese: přiblížit na vybraný vrchol
    if stav(|s| std::mem::take(&mut s.ukazat)) {
        if let Some(t) = &t {
            let v = t.vrcholy();
            if let Some(x) = v.get(vybrany()) {
                let delka = v.windows(2).map(|w| vzd(w[0].bod(), w[1].bod())).sum::<f64>();
                let r = (delka * 0.08).clamp(10.0, 150.0);
                let _ = k.prikaz(&format!("ZOOM W {},{} {},{}", x.x - r, x.y - r, x.x + r, x.y + r));
            }
        }
    }
    k.uchopy(crate::uchopy::spoj_trasy(crate::tra::uchopy::uchopy(&doc)));
}

#[cfg(test)]
mod testy {
    use super::*;

    fn v() -> Vec<Vrchol> {
        vec![
            Vrchol::new(0.0, 0.0, 0.0),
            Vrchol::s_prechodnicemi(95.0, 46.0, 60.0, 22.0, 22.0),
            Vrchol::new(175.0, 6.0, 45.0),
            Vrchol::new(262.0, 58.0, 0.0),
        ]
    }

    #[test]
    fn tabulka_vrcholu() {
        let r = radky(&v(), 1);
        assert_eq!(r[1][0], "* 2");
        assert_eq!(r[1][3], "p");
        assert_eq!(r[2][3], "k");
        assert_eq!(r[0][3], "");
        assert_eq!(r[2][5], "");
        assert!(r[1][7].contains('/'));
        assert!(r[1][8].ends_with('P'));
        // beze změny nic
        assert!(nove_vrcholy(&v(), 1, &Zmena::Tabulka(r.clone())).unwrap().is_none());
        // přepsané L2
        let mut rows = r.clone();
        rows[1][6] = "30".into();
        let n = nove_vrcholy(&v(), 1, &Zmena::Tabulka(rows)).unwrap().unwrap();
        assert_eq!((n[1].l1, n[1].l2), (22.0, 30.0));
        // typ „b“ = bez oblouku, „p“ u kružnice doplní přechodnice z nastavení
        let mut rows = r.clone();
        rows[1][3] = "b".into();
        rows[2][3] = "p".into();
        let n = nove_vrcholy(&v(), 1, &Zmena::Tabulka(rows)).unwrap().unwrap();
        assert_eq!(n[1].r, 0.0);
        assert!(n[2].ma_prechodnice());
    }

    #[test]
    fn detail_a_typ() {
        let n = nove_vrcholy(&v(), 2, &Zmena::Pole("d_r".into(), "50".into())).unwrap().unwrap();
        assert_eq!(n[2].r, 50.0);
        let n = nove_vrcholy(&v(), 1, &Zmena::Typ(1)).unwrap().unwrap();
        assert!(!n[1].ma_prechodnice() && n[1].r == 60.0);
        assert!(nove_vrcholy(&v(), 0, &Zmena::Typ(2)).unwrap().is_none());
    }

    #[test]
    fn vlozeni_podle_nastaveni() {
        let mut w = v();
        w[2].r = 0.0;
        assert_eq!(dej_oblouk(&mut w, 2, true), Vlozeni::Ano);
        assert!(w[2].ma_prechodnice());
        // obrovský poloměr se zmenší, aby se vešel
        stav(|s| s.n_r = 300.0);
        let mut w = v();
        let r = dej_oblouk(&mut w, 2, false);
        stav(|s| s.n_r = 20.0);
        assert_eq!(r, Vlozeni::Zmenseno);
        assert!(w[2].r < 300.0 && geometrie(&w, 0.0).is_ok());
        // malý úhel vrcholu: přechodnice se zkrátí, poloměr zůstane
        let mut w = vec![Vrchol::new(0.0, 0.0, 0.0), Vrchol::new(100.0, 0.0, 30.0), Vrchol::new(200.0, 10.0, 0.0)];
        assert_eq!(dej_oblouk(&mut w, 1, true), Vlozeni::Zmenseno);
        assert!(w[1].r == 30.0 && w[1].ma_prechodnice() && w[1].l1 < 20.0);
    }

    #[test]
    fn kontrola_trasy() {
        let k = kontrola(&v(), 0.0);
        assert!(k.iter().all(|t| !t.starts_with('✕')), "{k:?}");
        let mut w = v();
        w[1].r = 400.0;
        let k = kontrola(&w, 0.0);
        assert!(k[0].starts_with('✕'));
    }
}
