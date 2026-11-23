//! Panel „Editor nivelety“ v doku Open CAD Studia.
//!
//! Hlavička (trasa, pohled, délka, počet vrcholů), tabulka vrcholů se sloupci
//! R | L | K (přepsaný sloupec = zadání, ostatní se dopočtou; hvězdička =
//! zadaná hodnota), detail vybraného vrcholu (staničení, výška, sklon před
//! a za, oblouk, T a y), nástroje a kontrola.
//!
//! Změny polí chodí jako upozornění `PanelAction` při každém stisku klávesy
//! a bez přístupu k výkresu. Panel si je proto jen zapamatuje a správce
//! (pracovní vlákno s kanálem do výkresu) je po krátké pauze v psaní
//! zapíše do výkresu a panel obnoví. Tah úchopem panel obnoví přes
//! `uchopy::spust_obnovu`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ocs_plugin_api::dialog::{DialogControl, DialogValue, PanelDef, TextStyle};
use ocs_plugin_api::host::Document as CadDocument;
use trasy::niveleta::{self, Niveleta, TypOblouku, VrcholNivelety, Zadani};
use crate::lang::tr;
use trasy::stanice::cislo;
use trasy::trf;

use crate::tra::objekt::{self, ObjektTrasy};
use crate::vykres::{PresKanal, Vykres};

pub const ID: &str = "trasy_editor_nivelety";
pub const PRIKAZ: &str = "TRA_EDITOR_NIVELETY";

/// Pauza v psaní, po které se změna z panelu zapíše do výkresu.
const PAUZA: Duration = Duration::from_millis(900);

/// Typy zadání oblouku v nabídce detailu (pořadí = index volby).
const TYPY: [&str; 3] = ["R", "L", "K"];

#[derive(Clone, Debug)]
enum Zmena {
    Tabulka(Vec<Vec<String>>),
    /// Pole detailu vybraného vrcholu (id, text).
    Pole(String, String),
    /// Typ zadání oblouku vybraného vrcholu (index do `TYPY`).
    Typ(usize),
    /// Typ oblouku vybraného vrcholu (parabola, nesouměrný, kružnice).
    Druh(usize),
}

struct Stav {
    /// Id trasy, jejíž niveletu panel ukazuje.
    trasa: Option<String>,
    /// Vybraný vrchol (index).
    vybrany: usize,
    zmena: Option<(Zmena, Instant)>,
    /// Panel obnovit hned (změna výběru vrcholu).
    obnovit: bool,
    /// Pohled, pro který se panel naposledy sám otevřel (klik na niveletu).
    otevren_pro: Option<u64>,
    /// Nastavení oblouku pro Vložit oblouk, Do všech vrcholů a Přidat vrchol.
    n_typ: usize,
    n_zad: usize,
    n_hod: f64,
    /// Podíl L1 z délky nesouměrného oblouku (40 : 60 = 0,4).
    n_podil: f64,
    /// Délka nesouměrného oblouku L [m] (zadává se jen délkou).
    n_l: f64,
    /// Zvýraznit vybraný vrchol v pohledu (úchop na niveletě).
    zvyraznit: bool,
    /// Přiblížit výkres k vybranému vrcholu (tlačítko Ukázat ve výkrese).
    ukazat: bool,
    /// Zapnout / vypnout popisy oblouků v pohledech trasy (čeká na zápis).
    popisy: Option<bool>,
    /// Při změně sklonu / délky úseku posunout zbytek nivelety (jinak držet konec).
    posun: bool,
}

static STAV: Mutex<Stav> = Mutex::new(Stav {
    trasa: None,
    vybrany: 0,
    zmena: None,
    obnovit: false,
    otevren_pro: None,
    n_typ: 0,
    n_zad: 0,
    n_hod: 50.0,
    n_podil: 0.4,
    n_l: 40.0,
    zvyraznit: true,
    ukazat: false,
    popisy: None,
    posun: true,
});
static OTEVREN: AtomicBool = AtomicBool::new(false);

fn stav<R>(f: impl FnOnce(&mut Stav) -> R) -> R {
    f(&mut STAV.lock().unwrap_or_else(|e| e.into_inner()))
}

pub fn otevren() -> bool {
    OTEVREN.load(Ordering::SeqCst)
}

pub fn aktivni() -> Option<String> {
    stav(|s| s.trasa.clone())
}

pub fn vybrany() -> usize {
    stav(|s| s.vybrany)
}

/// Panel bude ukazovat niveletu trasy `id` (jiná trasa = zahodit rozepsané změny).
pub fn nastav_aktivni(id: &str) {
    stav(|s| {
        if s.trasa.as_deref() != Some(id) {
            s.zmena = None;
            s.vybrany = 0;
        }
        s.trasa = Some(id.to_string());
    });
}

pub fn nastav_vybrany(i: usize) {
    stav(|s| s.vybrany = i);
}

/// Vybere vrchol a požádá o obnovu panelu (klik na úchop vrcholu v pohledu).
pub fn vyber_a_obnov(i: usize) {
    stav(|s| {
        s.vybrany = i;
        s.obnovit = true;
    });
}

/// Nastavení oblouku z panelu: typ (0 parabola, 1 nesouměrný, 2 kružnice),
/// zadání (R / L / K), hodnota a podíl L1.
pub fn nastaveni_oblouku() -> (usize, &'static str, f64, f64) {
    stav(|s| if s.n_typ == 1 { (1, "L", s.n_l, s.n_podil) } else { (s.n_typ, TYPY[s.n_zad.min(2)], s.n_hod, s.n_podil) })
}

/// Vrchol, který se má zvýraznit v pohledu: (id trasy, index) – jen když je
/// panel otevřený a zvýraznění zapnuté.
pub fn zvyrazneny_vrchol() -> Option<(String, usize)> {
    if !otevren() {
        return None;
    }
    stav(|s| if s.zvyraznit { s.trasa.clone().map(|t| (t, s.vybrany)) } else { None })
}

/// Posunout zbytek nivelety při změně sklonu / délky úseku?
pub fn posun_zbytku() -> bool {
    stav(|s| s.posun)
}

/// Vloží oblouk podle nastavení panelu do vrcholu `k`.
pub fn dej_oblouk(v: &mut [VrcholNivelety], k: usize) -> niveleta::VlozeniOblouku {
    let (typ, zad, h, podil) = nastaveni_oblouku();
    niveleta::dej_oblouk(v, k, typ, zad, h, podil)
}

/// Podíl L1 z textu „40 : 60“ (nebo „0,4“).
fn cti_podil(t: &str) -> Option<f64> {
    match t.split_once(':') {
        Some((a, b)) => match (cti(a), cti(b)) {
            (Some(a), Some(b)) if a > 0.0 && b > 0.0 => Some(a / (a + b)),
            _ => None,
        },
        None => cti(t).filter(|x| *x > 0.0 && *x < 1.0),
    }
}

/// Má se panel sám otevřít pro vybraný pohled? (jen poprvé pro daný pohled)
pub fn otevrit_pro(pohled: Option<u64>) -> bool {
    stav(|s| {
        let nove = pohled.is_some() && s.otevren_pro != pohled;
        s.otevren_pro = pohled;
        nove
    })
}

/// Číslo z buňky nebo pole (desetinná čárka, mezery, hvězdička zadané hodnoty, ‰).
pub fn cti(t: &str) -> Option<f64> {
    let t: String = t.chars().filter(|c| !matches!(c, '*' | ' ' | '\u{a0}' | '‰')).collect();
    t.replace(',', ".").parse().ok().filter(|x: &f64| x.is_finite())
}

fn tlacitko(id: &str, label: &str, prikaz: &str) -> DialogControl {
    DialogControl::Button { id: id.into(), label: label.into(), action: Some(prikaz.into()), align_h: None, width: None }
}

fn radek(c: Vec<DialogControl>) -> DialogControl {
    DialogControl::Row { controls: c, align_h: None, align_v: None }
}

/// Nadpis části panelu s mezerou nad ním (prázdný řádek – panel jiný odstup nemá).
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

fn text(id: &str, label: &str) -> DialogControl {
    DialogControl::Text { id: id.into(), label: label.into(), default: String::new() }
}

/// Řada zarovnaná nahoru (sloupce nástrojů různé výšky).
fn radek_nahore(c: Vec<DialogControl>) -> DialogControl {
    DialogControl::Row { controls: c, align_h: None, align_v: Some(ocs_plugin_api::dialog::VerticalAlign::Top) }
}

fn sloupec(nazev: &str, c: Vec<DialogControl>) -> DialogControl {
    let mut v = vec![DialogControl::StyledText { text: nazev.into(), style: TextStyle::Muted, align_h: None }];
    v.extend(c);
    DialogControl::Column { controls: v, align_h: None, align_v: None }
}

pub fn definice() -> PanelDef {
    OTEVREN.store(true, Ordering::SeqCst);
    let (n_typ, n_zad, n_hod, n_podil, n_l, posun, zvyraznit) = stav(|s| (s.n_typ, s.n_zad, s.n_hod, s.n_podil, s.n_l, s.posun, s.zvyraznit));
    PanelDef::new(ID, tr("Editor nivelety", "Design profile editor"))
        .with_command(PRIKAZ)
        .with_width(540.0)
        .with_control(DialogControl::Label { id: "info".into(), text: String::new(), align_h: None })
        .with_control(nadpis(tr("Vrcholy", "Vertices")))
        .with_control(DialogControl::Table {
            id: "vrcholy".into(),
            label: None,
            headers: vec![
                "#".into(),
                tr("Staničení", "Stationing").into(),
                tr("Výška", "Elevation").into(),
                tr("Typ", "Type").into(),
                "R".into(),
                "L".into(),
                "K".into(),
                tr("Sklon [‰]", "Grade [‰]").into(),
                tr("Délka [m]", "Length [m]").into(),
            ],
            rows: Vec::new(),
            allow_add: false,
            allow_delete: false,
        })
        .with_control(nadpis(tr("Vybraný vrchol", "Selected vertex")))
        .with_control(DialogControl::Choice { id: "vrchol".into(), label: tr("Vrchol", "Vertex").into(), options: vec!["1".into()], default_index: 0 })
        .with_control(radek(vec![
            DialogControl::Button { id: "pred".into(), label: tr("◀ Předchozí", "◀ Previous").into(), action: None, align_h: None, width: None },
            DialogControl::Button { id: "ukazat".into(), label: tr("Ukázat ve výkrese", "Show in drawing").into(), action: None, align_h: None, width: None },
            DialogControl::Button { id: "dalsi".into(), label: tr("Další ▶", "Next ▶").into(), action: None, align_h: None, width: None },
        ]))
        // zaškrtávátko samostatně – v řádku s tlačítkem mu OCS neobnoví stav
        .with_control(DialogControl::Checkbox { id: "zvyraznit".into(), label: tr("Zvýraznit v pohledu", "Highlight in view").into(), default: zvyraznit })
        .with_control(DialogControl::Checkbox { id: "popisy".into(), label: tr("Popisy oblouků", "Curve labels").into(), default: true })
        .with_control(DialogControl::Fields {
            id: "detail".into(),
            title: None,
            controls: vec![
                text("d_s", tr("Staničení [m]", "Chainage [m]")),
                text("d_z", tr("Výška [m n. m.]", "Elevation [m a.s.l.]")),
                text("d_ip", tr("Sklon před [‰]", "Grade behind [‰]")),
                text("d_dp", tr("Délka před [m]", "Length behind [m]")),
                text("d_iz", tr("Sklon za [‰]", "Grade ahead [‰]")),
                text("d_dz", tr("Délka za [m]", "Length ahead [m]")),
                DialogControl::Choice {
                    id: "d_druh".into(),
                    label: tr("Typ oblouku", "Curve type").into(),
                    options: vec![tr("bez oblouku", "no curve").into(), tr("parabola (souměrná)", "parabola (symmetrical)").into(), tr("nesouměrný", "unsymmetrical").into(), tr("kružnice", "circle").into()],
                    default_index: 0,
                },
                DialogControl::Choice {
                    id: "d_typ".into(),
                    label: tr("Oblouk zadán", "Curve set by").into(),
                    options: vec![tr("poloměrem R", "radius R").into(), tr("délkou L", "length L").into(), tr("hodnotou K", "K value").into()],
                    default_index: 0,
                },
                text("d_hod", tr("Hodnota (R / L / K)", "Value (R / L / K)")),
                text("d_l1", tr("L1 před vrcholem [m]", "L1 before vertex [m]")),
                text("d_l2", tr("L2 za vrcholem [m]", "L2 after vertex [m]")),
                DialogControl::Label { id: "d_ty".into(), text: String::new(), align_h: None },
            ],
            collapsed: false,
        })
        .with_control(nadpis(tr("Nástroje", "Tools")))
        .with_control(radek_nahore(vec![
            sloupec(tr("Vrcholy", "Vertices"), vec![
                tlacitko("pridat", tr("+ Přidat vrchol", "+ Add vertex"), "TRA_NIV_PRIDAT"),
                tlacitko("smazat", tr("✕ Smazat vrchol", "✕ Delete vertex"), "TRA_NIVELETA_SMAZAT"),
                tlacitko("zvednout", tr("↑↓ Zvednout / snížit", "↑↓ Raise / lower"), "TRA_NIV_ZVEDNOUT"),
                tlacitko("nateren", tr("↓ Vrchol na terén", "↓ Vertex to terrain"), "TRA_NIV_NATEREN"),
            ]),
            sloupec(tr("Oblouky", "Curves"), vec![
                tlacitko("vlozit", tr("⌒ Vložit oblouk", "⌒ Insert curve"), "TRA_NIV_VLOZIT_OBLOUK"),
                tlacitko("odstranit", tr("✕ Odstranit oblouk", "✕ Remove curve"), "TRA_NIV_ODSTRANIT_OBLOUK"),
                tlacitko("vsechny", tr("⌒⌒ Do všech vrcholů", "⌒⌒ At all vertices"), "TRA_NIV_OBLOUKY_VSE"),
            ]),
            sloupec(tr("Úseky", "Segments"), vec![
                tlacitko("sklon_u", tr("∠ Změnit sklon", "∠ Change grade"), "TRA_NIV_USEK_SKLON"),
                tlacitko("delka_u", tr("↔ Změnit délku", "↔ Change length"), "TRA_NIV_USEK_DELKA"),
                tlacitko("bodem", tr("• Úsek bodem", "• Segment through point"), "TRA_NIV_USEK_BODEM"),
                tlacitko("prodlouzit", tr("→ Prodloužit", "→ Extend"), "TRA_NIV_PRODLOUZIT"),
            ]),
        ]))
        .with_control(DialogControl::Checkbox { id: "drzet".into(), label: tr("Držet konec nivelety", "Keep profile end").into(), default: !posun })
        .with_control(DialogControl::StyledText {
            text: tr("Při změně sklonu nebo délky úseku se posune jen konec úseku. Nezaškrtnuto: posune se celý zbytek nivelety.", "When a segment grade or length changes, only the segment end moves. Unchecked: the whole rest of the profile moves.").into(),
            style: TextStyle::Muted,
            align_h: None,
        })
        .with_control(nadpis(tr("Nastavení oblouku při vložení", "Curve settings for inserting")))
        .with_control(DialogControl::Fields {
            id: "nastaveni".into(),
            title: None,
            controls: vec![
                DialogControl::Choice {
                    id: "n_typ".into(),
                    label: tr("Typ", "Type").into(),
                    options: vec![tr("parabola", "parabola").into(), tr("nesouměrný", "unsymmetrical").into(), tr("kružnice", "circle").into()],
                    default_index: n_typ,
                },
                DialogControl::Choice {
                    id: "n_zad".into(),
                    label: tr("Zadání", "Input").into(),
                    options: vec![tr("poloměr R", "radius R").into(), tr("délka L", "length L").into(), tr("hodnota K", "K value").into()],
                    default_index: n_zad,
                },
                DialogControl::Text { id: "n_hod".into(), label: tr("Hodnota", "Value").into(), default: cislo(n_hod, 2, false) },
                DialogControl::Text { id: "n_l".into(), label: tr("Délka L [m]", "Length L [m]").into(), default: cislo(n_l, 2, false) },
                DialogControl::Text {
                    id: "n_pom".into(),
                    label: tr("L1 : L2", "L1 : L2").into(),
                    default: format!("{} : {}", cislo(n_podil * 100.0, 0, false), cislo(100.0 - n_podil * 100.0, 0, false)),
                },
            ],
            collapsed: false,
        })
}

/// Viditelnost polí nastavení oblouku podle typu (nesouměrný = jen délka a poměr).
fn viditelnost_nastaveni(m: &mut HashMap<String, DialogValue>) {
    let nes = stav(|s| s.n_typ == 1);
    for (id, vid) in [("n_zad", !nes), ("n_hod", !nes), ("n_l", nes), ("n_pom", nes)] {
        m.insert(format!("{id}:visible"), DialogValue::Visible(vid));
    }
}

/// Trasa, jejíž niveletu panel ukazuje: aktivní, jinak první s niveletou.
pub fn trasa_panelu(doc: &CadDocument) -> Option<ObjektTrasy> {
    let vse: Vec<ObjektTrasy> = objekt::trasy(doc).into_iter().filter(|t| !t.def.nivelety.is_empty()).collect();
    let id = aktivni();
    vse.iter().find(|t| Some(&t.def.id) == id.as_ref()).cloned().or_else(|| vse.into_iter().next())
}

/// Hodnoty R, L, K vnitřního vrcholu (bez oblouku nuly; délka bez lomu zůstává).
fn rlk(n: &Niveleta, k: usize) -> [f64; 3] {
    match n.oblouky().into_iter().find(|o| o.vrchol == k) {
        Some(o) => [o.r, o.delka, o.k],
        None => match (n.vrcholy[k].typ, n.vrcholy[k].zadani) {
            (TypOblouku::Nesoumerny { l1, l2 }, _) => [0.0, l1 + l2, 0.0],
            (_, Zadani::L(l)) => [0.0, l, 0.0],
            _ => [0.0, 0.0, 0.0],
        },
    }
}

fn index_zadani(z: Zadani) -> usize {
    match z {
        Zadani::R => 0,
        Zadani::L(_) => 1,
        Zadani::K => 2,
    }
}

/// Má vnitřní vrchol zadaný oblouk? (R > 0, délka L, nebo nesouměrný)
fn zadan_oblouk(n: &Niveleta, k: usize) -> bool {
    if k == 0 || k + 1 >= n.vrcholy.len() {
        return false;
    }
    let v = n.vrcholy[k];
    match (v.typ, v.zadani) {
        (TypOblouku::Nesoumerny { l1, l2 }, _) => l1 > 0.0 && l2 > 0.0,
        (_, Zadani::L(l)) => l > 0.0,
        _ => v.r > 0.0,
    }
}

/// Zkratka typu oblouku do tabulky.
fn nazev_typu(t: TypOblouku) -> &'static str {
    match t {
        TypOblouku::Parabola => tr("parabola", "parabola"),
        TypOblouku::Nesoumerny { .. } => tr("nesoum.", "unsym."),
        TypOblouku::Kruh => tr("kružnice", "circle"),
    }
}

/// Typ oblouku podle textu buňky (první písmeno p / n / k, anglicky p / u / c;
/// prázdné, „–“ nebo „b“ = bez oblouku → 3).
fn typ_z_textu(t: &str) -> Option<usize> {
    let Some(c) = t.trim().chars().next() else { return Some(3) };
    match c.to_lowercase().next()? {
        'p' => Some(0),
        'n' | 'u' => Some(1),
        'k' | 'c' => Some(2),
        '–' | '-' | 'b' | '0' => Some(3),
        _ => None,
    }
}

/// Vrchol `k` s typem oblouku `t` (0 parabola, 1 nesouměrný, 2 kružnice,
/// 3 bez oblouku). Vrchol bez oblouku dostane oblouk podle nastavení panelu
/// (s tímto typem; když se nevejde, zůstane bez oblouku).
fn zmen_typ(n: &Niveleta, k: usize, t: usize) -> VrcholNivelety {
    let v = n.vrcholy[k];
    if t >= 3 {
        return VrcholNivelety::new(v.s, v.z, 0.0);
    }
    if zadan_oblouk(n, k) {
        return s_typem(n, k, t);
    }
    let (_, zad, h, podil) = nastaveni_oblouku();
    let mut vr = n.vrcholy.clone();
    let _ = niveleta::dej_oblouk(&mut vr, k, t, zad, h, podil);
    vr[k]
}

/// Buňky R, L, K vrcholu; zadaná hodnota má hvězdičku. Krajní vrchol prázdné,
/// u nesouměrného oblouku jen L (= L1 + L2), R a K pomlčka.
pub fn bunky_rlk(n: &Niveleta, k: usize) -> [String; 3] {
    if k == 0 || k + 1 >= n.vrcholy.len() {
        return [String::new(), String::new(), String::new()];
    }
    if !zadan_oblouk(n, k) {
        return [String::new(), String::new(), String::new()];
    }
    let h = rlk(n, k);
    if let TypOblouku::Nesoumerny { .. } = n.vrcholy[k].typ {
        return ["–".into(), cislo(h[1], 2, false), "–".into()];
    }
    let zad = index_zadani(n.vrcholy[k].zadani);
    let des = [1, 2, 2];
    std::array::from_fn(|i| {
        let t = cislo(h[i], des[i], false);
        if i == zad && h[i] > 0.0 { format!("{t} *") } else { t }
    })
}

/// Vrchol podle buněk R, L, K: platí sloupec, který se proti původním buňkám
/// změnil; beze změny zůstane původní zadání (i typ oblouku). Bez původního
/// vrcholu platí první vyplněný sloupec. U nesouměrného oblouku jde měnit jen L.
pub fn vrchol_z_bunek(s: f64, z: f64, nove: &[String], puvodni: Option<(&VrcholNivelety, &[String])>) -> VrcholNivelety {
    let bunka = |b: &[String], i: usize| b.get(i).and_then(|c| cti(c));
    match puvodni {
        Some((v, puv)) => {
            let zmena = (0..3).find(|&i| match (bunka(nove, i), bunka(puv, i)) {
                (Some(a), Some(b)) => (a - b).abs() > 1e-9,
                (a, b) => a.is_some() != b.is_some(),
            });
            let v = VrcholNivelety { s, z, ..*v };
            match zmena {
                Some(i) if matches!(v.typ, TypOblouku::Nesoumerny { .. }) && i != 1 => v,
                Some(i) => v.se_zadanim(TYPY[i], bunka(nove, i).unwrap_or(0.0)),
                None => v,
            }
        }
        None => match (0..3).find(|&i| bunka(nove, i).is_some_and(|h| h > 0.0)) {
            Some(i) => VrcholNivelety::zadany(s, z, TYPY[i], bunka(nove, i).unwrap_or(0.0)),
            None => VrcholNivelety::new(s, z, 0.0),
        },
    }
}

/// Vrchol s jiným typem oblouku (index: parabola, nesouměrný, kružnice).
/// Nesouměrný dostane L1 : L2 = 40 : 60 z dosavadní délky (bez oblouku 40 m);
/// zpět na parabolu / kružnici se drží délka L.
fn s_typem(n: &Niveleta, k: usize, typ: usize) -> VrcholNivelety {
    let v = n.vrcholy[k];
    let l = rlk(n, k)[1];
    match (typ, v.typ) {
        (1, TypOblouku::Nesoumerny { .. }) | (0, TypOblouku::Parabola) | (2, TypOblouku::Kruh) => v,
        (1, _) => {
            let l = if l > 0.0 { l } else { 40.0 };
            VrcholNivelety { typ: TypOblouku::Nesoumerny { l1: (l * 0.4 * 100.0).round() / 100.0, l2: (l * 0.6 * 100.0).round() / 100.0 }, ..v }
        }
        (t, TypOblouku::Nesoumerny { .. }) => {
            let typ = if t == 2 { TypOblouku::Kruh } else { TypOblouku::Parabola };
            VrcholNivelety { typ, ..VrcholNivelety::zadany(v.s, v.z, "L", l) }
        }
        (t, _) => VrcholNivelety { typ: if t == 2 { TypOblouku::Kruh } else { TypOblouku::Parabola }, ..v },
    }
}

fn promile(i: Option<f64>) -> String {
    i.map_or(String::new(), |i| cislo(i * 1000.0, 2, false))
}

/// Řádky tabulky panelu: #, staničení, výška, typ, R, L, K, sklon za, délka úseku za.
pub fn radky(n: &Niveleta) -> Vec<Vec<String>> {
    let posledni = n.vrcholy.len().saturating_sub(1);
    (0..n.vrcholy.len())
        .map(|k| {
            let v = n.vrcholy[k];
            let [r, l, kk] = bunky_rlk(n, k);
            let typ = if k == 0 || k == posledni {
                String::new()
            } else if zadan_oblouk(n, k) {
                nazev_typu(v.typ).to_string()
            } else {
                "–".to_string()
            };
            let sz = niveleta::sklon_za(&n.vrcholy, k).map_or(String::new(), |i| cislo(i * 1000.0, 2, false));
            let dl = n.vrcholy.get(k + 1).map_or(String::new(), |x| cislo(x.s - v.s, 2, false));
            vec![(k + 1).to_string(), cislo(v.s, 3, false), cislo(v.z, 3, false), typ, r, l, kk, sz, dl]
        })
        .collect()
}

/// Hodnoty panelu pro trasu (bez stahování výkresu).
pub fn hodnoty_trasy(doc: &CadDocument, t: Option<&ObjektTrasy>) -> HashMap<String, DialogValue> {
    let mut m = HashMap::new();
    let Some((t, n)) = t.and_then(|t| t.def.nivelety.first().map(|n| (t, n))) else {
        m.insert("info".into(), DialogValue::Text(tr("Žádná niveleta – klikněte na pohled profilu s niveletou, nebo ji nakreslete tlačítkem Niveleta.", "No design profile – click a profile view with a design profile, or draw one with the Design profile button.").into()));
        m.insert("vrcholy".into(), DialogValue::Table(Vec::new()));
        return m;
    };
    nastav_aktivni(&t.def.id);
    let k = vybrany().min(n.vrcholy.len().saturating_sub(1));
    nastav_vybrany(k);
    let pohled = crate::tra::pohled::pohledy(doc).into_iter().find(|p| p.def.trasa == t.def.id);
    let delka = n.vrcholy.last().map_or(0.0, |v| v.s) - n.vrcholy.first().map_or(0.0, |v| v.s);
    m.insert(
        "info".into(),
        DialogValue::Text(trf!(
            "Trasa {} · pohled {} · délka {} m · vrcholů {}", "Alignment {} · view {} · length {} m · vertices {}",
            t.def.jmeno,
            pohled.as_ref().map_or("–".to_string(), |p| p.def.id.clone()),
            cislo(delka, 2, true),
            n.vrcholy.len()
        )),
    );
    m.insert("vrcholy".into(), DialogValue::Table(radky(n)));
    let seznam: Vec<String> = n
        .vrcholy
        .iter()
        .enumerate()
        .map(|(i, x)| {
            let typ = if i == 0 || i + 1 == n.vrcholy.len() {
                tr("konec", "end").to_string()
            } else if zadan_oblouk(n, i) {
                nazev_typu(x.typ).to_string()
            } else {
                tr("bez oblouku", "no curve").to_string()
            };
            format!("{} – {} m ({})", i + 1, cislo(x.s, 3, true), typ)
        })
        .collect();
    m.insert("vrchol".into(), DialogValue::ChoiceOptions { options: seznam, selected_index: k });
    let v = n.vrcholy[k];
    let vnitrni = k > 0 && k + 1 < n.vrcholy.len();
    let nes = match v.typ {
        TypOblouku::Nesoumerny { l1, l2 } => Some((l1, l2)),
        _ => None,
    };
    m.insert("d_s".into(), DialogValue::Text(cislo(v.s, 3, false)));
    m.insert("d_z".into(), DialogValue::Text(cislo(v.z, 3, false)));
    m.insert("d_ip".into(), DialogValue::Text(promile(niveleta::sklon_pred(&n.vrcholy, k))));
    m.insert("d_iz".into(), DialogValue::Text(promile(niveleta::sklon_za(&n.vrcholy, k))));
    let delka = |a: usize, b: usize| n.vrcholy.get(b).filter(|_| b > a).map_or(String::new(), |x| cislo(x.s - n.vrcholy[a].s, 2, false));
    m.insert("d_dp".into(), DialogValue::Text(if k > 0 { delka(k - 1, k) } else { String::new() }));
    m.insert("d_dz".into(), DialogValue::Text(delka(k, k + 1)));
    let druhy = [tr("bez oblouku", "no curve"), tr("parabola (souměrná)", "parabola (symmetrical)"), tr("nesouměrný", "unsymmetrical"), tr("kružnice", "circle")];
    let ma = zadan_oblouk(n, k);
    let di = if ma { v.typ.index() + 1 } else { 0 };
    m.insert("d_druh".into(), DialogValue::Choice { index: di, text: druhy[di].into() });
    let zi = index_zadani(v.zadani);
    let moznosti = [tr("poloměrem R", "radius R"), tr("délkou L", "length L"), tr("hodnotou K", "K value")];
    m.insert("d_typ".into(), DialogValue::Choice { index: zi, text: moznosti[zi].into() });
    let h = if ma && nes.is_none() { cislo(rlk(n, k)[zi], 2, false) } else { String::new() };
    m.insert("d_hod".into(), DialogValue::Text(h));
    m.insert("d_l1".into(), DialogValue::Text(nes.map_or(String::new(), |x| cislo(x.0, 2, false))));
    m.insert("d_l2".into(), DialogValue::Text(nes.map_or(String::new(), |x| cislo(x.1, 2, false))));
    let ty = if !vnitrni {
        tr("Krajní vrchol – bez oblouku.", "End vertex – no curve.").to_string()
    } else if let Some(o) = n.oblouky().into_iter().find(|o| o.vrchol == k) {
        let druh = if o.vypukly { tr("Vypuklý", "Crest") } else { tr("Údolnicový", "Sag") };
        match o.typ {
            TypOblouku::Nesoumerny { .. } => trf!(
                "{} nesouměrný oblouk: L1 = {} m · L2 = {} m · L = {} m · y = {} m (R a K se u nesouměrného neuvádějí)", "{} unsymmetrical curve: L1 = {} m · L2 = {} m · L = {} m · y = {} m (R and K are not given for unsymmetrical curves)",
                druh,
                cislo(o.t1, 2, true),
                cislo(o.t2, 2, true),
                cislo(o.delka, 2, true),
                cislo(o.vzepeti, 3, true)
            ),
            _ => trf!(
                "{} oblouk: R = {} m · L = {} m · K = {} · T = {} m · y = {} m", "{} curve: R = {} m · L = {} m · K = {} · T = {} m · y = {} m",
                druh,
                cislo(o.r, 1, true),
                cislo(o.delka, 2, true),
                cislo(o.k, 2, true),
                cislo(o.tecna, 2, true),
                cislo(o.vzepeti, 3, true)
            ),
        }
    } else {
        tr("Bez oblouku.", "No curve.").to_string()
    };
    m.insert("d_ty".into(), DialogValue::Text(ty));
    // pole, která pro vybraný vrchol nejdou vyplnit, se skryjí
    let posledni = n.vrcholy.len() - 1;
    for (id, vid) in [
        ("d_ip", k > 0),
        ("d_dp", k > 0),
        ("d_iz", k < posledni),
        ("d_dz", k < posledni),
        ("d_druh", vnitrni),
        ("d_typ", vnitrni && ma && nes.is_none()),
        ("d_hod", vnitrni && ma && nes.is_none()),
        ("d_l1", vnitrni && nes.is_some()),
        ("d_l2", vnitrni && nes.is_some()),
    ] {
        m.insert(format!("{id}:visible"), DialogValue::Visible(vid));
    }
    viditelnost_nastaveni(&mut m);
    // zaškrtávátka (výchozí hodnotu z definice OCS v panelu neukáže)
    let (zv, posun) = stav(|s| (s.zvyraznit, s.posun));
    m.insert("zvyraznit".into(), DialogValue::Bool(zv));
    if let Some(p) = &pohled {
        let styly = crate::styly::styly_profilu(doc);
        let nast = trasy::pohled::nastaveni_s(&p.def, &styly);
        m.insert("popisy".into(), DialogValue::Bool(nast.ano("popis_oblouku")));
    }
    m.insert("drzet".into(), DialogValue::Bool(!posun));
    m
}

pub fn hodnoty(doc: &CadDocument) -> HashMap<String, DialogValue> {
    hodnoty_trasy(doc, trasa_panelu(doc).as_ref())
}

/// Změna pole v panelu (upozornění bez přístupu k výkresu).
pub fn akce(control_id: &str, _action: &str, value: Option<DialogValue>) {
    let ted = Instant::now();
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
            let x = match v {
                DialogValue::Choice { index, .. } => Some(index as f64 + 1.0),
                DialogValue::Number(x) => Some(x),
                DialogValue::Text(t) => cti(&t),
                _ => None,
            };
            if let Some(x) = x.filter(|x| *x >= 1.0) {
                stav(|s| {
                    s.vybrany = x.round() as usize - 1;
                    s.obnovit = true;
                });
            }
        }
        // volba v nabídce platí hned (bez čekání na dopsání)
        ("d_typ", Some(DialogValue::Choice { index, .. })) => stav(|s| s.zmena = Some((Zmena::Typ(index), ted - PAUZA))),
        ("d_druh", Some(DialogValue::Choice { index, .. })) => stav(|s| s.zmena = Some((Zmena::Druh(index), ted - PAUZA))),
        (id @ ("d_s" | "d_z" | "d_ip" | "d_iz" | "d_dp" | "d_dz" | "d_hod" | "d_l1" | "d_l2"), Some(DialogValue::Text(t))) => {
            stav(|s| s.zmena = Some((Zmena::Pole(id.to_string(), t), ted)))
        }
        // nastavení panelu – jen zapamatovat (výkres se nemění)
        ("n_typ", Some(DialogValue::Choice { index, .. })) => stav(|s| {
            s.n_typ = index.min(2);
            s.obnovit = true;
        }),
        ("n_zad", Some(DialogValue::Choice { index, .. })) => stav(|s| s.n_zad = index.min(2)),
        ("drzet", Some(DialogValue::Bool(b))) => stav(|s| s.posun = !b),
        ("popisy", Some(DialogValue::Bool(b))) => stav(|s| {
            s.popisy = Some(b);
            s.obnovit = true;
        }),
        ("ukazat", _) => stav(|s| {
            s.ukazat = true;
            s.obnovit = true;
        }),
        ("zvyraznit", Some(DialogValue::Bool(b))) => stav(|s| {
            s.zvyraznit = b;
            s.obnovit = true;
        }),
        ("n_hod", Some(DialogValue::Text(t))) => {
            if let Some(x) = cti(&t).filter(|x| *x > 0.0) {
                stav(|s| s.n_hod = x);
            }
        }
        ("n_l", Some(DialogValue::Text(t))) => {
            if let Some(x) = cti(&t).filter(|x| *x > 0.0) {
                stav(|s| s.n_l = x);
            }
        }
        ("n_pom", Some(DialogValue::Text(t))) => {
            if let Some(x) = cti_podil(&t) {
                stav(|s| s.n_podil = x);
            }
        }

        _ => {}
    }
}

/// Je co zapsat nebo obnovit? (pauza v psaní uplynula)
pub fn je_cas() -> bool {
    stav(|s| s.obnovit || s.zmena.as_ref().is_some_and(|(_, t)| t.elapsed() >= PAUZA))
}

/// Nové vrcholy nivelety podle změny z panelu (`None` = nic se nemění).
fn nove_vrcholy(n: &Niveleta, k: usize, z: &Zmena) -> Result<Option<Vec<VrcholNivelety>>, String> {
    let mut v = n.vrcholy.clone();
    let vnitrni = k > 0 && k + 1 < v.len();
    match z {
        Zmena::Tabulka(rows) => {
            if rows.len() != v.len() {
                return Ok(None);
            }
            let puvodni = radky(n);
            let mut sklony_delky: Vec<(usize, Option<f64>, Option<f64>)> = Vec::new();
            for (i, r) in rows.iter().enumerate() {
                let (Some(s), Some(zz)) = (r.get(1).and_then(|c| cti(c)), r.get(2).and_then(|c| cti(c))) else {
                    return Err(trf!("řádek {} nemá platné staničení a výšku", "row {} has no valid station and elevation", i + 1));
                };
                // nepřepsaná buňka drží přesnou hodnotu (tabulka ukazuje jen 3 desetinná místa)
                let s = if r[1] == puvodni[i][1] { n.vrcholy[i].s } else { s };
                let zz = if r[2] == puvodni[i][2] { n.vrcholy[i].z } else { zz };
                let nove = r.get(4..7).unwrap_or(&[]);
                v[i] = vrchol_z_bunek(s, zz, nove, Some((&n.vrcholy[i], &puvodni[i][4..7])));
                // přepsaný sklon a délka úseku za vrcholem (po ostatních buňkách)
                if i + 1 < rows.len() {
                    let zmenena = |c: usize| match (r.get(c).and_then(|x| cti(x)), puvodni[i].get(c).and_then(|x| cti(x))) {
                        (Some(a), Some(b)) => ((a - b).abs() > 1e-9).then_some(a),
                        _ => None,
                    };
                    if let Some(x) = zmenena(7) {
                        sklony_delky.push((i, Some(x / 1000.0), None));
                    }
                    if let Some(x) = zmenena(8) {
                        sklony_delky.push((i, None, Some(x)));
                    }
                }
                // přepsaný typ oblouku (jen u vnitřních vrcholů)
                if i > 0 && i + 1 < v.len() && r.get(3) != puvodni[i].get(3) {
                    if let Some(t) = r.get(3).and_then(|c| typ_z_textu(c)) {
                        let mut n2 = n.clone();
                        n2.vrcholy[i] = v[i];
                        v[i] = zmen_typ(&n2, i, t);
                    }
                }
            }
            let posun = posun_zbytku();
            for (i, sklon, delka) in sklony_delky {
                if let Some(x) = sklon {
                    niveleta::nastav_sklon_useku(&mut v, i, x, posun);
                }
                if let Some(x) = delka {
                    if !niveleta::nastav_delku_useku(&mut v, i, x, posun) {
                        return Err(tr("délka úseku se nevejde – zkraťte ji, nebo přepněte na „posunout zbytek nivelety“", "the segment length does not fit – shorten it, or switch to “shift the rest of the profile”").into());
                    }
                }
            }
        }
        Zmena::Pole(id, t) => {
            let Some(x) = cti(t) else { return Ok(None) };
            match id.as_str() {
                "d_s" => v[k].s = x,
                "d_z" => v[k].z = x,
                "d_ip" if k > 0 => niveleta::nastav_sklon_useku(&mut v, k - 1, x / 1000.0, posun_zbytku()),
                "d_iz" => niveleta::nastav_sklon_useku(&mut v, k, x / 1000.0, posun_zbytku()),
                "d_dp" | "d_dz" => {
                    let u = if id == "d_dp" { k.checked_sub(1) } else { Some(k) };
                    let Some(u) = u else { return Ok(None) };
                    if !niveleta::nastav_delku_useku(&mut v, u, x, posun_zbytku()) {
                        return Err(tr("délka úseku se nevejde – zkraťte ji, nebo přepněte na „posunout zbytek nivelety“", "the segment length does not fit – shorten it, or switch to “shift the rest of the profile”").into());
                    }
                }
                "d_hod" if vnitrni => {
                    if let TypOblouku::Nesoumerny { .. } = v[k].typ {
                        return Ok(None);
                    }
                    let typ = TYPY[index_zadani(v[k].zadani)];
                    v[k] = v[k].se_zadanim(typ, x);
                }
                "d_l1" | "d_l2" if vnitrni => {
                    let TypOblouku::Nesoumerny { l1, l2 } = v[k].typ else { return Ok(None) };
                    let x = x.max(0.01);
                    v[k].typ = if id == "d_l1" { TypOblouku::Nesoumerny { l1: x, l2 } } else { TypOblouku::Nesoumerny { l1, l2: x } };
                }
                _ => return Ok(None),
            }
        }
        Zmena::Typ(i) => {
            if !vnitrni || matches!(v[k].typ, TypOblouku::Nesoumerny { .. }) {
                return Ok(None);
            }
            let h = rlk(n, k);
            v[k] = v[k].se_zadanim(TYPY[(*i).min(2)], h[(*i).min(2)]);
        }
        Zmena::Druh(i) => {
            if !vnitrni {
                return Ok(None);
            }
            // nabídka: 0 bez oblouku, 1 parabola, 2 nesouměrný, 3 kružnice
            v[k] = zmen_typ(n, k, if *i == 0 { 3 } else { (*i - 1).min(2) });
        }
    }
    if v == n.vrcholy {
        return Ok(None);
    }
    v.sort_by(|a, b| a.s.total_cmp(&b.s));
    Ok(Some(v))
}

/// Zapíše rozepsanou změnu z panelu do výkresu a panel obnoví (správce po pauze v psaní).
pub fn proved(k: &mut PresKanal) {
    let (zmena, _) = stav(|s| {
        s.obnovit = false;
        (s.zmena.take(), ())
    });
    let Ok(doc) = k.dokument() else { return };
    let Some(t) = trasa_panelu(&doc) else { return };
    if let Some((z, _)) = zmena {
        let n = &t.def.nivelety[0];
        // úprava v tabulce vybere řádek, ve kterém se psalo
        if let Zmena::Tabulka(rows) = &z {
            let puvodni = radky(n);
            if let Some(r) = rows.iter().zip(&puvodni).position(|(a, b)| a != b) {
                nastav_vybrany(r);
            }
        }
        let i = vybrany().min(n.vrcholy.len().saturating_sub(1));
        match nove_vrcholy(n, i, &z) {
            Ok(Some(v)) => {
                let s_vybrany = v.get(i).map(|x| x.s);
                let r = crate::tra::uloz_niveletu_cmd(k, &doc, &t, v.clone(), tr("Úprava nivelety v editoru", "Design profile edit"));
                // z pracovního vlákna drží OCS krok Zpět otevřený, dokud ho nezavřeme –
                // jinak by se další úpravy z panelu slily do jednoho kroku
                k.konec_kroku();
                match r {
                    Ok(p) => {
                        // vybraný vrchol po přeřazení podle staničení
                        if let Some(s) = s_vybrany {
                            if let Some(j) = v.iter().position(|x| x.s == s) {
                                nastav_vybrany(j);
                            }
                        }
                        k.info(&trf!("Trasy: niveleta trasy {} upravena, překresleno pohledů: {}.", "Alignments: design profile of alignment {} updated, views redrawn: {}.", t.def.jmeno, p));
                    }
                    Err(e) => k.chyba(&trf!("Trasy: změna se nepřijala – {}", "Alignments: change rejected – {}", e)),
                }
            }
            Ok(None) => {}
            Err(e) => k.chyba(&trf!("Trasy: změna se nepřijala – {}", "Alignments: change rejected – {}", e)),
        }
    }
    if let Some(b) = stav(|s| s.popisy.take()) {
        let pohledy: Vec<_> = crate::tra::pohled::pohledy(&doc).into_iter().filter(|p| p.def.trasa == t.def.id).collect();
        let mut n = 0;
        // překreslit hned i v režimu „přepočet po dokončení úprav“
        let _hned = crate::prepocet::Vynuceni::zacni();
        for p in pohledy {
            let mut def = p.def.clone();
            def.vyjimky.insert("popis_oblouku".into(), if b { "1" } else { "0" }.into());
            if crate::tra::prospektor::uprav_pohled(k, p.insert, &def).is_ok() {
                n += 1;
            }
            k.konec_kroku();
        }
        k.hotovo();
        if n > 0 {
            k.info(&if b {
                trf!("Trasy: popisy oblouků zapnuty (pohledů: {}).", "Alignments: curve labels on (views: {}).", n)
            } else {
                trf!("Trasy: popisy oblouků vypnuty (pohledů: {}).", "Alignments: curve labels off (views: {}).", n)
            });
        }
    }
    let doc = k.dokument().unwrap_or(doc);
    let t = trasa_panelu(&doc);
    k.panel_id(ID, hodnoty_trasy(&doc, t.as_ref()));
    // Ukázat ve výkrese: přiblížit na vybraný vrchol v prvním pohledu trasy
    if stav(|s| std::mem::take(&mut s.ukazat)) {
        if let Some(t) = &t {
            let k_v = vybrany();
            let bod = t.def.nivelety.first().and_then(|n| n.vrcholy.get(k_v)).and_then(|v| {
                crate::tra::pohled::pohledy(&doc).into_iter().find(|p| p.def.trasa == t.def.id).and_then(|p| {
                    let sirka = p.def.osy.map_or(50.0, |o| (o[1] - o[0]).abs());
                    p.na_vykres(v.s, v.z).map(|q| (q, (sirka * 0.15).max(5.0)))
                })
            });
            if let Some((q, r)) = bod {
                let _ = k.prikaz(&format!("ZOOM W {},{} {},{}", q[0] - r, q[1] - r, q[0] + r, q[1] + r));
            }
        }
    }
    k.uchopy(crate::uchopy::spoj_trasy(crate::tra::uchopy::uchopy(&doc)));
}

/// Výška terénu (profil povrchu pohledu) ve staničení `s`.
pub fn vyska_terenu(useky: &[Vec<[f64; 2]>], s: f64) -> Option<f64> {
    for u in useky {
        for w in u.windows(2) {
            let (a, b) = (w[0], w[1]);
            if s >= a[0].min(b[0]) - 1e-9 && s <= a[0].max(b[0]) + 1e-9 {
                if (b[0] - a[0]).abs() < 1e-12 {
                    return Some(a[1]);
                }
                return Some(a[1] + (b[1] - a[1]) * (s - a[0]) / (b[0] - a[0]));
            }
        }
    }
    None
}

#[cfg(test)]
mod testy {
    use super::*;

    fn n() -> Niveleta {
        Niveleta::nova("N", "", vec![
            VrcholNivelety::new(0.0, 100.0, 0.0),
            VrcholNivelety::new(100.0, 102.0, 2000.0),
            VrcholNivelety::new(200.0, 101.0, 0.0),
        ])
    }

    #[test]
    fn tabulka_r_l_k() {
        let n = n();
        let r = radky(&n);
        assert_eq!(r[1][4..7], ["2000.0 *".to_string(), "60.00".into(), "20.00".into()]);
        assert_eq!(r[1][3], "parabola");
        assert_eq!(r[0][4], "");
        assert_eq!(r[0][7], "20.00");
        // přepsaná délka L platí, hvězdička se při čtení ignoruje
        let mut rows = r.clone();
        rows[1][5] = "90".into();
        let v = nove_vrcholy(&n, 0, &Zmena::Tabulka(rows)).unwrap().unwrap();
        assert_eq!(v[1].zadani, Zadani::L(90.0));
        // beze změny nic – ani zaokrouhlení výšek na 3 desetinná místa
        assert!(nove_vrcholy(&n, 0, &Zmena::Tabulka(r)).unwrap().is_none());
        let mut n2 = n.clone();
        n2.vrcholy[0].z = 100.123_456_7;
        let mut rows = radky(&n2);
        rows[1][5] = "90".into();
        let v = nove_vrcholy(&n2, 0, &Zmena::Tabulka(rows)).unwrap().unwrap();
        assert_eq!(v[0].z, 100.123_456_7);
    }

    #[test]
    fn detail_sklon_a_typ() {
        let n = n();
        let v = nove_vrcholy(&n, 1, &Zmena::Pole("d_iz".into(), "-30".into())).unwrap().unwrap();
        assert!((v[2].z - 99.0).abs() < 1e-9);
        let v = nove_vrcholy(&n, 1, &Zmena::Typ(1)).unwrap().unwrap();
        assert_eq!(v[1].zadani, Zadani::L(60.0));
        let v = nove_vrcholy(&n, 1, &Zmena::Pole("d_hod".into(), "1500".into())).unwrap().unwrap();
        assert_eq!((v[1].zadani, v[1].r), (Zadani::R, 1500.0));
    }

    #[test]
    fn typy_oblouku_v_panelu() {
        let n = n();
        // tabulka: „n“ v typu = nesouměrný 40 : 60 z L = 60
        let mut rows = radky(&n);
        rows[1][3] = "n".into();
        let v = nove_vrcholy(&n, 0, &Zmena::Tabulka(rows)).unwrap().unwrap();
        assert_eq!(v[1].typ, TypOblouku::Nesoumerny { l1: 24.0, l2: 36.0 });
        let n2 = Niveleta::nova("N", "", v);
        let r2 = radky(&n2);
        assert_eq!(r2[1][3..7], ["nesoum.".to_string(), "–".into(), "60.00".into(), "–".into()]);
        // detail: L1 a L2
        let v = nove_vrcholy(&n2, 1, &Zmena::Pole("d_l2".into(), "50".into())).unwrap().unwrap();
        assert_eq!(v[1].typ, TypOblouku::Nesoumerny { l1: 24.0, l2: 50.0 });
        // R u nesouměrného nic nezmění, L ano
        let mut rows = r2.clone();
        rows[1][4] = "900".into();
        assert!(nove_vrcholy(&n2, 0, &Zmena::Tabulka(rows)).unwrap().is_none());
        // zpět na kružnici: drží se délka L
        let v = nove_vrcholy(&n2, 1, &Zmena::Druh(3)).unwrap().unwrap();
        assert_eq!((v[1].typ, v[1].zadani), (TypOblouku::Kruh, Zadani::L(60.0)));
        // bez oblouku: typ „–“ a prázdné R, L, K; volba „bez oblouku“ oblouk odstraní
        let v = nove_vrcholy(&n, 1, &Zmena::Druh(0)).unwrap().unwrap();
        let n3 = Niveleta::nova("N", "", v);
        let r3 = radky(&n3);
        assert_eq!(r3[1][3..7], ["–".to_string(), String::new(), String::new(), String::new()]);
        // typ „p“ do vrcholu bez oblouku vloží oblouk podle nastavení
        let mut rows = r3.clone();
        rows[1][3] = "p".into();
        let v = nove_vrcholy(&n3, 0, &Zmena::Tabulka(rows)).unwrap().unwrap();
        assert!(v[1].r > 0.0);
    }

    #[test]
    fn teren_ve_staniceni() {
        let u = vec![vec![[0.0, 10.0], [10.0, 20.0]], vec![[20.0, 5.0], [30.0, 5.0]]];
        assert_eq!(vyska_terenu(&u, 5.0), Some(15.0));
        assert_eq!(vyska_terenu(&u, 25.0), Some(5.0));
        assert_eq!(vyska_terenu(&u, 15.0), None);
    }
}
