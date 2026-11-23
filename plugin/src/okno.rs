//! Okno Prospektor (egui) – strom výkresu: body, skupiny bodů, povrchy.
//!
//! Open CAD Studio zatím neumí panely pro pluginy, proto si plugin otevírá
//! vlastní okno. Okno jen zobrazuje `spravce::stav()` a posílá `spravce::Akce`;
//! veškerá logika je ve `spravce`, takže okno půjde později nahradit panelem.

use crate::lang::tr;
use teren::trf;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense};
use egui::collapsing_header::CollapsingState;

use ocs_plugin_api::host::Handle;
use teren::body::{Bod, Oddelovac};
use teren::skupiny::{kody_z_textu, SeznamCisel, SkupinaBodu, VSECHNY};

use crate::cogo::BodVykresu;
use crate::nastaveni;
use crate::povrchy::{cislo_cz, DruhPolozky, TypObjektu, HRANY, INTERVALY, TYPY_OBJEKTU};
#[cfg(feature = "ukazka")]
mod foto;
mod editor_profilu;
#[cfg(any(feature = "ukazka", test))]
pub use editor_profilu::ukazkove_podklady;
mod import_nahled;
#[doc(hidden)]
pub use import_nahled::ukazka as ukazka_importu;
mod karta;
mod napoveda;
mod pruvodce;
mod pohled3d;
mod strom;
mod trasy;
mod vyber;

use crate::styly::{self, Barva, StylBodu, StylPovrchu, BARVY, POPISKY, POPISY_VRSTEVNIC, ZNACKY};
use crate::spravce::{self, Akce, PrehledPovrchu, Skok, StavSpravce, Zmena};

static OTEVRENO: AtomicBool = AtomicBool::new(false);

fn pozadavek_napovedy() -> &'static Mutex<Option<String>> {
    static N: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    N.get_or_init(|| Mutex::new(None))
}

static POZADAVEK_PRUVODCE: AtomicBool = AtomicBool::new(false);

fn pozadavek_importu() -> &'static Mutex<Option<String>> {
    static N: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    N.get_or_init(|| Mutex::new(None))
}

/// Otevře Prospektor s náhledem importu souboru bodů.
pub fn otevri_import(cesta: &str) {
    *pozadavek_importu().lock().unwrap_or_else(|e| e.into_inner()) = Some(cesta.to_string());
    let jen_pro_import = !OTEVRENO.load(Ordering::SeqCst);
    otevri();
    if jen_pro_import {
        PROSPEKTOR_JEN_PRO_IMPORT.store(true, Ordering::SeqCst);
    }
}

/// Prospektor se otevřel jen kvůli oknu importu (po importu se zavře).
static PROSPEKTOR_JEN_PRO_IMPORT: AtomicBool = AtomicBool::new(false);

/// Otevře Prospektor s průvodcem novým povrchem.
pub fn otevri_pruvodce() {
    POZADAVEK_PRUVODCE.store(true, Ordering::SeqCst);
    otevri();
}

fn pozadavek_3d() -> &'static Mutex<Option<Vec<String>>> {
    static P: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(None))
}

/// Otevře Prospektor v prohlížeči 3D s danými povrchy (id).
pub fn otevri_3d(povrchy: Vec<String>) {
    *pozadavek_3d().lock().unwrap_or_else(|e| e.into_inner()) = Some(povrchy);
    otevri();
}

/// Otevře Prospektor s nápovědou (`tema` = id tématu, např. „teren“).
pub fn otevri_napovedu(tema: &str) {
    *pozadavek_napovedy().lock().unwrap_or_else(|e| e.into_inner()) = Some(tema.to_string());
    otevri();
}

fn pozadavky() -> &'static Mutex<Option<Sender<()>>> {
    static P: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(None))
}

fn kontext() -> &'static Mutex<Option<egui::Context>> {
    static K: OnceLock<Mutex<Option<egui::Context>>> = OnceLock::new();
    K.get_or_init(|| Mutex::new(None))
}

/// Překreslí okno (volá se po změně stavu z jiného vlákna).
pub fn prekresli() {
    if let Some(c) = kontext().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        c.request_repaint();
    }
}

/// Otevře okno (nebo ho vytáhne dopředu, pokud už je otevřené).
pub fn otevri() {
    // otevření z jiného místa než z importu – Prospektor po importu zůstane
    PROSPEKTOR_JEN_PRO_IMPORT.store(false, Ordering::SeqCst);
    if OTEVRENO.load(Ordering::SeqCst) {
        if let Some(c) = kontext().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            c.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            c.send_viewport_cmd(egui::ViewportCommand::Focus);
            c.request_repaint();
        }
        return;
    }
    let mut p = pozadavky().lock().unwrap_or_else(|e| e.into_inner());
    if p.is_none() {
        // okno má vlastní vlákno po celou dobu běhu pluginu – smyčka událostí
        // se smí vytvořit jen jednou a pak se znovu používá
        let (tx, rx) = mpsc::channel::<()>();
        *p = Some(tx);
        std::thread::Builder::new()
            .name("teren-okno".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    OTEVRENO.store(true, Ordering::SeqCst);
                    spravce::nastav_okno_otevreno(true);
                    spravce::posli(Akce::Obnovit);
                    if let Err(e) = spust() {
                        eprintln!("[teren] okno Prospektor: {e}");
                    }
                    OTEVRENO.store(false, Ordering::SeqCst);
                    spravce::nastav_okno_otevreno(false);
                    *kontext().lock().unwrap_or_else(|e| e.into_inner()) = None;
                }
            })
            .ok();
    }
    if let Some(tx) = p.as_ref() {
        let _ = tx.send(());
    }
}

/// Ukázka okna bez Open CAD Studia (examples/ukazka_okna.rs).
#[cfg(feature = "ukazka")]
pub fn spust_ukazku() {
    if let Err(e) = spust() {
        eprintln!("[teren] ukázka: {e}"); // i18n:ne
    }
}

fn spust() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(tr("Terén – Prospektor", "Terén – Prospector"))
        .with_inner_size([820.0, 600.0])
        .with_min_inner_size([300.0, 240.0]);
    if let Some([x, y, w, h]) = nastaveni::akt().okno {
        viewport = viewport.with_position([x, y]).with_inner_size([w, h]);
    }
    if nastaveni::akt().okno_navrchu {
        viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnTop);
    }
    let mut moznosti = eframe::NativeOptions {
        viewport,
        run_and_return: true,
        ..Default::default()
    };
    moznosti.event_loop_builder = Some(Box::new(|b| {
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            b.with_any_thread(true);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_any_thread(true);
        }
        let _ = b;
    }));
    eframe::run_native(
        tr("Terén – Prospektor", "Terén – Prospector"),
        moznosti,
        Box::new(|cc| {
            *kontext().lock().unwrap_or_else(|e| e.into_inner()) = Some(cc.egui_ctx.clone());
            nastav_vzhled(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let uzel = if nastaveni::akt().rezim_vykres { Uzel::Vykres } else { Uzel::Vyber };
            Ok(Box::new(Okno { uzel, ..Default::default() }))
        }),
    )
}

/// Tmavé téma sladěné s Open CAD Studiem (barvy jako náhled Prospektoru).
fn nastav_vzhled(ctx: &egui::Context) {
    let rgb = Color32::from_rgb;
    let mut v = egui::Visuals::dark();
    v.panel_fill = rgb(30, 33, 37);
    v.window_fill = rgb(36, 40, 45);
    v.extreme_bg_color = rgb(21, 23, 26);
    v.faint_bg_color = rgb(35, 39, 44);
    v.code_bg_color = rgb(42, 46, 52);
    v.hyperlink_color = rgb(88, 166, 255);
    v.window_stroke = egui::Stroke::new(1.0, rgb(58, 63, 70));
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.menu_corner_radius = egui::CornerRadius::same(6);
    v.selection.bg_fill = rgb(31, 61, 92);
    v.selection.stroke = egui::Stroke::new(1.0, rgb(207, 227, 255));
    let r = egui::CornerRadius::same(5);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = rgb(30, 33, 37);
    w.noninteractive.weak_bg_fill = rgb(30, 33, 37);
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0, rgb(51, 56, 63));
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0, rgb(201, 205, 211));
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = rgb(42, 46, 52);
    w.inactive.weak_bg_fill = rgb(42, 46, 52);
    w.inactive.bg_stroke = egui::Stroke::new(1.0, rgb(59, 65, 72));
    w.inactive.fg_stroke = egui::Stroke::new(1.0, rgb(212, 215, 220));
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = rgb(48, 53, 60);
    w.hovered.weak_bg_fill = rgb(48, 53, 60);
    w.hovered.bg_stroke = egui::Stroke::new(1.0, rgb(84, 94, 107));
    w.hovered.fg_stroke = egui::Stroke::new(1.5, rgb(236, 238, 241));
    w.hovered.corner_radius = r;
    w.active.bg_fill = rgb(36, 71, 107);
    w.active.weak_bg_fill = rgb(36, 71, 107);
    w.active.bg_stroke = egui::Stroke::new(1.0, rgb(59, 119, 184));
    w.active.fg_stroke = egui::Stroke::new(1.5, Color32::WHITE);
    w.active.corner_radius = r;
    w.open.bg_fill = rgb(42, 46, 52);
    w.open.weak_bg_fill = rgb(42, 46, 52);
    w.open.bg_stroke = egui::Stroke::new(1.0, rgb(59, 119, 184));
    w.open.corner_radius = r;
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(9.0, 4.0);
        s.spacing.interact_size.y = 22.0;
        s.spacing.menu_margin = egui::Margin::same(6);
    });
}

// ── Stav okna ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cast {
    Hranice,
    Spojnice,
    Objekty,
    Upravy,
    Skupiny,
}

impl Cast {
    fn nazev(self) -> &'static str {
        match self {
            Cast::Hranice => tr("Hranice a díry", "Boundaries and holes"),
            Cast::Spojnice => tr("Spojnice", "Breaklines"),
            Cast::Objekty => tr("Objekty výkresu", "Drawing objects"),
            Cast::Upravy => tr("Úpravy trojúhelníků", "Triangle edits"),
            Cast::Skupiny => tr("Skupiny bodů", "Point groups"),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
enum Uzel {
    #[default]
    Vykres,
    /// Co je vybrané ve výkrese (dřív režim Výběr).
    Vyber,
    Body,
    Skupiny,
    Skupina(String),
    Povrchy,
    Povrch(String),
    Definice(String, Cast),
    Upozorneni(String),
    Kontrola,
    StylyBodu,
    StylBodu(String),
    StylyPovrchu,
    StylPovrchu(String),
    StylyProfilu,
    StylProfilu(String),
    StylyTras,
    StylTrasy(String),
    Nastaveni,
    Trasy,
    Trasa(String),
    Pohled(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Razeni {
    #[default]
    Poradi,
    Cislo,
    Y,
    X,
    Z,
    Popis,
    Zdroj,
    Styl,
}

/// Buňka tabulky bodů upravovaná na místě (číslo, kód, výška).
struct EditBunky {
    h: Handle,
    sloupec: Razeni,
    text: String,
    fokus: bool,
}

fn upravitelny(r: Razeni) -> bool {
    matches!(r, Razeni::Cislo | Razeni::Popis | Razeni::Z)
}

fn text_bunky(b: &Bod, r: Razeni) -> String {
    match r {
        Razeni::Cislo => b.cislo.clone(),
        Razeni::Popis => b.popis.clone(),
        _ => format!("{:.3}", b.z).replace('.', ","),
    }
}

/// Rozpracovaný export skupiny.
struct Export {
    format: String,
    oddelovac: Oddelovac,
    cp1250: bool,
    cesta: String,
}

impl Default for Export {
    fn default() -> Self {
        Self { format: "PYXZD".into(), oddelovac: Oddelovac::Mezera, cp1250: true, cesta: String::new() }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Rezim {
    /// Strom výkresu a podrobnosti.
    #[default]
    Vykres,
    /// Prohlížeč povrchu 3D.
    Pohled3D,
}

#[derive(Default)]
struct Okno {
    rezim: Rezim,
    pohled: pohled3d::Pohled3D,
    /// Úprava bodu v uzlu Vybráno ve výkrese: (bod, číslo, kód/popis, výška).
    vyber_edit: Option<(Handle, String, String, f64)>,
    /// Filtry tabulky bodů.
    filtr_skupina: Option<String>,
    filtr_kody: HashSet<String>,
    /// Filtr výšky (od, do) – rozepsaný text.
    filtr_z: (String, String),
    skryte_sloupce: HashSet<&'static str>,
    vyber_povrch_id: String,
    /// Rozpracovaný vlastní krok vrstevnic (povrch, vedlejší, hlavní).
    vlastni_krok: Option<(String, f64, f64)>,
    /// Na kartě povrchu se přejmenovává (pole místo nadpisu).
    prejmenovat: bool,
    /// Předchozí položky stromu (Alt+← vrátí zpět).
    historie: Vec<Uzel>,
    /// Záložka editoru stylu bodu: 0 Značka, 1 Popis, 2 Použití.
    zalozka_stylu: u8,
    /// Záložka skupiny: 0 Pravidla, 1 Body, 2 Export.
    zalozka_skupiny: u8,
    /// Záložka karty povrchu (Přehled / Export).
    zalozka_povrchu: karta::ZalozkaPovrchu,
    /// Export povrchu: (id povrchu, cesta, DXF místo LandXML).
    export_povrchu: (String, String, bool),
    /// Náhled importu bodů.
    nahled_importu: Option<import_nahled::NahledImportu>,
    /// Průvodce novým povrchem.
    pruvodce: Option<pruvodce::Pruvodce>,
    /// Otevřené téma nápovědy.
    napoveda: Option<String>,
    /// Rozepsané vyloučené kódy v Nastavení.
    nast_kody: Option<String>,
    /// Otevřená kategorie Nastavení.
    kat_nastaveni: u8,
    /// Čeká se na potvrzení „Obnovit výchozí“.
    potvrdit_vychozi: bool,
    uzel: Uzel,
    /// Příští skok provést i z uzlu Vybráno (uživatel klikl Najít v Prospektoru).
    vynutit_skok: bool,
    hledat: String,
    verze: u64,
    skok: u64,
    // tabulka bodů
    vybrane_body: HashSet<Handle>,
    posledni_klik: Option<usize>,
    razeni: Razeni,
    sestupne: bool,
    posunout_na: Option<Handle>,
    // úprava bodu
    /// Buňka tabulky bodů upravovaná na místě.
    edit_bunka: Option<EditBunky>,
    edit_bod: Option<(Handle, String, f64, String)>,
    /// Hromadná úprava vybraných bodů: výška, kód.
    hrom_vyska: String,
    hrom_kod: String,
    edit_puvodni: Option<Bod>,
    posledni_dalsi: u64,
    precislovat_od: u64,
    precislovat_krok: u64,
    // skupina
    edit_skupina: Option<SkupinaBodu>,
    skupina_zmenena: bool,
    /// Kdy se skupina naposledy změnila (ukládá se samo chvilku po úpravě).
    zmena_skupiny: Option<std::time::Instant>,
    /// Kdy se upravovaný styl naposledy změnil.
    zmena_stylu: Option<std::time::Instant>,
    /// Rozepsaný kód k vyloučení z povrchu.
    novy_kod_povrchu: String,
    /// Zobrazené (i prázdné) řádky pravidel: zahrnout kód/číslo/soubor, vyloučit kód/číslo.
    skup_ukaz: [bool; 5],
    /// Rozepsané vstupy: kód zahrnout, kód vyloučit, přidaný bod, odebraný bod.
    skup_novy: [String; 4],
    /// Bod kliknutý v náhledu skupiny.
    skup_bod: Option<Handle>,
    export: Export,
    // povrch
    jmeno: String,
    kody: String,
    vybrane_povrchy: HashSet<String>,
    typ_objektu: Option<TypObjektu>,
    zachovat_hrany: bool,
    pridat_skupinu: String,
    zvyraznena: Option<Handle>,
    nove_jmeno_povrchu: String,
    /// Rozepsané vlastnosti trasy: (id trasy, jméno, počáteční staničení, styl).
    jmeno_trasy: (String, String, f64, String),
    /// Otevřený editor vzhledu profilu (vlastní okno).
    editor_profilu: Option<editor_profilu::EditorProfilu>,
    /// Rozpracovaná úprava stylu trasy.
    uprava_stylu_trasy: Option<::trasy::styl::Styl>,
    /// Povrch, jehož větev se má ve stromu rozbalit.
    rozbalit: Option<String>,
    // styly
    edit_styl_bodu: Option<StylBodu>,
    /// Text kódů v editoru stylu bodů (a pro který styl).
    kody_stylu: (String, String),
    edit_styl_povrchu: Option<StylPovrchu>,
    styl_zmenen: bool,
    /// Poloha a velikost okna k uložení (a kdy se naposledy změnila).
    geometrie: Option<([f32; 4], std::time::Instant)>,
    ulozena_geometrie: Option<[f32; 4]>,
    /// Ukázka: automatické snímky stránek.
    #[cfg(feature = "ukazka")]
    foto: Option<foto::Foto>,
}

/// Sloupce tabulky bodů, které jde skrýt.
const SKRYVATELNE_SLOUPCE: [&str; 5] = ["Styl", "Y", "X", "Kód", "Soubor"];

const ZELENA: Color32 = Color32::from_rgb(90, 200, 120);
const ORANZOVA: Color32 = Color32::from_rgb(240, 165, 60);
const CERVENA: Color32 = Color32::from_rgb(235, 90, 80);
const SEDA: Color32 = Color32::from_rgb(150, 155, 160);
const MODRA: Color32 = Color32::from_rgb(110, 170, 240);

/// Poslední smazání: text hlášky a kdy se stalo.
fn hlaska_smazani() -> &'static Mutex<Option<(String, std::time::Instant)>> {
    static H: OnceLock<Mutex<Option<(String, std::time::Instant)>>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(None))
}

/// Hláška vpravo dole s radou „vrátíte to Ctrl+Z ve výkrese“.
fn hlaska(text: String) {
    *hlaska_smazani().lock().unwrap_or_else(|e| e.into_inner()) = Some((text, std::time::Instant::now()));
}

/// Uložit hned po úpravě: až uživatel nepíše (žádné pole nemá fokus),
/// nebo 1,5 s po poslední změně. `zmena` = změna v tomto snímku.
fn ulozit_ted(ui: &egui::Ui, cas: &mut Option<std::time::Instant>, zmena: bool) -> bool {
    let ted = std::time::Instant::now();
    if zmena {
        *cas = Some(ted);
    }
    let Some(t) = *cas else { return false };
    let pise = ui.memory(|m| m.focused().is_some());
    if !pise || ted.duration_since(t).as_millis() > 1500 {
        *cas = None;
        return true;
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
    false
}

/// Pošle mazací akci a ukáže hlášku s radou, jak ji vrátit.
fn smazat(text: String, akce: Akce) {
    spravce::posli(akce);
    *hlaska_smazani().lock().unwrap_or_else(|e| e.into_inner()) = Some((text, std::time::Instant::now()));
}

fn pocet_bodu_text(n: usize) -> String {
    match n {
        1 => tr("1 bod", "1 point").to_string(),
        2..=4 => trf!("{} body", "{} points", n),
        _ => trf!("{} bodů", "{} points", cislo_z_poctu(n)),
    }
}

fn cislo_z_poctu(n: usize) -> String {
    czi(n)
}

fn pocet_smazanych(n: usize) -> String {
    match n {
        1 => tr("Bod je smazaný.", "The point was deleted.").to_string(),
        2..=4 => trf!("{} body jsou smazané.", "{} points were deleted.", n),
        _ => trf!("{} bodů je smazaných.", "{} points were deleted.", n),
    }
}

/// Hláška po smazání (vpravo dole, 8 s): co se smazalo a jak to vrátit.
fn ukaz_hlasku_smazani(ctx: &egui::Context) {
    let mut g = hlaska_smazani().lock().unwrap_or_else(|e| e.into_inner());
    let Some((text, kdy)) = g.as_ref() else { return };
    if kdy.elapsed() > std::time::Duration::from_secs(8) {
        *g = None;
        return;
    }
    let mut zavrit = false;
    egui::Area::new(egui::Id::new("hlaska_smazani"))
        .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -34.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(text).strong());
                    ui.label(RichText::new(tr("Vrátíte to Ctrl+Z ve výkrese.", "Undo it with Ctrl+Z in the drawing.")).color(SEDA));
                    if ui.small_button("×").clicked() {
                        zavrit = true;
                    }
                });
            });
        });
    if zavrit {
        *g = None;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}

/// Číslo česky: desetinná čárka a mezera v tisících („18 240,50“);
/// v angličtině „18,240.50“.
pub(crate) fn cz(v: f64, des: usize) -> String {
    let (oddelovac, carka) = if crate::lang::anglicky() { (',', '.') } else { ('\u{a0}', ',') };
    let t = format!("{:.*}", des, v.abs());
    let (cela, zbytek) = t.split_once('.').map_or((t.as_str(), ""), |(a, b)| (a, b));
    let mut s = String::new();
    for (i, c) in cela.chars().enumerate() {
        if i > 0 && (cela.len() - i) % 3 == 0 {
            s.push(oddelovac);
        }
        s.push(c);
    }
    let znam = if v < 0.0 && t.chars().any(|c| c.is_ascii_digit() && c != '0') { "−" } else { "" };
    if zbytek.is_empty() { format!("{znam}{s}") } else { format!("{znam}{s}{carka}{zbytek}") }
}

/// Celé číslo česky s mezerou v tisících („1 532“).
pub(crate) fn czi(n: usize) -> String {
    cz(n as f64, 0)
}

/// Úzké (kompaktní) nebo široké okno; výška zůstává.
fn zmen_velikost(ctx: &egui::Context, uzke: bool) {
    let vyska = ctx.input(|i| i.viewport().inner_rect.map_or(600.0, |r| r.height())).max(420.0);
    let sirka = if uzke { 380.0 } else { 900.0 };
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(sirka, vyska)));
}

fn interval_text(i: f64, h: f64) -> String {
    format!("{} / {} m", cislo_cz(i), cislo_cz(h))
}

fn hrana_text(h: Option<f64>) -> String {
    match h {
        Some(h) => format!("{} m", cislo_cz(h)),
        None => tr("bez omezení", "no limit").into(),
    }
}

fn shoda(text: &str, hledat: &str) -> bool {
    hledat.is_empty() || text.to_lowercase().contains(&hledat.to_lowercase())
}

fn bod_odpovida(b: &BodVykresu, hledat: &str) -> bool {
    hledat.is_empty() || shoda(&b.bod.cislo, hledat) || shoda(&b.bod.popis, hledat) || shoda(&b.zdroj, hledat)
}

impl eframe::App for Okno {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let stav = spravce::stav();
        // průběžně se dívat po změnách (stav mění jiné vlákno)
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
        self.zpracuj_skok(ui.ctx(), &stav);
        #[cfg(feature = "ukazka")]
        self.foto_krok(ui.ctx());
        self.hlidej_geometrii(ui.ctx());
        if let Some(t) = pozadavek_napovedy().lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.napoveda = Some(t);
        }
        // Ctrl+F = hledání, Alt+← = zpět na předchozí položku
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            ui.memory_mut(|m| m.request_focus(egui::Id::new("pole_hledani")));
        }
        if ui.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
            if let Some(u) = self.historie.pop() {
                let delka = self.historie.len();
                self.vyber_uzel(u);
                self.historie.truncate(delka);
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::F1)) {
            self.napoveda = if self.napoveda.is_some() { None } else { Some(self.tema_napovedy()) };
        }
        if let Some(p) = pozadavek_3d().lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.pohled.otevri(p);
            self.rezim = Rezim::Pohled3D;
        }
        if POZADAVEK_PRUVODCE.swap(false, Ordering::SeqCst) {
            self.pruvodce = Some(pruvodce::Pruvodce::novy(&stav));
        }
        if let Some(c) = pozadavek_importu().lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.nahled_importu = Some(import_nahled::NahledImportu::novy(&c));
        }
        let ctx = ui.ctx().clone();
        self.okno_importu(&ctx, &stav);
        self.okno_pruvodce(&ctx, &stav);
        self.okno_napovedy(&ctx);
        if let Some(e) = self.editor_profilu.as_mut() {
            e.okno(&ctx, &stav);
            if !e.otevreno {
                self.editor_profilu = None;
            }
        }
        if self.verze != stav.verze {
            self.nacti_buffery(&stav);
            self.verze = stav.verze;
        }

        egui::Panel::top("horni").show(ui, |ui| {
            ui.add_space(4.0);
            self.hlavicka(ui, &stav);
            ui.add_space(3.0);
        });
        egui::Panel::bottom("dolni").show(ui, |ui| {
            ui.add_space(2.0);
            let chyba = stav.zprava.starts_with(tr("Chyba", "Error"));
            let barva = if chyba { CERVENA } else { SEDA };
            ui.add(egui::Label::new(RichText::new(if stav.zprava.is_empty() { " " } else { &stav.zprava }).color(barva)).truncate())
                .on_hover_text(&stav.zprava);
            ui.add_space(2.0);
        });
        ukaz_hlasku_smazani(ui.ctx());
        if self.rezim == Rezim::Pohled3D {
            self.panel_3d(ui, &stav);
            return;
        }
        // úzké okno: strom nahoře, podrobnosti pod ním
        if ui.available_width() < 620.0 {
            egui::Panel::top("strom_nahore").default_size(220.0).min_size(80.0).resizable(true).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("strom_s").auto_shrink([false, false]).show(ui, |ui| self.strom(ui, &stav));
            });
        } else {
            egui::Panel::left("strom").default_size(270.0).min_size(180.0).resizable(true).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("strom_s").auto_shrink([false, false]).show(ui, |ui| self.strom(ui, &stav));
            });
        }
        egui::CentralPanel::default_margins().show(ui, |ui| self.detail(ui, &stav));
    }
}

impl Okno {
    fn vyber_uzel(&mut self, u: Uzel) {
        if let Uzel::Definice(id, _) | Uzel::Upozorneni(id) = &u {
            self.rozbalit = Some(id.clone());
        }
        if self.uzel != u {
            let stary = std::mem::replace(&mut self.uzel, u);
            self.historie.push(stary);
            if self.historie.len() > 40 {
                self.historie.remove(0);
            }
            self.edit_skupina = None;
            self.skupina_zmenena = false;
            self.edit_styl_bodu = None;
            self.edit_styl_povrchu = None;
            self.styl_zmenen = false;
            self.export.cesta.clear();
            self.nove_jmeno_povrchu.clear();
            self.zalozka_povrchu = karta::ZalozkaPovrchu::Prehled;
            self.prejmenovat = false;
            self.zalozka_skupiny = 0;
            self.verze = 0;
        }
    }

    fn zpracuj_skok(&mut self, ctx: &egui::Context, stav: &StavSpravce) {
        let Some((n, skok)) = &stav.skok else { return };
        if *n == self.skok {
            return;
        }
        self.skok = *n;
        let vynutit = std::mem::take(&mut self.vynutit_skok);
        if self.uzel == Uzel::Vyber && !vynutit && matches!(skok, Skok::Povrch(_) | Skok::Polozka(..) | Skok::Body(_) | Skok::Trasa(_) | Skok::Pohled(_)) {
            // sledování výběru: uzel Vybráno ve výkrese ukazuje výběr sám
            return;
        }
        let otevri = |id: egui::Id| {
            let mut s = CollapsingState::load_with_default_open(ctx, id, true);
            s.set_open(true);
            s.store(ctx);
        };
        match skok {
            Skok::Povrch(id) => {
                otevri(egui::Id::new("u_povrchy"));
                self.vyber_uzel(Uzel::Povrch(id.clone()));
            }
            Skok::Polozka(id, h) => {
                let cast = stav
                    .povrchy
                    .iter()
                    .find(|p| &p.id == id)
                    .and_then(|p| p.polozky.iter().find(|x| x.handle == *h))
                    .map(|x| match x.druh {
                        DruhPolozky::Spojnice => Cast::Spojnice,
                        DruhPolozky::Hranice | DruhPolozky::Dira => Cast::Hranice,
                        DruhPolozky::Objekt(_) => Cast::Objekty,
                    })
                    .unwrap_or(Cast::Spojnice);
                otevri(egui::Id::new("u_povrchy"));
                otevri(egui::Id::new(("u_povrch", id.as_str())));
                otevri(egui::Id::new(("u_definice", id.as_str())));
                self.zvyraznena = Some(*h);
                self.vyber_uzel(Uzel::Definice(id.clone(), cast));
            }
            Skok::Body(h) => {
                self.vybrane_body = h.iter().copied().collect();
                self.posunout_na = h.first().copied();
                self.hledat.clear();
                self.vyber_uzel(Uzel::Body);
            }
            Skok::Skupina(id) => {
                otevri(egui::Id::new("u_skupiny"));
                self.vyber_uzel(Uzel::Skupina(id.clone()));
            }
            Skok::StylBodu(id) => {
                otevri(egui::Id::new("u_styly"));
                otevri(egui::Id::new("u_styly_bodu"));
                self.vyber_uzel(Uzel::StylBodu(id.clone()));
            }
            Skok::StylPovrchu(id) => {
                otevri(egui::Id::new("u_styly"));
                otevri(egui::Id::new("u_styly_povrchu"));
                self.vyber_uzel(Uzel::StylPovrchu(id.clone()));
            }
            Skok::StylyBodu => {
                otevri(egui::Id::new("u_styly"));
                self.vyber_uzel(Uzel::StylyBodu);
            }
            Skok::StylyPovrchu => {
                otevri(egui::Id::new("u_styly"));
                self.vyber_uzel(Uzel::StylyPovrchu);
            }
            Skok::StylProfilu(id) => {
                self.vyber_uzel(Uzel::StylProfilu(id.clone()));
            }
            Skok::StylyProfilu => {
                self.vyber_uzel(Uzel::StylyProfilu);
            }
            Skok::StylTrasy(id) => {
                self.uprava_stylu_trasy = None;
                self.vyber_uzel(Uzel::StylTrasy(id.clone()));
            }
            Skok::StylyTras => {
                self.vyber_uzel(Uzel::StylyTras);
            }
            Skok::Trasa(id) => {
                otevri(egui::Id::new("u_trasy"));
                self.vyber_uzel(Uzel::Trasa(id.clone()));
            }
            Skok::Pohled(id) => {
                otevri(egui::Id::new("u_trasy"));
                if let Some(t) = stav.trasy.iter().find(|t| t.pohledy.iter().any(|p| &p.id == id)) {
                    otevri(egui::Id::new(("u_trasa", t.id.as_str())));
                }
                self.vyber_uzel(Uzel::Pohled(id.clone()));
            }
        }
    }

    /// Načte editovatelné hodnoty ze stavu (po změně výkresu).
    fn nacti_buffery(&mut self, stav: &StavSpravce) {
        let id = match &self.uzel {
            Uzel::Povrch(id) | Uzel::Definice(id, _) | Uzel::Upozorneni(id) => Some(id.clone()),
            _ => None,
        };
        if let Some(p) = id.and_then(|id| stav.povrchy.iter().find(|p| p.id == id)) {
            self.jmeno = p.jmeno.clone();
            self.kody = p.vyloucene_kody.clone();
        }
        if let Uzel::Skupina(id) = &self.uzel {
            if !self.skupina_zmenena {
                self.edit_skupina = stav.skupiny.iter().find(|s| &s.skupina.id == id).map(|s| s.skupina.clone());
            }
        }
        if !self.styl_zmenen {
            match &self.uzel {
                Uzel::StylBodu(id) => self.edit_styl_bodu = stav.styly_bodu.iter().find(|s| &s.id == id).cloned(),
                Uzel::StylPovrchu(id) => self.edit_styl_povrchu = stav.styly_povrchu.iter().find(|s| &s.id == id).cloned(),
                _ => {}
            }
        }
        // úprava bodu – jen když je vybraný právě jeden; rozepsané hodnoty
        // se přepíšou jen tehdy, když se bod ve výkrese změnil
        self.vybrane_body.retain(|h| stav.body.iter().any(|b| b.handle == *h));
        let zmeneno = match (&self.edit_bod, &self.edit_puvodni) {
            (Some((h, ..)), Some(puv)) => stav.body.iter().find(|b| b.handle == *h).map_or(true, |b| &b.bod != puv),
            _ => true,
        };
        if zmeneno {
            self.nacti_edit_bodu(stav);
        }
        if self.posledni_dalsi != stav.dalsi_cislo {
            self.posledni_dalsi = stav.dalsi_cislo;
            self.precislovat_od = stav.dalsi_cislo.max(1);
        }
        if self.precislovat_krok == 0 {
            self.precislovat_krok = 1;
        }
    }

    fn nacti_edit_bodu(&mut self, stav: &StavSpravce) {
        let bod = if self.vybrane_body.len() == 1 {
            let h = *self.vybrane_body.iter().next().unwrap();
            stav.body.iter().find(|b| b.handle == h)
        } else {
            None
        };
        self.edit_bod = bod.map(|b| (b.handle, b.bod.cislo.clone(), b.bod.z, b.bod.popis.clone()));
        self.edit_puvodni = bod.map(|b| b.bod.clone());
    }

    /// Téma nápovědy podle toho, co je v Prospektoru otevřené.
    fn tema_napovedy(&self) -> String {
        match (&self.rezim, &self.uzel) {
            (Rezim::Pohled3D, _) => "3d",
            (_, Uzel::Skupiny | Uzel::Skupina(_)) => "skupiny",
            (_, Uzel::StylyBodu | Uzel::StylBodu(_) | Uzel::StylyPovrchu | Uzel::StylPovrchu(_) | Uzel::StylyProfilu | Uzel::StylProfilu(_) | Uzel::StylyTras | Uzel::StylTrasy(_)) => "styly",
            (_, Uzel::Definice(_, Cast::Upravy)) => "upravy",
            (_, Uzel::Povrch(_) | Uzel::Povrchy | Uzel::Definice(..) | Uzel::Body) => "teren",
            _ => "prospektor",
        }
        .to_string()
    }

    // ── Nastavení ────────────────────────────────────────────────────────────

    fn detail_nastaveni(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        ui.horizontal(|ui| {
            nadpis(ui, tr("Nastavení", "Settings"));
            ui.label(RichText::new(tr("platí pro všechny výkresy (uloženo v profilu uživatele)", "applies to all drawings (stored in the user profile)")).color(SEDA));
        });
        let kategorie = [
            (0u8, tr("Obecné", "General")),
            (1, tr("Výkres a měřítko", "Drawing and scale")),
            (2, tr("Body a import", "Points and import")),
            (3, tr("Povrchy", "Surfaces")),
            (4, tr("Kontrola zaměření", "Survey check")),
            (5, tr("Okno Prospektoru", "Prospector window")),
        ];
        let siroke = ui.available_width() >= 560.0;
        let menu = |ui: &mut egui::Ui, kat: &mut u8| {
            for (k, t) in kategorie {
                if ui.selectable_label(*kat == k, t).clicked() {
                    *kat = k;
                }
            }
        };
        let mut kat = self.kat_nastaveni;
        if siroke {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(170.0, 0.0), egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    ui.set_width(170.0);
                    ui.add_space(4.0);
                    menu(ui, &mut kat);
                    ui.add_space(16.0);
                    self.obnovit_vychozi(ui);
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    self.kategorie_nastaveni(ui, stav, kat);
                });
            });
        } else {
            ui.horizontal_wrapped(|ui| menu(ui, &mut kat));
            ui.separator();
            self.kategorie_nastaveni(ui, stav, kat);
            ui.add_space(14.0);
            self.obnovit_vychozi(ui);
        }
        self.kat_nastaveni = kat;
    }

    fn obnovit_vychozi(&mut self, ui: &mut egui::Ui) {
        if !self.potvrdit_vychozi {
            if ui.button(tr("Obnovit výchozí…", "Restore defaults…")).on_hover_text(tr("vrátí všechna nastavení na výchozí hodnoty", "resets all settings to their default values")).clicked() {
                self.potvrdit_vychozi = true;
            }
            return;
        }
        ui.label(RichText::new(tr("Vrátit všechna nastavení na výchozí?", "Reset all settings to defaults?")).color(ORANZOVA));
        ui.horizontal(|ui| {
            if ui.button(tr("Ano, obnovit", "Yes, restore")).clicked() {
                let n = nastaveni::akt();
                let okno = n.okno;
                let vychozi = nastaveni::Nastaveni::default();
                spravce::nastav_sledovani(vychozi.sledovat_vyber);
                if n.povrch_nevybirat != vychozi.povrch_nevybirat {
                    spravce::posli(Akce::PovrchyNevybirat(vychozi.povrch_nevybirat));
                }
                nastaveni::zmen(|x| *x = nastaveni::Nastaveni { okno, ..vychozi });
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
                self.nast_kody = None;
                self.potvrdit_vychozi = false;
            }
            if ui.button(tr("Ne", "No")).clicked() {
                self.potvrdit_vychozi = false;
            }
        });
    }

    fn kategorie_nastaveni(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, kat: u8) {
        let n = nastaveni::akt();
        let mrizka = |id: &str| egui::Grid::new(id).num_columns(2).spacing([14.0, 8.0]).min_col_width(190.0);
        match kat {
            0 => {
                mrizka("nast_obecne").show(ui, |ui| {
                    ui.label("Jazyk / Language");
                    let mut volby: Vec<(String, String)> = vec![
                        ("auto".into(), "Automaticky / Automatic (Open CAD Studio)".into()), // i18n:ne
                        ("cs".into(), "Čeština".into()), // i18n:ne
                        ("en".into(), "English".into()),
                    ];
                    for k in crate::lang::dostupne_jazyky().into_iter().filter(|k| k != "cs" && k != "en") {
                        volby.push((k.clone(), k));
                    }
                    let akt = volby.iter().find(|v| v.0 == n.jazyk).map_or(volby[0].1.clone(), |v| v.1.clone());
                    egui::ComboBox::from_id_salt("nast_jazyk")
                        .selected_text(akt)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (k, t) in volby {
                                if ui.selectable_label(n.jazyk == k, t).clicked() && n.jazyk != k {
                                    nastaveni::zmen(|x| x.jazyk = k.clone());
                                }
                            }
                        })
                        .response
                        .on_hover_text("Projeví se po novém spuštění Open CAD Studia. / Takes effect after Open CAD Studio is restarted."); // i18n:ne
                    ui.end_row();
                    ui.label(tr("Sledovat výběr ve výkrese", "Follow selection in the drawing"));
                    let mut sl = spravce::sledovat_vyber();
                    if ui
                        .checkbox(&mut sl, "")
                        .on_hover_text(tr("Prospektor sám ukáže bod nebo povrch, na který kliknete ve výkrese", "the Prospector shows the point or surface you click in the drawing"))
                        .changed()
                    {
                        spravce::nastav_sledovani(sl);
                        nastaveni::zmen(|x| x.sledovat_vyber = sl);
                    }
                    ui.end_row();
                    ui.label(tr("Ukazovat postup „Jak na to“", "Show the \"How to\" steps"));
                    let mut postup = !n.postup_skryt;
                    if ui.checkbox(&mut postup, "").on_hover_text(tr("na přehledu výkresu (klik na název výkresu nahoře)", "on the drawing overview (click the drawing name at the top)")).changed() {
                        nastaveni::zmen(|x| x.postup_skryt = !postup);
                    }
                    ui.end_row();
                });
                if nastaveni::akt().jazyk != n.jazyk || jazyk_zmenen(&n.jazyk) {
                    ui.label(
                        RichText::new("Projeví se po novém spuštění Open CAD Studia. / Takes effect after Open CAD Studio is restarted.") // i18n:ne
                            .color(ORANZOVA),
                    );
                }
            }
            1 => {
                mrizka("nast_vykres").show(ui, |ui| {
                    ui.label(tr("Měřítko", "Scale"));
                    egui::ComboBox::from_id_salt("nast_meritko")
                        .selected_text(format!("1:{}", stav.meritko.round()))
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for m in [100u32, 200, 250, 500, 1000, 2000] {
                                if ui.selectable_label((stav.meritko - m as f64).abs() < 0.5, format!("1:{m}")).clicked() {
                                    spravce::posli(Akce::Meritko(m));
                                }
                            }
                        })
                        .response
                        .on_hover_text(tr("velikost značek a popisů; měřítko poznámek ve stavovém řádku (jiné než 1:1) má přednost", "size of markers and labels; the annotation scale in the status bar (other than 1:1) takes precedence"));
                    ui.end_row();
                    ui.label(tr("Odsunout překrývající se popisy", "Move overlapping labels apart"));
                    let mut ods = n.odsazeni_popisku;
                    if ui
                        .checkbox(&mut ods, "")
                        .on_hover_text(tr("popisy bodů blízko u sebe se posunou, aby se nepřekrývaly; projeví se po překreslení bodů", "point labels close to each other are moved so they do not overlap; takes effect after the points are redrawn"))
                        .changed()
                    {
                        nastaveni::zmen(|x| x.odsazeni_popisku = ods);
                        spravce::posli(Akce::ObnovitPopisy);
                    }
                    ui.end_row();
                    ui.label(tr("Posunutím popisu posunout jen popis", "Moving a label moves only the label"));
                    let mut jen = n.posun_jen_popis;
                    if ui
                        .checkbox(&mut jen, "")
                        .on_hover_text(tr("jinak se posune celý bod (i s výškou a povrchem); posunutím značky se bod posune vždy", "otherwise the whole point moves (with its elevation and the surface); moving the marker always moves the point"))
                        .changed()
                    {
                        nastaveni::zmen(|x| x.posun_jen_popis = jen);
                    }
                    ui.end_row();
                    ui.label(tr("Značky a popisy bodů", "Point markers and labels"));
                    if ui.button(tr("Překreslit body", "Redraw points")).on_hover_text(tr("znovu vykreslí značky a popisy všech bodů", "redraws the markers and labels of all points")).clicked() {
                        spravce::posli(Akce::ObnovitPopisy);
                    }
                    ui.end_row();
                    ui.label(tr("Šablona výkresu", "Drawing template"));
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(tr("Připravit výkres ze šablony", "Prepare the drawing from the template"))
                            .on_hover_text(tr("hladiny Terénu, značka bodu podle stylu, měřítko a styly ze šablony – jedním kliknutím", "Terén layers, point marker by style, scale and styles from the template – in one click"))
                            .clicked()
                        {
                            spravce::posli(Akce::PripravVykres);
                        }
                        if ui.button(tr("Uložit tento výkres jako šablonu", "Save this drawing as the template")).on_hover_text(tr("styly bodů a povrchů a měřítko", "point and surface styles and the scale")).clicked() {
                            spravce::posli(Akce::SablonaUloz);
                        }
                    });
                    ui.end_row();
                    ui.label("");
                    ui.label(
                        RichText::new(match crate::styly::meritko_sablony() {
                            Some(m) => trf!("šablona je uložená v profilu uživatele (měřítko 1:{}); totéž je na pásu karet: Připravit výkres", "the template is saved in the user profile (scale 1:{}); the same is on the ribbon: Prepare drawing", m),
                            None if crate::styly::sablona_existuje() => tr("šablona je uložená v profilu uživatele", "the template is saved in the user profile").to_string(),
                            None => tr("šablona zatím není uložená – bez ní se připraví jen hladiny a značky bodů", "no template is saved yet – without it only layers and point markers are prepared").to_string(),
                        })
                        .small()
                        .color(SEDA),
                    );
                    ui.end_row();
                });
            }
            2 => {
                mrizka("nast_import").show(ui, |ui| {
                    ui.label(tr("Náhled před importem", "Preview before import"));
                    let mut nahled = n.nahled_importu;
                    if ui.checkbox(&mut nahled, "").on_hover_text(tr("před importem ukázat náhled souboru (formát, sloupce, první body)", "show a file preview before import (format, columns, first points)")).changed() {
                        nastaveni::zmen(|x| x.nahled_importu = nahled);
                    }
                    ui.end_row();
                });
                ui.add_space(6.0);
                nastaveni_importu(ui);
                ui.add_space(10.0);
                ui.label(RichText::new(tr("Nové body", "New points")).strong());
                nastaveni_novych_bodu(ui, stav);
            }
            3 => {
                mrizka("nast_povrchy").show(ui, |ui| {
                    ui.label(tr("Nový povrch bez kódů", "New surface without codes"));
                    let mut kody = self.nast_kody.clone().unwrap_or_else(|| n.vyloucene_kody.join(", "));
                    let r = ui.add(egui::TextEdit::singleline(&mut kody).hint_text(tr("např. STR", "e.g. STR")).desired_width(160.0)).on_hover_text(
                        tr("body s těmito kódy se do nového povrchu neberou (stromy, sloupy…); u hotového povrchu se mění v jeho nastavení", "points with these codes are not taken into a new surface (trees, poles…); for an existing surface change it in its settings"),
                    );
                    if r.changed() {
                        self.nast_kody = Some(kody.clone());
                    }
                    if r.lost_focus() {
                        nastaveni::zmen(|x| x.vyloucene_kody = kody_z_textu(&kody));
                        self.nast_kody = None;
                    }
                    ui.end_row();
                    ui.label(tr("Přichycení spojnic na bod", "Breakline snap to point"));
                    let mut tol = n.tolerance_bodu;
                    if ui
                        .add(egui::DragValue::new(&mut tol).range(0.01..=5.0).speed(0.01).max_decimals(2).suffix(" m"))
                        .on_hover_text(tr("vrchol spojnice blíž k bodu zaměření než tato vzdálenost převezme výšku bodu", "a breakline vertex closer to a survey point than this distance takes the point's elevation"))
                        .changed()
                    {
                        nastaveni::zmen(|x| x.tolerance_bodu = tol);
                    }
                    ui.end_row();
                    ui.label(tr("Výběr kliknutím", "Selection by click"));
                    zaskrtnuti_nevybirat(ui);
                    ui.end_row();
                });
            }
            4 => {
                mrizka("nast_kontrola").show(ui, |ui| {
                    ui.label(tr("Práh odchylky", "Deviation threshold"));
                    let mut prah = n.prah_kontroly;
                    if ui
                        .add(egui::DragValue::new(&mut prah).range(0.1..=10.0).speed(0.05).suffix(" m").max_decimals(2).custom_formatter(|v, _| cz(v, 2)).custom_parser(|t| t.replace(',', ".").trim().parse().ok()))
                        .on_hover_text(tr("bod, jehož výška se od okolí liší víc, se označí jako podezřelý", "a point whose elevation differs from its surroundings by more is marked as suspicious"))
                        .changed()
                    {
                        nastaveni::zmen(|x| x.prah_kontroly = prah);
                    }
                    ui.end_row();
                });
            }
            _ => {
                mrizka("nast_okno").show(ui, |ui| {
                    ui.label(tr("Po otevření ukázat", "On opening, show"));
                    let mut vyber = !n.rezim_vykres;
                    egui::ComboBox::from_id_salt("nast_po_otevreni")
                        .selected_text(if vyber { tr("Vybráno ve výkrese", "Selected in drawing") } else { tr("přehled výkresu", "drawing overview") })
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut vyber, true, tr("Vybráno ve výkrese", "Selected in drawing"));
                            ui.selectable_value(&mut vyber, false, tr("přehled výkresu", "drawing overview"));
                        });
                    if vyber == n.rezim_vykres {
                        nastaveni::zmen(|x| x.rezim_vykres = !vyber);
                    }
                    ui.end_row();
                    ui.label(tr("Vždy navrchu", "Always on top"));
                    let mut navrchu = n.okno_navrchu;
                    if ui.checkbox(&mut navrchu, "").on_hover_text(tr("okno Prospektoru zůstane nad Open CAD Studiem", "the Prospector window stays above Open CAD Studio")).changed() {
                        nastaveni::zmen(|x| x.okno_navrchu = navrchu);
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(if navrchu {
                            egui::WindowLevel::AlwaysOnTop
                        } else {
                            egui::WindowLevel::Normal
                        }));
                    }
                    ui.end_row();
                    ui.label(tr("Velikost okna", "Window size"));
                    ui.horizontal(|ui| {
                        if ui
                            .button(tr("Úzké", "Narrow"))
                            .on_hover_text(tr("úzký pruh se stromem nahoře a podrobnostmi pod ním – nezakrývá výkres", "narrow strip with the tree on top and details below – does not cover the drawing"))
                            .clicked()
                        {
                            zmen_velikost(ui.ctx(), true);
                        }
                        if ui.button(tr("Široké", "Wide")).on_hover_text(tr("strom vlevo, podrobnosti vpravo", "tree on the left, details on the right")).clicked() {
                            zmen_velikost(ui.ctx(), false);
                        }
                    });
                    ui.end_row();
                });
            }
        }
    }

    // ── Kontextové nabídky (pravé tlačítko) ─────────────────────────────────

    fn menu_skupiny(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, s: &SkupinaBodu) {
        let u = Uzel::Skupina(s.id.clone());
        let vestavena = s.id == VSECHNY;
        if !vestavena && ui.button(tr("Upravit skupinu", "Edit group")).clicked() {
            self.vyber_uzel(u.clone());
        }
        if ui.button(tr("Nový povrch ze skupiny", "New surface from the group")).clicked() {
            spravce::posli(Akce::PovrchZeSkupiny(s.id.clone(), None, false));
        }
        if ui.button(tr("Vybrat body ve výkrese", "Select points in the drawing")).clicked() {
            let f = s.filtr();
            let h: Vec<Handle> = stav.body.iter().filter(|b| f.obsahuje(&b.bod, &b.zdroj)).map(|b| b.handle).collect();
            ukazat_body(stav, &h);
        }
        if ui.button(tr("Exportovat body…", "Export points…")).clicked() {
            self.vyber_uzel(u);
        }
        if !vestavena {
            ui.separator();
            if ui.button(RichText::new(tr("Smazat skupinu", "Delete group")).color(CERVENA)).on_hover_text(tr("body ve výkrese zůstanou", "the points stay in the drawing")).clicked() {
                smazat(trf!("Skupina „{}“ je smazaná (body ve výkrese zůstaly).", "Group \"{}\" was deleted (the points stay in the drawing).", s.jmeno), Akce::SkupinaSmazat(s.id.clone()));
            }
        }
    }

    fn menu_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, p: &PrehledPovrchu) {
        let id = p.id.clone();
        let nastav = |z: Zmena| spravce::posli(Akce::Nastav(vec![id.clone()], z));
        if ui.add_enabled(!p.aktivni, egui::Button::new(tr("Nastavit jako aktivní", "Set as active"))).clicked() {
            spravce::posli(Akce::Aktivovat(id.clone()));
        }
        if ui.button(tr("Přepočítat", "Recalculate")).clicked() {
            spravce::posli(Akce::Prepocitat(vec![id.clone()]));
        }
        let mut auto = p.auto;
        if ui.checkbox(&mut auto, tr("Automaticky přepočítávat", "Recalculate automatically")).changed() {
            nastav(Zmena::Auto(auto));
        }
        if ui.button(tr("Prohlédnout ve 3D", "View in 3D")).clicked() {
            self.pohled.otevri(vec![id.clone()]);
            self.rezim = Rezim::Pohled3D;
        }
        ui.separator();
        ui.menu_button(tr("Styl", "Style"), |ui| {
            for s in &stav.styly_povrchu {
                let sel = s.id.eq_ignore_ascii_case(&p.styl);
                if ui.selectable_label(sel, &s.jmeno).clicked() {
                    if !sel {
                        nastav(Zmena::Styl(s.id.clone()));
                    }
                    ui.close();
                }
            }
        });
        let krok_stylu = stav.styly_povrchu.iter().find(|s| s.id.eq_ignore_ascii_case(&p.styl)).and_then(|s| s.interval);
        ui.add_enabled_ui(krok_stylu.is_none(), |ui| {
            ui.menu_button(tr("Vrstevnice po", "Contour interval"), |ui| {
                for (i, h) in INTERVALY {
                    let sel = (i - p.interval).abs() < 1e-9 && (h - p.hlavni).abs() < 1e-9;
                    if ui.selectable_label(sel, interval_text(i, h)).clicked() {
                        if !sel {
                            nastav(Zmena::Interval(i, h));
                        }
                        ui.close();
                    }
                }
                if ui.button(tr("vlastní…", "custom…")).clicked() {
                    self.vlastni_krok = Some((id.clone(), p.interval, p.hlavni));
                    self.vyber_uzel(Uzel::Povrch(id.clone()));
                    ui.close();
                }
            })
            .response
            .on_disabled_hover_text(tr("krok vrstevnic určuje styl povrchu", "the contour interval is set by the surface style"));
        });
        ui.separator();
        if ui.button(tr("Přejmenovat…", "Rename…")).clicked() {
            self.vyber_uzel(Uzel::Povrch(id.clone()));
            self.prejmenovat = true;
        }
        if ui.button(tr("Export povrchu…", "Surface export…")).on_hover_text(tr("LandXML nebo DXF s 3D plochami", "LandXML or DXF with 3D faces")).clicked() {
            self.vyber_uzel(Uzel::Povrch(id.clone()));
            self.zalozka_povrchu = karta::ZalozkaPovrchu::Export;
        }
        if ui.button(RichText::new(tr("Smazat povrch", "Delete surface")).color(CERVENA)).clicked() {
            smazat(trf!("Povrch „{}“ je smazaný.", "Surface \"{}\" was deleted.", p.jmeno), Akce::Smazat(id.clone()));
        }
    }

    /// Nabídka pro body v tabulce (vybrané řádky).
    fn menu_bodu(ui: &mut egui::Ui, stav: &StavSpravce, h: &[Handle]) {
        let n = h.len();
        ui.label(RichText::new(if n == 1 { tr("1 bod", "1 point").to_string() } else { trf!("{} bodů", "{} points", n) }).small().color(SEDA));
        if ui.button(tr("Ukázat ve výkrese", "Show in the drawing")).clicked() {
            ukazat_body(stav, h);
        }
        ui.menu_button(tr("Styl", "Style"), |ui| {
            if ui.button(tr("podle skupiny / kódu", "by group / code")).clicked() {
                spravce::posli(Akce::StylBodu(h.to_vec(), String::new()));
                ui.close();
            }
            ui.separator();
            for s in &stav.styly_bodu {
                if ui.button(&s.jmeno).clicked() {
                    spravce::posli(Akce::StylBodu(h.to_vec(), s.id.clone()));
                    ui.close();
                }
            }
        });
        ui.menu_button(tr("Do skupiny", "To group"), |ui| {
            for g in stav.skupiny.iter().filter(|g| g.skupina.id != VSECHNY) {
                if ui.button(&g.skupina.jmeno).clicked() {
                    spravce::posli(Akce::SkupinaBody(g.skupina.id.clone(), h.to_vec(), true));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(tr("Nová skupina z těchto bodů", "New group from these points")).clicked() {
                spravce::posli(Akce::SkupinaNova(h.to_vec()));
                ui.close();
            }
        });
        let skupiny: Vec<_> = stav.skupiny.iter().filter(|g| g.skupina.id != VSECHNY).collect();
        if !skupiny.is_empty() {
            ui.menu_button(tr("Ze skupiny odebrat", "Remove from group"), |ui| {
                for g in skupiny {
                    if ui.button(&g.skupina.jmeno).clicked() {
                        spravce::posli(Akce::SkupinaBody(g.skupina.id.clone(), h.to_vec(), false));
                        ui.close();
                    }
                }
            });
        }
        if n == 1 {
            if let Some(b) = stav.body.iter().find(|b| b.handle == h[0]) {
                if ui.button(tr("Vynechat z aktivního povrchu", "Exclude from the active surface")).clicked() {
                    spravce::posli(Akce::VyloucitBod(b.bod.x, b.bod.y));
                }
            }
        }
        ui.separator();
        if ui.button(RichText::new(if n == 1 { tr("Smazat bod", "Delete point") } else { tr("Smazat body", "Delete points") }).color(CERVENA)).clicked() {
            smazat(pocet_smazanych(h.len()), Akce::BodySmazat(h.to_vec()));
        }
    }

    // ── Detail ───────────────────────────────────────────────────────────────

    fn detail(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        match self.uzel.clone() {
            Uzel::Vykres => self.detail_vykres(ui, stav),
            Uzel::Vyber => {
                egui::ScrollArea::vertical().id_salt("vyber_s").auto_shrink([false, false]).show(ui, |ui| self.panel_vyberu(ui, stav));
            }
            Uzel::Body => self.detail_body(ui, stav),
            Uzel::Skupiny => self.detail_skupiny(ui, stav),
            Uzel::Skupina(id) => self.detail_skupina(ui, stav, &id),
            Uzel::Povrchy => self.detail_povrchy(ui, stav),
            Uzel::Povrch(id) | Uzel::Upozorneni(id) | Uzel::Definice(id, _) => {
                let Some(p) = stav.povrchy.iter().find(|p| p.id == id).cloned() else {
                    ui.label(RichText::new(tr("Povrch už ve výkrese není.", "The surface is no longer in the drawing.")).color(SEDA));
                    return;
                };
                self.karta_povrchu(ui, stav, &p);
            }
            Uzel::Kontrola => kontrola(ui, stav),
            Uzel::Nastaveni => {
                egui::ScrollArea::vertical().id_salt("nast_s").auto_shrink([false, false]).show(ui, |ui| self.detail_nastaveni(ui, stav));
            }
            Uzel::StylyBodu => {
                egui::ScrollArea::vertical().id_salt("styly_b").auto_shrink([false, false]).show(ui, |ui| self.detail_styly_bodu(ui, stav));
            }
            Uzel::StylyPovrchu => {
                egui::ScrollArea::vertical().id_salt("styly_p").auto_shrink([false, false]).show(ui, |ui| self.detail_styly_povrchu(ui, stav));
            }
            Uzel::StylBodu(id) => {
                egui::ScrollArea::both().id_salt("styl_b").auto_shrink([false, false]).show(ui, |ui| self.detail_styl_bodu(ui, stav, &id));
            }
            Uzel::StylPovrchu(id) => {
                egui::ScrollArea::both().id_salt("styl_p").auto_shrink([false, false]).show(ui, |ui| self.detail_styl_povrchu(ui, stav, &id));
            }
            Uzel::Trasy => self.detail_trasy(ui, stav),
            Uzel::Trasa(id) => {
                egui::ScrollArea::vertical().id_salt("trasa_s").auto_shrink([false, false]).show(ui, |ui| self.karta_trasy(ui, stav, &id));
            }
            Uzel::Pohled(id) => {
                egui::ScrollArea::vertical().id_salt("pohled_s").auto_shrink([false, false]).show(ui, |ui| self.karta_pohledu(ui, stav, &id));
            }
            Uzel::StylyProfilu => {
                egui::ScrollArea::vertical().id_salt("styly_pp").auto_shrink([false, false]).show(ui, |ui| self.detail_styly_profilu(ui, stav));
            }
            Uzel::StylProfilu(id) => {
                egui::ScrollArea::vertical().id_salt("styl_pp").auto_shrink([false, false]).show(ui, |ui| self.karta_stylu_profilu(ui, stav, &id));
            }
            Uzel::StylyTras => {
                egui::ScrollArea::vertical().id_salt("styly_tr").auto_shrink([false, false]).show(ui, |ui| self.detail_styly_tras(ui, stav));
            }
            Uzel::StylTrasy(id) => {
                egui::ScrollArea::vertical().id_salt("styl_tr").auto_shrink([false, false]).show(ui, |ui| self.karta_stylu_trasy(ui, stav, &id));
            }
        }
    }

    fn detail_vykres(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let nazev = std::path::Path::new(&stav.vykres)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| stav.vykres.clone());
        karta::nadpis_karty(ui, &strom::Ikona::Svg(strom::ik!("ter_prospektor")), &nazev, |ui| {
            karta::pilulka(ui, &format!("1:{}", cislo_cz(stav.meritko)), SEDA)
                .on_hover_text(tr("měřítko poznámek nastavené vpravo dole ve stavovém řádku; při 1:1 platí měřítko z pásu karet Terén", "the annotation scale set at the bottom right of the status bar; at 1:1 the scale from the Terén ribbon applies"));
        });
        if !stav.vykres.is_empty() && stav.vykres != nazev {
            ui.label(RichText::new(&stav.vykres).small().color(SEDA));
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if karta::kpi(ui, &czi(stav.body.len()), tr("bodů", "points")).interact(Sense::click()).clicked() {
                self.vyber_uzel(Uzel::Body);
            }
            if karta::kpi(ui, &stav.skupiny.len().to_string(), tr("skupin bodů", "point groups")).interact(Sense::click()).clicked() {
                self.vyber_uzel(Uzel::Skupiny);
            }
            if karta::kpi(ui, &stav.povrchy.len().to_string(), tr("povrchů", "surfaces")).interact(Sense::click()).clicked() {
                self.vyber_uzel(Uzel::Povrchy);
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(tr("Překreslit body", "Redraw points"))
                .on_hover_text(tr("smaže značky a popisy bodů a vytvoří je znovu podle stylů a měřítka", "deletes the point markers and labels and creates them again by styles and scale"))
                .clicked()
            {
                spravce::posli(Akce::ObnovitPopisy);
            }
            if ui
                .add(egui::Button::image_and_text(
                    egui::Image::new(strom::ik!("ter_sablona")).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                    tr("Připravit výkres ze šablony", "Prepare the drawing from the template"),
                ))
                .on_hover_text(tr("hladiny Terénu, značka bodu podle stylu, měřítko a styly ze šablony – jedním kliknutím", "Terén layers, point marker by style, scale and styles from the template – in one click"))
                .clicked()
            {
                spravce::posli(Akce::PripravVykres);
            }
        });
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        if nastaveni::akt().postup_skryt {
            if ui.small_button(tr("Ukázat postup „Jak na to“", "Show the \"How to\" steps")).clicked() {
                nastaveni::zmen(|n| n.postup_skryt = false);
            }
        } else {
            self.postup(ui, stav);
        }
    }

    /// Horní lišta: výkres, hledání, výběr z výkresu, nápověda.
    fn hlavicka(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let uzke = ui.available_width() < 700.0;
        ui.horizontal(|ui| {
            if self.rezim == Rezim::Pohled3D {
                if ui.button(tr("← Zpět do stromu", "← Back to the tree")).clicked() {
                    self.rezim = Rezim::Vykres;
                }
                ui.label(RichText::new(tr("Prohlížeč 3D", "3D viewer")).strong());
            } else {
                strom::kresli_ikonu(
                    ui,
                    &strom::Ikona::Svg(strom::ik!("ter_prospektor")),
                    ui.cursor().left_center() + egui::vec2(8.0, 0.0),
                    false,
                );
                ui.add_space(18.0);
                let nazev = std::path::Path::new(&stav.vykres)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| stav.vykres.clone());
                let r = ui
                    .selectable_label(self.uzel == Uzel::Vykres, RichText::new(&nazev).strong())
                    .on_hover_text(trf!("{}
klik = přehled výkresu a postup „Jak na to“", "{}\nclick = drawing overview and the \"How to\" steps", stav.vykres));
                if r.clicked() {
                    self.vyber_uzel(Uzel::Vykres);
                }
                if !uzke {
                    ui.label(RichText::new(format!("1:{}", cislo_cz(stav.meritko))).color(SEDA))
                        .on_hover_text(tr("měřítko výkresu (velikost značek a popisů)", "drawing scale (size of markers and labels)"));
                    ui.add_space(8.0);
                    self.pole_hledani(ui, 220.0);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⟳").on_hover_text(tr("Načíst výkres znovu", "Reload the drawing")).clicked() {
                    spravce::posli(Akce::Obnovit);
                }
                if ui.selectable_label(self.napoveda.is_some(), " ? ").on_hover_text(tr("Nápověda k tomu, co je otevřené (F1)", "Help for what is open (F1)")).clicked() {
                    self.napoveda = if self.napoveda.is_some() { None } else { Some(self.tema_napovedy()) };
                }
                if self.rezim != Rezim::Pohled3D {
                    let mut sl = spravce::sledovat_vyber();
                    if ui
                        .selectable_label(sl, if uzke { "◎" } else { tr("◎ Sledovat výběr", "◎ Follow selection") })
                        .on_hover_text(tr("Zapnuto: Prospektor sám ukáže bod nebo povrch, na který kliknete ve výkrese", "On: the Prospector shows the point or surface you click in the drawing"))
                        .clicked()
                    {
                        sl = !sl;
                        spravce::nastav_sledovani(sl);
                        nastaveni::zmen(|n| n.sledovat_vyber = sl);
                    }
                    if ui
                        .button(tr("Najít v Prospektoru", "Find in Prospector"))
                        .on_hover_text(tr("Vyberte ve výkrese bod, čáru nebo trojúhelník povrchu – Prospektor na něj skočí", "Select a point, line or surface triangle in the drawing – the Prospector jumps to it"))
                        .clicked()
                    {
                        self.vynutit_skok = true;
                        spravce::posli(Akce::VyberZVykresu);
                    }
                    self.aktivni_v_liste(ui, stav, uzke);
                    self.prepocet_v_liste(ui, uzke);
                }
                if stav.pracuji {
                    ui.spinner();
                }
            });
        });
        if uzke && self.rezim != Rezim::Pohled3D {
            ui.horizontal(|ui| self.pole_hledani(ui, (ui.available_width() - 40.0).max(100.0)));
        }
    }

    /// Režim přepočtu povrchů a profilů v horní liště; když něco čeká,
    /// tlačítko „Přepočítat“.
    fn prepocet_v_liste(&mut self, ui: &mut egui::Ui, uzke: bool) {
        use crate::prepocet::{self, Rezim};
        let ted = prepocet::rezim();
        let nazev = |r: Rezim| match r {
            Rezim::Automaticky => tr("hned", "immediately"),
            Rezim::PoDokonceni => tr("po dokončení úprav", "when editing is done"),
            Rezim::Pozastaveno => tr("pozastaven", "paused"),
        };
        if prepocet::ceka_neco()
            && ui
                .add(egui::Button::new(RichText::new(tr("⟳ Přepočítat", "⟳ Recalculate")).color(ORANZOVA)))
                .on_hover_text(tr("povrchy nebo profily čekají na přepočet – klik je přepočítá hned", "surfaces or profiles are waiting to be recalculated – click to recalculate now"))
                .clicked()
        {
            prepocet::pozadej();
        }
        let mut vybrany = ted;
        egui::ComboBox::from_id_salt("rezim_prepoctu")
            .selected_text(if ted == Rezim::Pozastaveno { RichText::new(nazev(ted)).color(ORANZOVA) } else { RichText::new(nazev(ted)) })
            .width(if uzke { 90.0 } else { 140.0 })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut vybrany, Rezim::PoDokonceni, tr("po dokončení úprav (po chvíli klidu)", "when editing is done (after a short pause)"));
                ui.selectable_value(&mut vybrany, Rezim::Automaticky, tr("hned po každé změně", "right after each change"));
                ui.selectable_value(&mut vybrany, Rezim::Pozastaveno, tr("pozastavit", "pause"));
            })
            .response
            .on_hover_text(tr("Kdy se přepočítají povrchy a podélné profily po změně výkresu", "When surfaces and longitudinal profiles are recalculated after a drawing change"));
        if vybrany != ted {
            prepocet::nastav(vybrany);
            nastaveni::zmen(|n| n.prepocet = vybrany.klic().into());
        }
        if !uzke {
            ui.label(RichText::new(tr("Přepočet", "Recalculation")).color(SEDA));
        }
    }

    /// Aktivní povrch v horní liště (výběr se stavovou tečkou; zastaralý má ⟳).
    fn aktivni_v_liste(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, uzke: bool) {
        if stav.povrchy.is_empty() {
            return;
        }
        let akt = stav.povrchy.iter().find(|p| p.aktivni);
        if let Some(p) = akt.filter(|p| p.zastaraly) {
            if ui
                .add(egui::Button::new(RichText::new("⟳").color(ORANZOVA)))
                .on_hover_text(trf!("„{}“ je zastaralý – klik ho přepočítá", "\"{}\" is out of date – click to recalculate", p.jmeno))
                .clicked()
            {
                spravce::posli(Akce::Prepocitat(vec![p.id.clone()]));
            }
        }
        let jmeno = akt.map_or(tr("žádný", "none").to_string(), |p| p.jmeno.clone());
        egui::ComboBox::from_id_salt("aktivni_povrch")
            .selected_text(RichText::new(jmeno).strong())
            .width(if uzke { 120.0 } else { 170.0 })
            .show_ui(ui, |ui| {
                for p in &stav.povrchy {
                    let t = if p.zastaraly { trf!("{} (zastaralý)", "{} (out of date)", p.jmeno) } else { p.jmeno.clone() };
                    if ui.selectable_label(p.aktivni, t).clicked() && !p.aktivni {
                        spravce::posli(Akce::Aktivovat(p.id.clone()));
                    }
                }
            })
            .response
            .on_hover_text(tr("Aktivní povrch – s ním pracují nástroje Úpravy, Výška v bodě a Bod na povrchu", "Active surface – used by the Edits tools, Elevation at point and Point on surface"));
        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, if akt.is_some_and(|p| p.zastaraly) { ORANZOVA } else { ZELENA });
        if !uzke {
            ui.label(RichText::new(tr("Aktivní povrch", "Active surface")).color(SEDA));
        }
    }

    fn pole_hledani(&mut self, ui: &mut egui::Ui, sirka: f32) {
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.hledat)
                .id(egui::Id::new("pole_hledani"))
                .hint_text(tr("🔍 Hledat body, skupiny, povrchy (Ctrl+F)", "🔍 Search points, groups, surfaces (Ctrl+F)"))
                .desired_width(sirka),
        );
        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.hledat.clear();
        }
        if !self.hledat.is_empty() && ui.small_button("×").on_hover_text(tr("Zrušit hledání", "Clear search")).clicked() {
            self.hledat.clear();
        }
    }

    // ── Body ─────────────────────────────────────────────────────────────────

    fn detail_body(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        // ── karta ──
        let skryte = stav.styl_bodu.values().filter(|(id, _)| stav.styly_bodu.iter().any(|s| &s.id == id && s.skryt)).count();
        karta::nadpis_karty(ui, &strom::Ikona::Svg(strom::ik!("ter_seznam")), tr("Body", "Points"), |ui| {
            karta::pilulka(ui, &trf!("{} bodů", "{} points", czi(stav.body.len())), SEDA);
            if skryte > 0 {
                karta::pilulka(ui, &trf!("{} skrytých", "{} hidden", skryte), SEDA).on_hover_text(tr("body se stylem Nezobrazovat", "points with the Do not display style"));
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if karta::tlacitko(ui, Some(strom::ik!("ter_body_import")), tr("Import bodů…", "Import points…"), true).clicked() {
                self.nahled_importu = Some(import_nahled::NahledImportu::novy(""));
            }
            ui.menu_button(tr("Nový bod ⏷", "New point ⏷"), |ui| {
                if ui
                    .add(egui::Button::image_and_text(
                        egui::Image::new(strom::ik!("ter_bod_novy")).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        tr("Ruční bod (klik nebo x,y,z)", "Manual point (click or x,y,z)"),
                    ))
                    .clicked()
                {
                    spravce::posli(Akce::Navod(Vec::new(), tr("Na pásu karet Terén klikněte na Nový bod › Ruční bod a klikněte do výkresu nebo zadejte x,y.", "On the Terén ribbon tab click New point › Manual point, then click in the drawing or enter x,y.").into()));
                    ui.close();
                }
                if ui
                    .add(egui::Button::image_and_text(
                        egui::Image::new(strom::ik!("ter_bod_na_povrchu")).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        tr("Bod na povrchu", "Point on surface"),
                    ))
                    .clicked()
                {
                    spravce::posli(Akce::Navod(Vec::new(), tr("Na pásu karet Terén klikněte na Nový bod › Bod na povrchu a klikejte do výkresu.", "On the Terén ribbon tab click New point › Point on surface, then click in the drawing.").into()));
                    ui.close();
                }
            })
            .response
            .on_hover_text(tr("Nový bod se zadává ve výkrese tlačítkem Nový bod na pásu karet Terén (klik nebo souřadnice)", "New points are entered in the drawing with the New point button on the Terén ribbon tab (click or coordinates)"));
            if karta::tlacitko(ui, Some(strom::ik!("ter_styl_bodu")), tr("Překreslit body", "Redraw points"), false)
                .on_hover_text(tr("znovu vykreslí značky a popisy všech bodů podle stylů a měřítka", "redraws the markers and labels of all points according to styles and scale"))
                .clicked()
            {
                spravce::posli(Akce::ObnovitPopisy);
            }
            if karta::tlacitko(ui, Some(strom::ik!("ter_vyska_v_bode")), tr("Zkontrolovat zaměření", "Check the survey"), false).clicked() {
                spravce::posli(Akce::Kontrola);
                self.vyber_uzel(Uzel::Kontrola);
            }
            ui.menu_button(tr("Další ⏷", "More ⏷"), |ui| {
                if ui.button(tr("Nastavení nových bodů a importu…", "Settings for new points and import…")).clicked() {
                    self.kat_nastaveni = 2;
                    self.vyber_uzel(Uzel::Nastaveni);
                    ui.close();
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();

        if stav.body.is_empty() {
            karta::prazdny_stav(
                ui,
                strom::ik!("ter_body_import"),
                tr("Ve výkrese zatím nejsou body", "There are no points in the drawing yet"),
                tr("Importujte zaměření (TXT, CSV, XYZ, LandXML, GSI, Excel…) nebo vytvořte bod tlačítkem Nový bod na pásu karet.", "Import a survey (TXT, CSV, XYZ, LandXML, GSI, Excel…) or create a point with New point on the ribbon."),
                |ui| {
                    if karta::tlacitko(ui, Some(strom::ik!("ter_body_import")), tr("Import bodů…", "Import points…"), true).clicked() {
                        self.nahled_importu = Some(import_nahled::NahledImportu::novy(""));
                    }
                },
            );
            return;
        }

        // ── filtry ──
        self.filtry_bodu(ui, stav);
        let filtr_skupiny = self
            .filtr_skupina
            .as_ref()
            .and_then(|id| stav.skupiny.iter().find(|g| &g.skupina.id == id))
            .map(|g| g.skupina.filtr());
        let cislo_z = |t: &str| t.trim().replace(',', ".").parse::<f64>().ok().filter(|z| z.is_finite());
        let (z_od, z_do) = (cislo_z(&self.filtr_z.0), cislo_z(&self.filtr_z.1));
        let body: Vec<&BodVykresu> = stav
            .body
            .iter()
            .filter(|b| bod_odpovida(b, &self.hledat))
            .filter(|b| filtr_skupiny.as_ref().is_none_or(|f| f.obsahuje(&b.bod, &b.zdroj)))
            .filter(|b| self.filtr_kody.is_empty() || self.filtr_kody.contains(&b.bod.kod().to_uppercase()))
            .filter(|b| z_od.is_none_or(|z| b.bod.z >= z) && z_do.is_none_or(|z| b.bod.z <= z))
            .collect();
        let filtr = self.filtr_skupina.is_some() || !self.filtr_kody.is_empty() || z_od.is_some() || z_do.is_some() || !self.hledat.is_empty();
        let vybrane: Vec<Handle> = body.iter().map(|b| b.handle).filter(|h| self.vybrane_body.contains(h)).collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new(trf!("Zobrazeno {} z {}", "Showing {} of {}", cislo_z_poctu(body.len()), cislo_z_poctu(stav.body.len()))).color(SEDA));
            if filtr && ui.small_button(tr("zrušit filtr", "clear filter")).clicked() {
                self.filtr_kody.clear();
                self.filtr_skupina = None;
                self.filtr_z = Default::default();
                self.hledat.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(tr("Sloupce ⏷", "Columns ⏷"), |ui| {
                    for s in SKRYVATELNE_SLOUPCE {
                        let mut vid = !self.skryte_sloupce.contains(s);
                        if ui.checkbox(&mut vid, crate::lang::tr_tab(s)).changed() {
                            if vid {
                                self.skryte_sloupce.remove(s);
                            } else {
                                self.skryte_sloupce.insert(s);
                            }
                        }
                    }
                });
                ui.label(RichText::new(tr("šipky · Ctrl+A · Delete · Esc · F2", "arrows · Ctrl+A · Delete · Esc · F2")).small().color(SEDA))
                    .on_hover_text(tr("šipky = pohyb · Ctrl+A vše · Delete smazat · Esc zrušit výběr · F2 / dvojklik upravit buňku · Enter ukázat ve výkrese", "arrows = move · Ctrl+A all · Delete delete · Esc clear selection · F2 / double-click edit cell · Enter show in drawing"));
            });
        });

        // ── lišta výběru (pevná výška, aby tabulka neposkakovala) ──
        self.lista_vyberu(ui, stav, &body, &vybrane);
        ui.separator();
        self.tabulka_bodu(ui, stav, &body);
    }

    /// Filtry tabulky bodů: skupina, kódy, výška, sloupce.
    fn filtry_bodu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Skupina", "Group")).color(SEDA));
            if self.filtr_skupina.as_ref().is_some_and(|id| !stav.skupiny.iter().any(|g| &g.skupina.id == id)) {
                self.filtr_skupina = None;
            }
            let jmeno = self
                .filtr_skupina
                .as_ref()
                .and_then(|id| stav.skupiny.iter().find(|g| &g.skupina.id == id))
                .map_or(tr("všechny body", "all points").to_string(), |g| g.skupina.jmeno.clone());
            egui::ComboBox::from_id_salt("filtr_skupina").selected_text(jmeno).width(150.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.filtr_skupina, None, tr("všechny body", "all points"));
                for g in stav.skupiny.iter().filter(|g| g.skupina.id != VSECHNY) {
                    ui.selectable_value(&mut self.filtr_skupina, Some(g.skupina.id.clone()), format!("{} ({})", g.skupina.jmeno, g.pocet));
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new(tr("Kód", "Code")).color(SEDA));
            // nejčastější kódy
            let mut kody: Vec<(String, usize)> = Vec::new();
            for b in &stav.body {
                let k = b.bod.kod().to_uppercase();
                if k.is_empty() {
                    continue;
                }
                match kody.iter_mut().find(|x| x.0 == k) {
                    Some(x) => x.1 += 1,
                    None => kody.push((k, 1)),
                }
            }
            kody.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let ukazat = 8;
            if kody.is_empty() {
                ui.label(RichText::new(tr("body nemají kódy", "the points have no codes")).small().color(SEDA));
            }
            for (k, n) in kody.iter().take(ukazat) {
                let zap = self.filtr_kody.contains(k);
                let stitek = egui::Button::new(RichText::new(k).size(12.0)).selected(zap).corner_radius(10.0);
                if ui.add(stitek).on_hover_text(trf!("{} bodů", "{} points", n)).clicked() {
                    if zap {
                        self.filtr_kody.remove(k);
                    } else {
                        self.filtr_kody.insert(k.clone());
                    }
                }
            }
            if kody.len() > ukazat {
                ui.menu_button(tr("další ⏷", "more ⏷"), |ui| {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (k, n) in kody.iter().skip(ukazat) {
                            let mut zap = self.filtr_kody.contains(k);
                            if ui.checkbox(&mut zap, format!("{k} ({n})")).changed() {
                                if zap {
                                    self.filtr_kody.insert(k.clone());
                                } else {
                                    self.filtr_kody.remove(k);
                                }
                            }
                        }
                    });
                });
            }
            ui.add_space(8.0);
            ui.label(RichText::new(tr("Výška", "Elevation")).color(SEDA)).on_hover_text(tr("jen body s výškou v tomto rozsahu (m)", "only points with elevation in this range (m)"));
            ui.add(egui::TextEdit::singleline(&mut self.filtr_z.0).desired_width(56.0).hint_text(tr("od", "from")));
            ui.label("–");
            ui.add(egui::TextEdit::singleline(&mut self.filtr_z.1).desired_width(56.0).hint_text(tr("do", "to")));
        });
    }

    /// Lišta nad tabulkou: co jde udělat s vybranými body (bez výběru nápověda).
    fn lista_vyberu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, viditelne: &[&BodVykresu], vybrane: &[Handle]) {
        let vybrane: Vec<Handle> = vybrane.to_vec();
        let n = vybrane.len();
        let vyska = 30.0;
        let sirka = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(sirka, vyska), Sense::hover());
        if n > 0 {
            ui.painter().rect_filled(rect, 5.0, ui.visuals().selection.bg_fill.gamma_multiply(0.45));
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let ui = &mut child;
        if n == 0 {
            ui.label(RichText::new(tr("Vyberte body kliknutím (Shift = rozsah, Ctrl = přidat)", "Select points by clicking (Shift = range, Ctrl = add)")).color(SEDA));
            if ui.small_button(tr("Vybrat vše", "Select all")).clicked() {
                self.vybrane_body = viditelne.iter().map(|b| b.handle).collect();
                self.nacti_edit_bodu(stav);
            }
            if ui.small_button(tr("Najít v Prospektoru", "Find in Prospector")).on_hover_text(tr("převezme body vybrané ve výkrese", "takes the points selected in the drawing")).clicked() {
                self.vynutit_skok = true;
                spravce::posli(Akce::VyberZVykresu);
            }
            return;
        }
        ui.label(RichText::new(pocet_bodu_text(n)).strong());
        if ui.button(tr("Ukázat", "Show")).on_hover_text(tr("přiblíží výkres na vybrané body", "zooms the drawing to the selected points")).clicked() {
            ukazat_body(stav, &vybrane);
        }
        ui.menu_button(tr("Výška ⏷", "Elevation ⏷"), |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.hrom_vyska).hint_text(tr("např. 0.15", "e.g. 0.15")).desired_width(80.0));
                ui.label("m");
            });
            let z = self.hrom_vyska.trim().replace(',', ".").parse::<f64>().ok().filter(|z| z.is_finite());
            ui.horizontal(|ui| {
                if ui.add_enabled(z.is_some(), egui::Button::new(tr("Nastavit", "Set"))).on_hover_text(tr("všem vybraným bodům tuto výšku", "set this elevation for all selected points")).clicked() {
                    spravce::posli(Akce::BodyHromadne(vybrane.clone(), z.map(|z| (z, false)), None));
                    ui.close();
                }
                if ui
                    .add_enabled(z.is_some(), egui::Button::new(tr("Přičíst", "Add")))
                    .on_hover_text(tr("k výšce vybraných bodů přičíst (záporné = odečíst)", "add to the elevation of selected points (negative = subtract)"))
                    .clicked()
                {
                    spravce::posli(Akce::BodyHromadne(vybrane.clone(), z.map(|z| (z, true)), None));
                    ui.close();
                }
            });
        });
        ui.menu_button(tr("Kód ⏷", "Code ⏷"), |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.hrom_kod).hint_text(tr("nový kód", "new code")).desired_width(110.0));
            if ui.button(tr("Nastavit kód", "Set code")).on_hover_text(tr("všem vybraným bodům tento kód (popis)", "set this code (description) for all selected points")).clicked() {
                spravce::posli(Akce::BodyHromadne(vybrane.clone(), None, Some(self.hrom_kod.trim().to_string())));
                ui.close();
            }
        });
        ui.menu_button(tr("Styl ⏷", "Style ⏷"), |ui| {
            if ui.button(tr("podle skupiny / kódu", "by group / code")).clicked() {
                spravce::posli(Akce::StylBodu(vybrane.clone(), String::new()));
                ui.close();
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for st in &stav.styly_bodu {
                    if ui.button(&st.jmeno).clicked() {
                        spravce::posli(Akce::StylBodu(vybrane.clone(), st.id.clone()));
                        ui.close();
                    }
                }
            });
        });
        ui.menu_button(tr("Skupina ⏷", "Group ⏷"), |ui| {
            if ui.button(tr("Nová skupina z vybraných", "New group from selected")).clicked() {
                spravce::posli(Akce::SkupinaNova(vybrane.clone()));
                ui.close();
            }
            let skupiny: Vec<_> = stav.skupiny.iter().filter(|s| s.skupina.id != VSECHNY).collect();
            if !skupiny.is_empty() {
                ui.separator();
                ui.label(RichText::new(tr("Přidat do skupiny", "Add to group")).small().color(SEDA));
                for g in &skupiny {
                    if ui.button(&g.skupina.jmeno).clicked() {
                        spravce::posli(Akce::SkupinaBody(g.skupina.id.clone(), vybrane.clone(), true));
                        ui.close();
                    }
                }
                ui.menu_button(tr("Odebrat ze skupiny", "Remove from group"), |ui| {
                    for g in &skupiny {
                        if ui.button(&g.skupina.jmeno).clicked() {
                            spravce::posli(Akce::SkupinaBody(g.skupina.id.clone(), vybrane.clone(), false));
                            ui.close();
                        }
                    }
                });
            }
        });
        ui.menu_button(tr("Přečíslovat…", "Renumber…"), |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("od", "from"));
                ui.add(egui::DragValue::new(&mut self.precislovat_od).range(0..=u64::MAX / 2));
                ui.label(tr("krok", "step"));
                ui.add(egui::DragValue::new(&mut self.precislovat_krok).range(1..=1000));
            });
            if ui.button(tr("Přečíslovat", "Renumber")).on_hover_text(tr("v pořadí tabulky", "in table order")).clicked() {
                spravce::posli(Akce::BodyPrecislovat(vybrane.clone(), self.precislovat_od, self.precislovat_krok.max(1)));
                ui.close();
            }
        });
        let odsunute = stav.body.iter().any(|b| b.posun.is_some() && vybrane.contains(&b.handle));
        if odsunute
            && ui.button(tr("Popisy na místo", "Reset labels")).on_hover_text(tr("ručně odsunuté popisy vrátit na výchozí místo", "move manually dragged labels back to their default position")).clicked()
        {
            spravce::posli(Akce::PopisyNaMisto(vybrane.clone()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("×").on_hover_text(tr("Zrušit výběr", "Clear selection")).clicked() {
                self.vybrane_body.clear();
                self.nacti_edit_bodu(stav);
            }
            if ui.button(RichText::new(tr("Smazat", "Delete")).color(CERVENA)).clicked() {
                smazat(pocet_smazanych(n), Akce::BodySmazat(vybrane.clone()));
                self.vybrane_body.clear();
            }
        });
    }

    /// Tabulka bodů s řazením a výběrem (vykresluje jen viditelné řádky).
    fn tabulka_bodu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, body: &[&BodVykresu]) {
        let mut poradi: Vec<&BodVykresu> = body.to_vec();
        let cislo = |c: &str| c.trim().parse::<f64>().unwrap_or(f64::MAX);
        match self.razeni {
            Razeni::Poradi => {}
            Razeni::Cislo => poradi.sort_by(|a, b| cislo(&a.bod.cislo).total_cmp(&cislo(&b.bod.cislo)).then(a.bod.cislo.cmp(&b.bod.cislo))),
            Razeni::Y => poradi.sort_by(|a, b| b.bod.x.total_cmp(&a.bod.x)),
            Razeni::X => poradi.sort_by(|a, b| b.bod.y.total_cmp(&a.bod.y)),
            Razeni::Z => poradi.sort_by(|a, b| a.bod.z.total_cmp(&b.bod.z)),
            Razeni::Popis => poradi.sort_by(|a, b| a.bod.popis.to_lowercase().cmp(&b.bod.popis.to_lowercase())),
            Razeni::Zdroj => poradi.sort_by(|a, b| a.zdroj.cmp(&b.zdroj)),
            Razeni::Styl => {
                let st = |b: &BodVykresu| stav.styl_bodu.get(&b.handle).map(|x| x.0.clone()).unwrap_or_default();
                poradi.sort_by_key(|b| st(b))
            }
        }
        if self.sestupne {
            poradi.reverse();
        }
        let vsechny: [(Razeni, &str, f32); 8] = [
            (Razeni::Poradi, "", 26.0),
            (Razeni::Cislo, tr("Číslo", "Number"), 70.0),
            (Razeni::Popis, tr("Kód", "Code"), 110.0),
            (Razeni::Z, "Z", 70.0),
            (Razeni::Styl, tr("Styl", "Style"), 245.0),
            (Razeni::Y, "Y", 105.0),
            (Razeni::X, "X", 115.0),
            (Razeni::Zdroj, tr("Soubor", "File"), 120.0),
        ];
        let sloupce: Vec<(Razeni, &str, f32)> =
            vsechny.into_iter().filter(|(_, n, _)| !self.skryte_sloupce.iter().any(|s| crate::lang::tr_tab(s) == *n)).collect();
        // klávesy tabulky (jen když se nepíše do pole)
        if self.edit_bunka.is_none() && ui.memory(|m| m.focused().is_none()) && !poradi.is_empty() {
            let (del, esc, vse, dolu, nahoru, enter, shift) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Delete),
                    i.key_pressed(egui::Key::Escape),
                    i.modifiers.command && i.key_pressed(egui::Key::A),
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::Enter),
                    i.modifiers.shift,
                )
            });
            if vse {
                self.vybrane_body = poradi.iter().map(|b| b.handle).collect();
            }
            if esc {
                self.vybrane_body.clear();
            }
            if del {
                let h: Vec<Handle> = poradi.iter().map(|b| b.handle).filter(|h| self.vybrane_body.contains(h)).collect();
                if !h.is_empty() {
                    smazat(pocet_smazanych(h.len()), Akce::BodySmazat(h));
                    self.vybrane_body.clear();
                }
            }
            if dolu || nahoru {
                let i = match self.posledni_klik {
                    Some(i) if dolu => (i + 1).min(poradi.len() - 1),
                    Some(i) => i.saturating_sub(1),
                    None => 0,
                };
                if !shift {
                    self.vybrane_body.clear();
                }
                self.vybrane_body.insert(poradi[i].handle);
                self.posledni_klik = Some(i);
                self.posunout_na = Some(poradi[i].handle);
            }
            if enter {
                let h: Vec<Handle> = poradi.iter().map(|b| b.handle).filter(|h| self.vybrane_body.contains(h)).collect();
                if !h.is_empty() {
                    ukazat_body(stav, &h);
                }
            }
            if vse || esc || dolu || nahoru {
                let st = spravce::stav();
                self.nacti_edit_bodu(&st);
            }
        }
        // F2 = upravit první upravitelnou buňku vybraného bodu
        if self.edit_bunka.is_none()
            && self.vybrane_body.len() == 1
            && ui.memory(|m| m.focused().is_none())
            && ui.input(|i| i.key_pressed(egui::Key::F2))
        {
            let h = *self.vybrane_body.iter().next().unwrap();
            if let (Some(b), Some(sl)) = (poradi.iter().find(|b| b.handle == h), sloupce.iter().find(|c| upravitelny(c.0))) {
                self.edit_bunka = Some(EditBunky { h, sloupec: sl.0, text: text_bunky(&b.bod, sl.0), fokus: true });
            }
        }
        let vyska = 20.0;
        let font = FontId::proportional(13.0);
        let sirka: f32 = sloupce.iter().map(|c| c.2).sum();
        // záhlaví
        let (rect, _) = ui.allocate_exact_size(egui::vec2(sirka, vyska), Sense::hover());
        let mut x = rect.left();
        for &(r, nazev, w) in &sloupce {
            let bunka = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, vyska));
            let resp = ui.interact(bunka, ui.id().with(("hlavicka", nazev)), Sense::click());
            let resp = if upravitelny(r) {
                resp.on_hover_text(tr("klik = řadit; dvojklik do buňky (nebo F2) = upravit", "click = sort; double-click a cell (or F2) = edit"))
            } else if nazev.is_empty() {
                resp.on_hover_text(tr("značka bodu podle stylu", "point marker by style"))
            } else {
                resp.on_hover_text(tr("klik = řadit", "click = sort"))
            };
            let sipka = if self.razeni == r { if self.sestupne { " ⏷" } else { " ⏶" } } else { "" };
            let barva = if resp.hovered() { MODRA } else { ui.visuals().strong_text_color() };
            if matches!(r, Razeni::Z | Razeni::Y | Razeni::X) {
                ui.painter().text(bunka.right_center() - egui::vec2(14.0, 0.0), Align2::RIGHT_CENTER, format!("{nazev}{sipka}"), font.clone(), barva);
            } else {
                ui.painter().text(bunka.left_center() + egui::vec2(4.0, 0.0), Align2::LEFT_CENTER, format!("{nazev}{sipka}"), font.clone(), barva);
            }
            if resp.clicked() && !nazev.is_empty() {
                if self.razeni == r {
                    if self.sestupne {
                        self.razeni = Razeni::Poradi;
                        self.sestupne = false;
                    } else {
                        self.sestupne = true;
                    }
                } else {
                    self.razeni = r;
                    self.sestupne = false;
                }
            }
            x += w;
        }
        ui.painter().hline(rect.x_range(), rect.bottom(), ui.visuals().widgets.noninteractive.bg_stroke);
        let cil = self.posunout_na.take().and_then(|h| poradi.iter().position(|b| b.handle == h));
        let mut sa = egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("tabulka_bodu");
        if let Some(i) = cil {
            sa = sa.vertical_scroll_offset((i as f32 * (vyska + ui.spacing().item_spacing.y) - 60.0).max(0.0));
        }
        let mut klik: Option<(usize, egui::Modifiers, bool)> = None;
        // dokončená úprava buňky: (handle, sloupec, text, přejít na další řádek)
        let mut ulozit: Option<(Handle, Razeni, String, bool)> = None;
        let mut zrusit = false;
        let mut zacit: Option<(Handle, Razeni)> = None;
        let mut zobrazene = 0..0;
        sa.show_rows(ui, vyska, poradi.len(), |ui, rozsah| {
            zobrazene = rozsah.clone();
            for i in rozsah {
                let b = poradi[i];
                let (rect, _) = ui.allocate_exact_size(egui::vec2(sirka, vyska), Sense::hover());
                // stálé id řádku – kontextová nabídka přežije posun tabulky
                let resp = ui.interact(rect, egui::Id::new(("radek_bodu", b.handle)), Sense::click());
                let vybrano = self.vybrane_body.contains(&b.handle);
                if vybrano {
                    ui.painter().rect_filled(rect, 2.0, ui.visuals().selection.bg_fill);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
                } else if i % 2 == 1 {
                    ui.painter().rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                }
                let (id_stylu, puvod) =
                    stav.styl_bodu.get(&b.handle).cloned().unwrap_or((styly::STANDARD.into(), styly::Puvod::Standard));
                let styl = stav.styly_bodu.iter().find(|x| x.id == id_stylu);
                let mut x = rect.left();
                let barva = if vybrano { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
                for (razeni, _, w) in &sloupce {
                    let bunka = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*w - 4.0, vyska));
                    if resp.double_clicked() && upravitelny(*razeni) && resp.interact_pointer_pos().is_some_and(|p| bunka.contains(p)) {
                        zacit = Some((b.handle, *razeni));
                    }
                    if let Some(e) = self.edit_bunka.as_mut().filter(|e| e.h == b.handle && e.sloupec == *razeni) {
                        let r = ui.put(
                            bunka,
                            egui::TextEdit::singleline(&mut e.text).font(font.clone()).margin(egui::vec2(3.0, 1.0)),
                        );
                        if e.fokus {
                            r.request_focus();
                            e.fokus = false;
                        } else if r.lost_focus() {
                            let (esc, dal) = ui.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Tab)));
                            if esc {
                                zrusit = true;
                            } else {
                                ulozit = Some((e.h, e.sloupec, e.text.clone(), dal));
                            }
                        }
                        x += w;
                        continue;
                    }
                    let p = ui.painter().with_clip_rect(bunka);
                    let lc = bunka.left_center() + egui::vec2(4.0, 0.0);
                    match *razeni {
                        Razeni::Poradi => {
                            if let Some(st) = styl {
                                let c = bunka.center();
                                let s = egui::Stroke::new(1.3, barva_znacky(st));
                                if st.skryt {
                                    p.line_segment([c + egui::vec2(-6.0, 6.0), c + egui::vec2(6.0, -6.0)], egui::Stroke::new(1.3, CERVENA));
                                } else {
                                    kresli_znacku_stylu(&p, c, 6.5, st, s);
                                }
                            }
                        }
                        Razeni::Styl => {
                            let jmeno = styl.map_or(id_stylu.clone(), |s| s.jmeno.clone());
                            let g = p.layout_no_wrap(jmeno, font.clone(), barva);
                            let sirka_textu = g.size().x;
                            p.galley(lc - egui::vec2(0.0, g.size().y / 2.0), g, barva);
                            let (t, bp) = match &puvod {
                                styly::Puvod::Rucne => (tr("ručně", "manually"), Color32::from_rgb(217, 178, 106)),
                                styly::Puvod::Kod(_) => (tr("kód", "code"), Color32::from_rgb(143, 191, 168)),
                                styly::Puvod::Skupina(_) => (tr("skupina", "group"), MODRA),
                                styly::Puvod::Standard => ("", SEDA),
                            };
                            p.text(lc + egui::vec2(sirka_textu + 6.0, 0.0), Align2::LEFT_CENTER, t, FontId::proportional(11.0), bp);
                        }
                        n => {
                            let t = match n {
                                Razeni::Cislo => b.bod.cislo.clone(),
                                Razeni::Popis => b.bod.popis.clone(),
                                Razeni::Z => cz(b.bod.z, 2),
                                Razeni::Y => cz(-b.bod.x, 2),
                                Razeni::X => cz(-b.bod.y, 2),
                                _ => b.zdroj.clone(),
                            };
                            if matches!(n, Razeni::Z | Razeni::Y | Razeni::X) {
                                // čísla zarovnaná vpravo
                                p.text(bunka.right_center() - egui::vec2(10.0, 0.0), Align2::RIGHT_CENTER, t, font.clone(), barva);
                            } else {
                                p.text(lc, Align2::LEFT_CENTER, t, font.clone(), barva);
                            }
                        }
                    }
                    x += w;
                }
                if resp.clicked() || resp.double_clicked() {
                    klik = Some((i, ui.input(|x| x.modifiers), resp.double_clicked()));
                }
                if resp.secondary_clicked() && !vybrano {
                    klik = Some((i, egui::Modifiers::NONE, false));
                }
                let h: Vec<Handle> = if vybrano { self.vybrane_body.iter().copied().collect() } else { vec![b.handle] };
                resp.context_menu(|ui| Self::menu_bodu(ui, stav, &h));
            }
        });
        if let Some((i, m, dvojklik)) = klik {
            let h = poradi[i].handle;
            if m.shift {
                if let Some(od) = self.posledni_klik {
                    let (a, b) = (od.min(i), od.max(i));
                    for b in &poradi[a..=b.min(poradi.len() - 1)] {
                        self.vybrane_body.insert(b.handle);
                    }
                }
            } else if m.command || m.ctrl {
                if !self.vybrane_body.remove(&h) {
                    self.vybrane_body.insert(h);
                }
                self.posledni_klik = Some(i);
            } else {
                self.vybrane_body.clear();
                self.vybrane_body.insert(h);
                self.posledni_klik = Some(i);
            }
            let stav = spravce::stav();
            self.nacti_edit_bodu(&stav);
            if dvojklik && zacit.is_none() {
                ukazat_body(&stav, &[h]);
            }
        }
        if zrusit {
            self.edit_bunka = None;
        }
        if let Some((h, sloupec, text, dal)) = ulozit {
            self.edit_bunka = None;
            if let Some(bv) = stav.body.iter().find(|b| b.handle == h) {
                let mut novy = bv.bod.clone();
                let t = text.trim();
                let platne = match sloupec {
                    Razeni::Cislo => {
                        novy.cislo = t.to_string();
                        true
                    }
                    Razeni::Popis => {
                        novy.popis = t.to_string();
                        true
                    }
                    _ => match t.replace(',', ".").parse::<f64>() {
                        Ok(z) if z.is_finite() => {
                            novy.z = z;
                            true
                        }
                        _ => false,
                    },
                };
                if platne && novy != bv.bod {
                    spravce::posli(Akce::BodUpravit(h, novy));
                }
            }
            if dal {
                if let Some(i) = poradi.iter().position(|b| b.handle == h) {
                    if let Some(dalsi) = poradi.get(i + 1) {
                        self.edit_bunka =
                            Some(EditBunky { h: dalsi.handle, sloupec, text: text_bunky(&dalsi.bod, sloupec), fokus: true });
                        self.vybrane_body.clear();
                        self.vybrane_body.insert(dalsi.handle);
                        self.posledni_klik = Some(i + 1);
                        if i + 2 >= zobrazene.end {
                            self.posunout_na = Some(dalsi.handle);
                        }
                    }
                }
            }
        }
        if let Some((h, sloupec)) = zacit {
            if let Some(b) = poradi.iter().find(|b| b.handle == h) {
                self.edit_bunka = Some(EditBunky { h, sloupec, text: text_bunky(&b.bod, sloupec), fokus: true });
            }
        }
    }

    // ── Skupiny ──────────────────────────────────────────────────────────────

    fn detail_skupiny(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        karta::nadpis_karty(ui, &strom::Ikona::Svg(strom::ik!("ter_skupina_bodu")), tr("Skupiny bodů", "Point groups"), |ui| {
            karta::pilulka(ui, &stav.skupiny.len().to_string(), SEDA);
            napoveda::otaznik(ui, tr("Skupina bodů", "Point group"));
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if karta::tlacitko(ui, Some(strom::ik!("ter_skupina_bodu")), tr("+ Nová skupina", "+ New group"), true).clicked() {
                spravce::posli(Akce::SkupinaNova(Vec::new()));
            }
            info(
                ui,
                tr("Skupina vybírá body podle kódu, čísla (rozsahu) nebo souboru importu. Povrch se skládá ze skupin, \
                 takže změna skupiny povrch označí jako zastaralý (s automatickým přepočtem se přepočítá sám).", "A group selects points by code, number (range) or import file. A surface is built from groups, so a change of a group marks the surface as out of date (with automatic recalculation it recalculates itself)."),
            );
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_salt("seznam_skupin").auto_shrink([false, false]).show(ui, |ui| {
            for s in &stav.skupiny {
                let mut popis = s.skupina.kriteria();
                if !s.povrchy.is_empty() {
                    popis = trf!("{} · používá {}", "{} · used by {}", popis, s.povrchy.join(", "));
                }
                let r = karta::radek_seznamu(
                    ui,
                    &strom::Ikona::Svg(strom::ik!("ter_skupina_bodu")),
                    &s.skupina.jmeno,
                    &popis,
                    &trf!("{} bodů", "{} points", czi(s.pocet)),
                    SEDA,
                );
                if r.clicked() {
                    self.vyber_uzel(Uzel::Skupina(s.skupina.id.clone()));
                }
                r.context_menu(|ui| self.menu_skupiny(ui, stav, &s.skupina));
            }
        });
    }

    fn detail_skupina(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, id: &str) {
        let Some(prehled) = stav.skupiny.iter().find(|s| s.skupina.id == id).cloned() else {
            ui.label(RichText::new(tr("Skupina už ve výkrese není.", "The group is no longer in the drawing.")).color(SEDA));
            return;
        };
        if self.edit_skupina.as_ref().map(|s| s.id.as_str()) != Some(id) {
            self.edit_skupina = Some(prehled.skupina.clone());
            self.skupina_zmenena = false;
            self.skup_ukaz = [false; 5];
            self.skup_novy = Default::default();
            self.skup_bod = None;
        }
        let vestavena = id == VSECHNY;
        egui::ScrollArea::vertical().id_salt(tr("skupina", "group")).auto_shrink([false, false]).show(ui, |ui| {
            karta::nadpis_karty(ui, &strom::Ikona::Svg(strom::ik!("ter_skupina_bodu")), &prehled.skupina.jmeno, |ui| {
                if vestavena {
                    karta::pilulka(ui, tr("vestavěná", "built-in"), SEDA).on_hover_text(tr("skupina se všemi body výkresu – nedá se měnit ani smazat", "group with all points of the drawing – cannot be changed or deleted"));
                }
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                karta::kpi(ui, &czi(prehled.pocet), tr("bodů", "points"));
                let pouziva = if prehled.povrchy.is_empty() { "–".to_string() } else { prehled.povrchy.join(", ") };
                karta::kpi(ui, &pouziva, tr("používají povrchy", "used by surfaces"));
                let styl = if prehled.skupina.styl.is_empty() {
                    tr("podle kódu", "by code").to_string()
                } else {
                    stav.styly_bodu.iter().find(|x| x.id == prehled.skupina.styl).map_or(prehled.skupina.styl.clone(), |x| x.jmeno.clone())
                };
                karta::kpi(ui, &styl, tr("styl bodů", "point style"));
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Vybrat body ve výkrese", "Select points in the drawing")).clicked() {
                    let f = prehled.skupina.filtr();
                    let h: Vec<Handle> = stav.body.iter().filter(|b| f.obsahuje(&b.bod, &b.zdroj)).map(|b| b.handle).collect();
                    ukazat_body(stav, &h);
                }
                if karta::tlacitko(ui, Some(strom::ik!("ter_povrch_z_bodu")), tr("Nový povrch ze skupiny", "New surface from the group"), false).clicked() {
                    spravce::posli(Akce::PovrchZeSkupiny(id.to_string(), None, false));
                }
                if ui.button(tr("Zobrazit v tabulce bodů", "Show in the point table")).clicked() {
                    self.filtr_skupina = if vestavena { None } else { Some(id.to_string()) };
                    self.vyber_uzel(Uzel::Body);
                }
                ui.menu_button(tr("Další ⏷", "More ⏷"), |ui| self.menu_skupiny(ui, stav, &prehled.skupina));
            });
            ui.add_space(4.0);
            ui.separator();
            // ── záložky Pravidla · Body · Export ──
            let zal = [
                (0u8, tr("Pravidla", "Rules").to_string()),
                (1, trf!("Body ({})", "Points ({})", czi(prehled.pocet))),
                (2, tr("Export a nový povrch", "Export and new surface").to_string()),
            ];
            if let Some(z) = karta::zalozky(ui, self.zalozka_skupiny, &zal) {
                self.zalozka_skupiny = z;
            }
            match self.zalozka_skupiny {
                0 => {
                    let siroke = ui.available_width() >= 780.0;
                    let levy = |ui: &mut egui::Ui, me: &mut Self| {
                        if vestavena {
                            ui.label(RichText::new(tr("Vestavěná skupina se všemi body výkresu – nedá se měnit ani smazat.", "Built-in group with all points of the drawing – it cannot be changed or deleted.")).color(SEDA));
                        } else {
                            me.editor_skupiny(ui, stav);
                        }
                    };
                    if siroke {
                        ui.horizontal_top(|ui| {
                            let w = (ui.available_width() * 0.56).max(420.0);
                            ui.allocate_ui_with_layout(egui::vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.set_width(w);
                                levy(ui, self);
                            });
                            ui.add_space(16.0);
                            ui.vertical(|ui| self.nahled_skupiny(ui, stav, &prehled.skupina));
                        });
                    } else {
                        levy(ui, self);
                        ui.add_space(6.0);
                        self.nahled_skupiny(ui, stav, &prehled.skupina);
                    }
                }
                1 => {
                    let f = prehled.skupina.filtr();
                    let body: Vec<&BodVykresu> =
                        stav.body.iter().filter(|b| f.obsahuje(&b.bod, &b.zdroj) && bod_odpovida(b, &self.hledat)).collect();
                    drop(f);
                    let vyska = ui.available_height().max(320.0);
                    ui.allocate_ui(egui::vec2(ui.available_width(), vyska), |ui| self.tabulka_bodu(ui, stav, &body));
                }
                _ => {
                    ui.label(RichText::new(tr("Nový povrch ze skupiny", "New surface from the group")).strong());
                    ui.horizontal(|ui| {
                        ui.label(tr("Jméno", "Name"));
                        if self.nove_jmeno_povrchu.is_empty() {
                            self.nove_jmeno_povrchu = trf!("Povrch {}", "Surface {}", prehled.skupina.jmeno);
                        }
                        ui.add(egui::TextEdit::singleline(&mut self.nove_jmeno_povrchu).desired_width(180.0));
                        if karta::tlacitko(ui, Some(strom::ik!("ter_povrch_z_bodu")), tr("Vytvořit povrch", "Create surface"), false).clicked() {
                            spravce::posli(Akce::PovrchZeSkupiny(id.to_string(), Some(self.nove_jmeno_povrchu.trim().to_string()), false));
                            self.nove_jmeno_povrchu.clear();
                        }
                    });
                    ui.add_space(8.0);
                    ui.separator();
                    self.export(ui, stav, &prehled.skupina);
                    if !vestavena {
                        ui.add_space(12.0);
                        ui.separator();
                        if ui.button(RichText::new(tr("Smazat skupinu", "Delete group")).color(CERVENA)).on_hover_text(tr("body ve výkrese zůstanou", "the points stay in the drawing")).clicked() {
                            smazat(trf!("Skupina „{}“ je smazaná (body ve výkrese zůstaly).", "Group \"{}\" was deleted (the points stay in the drawing).", prehled.skupina.jmeno), Akce::SkupinaSmazat(id.to_string()));
                        }
                    }
                }
            }
        });
    }

    fn editor_skupiny(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let Some(mut s) = self.edit_skupina.clone() else { return };
        let puvodni = s.clone();
        egui::Grid::new("editor_skupiny").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Jméno", "Name"));
            ui.add(egui::TextEdit::singleline(&mut s.jmeno).desired_width(260.0));
            ui.end_row();
            ui.label(tr("Popis", "Description"));
            ui.add(egui::TextEdit::singleline(&mut s.popis).desired_width(260.0));
            ui.end_row();
            ui.label(tr("Styl bodů", "Point style"));
            let jmeno = if s.styl.is_empty() {
                tr("výchozí (podle kódu, jinak Standard)", "default (by code, otherwise Standard)").to_string()
            } else {
                stav.styly_bodu.iter().find(|x| x.id == s.styl).map_or(s.styl.clone(), |x| x.jmeno.clone())
            };
            egui::ComboBox::from_id_salt("styl_skupiny").selected_text(jmeno).width(260.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.styl, String::new(), tr("výchozí (podle kódu, jinak Standard)", "default (by code, otherwise Standard)"));
                for st in &stav.styly_bodu {
                    ui.selectable_value(&mut s.styl, st.id.clone(), &st.jmeno);
                }
            });
            ui.end_row();
        });

        // počty bodů pro jednotlivá pravidla
        let f = s.filtr();
        let mut pocty = [0usize; 5];
        for b in &stav.body {
            let p = f.pravidla(&b.bod, &b.zdroj);
            for i in 0..3 {
                if p[i] == Some(true) {
                    pocty[i] += 1;
                }
            }
        }
        let bez_cisel = SeznamCisel::z_textu(&s.bez_cisel);
        pocty[3] = stav.body.iter().filter(|b| s.bez_kodu.iter().any(|k| k.eq_ignore_ascii_case(b.bod.kod()))).count();
        pocty[4] = stav.body.iter().filter(|b| bez_cisel.obsahuje(&b.bod.cislo)).count();
        let rucnich = stav.body.iter().filter(|b| b.zdroj == teren::skupiny::RUCNE).count();
        drop(f);

        // ── Zahrnout ──
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("ZAHRNOUT", "INCLUDE")).small().strong().color(SEDA));
            let mut r = if s.vse { 2 } else if s.vsechna { 1 } else { 0 };
            let texty = [tr("body, které splňují aspoň jedno pravidlo", "points matching at least one rule"), tr("body, které splňují všechna pravidla", "points matching all rules"), tr("všechny body výkresu", "all points of the drawing")];
            egui::ComboBox::from_id_salt("rezim_skupiny").selected_text(texty[r]).width(280.0).show_ui(ui, |ui| {
                for (i, t) in texty.iter().enumerate() {
                    ui.selectable_value(&mut r, i, *t);
                }
            });
            s.vse = r == 2;
            s.vsechna = r == 1;
        });
        let ukaz = &mut self.skup_ukaz;
        let novy = &mut self.skup_novy;
        if !s.vse {
            if !s.kody.is_empty() || ukaz[0] {
                if radek_pravidla(ui, tr("mají kód", "have code"), Some(pocty[0]), |ui| cipy_kodu(ui, &mut s.kody, &mut novy[0], tr("kód", "code"))) {
                    s.kody.clear();
                    ukaz[0] = false;
                }
            }
            if !s.cisla.trim().is_empty() || ukaz[1] {
                if radek_pravidla(ui, tr("mají číslo", "have number"), Some(pocty[1]), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.cisla).hint_text(tr("např. 1-50, 101, 205-210", "e.g. 1-50, 101, 205-210")).desired_width(220.0));
                }) {
                    s.cisla.clear();
                    ukaz[1] = false;
                }
            }
            if !s.zdroje.is_empty() || ukaz[2] {
                if radek_pravidla(ui, tr("jsou ze souboru", "are from file"), Some(pocty[2]), |ui| {
                    let mut pryc = None;
                    for (i, z) in s.zdroje.iter().enumerate() {
                        if cip(ui, z).clicked() {
                            pryc = Some(i);
                        }
                    }
                    if let Some(i) = pryc {
                        s.zdroje.remove(i);
                    }
                    let volne: Vec<&String> =
                        stav.zdroje.iter().filter(|z| !s.zdroje.iter().any(|x| x.eq_ignore_ascii_case(z))).collect();
                    if !volne.is_empty() {
                        ui.menu_button(tr("+ soubor", "+ file"), |ui| {
                            for z in volne {
                                if ui.button(z).clicked() {
                                    s.zdroje.push(z.clone());
                                    ui.close();
                                }
                            }
                        });
                    } else if s.zdroje.is_empty() {
                        ui.label(RichText::new(tr("žádné importované body", "no imported points")).color(SEDA));
                    }
                }) {
                    s.zdroje.clear();
                    ukaz[2] = false;
                }
            }
            let mut skryte: Vec<(usize, &str)> = Vec::new();
            if s.kody.is_empty() && !ukaz[0] {
                skryte.push((0, tr("mají kód", "have code")));
            }
            if s.cisla.trim().is_empty() && !ukaz[1] {
                skryte.push((1, tr("mají číslo", "have number")));
            }
            if s.zdroje.is_empty() && !ukaz[2] {
                skryte.push((2, tr("jsou ze souboru", "are from file")));
            }
            if s.kody.is_empty() && s.cisla.trim().is_empty() && s.zdroje.is_empty() && !ukaz.iter().take(3).any(|x| *x) {
                ui.label(RichText::new(tr("Zatím žádné pravidlo – skupina je prázdná (kromě ručně přidaných bodů).", "No rule yet – the group is empty (except manually added points).")).color(ORANZOVA));
            }
            if !skryte.is_empty() {
                ui.menu_button(tr("+ pravidlo", "+ rule"), |ui| {
                    for (i, t) in skryte {
                        if ui.button(t).clicked() {
                            ukaz[i] = true;
                            ui.close();
                        }
                    }
                });
            }
        } else {
            ui.label(RichText::new(tr("Skupina obsahuje každý bod výkresu – uplatní se jen vyloučení a ruční úpravy.", "The group contains every point of the drawing – only exclusions and manual changes apply.")).color(SEDA));
        }

        // ── Vyloučit ──
        ui.add_space(8.0);
        ui.label(RichText::new(tr("VŽDY VYLOUČIT BODY, KTERÉ", "ALWAYS EXCLUDE POINTS THAT")).small().strong().color(SEDA));
        if !s.bez_kodu.is_empty() || ukaz[3] {
            if radek_pravidla(ui, tr("mají kód", "have code"), Some(pocty[3]), |ui| cipy_kodu(ui, &mut s.bez_kodu, &mut novy[1], tr("kód", "code"))) {
                s.bez_kodu.clear();
                ukaz[3] = false;
            }
        }
        if !s.bez_cisel.trim().is_empty() || ukaz[4] {
            if radek_pravidla(ui, tr("mají číslo", "have number"), Some(pocty[4]), |ui| {
                ui.add(egui::TextEdit::singleline(&mut s.bez_cisel).hint_text(tr("např. 33, 40-45", "e.g. 33, 40-45")).desired_width(220.0));
            }) {
                s.bez_cisel.clear();
                ukaz[4] = false;
            }
        }
        if s.bez_rucnich {
            if radek_pravidla(ui, tr("jsou vytvořené ručně", "were created manually"), Some(rucnich), |ui| {
                ui.label(RichText::new(tr("(ne z importu)", "(not from an import)")).color(SEDA));
            }) {
                s.bez_rucnich = false;
            }
        }
        let mut skryte: Vec<(usize, &str)> = Vec::new();
        if s.bez_kodu.is_empty() && !ukaz[3] {
            skryte.push((3, tr("mají kód", "have code")));
        }
        if s.bez_cisel.trim().is_empty() && !ukaz[4] {
            skryte.push((4, tr("mají číslo", "have number")));
        }
        if !s.bez_rucnich {
            skryte.push((5, tr("jsou vytvořené ručně", "were created manually")));
        }
        if !skryte.is_empty() {
            ui.menu_button(tr("+ pravidlo", "+ rule"), |ui| {
                for (i, t) in skryte {
                    if ui.button(t).clicked() {
                        if i == 5 {
                            s.bez_rucnich = true;
                        } else {
                            ukaz[i] = true;
                        }
                        ui.close();
                    }
                }
            });
        }

        // ── Ručně ──
        ui.add_space(8.0);
        ui.label(RichText::new(tr("RUČNĚ", "MANUALLY")).small().strong().color(SEDA));
        let vybrana: Vec<String> = stav
            .body
            .iter()
            .filter(|b| stav.vyber_body.contains(&b.handle))
            .map(|b| b.bod.cislo.clone())
            .filter(|c| !c.trim().is_empty())
            .collect();
        for pridat in [true, false] {
            ui.horizontal_wrapped(|ui| {
                popisek_sloupce(ui, if pridat { tr("přidané body", "added points") } else { tr("odebrané body", "removed points") });
                let (cil, druhy, vstup) = if pridat {
                    (&mut s.pridane, &mut s.odebrane, &mut novy[2])
                } else {
                    (&mut s.odebrane, &mut s.pridane, &mut novy[3])
                };
                let mut sez = SeznamCisel::z_textu(cil);
                let text = sez.text();
                let casti: Vec<&str> = text.split(", ").filter(|x| !x.is_empty()).collect();
                if casti.len() > 30 {
                    ui.add(egui::TextEdit::multiline(cil).desired_rows(2).desired_width(260.0));
                } else {
                    let mut pryc = None;
                    for c in &casti {
                        if cip(ui, c).clicked() {
                            pryc = Some(c.to_string());
                        }
                    }
                    if let Some(c) = pryc {
                        *cil = casti.iter().filter(|x| **x != c).copied().collect::<Vec<_>>().join(", ");
                    }
                    let r = ui.add(egui::TextEdit::singleline(vstup).hint_text(tr("+ číslo", "+ number")).desired_width(60.0));
                    if r.lost_focus() && !vstup.trim().is_empty() {
                        let nova = SeznamCisel::z_textu(vstup);
                        *cil = format!("{cil}, {}", nova.text());
                        let mut d = SeznamCisel::z_textu(druhy);
                        d.odeber(&nova.text().split(", ").map(String::from).collect::<Vec<_>>());
                        *druhy = d.text();
                        vstup.clear();
                    }
                }
                let tl = ui
                    .add_enabled(!vybrana.is_empty(), egui::Button::new(tr("+ z výběru ve výkrese", "+ from the drawing selection")))
                    .on_hover_text(tr("vyberte body ve výkrese a klikněte", "select points in the drawing and click"))
                    .on_disabled_hover_text(tr("ve výkrese nejsou vybrané žádné body", "no points are selected in the drawing"));
                if tl.clicked() {
                    sez.pridej(&vybrana);
                    *cil = sez.text();
                    let mut d = SeznamCisel::z_textu(druhy);
                    d.odeber(&vybrana);
                    *druhy = d.text();
                }
            });
        }
        ui.label(RichText::new(tr("Ručně přidané a odebrané body mají přednost před pravidly.", "Manually added and removed points take precedence over the rules.")).small().color(SEDA));

        let zmena = s != puvodni;
        if zmena {
            self.skupina_zmenena = true;
        }
        let ulozit_hned = self.skupina_zmenena && ulozit_ted(ui, &mut self.zmena_skupiny, zmena);
        let f = s.filtr();
        let n = stav.body.iter().filter(|b| f.obsahuje(&b.bod, &b.zdroj)).count();
        drop(f);
        let dnes = stav.skupiny.iter().find(|x| x.skupina.id == s.id).map_or(0, |x| x.pocet);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ulozit_hned {
                let mut s = s.clone();
                for t in [&mut s.cisla, &mut s.bez_cisel, &mut s.pridane, &mut s.odebrane] {
                    *t = SeznamCisel::z_textu(t).text();
                }
                hlaska(trf!("Skupina „{}“ uložena.", "Group \"{}\" saved.", s.jmeno));
                spravce::posli(Akce::SkupinaUlozit(s));
                self.skupina_zmenena = false;
            }
            if self.skupina_zmenena {
                ui.label(RichText::new(tr("ukládám…", "saving…")).color(SEDA)).on_hover_text(tr("změna se uloží sama, jakmile dopíšete", "the change is saved automatically when you finish typing"));
                if ui.button(tr("Vrátit změny", "Revert changes")).clicked() {
                    self.edit_skupina = None;
                    self.skupina_zmenena = false;
                    self.skup_ukaz = [false; 5];
                    return;
                }
                ui.label(RichText::new(trf!("po uložení {} bodů (dnes {})", "after saving {} points (now {})", n, dnes)).color(ORANZOVA));
            }
        });
        if self.edit_skupina.is_some() {
            self.edit_skupina = Some(s);
        }
    }

    /// Mapka bodů výkresu obarvená podle členství ve (rozpracované) skupině.
    fn nahled_skupiny(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, ulozena: &SkupinaBodu) {
        use teren::skupiny::Clenstvi;
        let s = self.edit_skupina.clone().filter(|x| x.id == ulozena.id).unwrap_or_else(|| ulozena.clone());
        let f = s.filtr();
        let clen: Vec<Clenstvi> = stav.body.iter().map(|b| f.clenstvi(&b.bod, &b.zdroj)).collect();
        let barva = |c: Clenstvi| match c {
            Clenstvi::Pravidla => ZELENA,
            Clenstvi::Pridany => MODRA,
            Clenstvi::Vylouceny => ORANZOVA,
            Clenstvi::Odebrany => CERVENA,
            Clenstvi::Mimo => Color32::from_gray(85),
        };
        let ve = clen.iter().filter(|c| c.je_ve_skupine()).count();
        ui.label(RichText::new(trf!("{} bodů ve skupině z {}", "{} points in the group out of {}", ve, stav.body.len())).strong());
        let w = ui.available_width().clamp(200.0, 420.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, w * 0.72), egui::Sense::click());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 4.0, Color32::from_gray(22));
        if !stav.body.is_empty() {
            let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for b in &stav.body {
                x0 = x0.min(b.bod.x);
                x1 = x1.max(b.bod.x);
                y0 = y0.min(b.bod.y);
                y1 = y1.max(b.bod.y);
            }
            let vnitrek = rect.shrink(10.0);
            let m = ((vnitrek.width() as f64) / (x1 - x0).max(1e-6)).min((vnitrek.height() as f64) / (y1 - y0).max(1e-6));
            let cx = (x0 + x1) / 2.0;
            let cy = (y0 + y1) / 2.0;
            let na = |x: f64, y: f64| {
                egui::pos2(
                    vnitrek.center().x + ((x - cx) * m) as f32,
                    vnitrek.center().y - ((y - cy) * m) as f32,
                )
            };
            // nejdřív body mimo skupinu, ať barevné nejsou překryté
            for kolo in [false, true] {
                for (b, c) in stav.body.iter().zip(&clen) {
                    if (*c != Clenstvi::Mimo) == kolo {
                        p.circle_filled(na(b.bod.x, b.bod.y), if kolo { 2.6 } else { 1.8 }, barva(*c));
                    }
                }
            }
            if let Some(h) = self.skup_bod {
                if let Some(b) = stav.body.iter().find(|b| b.handle == h) {
                    p.circle_stroke(na(b.bod.x, b.bod.y), 6.0, egui::Stroke::new(1.5, Color32::WHITE));
                }
            }
            if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    self.skup_bod = stav
                        .body
                        .iter()
                        .map(|b| (b.handle, na(b.bod.x, b.bod.y).distance(pos)))
                        .filter(|(_, d)| *d < 10.0)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(h, _)| h);
                }
            }
        }
        // legenda
        let pocet = |c: Clenstvi| clen.iter().filter(|x| **x == c).count();
        let mut vyl = String::new();
        if !s.bez_kodu.is_empty() {
            vyl = trf!(" (kód {})", " (code {})", s.bez_kodu.join(", "));
        }
        let polozky = [
            (Clenstvi::Pravidla, tr("podle pravidel", "by rules").to_string()),
            (Clenstvi::Pridany, tr("přidané ručně", "added manually").to_string()),
            (Clenstvi::Vylouceny, trf!("vyloučené pravidlem{}", "excluded by a rule{}", vyl)),
            (Clenstvi::Odebrany, tr("odebrané ručně", "removed manually").to_string()),
            (Clenstvi::Mimo, tr("nesplňují pravidla", "do not match the rules").to_string()),
        ];
        egui::Grid::new("legenda_skupiny").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
            for (c, t) in polozky {
                let n = pocet(c);
                if n == 0 && !matches!(c, Clenstvi::Pravidla | Clenstvi::Mimo) {
                    continue;
                }
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, barva(c));
                ui.label(t);
                ui.label(RichText::new(n.to_string()).color(SEDA));
                ui.end_row();
            }
        });
        match self.skup_bod.and_then(|h| stav.body.iter().position(|b| b.handle == h)) {
            Some(i) => {
                let b = &stav.body[i];
                let proc = match clen[i] {
                    Clenstvi::Pravidla => {
                        let pr = f.pravidla(&b.bod, &b.zdroj);
                        let mut d = Vec::new();
                        if s.vse {
                            d.push(tr("všechny body", "all points").to_string());
                        }
                        if pr[0] == Some(true) {
                            d.push(trf!("kód {}", "code {}", b.bod.kod()));
                        }
                        if pr[1] == Some(true) {
                            d.push(tr("číslo v seznamu", "number in the list").into());
                        }
                        if pr[2] == Some(true) {
                            d.push(trf!("soubor {}", "file {}", b.zdroj));
                        }
                        trf!("patří – splňuje: {}", "belongs – matches: {}", d.join(", "))
                    }
                    Clenstvi::Pridany => tr("patří – přidaný ručně", "belongs – added manually").into(),
                    Clenstvi::Odebrany => tr("nepatří – odebraný ručně", "does not belong – removed manually").into(),
                    Clenstvi::Vylouceny => tr("nepatří – vyloučený pravidlem", "does not belong – excluded by a rule").into(),
                    Clenstvi::Mimo => tr("nepatří – nesplňuje pravidla zahrnutí", "does not belong – does not match the inclusion rules").into(),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(trf!("Bod {}", "Point {}", b.bod.cislo)).strong());
                    if !b.bod.kod().is_empty() {
                        ui.label(RichText::new(trf!("kód {}", "code {}", b.bod.kod())).color(SEDA));
                    }
                    ui.label(RichText::new(proc).color(barva(clen[i])));
                });
            }
            None => {
                ui.label(RichText::new(tr("Klik na bod v náhledu ukáže, proč ve skupině je / není.", "Clicking a point in the preview shows why it is / is not in the group.")).small().color(SEDA));
            }
        }
    }

    fn export(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, s: &SkupinaBodu) {
        ui.label(RichText::new(tr("Export bodů skupiny", "Export the group's points")).strong());
        let e = &mut self.export;
        if e.cesta.is_empty() {
            let soubor = format!("{}.txt", crate::cogo::bez_diakritiky(&s.jmeno).to_lowercase());
            let slozka = if stav.slozka.is_empty() {
                // neuložený výkres – Dokumenty uživatele
                std::env::var_os("USERPROFILE")
                    .or_else(|| std::env::var_os("HOME"))
                    .map(|h| {
                        let d = std::path::PathBuf::from(h).join("Documents");
                        if d.is_dir() { d } else { d.parent().map(|p| p.to_path_buf()).unwrap_or(d) }
                    })
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            } else {
                stav.slozka.clone()
            };
            e.cesta = if slozka.is_empty() { soubor } else { std::path::Path::new(&slozka).join(soubor).display().to_string() };
        }
        ui.horizontal(|ui| {
            ui.label(tr("Formát", "Format"));
            egui::ComboBox::from_id_salt("exp_format").selected_text(&e.format).width(140.0).show_ui(ui, |ui| {
                for (f, t) in [
                    ("PYXZD", tr("PYXZD – S-JTSK kladné", "PYXZD – S-JTSK positive")),
                    ("PNEZD", "PNEZD"),
                    ("PENZD", "PENZD"),
                    ("PYXZ", tr("PYXZ – bez popisu", "PYXZ – no description")),
                    ("PNEZ", tr("PNEZ – bez popisu", "PNEZ – no description")),
                ] {
                    ui.selectable_value(&mut e.format, f.to_string(), t);
                }
            });
            ui.label(tr("Oddělovač", "Delimiter"));
            let nazvy = [
                (Oddelovac::Mezera, "mezera"),
                (Oddelovac::Carka, tr("čárka", "comma")),
                (Oddelovac::Strednik, tr("středník", "semicolon")),
                (Oddelovac::Tabulator, tr("tabulátor", "tab")),
            ];
            let akt = nazvy.iter().find(|n| n.0 == e.oddelovac).map_or("mezera", |n| n.1);
            egui::ComboBox::from_id_salt("exp_odd").selected_text(akt).width(90.0).show_ui(ui, |ui| {
                for (o, t) in nazvy {
                    ui.selectable_value(&mut e.oddelovac, o, t);
                }
            });
            ui.checkbox(&mut e.cp1250, "Windows-1250").on_hover_text(tr("kódování pro starší programy; jinak UTF-8", "encoding for older programs; otherwise UTF-8"));
        });
        ui.horizontal(|ui| {
            ui.label(tr("Soubor", "File"));
            ui.add(egui::TextEdit::singleline(&mut e.cesta).desired_width(330.0));
            if ui.button(tr("Exportovat", "Export")).clicked() {
                spravce::posli(Akce::Export(s.id.clone(), e.cesta.clone(), e.format.clone(), e.oddelovac, e.cp1250));
            }
        });
    }

    // ── Povrchy ──────────────────────────────────────────────────────────────

    fn export_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, p: &PrehledPovrchu) {
        let e = &mut self.export_povrchu;
        let pripona = |dxf: bool| if dxf { "dxf" } else { "xml" };
        if e.0 != p.id || e.1.is_empty() {
            let soubor = format!("{}.{}", crate::cogo::bez_diakritiky(&p.jmeno).to_lowercase().replace(' ', "_"), pripona(e.2));
            let slozka = if stav.slozka.is_empty() {
                std::env::var_os("USERPROFILE")
                    .or_else(|| std::env::var_os("HOME"))
                    .map(|h| {
                        let d = std::path::PathBuf::from(h).join("Documents");
                        if d.is_dir() { d } else { d.parent().map(|p| p.to_path_buf()).unwrap_or(d) }
                    })
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            } else {
                stav.slozka.clone()
            };
            e.0 = p.id.clone();
            e.1 = if slozka.is_empty() { soubor } else { std::path::Path::new(&slozka).join(soubor).display().to_string() };
        }
        ui.label(RichText::new(tr("Export povrchu", "Surface export")).strong());
        ui.horizontal(|ui| {
            ui.label(tr("Formát", "Format"));
            let pred = e.2;
            ui.radio_value(&mut e.2, false, "LandXML").on_hover_text(
                tr("Výměnný formát povrchů – přesně tytéž trojúhelníky, body zaměření a obrys. Načte ho většina projekčních programů.", "Surface exchange format – exactly the same triangles, survey points and outline. Most design programs can read it."),
            );
            ui.radio_value(&mut e.2, true, tr("DXF – 3D plochy", "DXF – 3D faces")).on_hover_text(
                tr("Každý trojúhelník jako 3D plocha (3DFACE) – otevře ho každý CAD; povrch se z ploch dá znovu sestavit.", "Each triangle as a 3D face (3DFACE) – any CAD opens it; the surface can be rebuilt from the faces."),
            );
            if pred != e.2 {
                if let Some(t) = e.1.rsplit_once('.').map(|(a, _)| a.to_string()) {
                    e.1 = format!("{t}.{}", pripona(e.2));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr("Soubor", "File"));
            ui.add(egui::TextEdit::singleline(&mut e.1).desired_width(330.0));
            if karta::tlacitko(ui, Some(strom::ik!("ter_export_povrchu")), tr("Exportovat", "Export"), true).clicked() {
                spravce::posli(Akce::ExportPovrchu(p.id.clone(), e.1.clone(), !e.2));
            }
        });
        if p.zastaraly {
            ui.label(RichText::new(tr("Povrch je zastaralý – export se spočítá z aktuálních bodů a spojnic (jako po přepočtu).", "The surface is out of date – the export is computed from the current points and breaklines (as after a recalculation).")).small().color(ORANZOVA));
        }
    }

    fn detail_povrchy(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        karta::nadpis_karty(ui, &strom::Ikona::Svg(strom::ik!("ter_povrch_novy")), tr("Povrchy", "Surfaces"), |ui| {
            karta::pilulka(ui, &stav.povrchy.len().to_string(), SEDA);
        });
        ui.add_space(4.0);
        if stav.povrchy.is_empty() {
            self.prazdne_povrchy(ui, stav);
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if karta::tlacitko(ui, Some(strom::ik!("ter_povrch_novy")), tr("Nový povrch – průvodce…", "New surface – wizard…"), true).clicked() {
                self.pruvodce = Some(pruvodce::Pruvodce::novy(stav));
            }
            if karta::tlacitko(ui, Some(strom::ik!("ter_prepocitat")), tr("Přepočítat všechny", "Recalculate all"), false).clicked() {
                spravce::posli(Akce::Prepocitat(stav.povrchy.iter().map(|p| p.id.clone()).collect()));
            }
            zaskrtnuti_nevybirat(ui);
        });
        ui.add_space(4.0);
        self.vybrane_povrchy.retain(|id| stav.povrchy.iter().any(|p| &p.id == id));
        let vybrane: Vec<String> = stav.povrchy.iter().map(|p| p.id.clone()).filter(|id| self.vybrane_povrchy.contains(id)).collect();

        // lišta hromadných úprav – až po výběru (pevná výška, nic neposkakuje)
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::hover());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        if vybrane.is_empty() {
            child.label(RichText::new(tr("Zaškrtnutím vyberete víc povrchů pro hromadné úpravy", "Tick surfaces to edit several at once")).color(SEDA));
        } else {
            ui.painter().rect_filled(rect, 5.0, ui.visuals().selection.bg_fill.gamma_multiply(0.45));
            let ui = &mut child;
            ui.label(RichText::new(if vybrane.len() == 1 { tr("1 povrch", "1 surface").to_string() } else { trf!("{} povrchy", "{} surfaces", vybrane.len()) }).strong());
            if ui.button(tr("Přepočítat", "Recalculate")).clicked() {
                spravce::posli(Akce::Prepocitat(vybrane.clone()));
            }
            ui.menu_button(tr("Styl ⏷", "Style ⏷"), |ui| {
                for s in &stav.styly_povrchu {
                    if ui.button(&s.jmeno).clicked() {
                        spravce::posli(Akce::Nastav(vybrane.clone(), Zmena::Styl(s.id.clone())));
                        ui.close();
                    }
                }
            });
            ui.menu_button(tr("Vrstevnice ⏷", "Contours ⏷"), |ui| {
                for (i, h) in INTERVALY {
                    if ui.button(interval_text(i, h)).clicked() {
                        spravce::posli(Akce::Nastav(vybrane.clone(), Zmena::Interval(i, h)));
                        ui.close();
                    }
                }
            });
            ui.menu_button(tr("Přepočet ⏷", "Recalculation ⏷"), |ui| {
                if ui.button(tr("Automaticky přepočítávat", "Recalculate automatically")).clicked() {
                    spravce::posli(Akce::Nastav(vybrane.clone(), Zmena::Auto(true)));
                    ui.close();
                }
                if ui.button(tr("Ručně přepočítávat", "Recalculate manually")).clicked() {
                    spravce::posli(Akce::Nastav(vybrane.clone(), Zmena::Auto(false)));
                    ui.close();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text(tr("Zrušit výběr", "Clear selection")).clicked() {
                    self.vybrane_povrchy.clear();
                }
            });
        }
        ui.add_space(4.0);

        let hledat = self.hledat.clone();
        egui::ScrollArea::vertical().id_salt("seznam_povrchu").auto_shrink([false, false]).show(ui, |ui| {
            for p in stav.povrchy.iter().filter(|p| shoda(&p.jmeno, &hledat)) {
                ui.horizontal(|ui| {
                    let mut v = self.vybrane_povrchy.contains(&p.id);
                    if ui.checkbox(&mut v, "").on_hover_text(tr("vybrat pro hromadné úpravy", "select for bulk edits")).changed() {
                        if v {
                            self.vybrane_povrchy.insert(p.id.clone());
                        } else {
                            self.vybrane_povrchy.remove(&p.id);
                        }
                    }
                    let popis = trf!(
                        "{} · {} troj. · {} – {} m{}", "{} · {} triangles · {} – {} m{}",
                        nazev_stylu_povrchu(stav, &p.styl),
                        czi(p.stat.trojuhelniku),
                        cz(p.stat.z_min, 2),
                        cz(p.stat.z_max, 2),
                        if p.auto { tr(" · přepočet automaticky", " · recalculates automatically") } else { "" }
                    );
                    let (stav_t, barva) = if p.zastaraly { (tr("● zastaralý", "● out of date"), ORANZOVA) } else { (tr("● aktuální", "● up to date"), ZELENA) };
                    let jmeno = if p.aktivni { format!("★ {}", p.jmeno) } else { p.jmeno.clone() };
                    let r = karta::radek_seznamu(ui, &strom::Ikona::Svg(strom::ik!("ter_vrstevnice")), &jmeno, &popis, stav_t.trim_start_matches("● "), barva);
                    if r.clicked() {
                        if ui.input(|i| i.modifiers.command) {
                            if !self.vybrane_povrchy.remove(&p.id) {
                                self.vybrane_povrchy.insert(p.id.clone());
                            }
                        } else {
                            self.vyber_uzel(Uzel::Povrch(p.id.clone()));
                        }
                    }
                    if r.double_clicked() {
                        spravce::posli(Akce::Aktivovat(p.id.clone()));
                    }
                    r.context_menu(|ui| self.menu_povrchu(ui, stav, p));
                });
            }
        });
    }

    /// Prázdný stav: ve výkrese není povrch – poradí, co udělat.
    fn prazdne_povrchy(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let ma_body = !stav.body.is_empty();
        karta::prazdny_stav(
            ui,
            strom::ik!("ter_povrch_novy"),
            tr("Ve výkrese zatím není povrch", "There is no surface in the drawing yet"),
            &if ma_body {
                trf!("Body už máte ({}). Stávající terén se spočítá z nich (bez vyloučených kódů, např. stromů).", "You already have points ({}). The existing ground is computed from them (without excluded codes, e.g. trees).", czi(stav.body.len()))
            } else {
                tr("Nejdřív importujte body zaměření – povrch se z nich spočítá.", "Import the survey points first – the surface is computed from them.").to_string()
            },
            |ui| {
                if ma_body {
                    if karta::tlacitko(ui, Some(strom::ik!("ter_povrch_novy")), tr("Vytvořit stávající terén", "Create existing ground"), true).clicked() {
                        spravce::posli(Akce::NovyPovrchPas("Stavajici".into(), tr("Stávající terén", "Existing ground").into(), true));
                    }
                    if ui.button(tr("Průvodce…", "Wizard…")).clicked() {
                        self.pruvodce = Some(pruvodce::Pruvodce::novy(stav));
                    }
                } else if karta::tlacitko(ui, Some(strom::ik!("ter_body_import")), tr("Import bodů…", "Import points…"), true).clicked() {
                    self.nahled_importu = Some(import_nahled::NahledImportu::novy(""));
                }
            },
        );
    }

    /// Styl, vrstevnice, max. hrana a vyloučené kódy povrchu.
    fn nastaveni_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, p: &PrehledPovrchu) {
        let id = p.id.clone();
        let nastav = |z: Zmena| spravce::posli(Akce::Nastav(vec![id.clone()], z));
        egui::Grid::new("nastaveni").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Styl", "Style"));
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("styl").selected_text(nazev_stylu_povrchu(stav, &p.styl)).width(220.0).show_ui(ui, |ui| {
                    for st in &stav.styly_povrchu {
                        let sel = st.id.eq_ignore_ascii_case(&p.styl);
                        if ui.selectable_label(sel, &st.jmeno).clicked() && !sel {
                            nastav(Zmena::Styl(st.id.clone()));
                        }
                    }
                });
                if ui.small_button(tr("✏ Upravit styl", "✏ Edit style")).clicked() {
                    let id = stav
                        .styly_povrchu
                        .iter()
                        .find(|s| s.id.eq_ignore_ascii_case(&p.styl))
                        .map_or(p.styl.clone(), |s| s.id.clone());
                    self.vyber_uzel(Uzel::StylPovrchu(id));
                }
            });
            ui.end_row();
            let krok_stylu = stav.styly_povrchu.iter().find(|s| s.id.eq_ignore_ascii_case(&p.styl)).and_then(|s| s.interval);
            ui.label(tr("Vrstevnice", "Contours"));
            match krok_stylu {
                Some((i, h)) => {
                    ui.label(trf!("{} (podle stylu)", "{} (by style)", interval_text(i, h)));
                }
                None => {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("interval")
                            .selected_text(interval_text(p.interval, p.hlavni))
                            .width(160.0)
                            .show_ui(ui, |ui| {
                                for (i, h) in INTERVALY {
                                    let sel = (i - p.interval).abs() < 1e-9 && (h - p.hlavni).abs() < 1e-9;
                                    if ui.selectable_label(sel, interval_text(i, h)).clicked() && !sel {
                                        nastav(Zmena::Interval(i, h));
                                    }
                                }
                                if ui.selectable_label(false, tr("vlastní…", "custom…")).clicked() {
                                    self.vlastni_krok = Some((p.id.clone(), p.interval, p.hlavni));
                                }
                            });
                        if self.vlastni_krok.as_ref().is_some_and(|v| v.0 == p.id) {
                            let mut zavrit = false;
                            if let Some((_, i, h)) = self.vlastni_krok.as_mut() {
                                ui.add(egui::DragValue::new(i).range(0.01..=50.0).speed(0.01).max_decimals(3).suffix(" m"))
                                    .on_hover_text(tr("vedlejší vrstevnice", "minor contours"));
                                ui.label("/");
                                ui.add(egui::DragValue::new(h).range(0.01..=250.0).speed(0.05).max_decimals(3).suffix(" m"))
                                    .on_hover_text(tr("hlavní vrstevnice (násobek vedlejších)", "major contours (multiple of minor)"));
                                if ui.button(tr("Použít", "Apply")).clicked() {
                                    let hl = ((*h / *i).round().max(1.0)) * *i;
                                    nastav(Zmena::Interval(*i, hl));
                                    zavrit = true;
                                }
                                if ui.small_button("x").clicked() {
                                    zavrit = true;
                                }
                            }
                            if zavrit {
                                self.vlastni_krok = None;
                            }
                        }
                    });
                }
            }
            ui.end_row();
            ui.label(tr("Max. hrana", "Max. edge"));
            egui::ComboBox::from_id_salt("hrana").selected_text(hrana_text(p.max_hrana)).width(220.0).show_ui(ui, |ui| {
                for h in HRANY {
                    let h = (h > 0.0).then_some(h);
                    if ui.selectable_label(h == p.max_hrana, hrana_text(h)).clicked() && h != p.max_hrana {
                        nastav(Zmena::Hrana(h));
                    }
                }
            });
            ui.end_row();
            ui.label(tr("Vyloučit kódy", "Exclude codes")).on_hover_text(tr("body s těmito kódy povrch nepoužije (stromy, sloupy…); změna se uloží hned", "the surface ignores points with these codes (trees, poles…); the change is saved immediately"));
            ui.horizontal_wrapped(|ui| {
                let mut v = kody_z_textu(&p.vyloucene_kody);
                let pred = v.clone();
                cipy_kodu(ui, &mut v, &mut self.novy_kod_povrchu, tr("kód", "code"));
                if v != pred {
                    nastav(Zmena::Kody(v.join(", ")));
                    hlaska(trf!("Povrch „{}“: vyloučené kódy uloženy.", "Surface \"{}\": excluded codes saved.", p.jmeno));
                }
            });
            ui.end_row();
        });

    }

    /// Čáry definice (spojnice, hranice, díry) – zapnout/vypnout, ukázat, odebrat.
    fn polozky(&mut self, ui: &mut egui::Ui, p: &PrehledPovrchu, druhy: &[DruhPolozky]) {
        let polozky: Vec<_> = p.polozky.iter().filter(|x| druhy.contains(&x.druh)).collect();
        let napoveda = if druhy.contains(&DruhPolozky::Spojnice) {
            tr("Spojnice přidáte tlačítkem Spojnice přes body (klikáním) nebo Přidat spojnici (vybrané 3D čáry).", "Add breaklines with the Breakline through points button (by clicking) or Add breakline (selected 3D lines).")
        } else {
            tr("Hranici a díru přidáte tak, že vyberete uzavřenou křivku a kliknete na Hranice / Díra.", "Add a boundary or hole by selecting a closed polyline and clicking Boundary / Hole.")
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(napoveda).color(SEDA));
            napoveda::otaznik(ui, if druhy.contains(&DruhPolozky::Spojnice) { tr("Spojnice (lomová hrana)", "Breakline") } else { tr("Hranice / díra", "Boundary / hole") });
        });
        ui.add_space(4.0);
        if polozky.is_empty() {
            ui.label(RichText::new(tr("žádné", "none")).color(SEDA));
        }
        for (i, x) in polozky.iter().enumerate() {
            let jmeno = match x.druh {
                DruhPolozky::Spojnice => tr("Spojnice", "Breaklines"),
                DruhPolozky::Hranice => tr("Hranice", "Boundary"),
                DruhPolozky::Dira => tr("Díra", "Hole"),
                DruhPolozky::Objekt(_) => tr("Objekt", "Object"),
            };
            let r = ui.horizontal(|ui| {
                let mut zap = x.zapnuto;
                let text = trf!("{} {} – {} vrcholů, {:.1} m", "{} {} – {} vertices, {:.1} m", jmeno, i + 1, x.vrcholu, x.delka);
                let text = if self.zvyraznena == Some(x.handle) { RichText::new(text).color(MODRA).strong() } else { RichText::new(text) };
                if ui.checkbox(&mut zap, text).on_hover_text(tr("vypnutá položka zůstane ve výkrese, ale povrch ji nepoužije", "a disabled item stays in the drawing but the surface does not use it")).changed() {
                    spravce::posli(Akce::PolozkaZapnout(p.id.clone(), x.handle, zap));
                }
                if ui.small_button(tr("Ukázat", "Show")).clicked() {
                    spravce::posli(Akce::Ukazat(x.stred, x.rozmer, vec![x.handle]));
                }
                if ui.small_button(tr("Odebrat", "Remove")).clicked() {
                    spravce::posli(Akce::OdebratPolozku(p.id.clone(), x.handle));
                }
            });
            if self.zvyraznena == Some(x.handle) {
                r.response.scroll_to_me(Some(egui::Align::Center));
            }
        }
        self.zvyraznena = None;
    }

    fn detail_objekty(&mut self, ui: &mut egui::Ui, p: &PrehledPovrchu) {
        ui.label(
            RichText::new(
                tr("Body z objektů výkresu (např. výkres od geodeta s body jako bloky). Objekty zůstanou na svých \
                 hladinách; když se změní, povrch se přepočítá.", "Points from drawing objects (e.g. a surveyor's drawing with points as blocks). The objects stay on their layers; when they change, the surface is recalculated."),
            )
            .color(SEDA),
        );
        ui.add_space(4.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(RichText::new(tr("Přidat objekty výkresu", "Add drawing objects")).strong());
            egui::Grid::new("objekty").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(tr("Typ objektu", "Object type"));
                let typ = self.typ_objektu.get_or_insert(TypObjektu::Bloky);
                egui::ComboBox::from_id_salt("typ_obj").selected_text(typ.nazev()).width(220.0).show_ui(ui, |ui| {
                    for t in TYPY_OBJEKTU {
                        ui.selectable_value(typ, t, t.nazev());
                    }
                });
                ui.end_row();
                ui.label("");
                let ma = typ.ma_hrany();
                ui.add_enabled(ma, egui::Checkbox::new(&mut self.zachovat_hrany, tr("Zachovat hrany z objektů", "Keep edges from the objects")))
                    .on_hover_text(tr("čáry a hrany 3D ploch se použijí jako spojnice", "lines and 3D face edges are used as breaklines"));
                ui.end_row();
            });
            let typ = self.typ_objektu.unwrap_or(TypObjektu::Bloky);
            if ui.button(tr("Přidat objekty vybrané ve výkrese", "Add objects selected in the drawing")).clicked() {
                spravce::posli(Akce::PridatObjekty(p.id.clone(), typ, self.zachovat_hrany && typ.ma_hrany()));
            }
        });
        ui.add_space(6.0);
        let objekty: Vec<_> = p.polozky.iter().filter(|x| matches!(x.druh, DruhPolozky::Objekt(_))).collect();
        if objekty.is_empty() {
            ui.label(RichText::new(tr("žádné objekty", "no objects")).color(SEDA));
            return;
        }
        for t in TYPY_OBJEKTU {
            let tyto: Vec<_> = objekty.iter().filter(|x| x.druh == DruhPolozky::Objekt(t)).collect();
            if tyto.is_empty() {
                continue;
            }
            let bodu: usize = tyto.iter().map(|x| x.vrcholu).sum();
            egui::CollapsingHeader::new(trf!("{} – {} objektů, {} bodů", "{} – {} objects, {} points", t.nazev(), tyto.len(), bodu))
                .id_salt(("obj", t.kod()))
                .default_open(tyto.len() <= 30 || tyto.iter().any(|x| Some(x.handle) == self.zvyraznena))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.small_button(tr("Ukázat vše", "Show all")).clicked() {
                            let h: Vec<Handle> = tyto.iter().map(|x| x.handle).collect();
                            let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
                            for x in &tyto {
                                x0 = x0.min(x.stred[0] - x.rozmer);
                                x1 = x1.max(x.stred[0] + x.rozmer);
                                y0 = y0.min(x.stred[1] - x.rozmer);
                                y1 = y1.max(x.stred[1] + x.rozmer);
                            }
                            spravce::posli(Akce::Ukazat([(x0 + x1) / 2.0, (y0 + y1) / 2.0], ((x1 - x0).max(y1 - y0)) / 2.0, h));
                        }
                    });
                    for (i, x) in tyto.iter().enumerate().take(500) {
                        let r = ui.horizontal(|ui| {
                            let mut zap = x.zapnuto;
                            let mut text = format!("{} {}", t.mnozne(), i + 1);
                            if x.vrcholu > 1 {
                                text.push_str(&trf!(" – {} bodů", " – {} points", x.vrcholu));
                            }
                            if x.hrany {
                                text.push_str(tr(", s hranami", ", with edges"));
                            }
                            let text = if self.zvyraznena == Some(x.handle) { RichText::new(text).color(MODRA).strong() } else { RichText::new(text) };
                            if ui.checkbox(&mut zap, text).changed() {
                                spravce::posli(Akce::PolozkaZapnout(p.id.clone(), x.handle, zap));
                            }
                            if ui.small_button(tr("Ukázat", "Show")).clicked() {
                                spravce::posli(Akce::Ukazat(x.stred, x.rozmer, vec![x.handle]));
                            }
                            if ui.small_button(tr("Odebrat", "Remove")).clicked() {
                                spravce::posli(Akce::OdebratPolozku(p.id.clone(), x.handle));
                            }
                        });
                        if self.zvyraznena == Some(x.handle) {
                            r.response.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                    if tyto.len() > 500 {
                        ui.label(RichText::new(trf!("… a dalších {}", "… and {} more", tyto.len() - 500)).color(SEDA));
                    }
                });
        }
        self.zvyraznena = None;
    }

    fn detail_skupiny_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, p: &PrehledPovrchu) {
        ui.label(RichText::new(tr("Body povrchu = sjednocení zapnutých skupin, bez vyloučených kódů povrchu.", "Surface points = union of the enabled groups, without the surface's excluded codes.")).color(SEDA));
        ui.add_space(4.0);
        if p.skupiny.is_empty() {
            ui.label(RichText::new(tr("Povrch nebere žádné body (jen spojnice a objekty výkresu).", "The surface takes no points (only breaklines and drawing objects).")).color(SEDA));
        }
        for g in &p.skupiny {
            ui.horizontal(|ui| {
                let mut zap = g.zapnuto;
                let t = if g.chybi { RichText::new(&g.jmeno).color(CERVENA) } else { RichText::new(trf!("{} ({} bodů)", "{} ({} points)", g.jmeno, g.pocet)) };
                if ui.checkbox(&mut zap, t).changed() {
                    spravce::posli(Akce::SkupinaVPovrchu(p.id.clone(), g.id.clone(), Some(zap)));
                }
                if !g.chybi && ui.small_button(tr("Otevřít", "Open")).clicked() {
                    self.vyber_uzel(Uzel::Skupina(g.id.clone()));
                }
                if ui.small_button(tr("Odebrat", "Remove")).clicked() {
                    spravce::posli(Akce::SkupinaVPovrchu(p.id.clone(), g.id.clone(), None));
                }
            });
        }
        ui.add_space(6.0);
        let volne: Vec<_> = stav.skupiny.iter().filter(|s| !p.skupiny.iter().any(|g| g.id == s.skupina.id)).collect();
        if volne.is_empty() {
            return;
        }
        if !volne.iter().any(|s| s.skupina.id == self.pridat_skupinu) {
            self.pridat_skupinu = volne[0].skupina.id.clone();
        }
        ui.horizontal(|ui| {
            ui.label(tr("Přidat skupinu", "Add group"));
            let jmeno = volne.iter().find(|s| s.skupina.id == self.pridat_skupinu).map_or("", |s| s.skupina.jmeno.as_str());
            egui::ComboBox::from_id_salt("pridat_sk").selected_text(jmeno).width(180.0).show_ui(ui, |ui| {
                for s in &volne {
                    ui.selectable_value(&mut self.pridat_skupinu, s.skupina.id.clone(), format!("{} ({})", s.skupina.jmeno, s.pocet));
                }
            });
            if ui.button(tr("Přidat", "Add")).clicked() {
                spravce::posli(Akce::SkupinaVPovrchu(p.id.clone(), self.pridat_skupinu.clone(), Some(true)));
            }
        });
    }
}

/// Řádek pravidla skupiny: popis, obsah, počet bodů a ✕. Vrací true, když se má pravidlo odstranit.
fn radek_pravidla(ui: &mut egui::Ui, popis: &str, pocet: Option<usize>, obsah: impl FnOnce(&mut egui::Ui)) -> bool {
    let mut pryc = false;
    ui.horizontal_wrapped(|ui| {
        popisek_sloupce(ui, popis);
        obsah(ui);
        if let Some(n) = pocet {
            ui.label(RichText::new(format!("{n} b.")).small().color(SEDA)).on_hover_text(tr("bodů splňujících toto pravidlo", "points matching this rule"));
        }
        pryc = ui.small_button("×").on_hover_text(tr("odstranit pravidlo", "remove rule")).clicked();
    });
    pryc
}

/// Popisek v levém sloupci řádku pravidla (pevná šířka, zarovnaný vlevo).
fn popisek_sloupce(ui: &mut egui::Ui, t: &str) {
    ui.allocate_ui_with_layout(egui::vec2(130.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(130.0);
        ui.label(t);
    });
}

/// Zaoblený štítek „text ✕“ – klik ho odebere.
fn cip(ui: &mut egui::Ui, t: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(format!("{t}  ×"))).corner_radius(10.0)).on_hover_text(tr("odebrat", "remove"))
}

/// Štítky kódů s polem pro přidání dalšího.
fn cipy_kodu(ui: &mut egui::Ui, kody: &mut Vec<String>, novy: &mut String, hint: &str) {
    let mut pryc = None;
    for (i, k) in kody.iter().enumerate() {
        if cip(ui, k).clicked() {
            pryc = Some(i);
        }
    }
    if let Some(i) = pryc {
        kody.remove(i);
    }
    let r = ui.add(egui::TextEdit::singleline(novy).hint_text(format!("+ {hint}")).desired_width(60.0));
    if (r.lost_focus() || novy.contains([',', ' '])) && !novy.trim().is_empty() {
        for k in kody_z_textu(novy) {
            if !kody.contains(&k) {
                kody.push(k);
            }
        }
        novy.clear();
    }
}

fn nadpis(ui: &mut egui::Ui, t: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(t).heading());
    ui.add_space(4.0);
}

fn ukazat_body(stav: &StavSpravce, h: &[Handle]) {
    let body: Vec<_> = stav.body.iter().filter(|b| h.contains(&b.handle)).collect();
    if body.is_empty() {
        return;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for b in &body {
        x0 = x0.min(b.bod.x);
        x1 = x1.max(b.bod.x);
        y0 = y0.min(b.bod.y);
        y1 = y1.max(b.bod.y);
    }
    let r = ((x1 - x0).max(y1 - y0) / 2.0).max(5.0);
    spravce::posli(Akce::Ukazat([(x0 + x1) / 2.0, (y0 + y1) / 2.0], r, h.to_vec()));
}

fn detail_upozorneni(ui: &mut egui::Ui, p: &PrehledPovrchu) {
    nadpis(ui, &trf!("{} – upozornění", "{} – warnings", p.jmeno));
    for g in p.skupiny.iter().filter(|g| g.chybi) {
        ui.label(RichText::new(trf!("Skupina bodů {} ve výkrese chybí – odeberte ji z definice", "Point group {} is missing in the drawing – remove it from the definition", g.id)).color(CERVENA));
    }
    let Some(h) = &p.hlaseni else {
        ui.label(RichText::new(tr("Podrobnosti z přepočtu jsou k dispozici po přepočtu v tomto spuštění.", "Details are available after a recalculation in this session.")).color(SEDA));
        return;
    };
    for (a, b) in &h.krizeni_spojnic {
        ui.horizontal(|ui| {
            ui.label(tr("Spojnice se kříží – úsek nevložen", "Breaklines cross – segment not inserted"));
            if ui.small_button(tr("Ukázat", "Show")).clicked() {
                let st = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                spravce::posli(Akce::Ukazat(st, (a[0] - b[0]).hypot(a[1] - b[1]) / 2.0, Vec::new()));
            }
        });
    }
    for &i in &h.nepouzite_editace {
        let popis = p.upravy.iter().filter(|u| u.zapnuto).nth(i).map(|u| u.e.popis()).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(trf!("Úprava bez účinku: {}", "Edit without effect: {}", popis));
            if let Some(u) = p.upravy.iter().filter(|u| u.zapnuto).nth(i) {
                if ui.small_button(tr("Ukázat", "Show")).clicked() {
                    spravce::posli(Akce::Ukazat(u.e.poloha(), 5.0, Vec::new()));
                }
            }
        });
    }
    if h.rozdilne_vysky > 0 {
        ui.label(trf!("{} vrcholů spojnic leží na bodě s jinou výškou – použita výška bodu", "{} breakline vertices lie on a point with a different elevation – the point's elevation was used", h.rozdilne_vysky));
    }
}

fn detail_upravy(ui: &mut egui::Ui, p: &PrehledPovrchu) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(tr("Úpravy přidáte tlačítky ve skupině Úpravy trojúhelníků na záložce Terén.", "Add edits with the buttons in the Triangle edits group on the Terén tab.")).color(SEDA));
        napoveda::otaznik(ui, tr("Ruční úprava", "Manual edit"));
    });
    ui.add_space(4.0);
    if p.upravy.is_empty() {
        ui.label(RichText::new(tr("žádné", "none")).color(SEDA));
    }
    let id = &p.id;
    for (i, u) in p.upravy.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut zap = u.zapnuto;
            if ui.checkbox(&mut zap, format!("{}. {}", i + 1, u.e.popis())).changed() {
                spravce::posli(Akce::UpravaZapnout(id.clone(), i, zap));
            }
            if ui.small_button(tr("Ukázat", "Show")).clicked() {
                spravce::posli(Akce::Ukazat(u.e.poloha(), 5.0, Vec::new()));
            }
            if ui.small_button(tr("Smazat", "Delete")).clicked() {
                spravce::posli(Akce::UpravaSmazat(id.clone(), i));
            }
        });
    }
    if !p.upravy.is_empty() && ui.button(tr("Smazat všechny úpravy", "Delete all edits")).clicked() {
        spravce::posli(Akce::UpravySmazat(id.clone()));
    }
}

fn kontrola(ui: &mut egui::Ui, stav: &StavSpravce) {
    karta::nadpis_karty(ui, &strom::Ikona::Svg(strom::ik!("ter_vyska_v_bode")), tr("Kontrola zaměření", "Survey check"), |ui| match &stav.kontrola {
        None => {
            karta::pilulka(ui, tr("zatím neproběhla", "not run yet"), SEDA);
        }
        Some(s) if s.is_empty() => {
            karta::pilulka(ui, tr("● bez podezřelých bodů", "● no suspicious points"), ZELENA);
        }
        Some(s) => {
            karta::pilulka(ui, &trf!("● podezřelé body: {}", "● suspicious points: {}", s.len()), ORANZOVA);
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        info(
            ui,
            tr("Hledá body zaměření, jejichž výška výrazně vybočuje z okolí (překlep, bod na autě, v jámě…). \
             Podezřelé body se ve výkrese označí kroužkem na hladině TER_KONTROLA.", "Looks for survey points whose elevation differs markedly from their surroundings (typo, point on a car, in a pit…). Suspicious points are marked in the drawing with a circle on the TER_KONTROLA layer."),
        );
        if karta::tlacitko(ui, Some(strom::ik!("ter_vyska_v_bode")), tr("Zkontrolovat zaměření", "Check the survey"), true).clicked() {
            spravce::posli(Akce::Kontrola);
        }
        if ui.button(tr("Smazat značky kontroly", "Delete check marks")).on_hover_text(tr("kroužky a odchylky na hladině TER_KONTROLA", "circles and deviations on the TER_KONTROLA layer")).clicked() {
            spravce::posli(Akce::SmazatKontrolu);
        }
        ui.add_space(16.0);
        ui.label(tr("Práh odchylky", "Deviation threshold"));
        let mut prah = nastaveni::akt().prah_kontroly;
        if ui
            .add(egui::DragValue::new(&mut prah).range(0.1..=10.0).speed(0.05).suffix(" m").max_decimals(2).custom_formatter(|v, _| cz(v, 2)).custom_parser(|t| t.replace(',', ".").trim().parse().ok()))
            .changed()
        {
            nastaveni::zmen(|n| n.prah_kontroly = prah);
        }
    });
    ui.add_space(6.0);
    match &stav.kontrola {
        None => {
            ui.label(RichText::new(tr("Kontrola zatím neproběhla.", "The check has not run yet.")).color(SEDA));
        }
        Some(s) if s.is_empty() => {
            ui.label(RichText::new(tr("Žádný podezřelý bod.", "No suspicious point.")).color(ZELENA));
        }
        Some(s) => {
            mapa_kontroly(ui, stav, s);
            ui.add_space(6.0);
            let id_hl = egui::Id::new("kontrola_zvyrazneny");
            let zvyrazneny: Option<Handle> = ui.ctx().data(|d| d.get_temp(id_hl)).flatten();
            let mut najeto: Option<Handle> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for b in s {
                    let barva = if b.dz.abs() > 2.0 { CERVENA } else { ORANZOVA };
                    let fill = if zvyrazneny == Some(b.handle) { ui.visuals().selection.bg_fill } else { ui.visuals().extreme_bg_color };
                    let r = egui::Frame::new()
                        .fill(fill)
                        .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width().min(640.0));
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(if b.cislo.is_empty() { "–" } else { &b.cislo }).strong());
                                ui.label(RichText::new(trf!("výška {}", "elevation {}", cz(b.z, 2))).color(SEDA));
                                karta::pilulka(ui, &format!("{}{} m", if b.dz > 0.0 { "+" } else { "" }, cz(b.dz, 2)), barva);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button(tr("Odebrat z aktivního povrchu", "Remove from the active surface")).clicked() {
                                        spravce::posli(Akce::VyloucitBod(b.x, b.y));
                                    }
                                    if ui.small_button(tr("Ukázat", "Show")).clicked() {
                                        spravce::posli(Akce::Ukazat([b.x, b.y], 10.0, vec![b.handle]));
                                    }
                                });
                            });
                        })
                        .response;
                    if r.hovered() {
                        najeto = Some(b.handle);
                    }
                    ui.add_space(3.0);
                }
            });
            if najeto.is_some() {
                ui.ctx().data_mut(|d| d.insert_temp(id_hl, najeto));
            }
        }
    }
}

/// Mapka zaměření: všechny body šedě, podezřelé barevně (velikost podle odchylky).
/// Klik na podezřelý bod ho ukáže ve výkrese.
fn mapa_kontroly(ui: &mut egui::Ui, stav: &StavSpravce, podezrele: &[spravce::PodezrelyBod]) {
    if stav.body.is_empty() {
        return;
    }
    let w = ui.available_width().clamp(240.0, 640.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, w * 0.55), Sense::click());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_gray(22));
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for b in &stav.body {
        x0 = x0.min(b.bod.x);
        x1 = x1.max(b.bod.x);
        y0 = y0.min(b.bod.y);
        y1 = y1.max(b.bod.y);
    }
    let vnitrek = rect.shrink(12.0);
    let m = ((vnitrek.width() as f64) / (x1 - x0).max(1e-6)).min((vnitrek.height() as f64) / (y1 - y0).max(1e-6));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let na = |x: f64, y: f64| {
        egui::pos2(vnitrek.center().x + ((x - cx) * m) as f32, vnitrek.center().y - ((y - cy) * m) as f32)
    };
    for b in &stav.body {
        p.circle_filled(na(b.bod.x, b.bod.y), 1.6, Color32::from_gray(90));
    }
    let mozny = resp.hover_pos();
    let id_hl = egui::Id::new("kontrola_zvyrazneny");
    let zvyrazneny: Option<Handle> = ui.ctx().data(|d| d.get_temp(id_hl)).flatten();
    if let Some(b) = zvyrazneny.and_then(|h| podezrele.iter().find(|b| b.handle == h)) {
        p.circle_stroke(na(b.x, b.y), 13.0, egui::Stroke::new(2.0, MODRA));
    }
    let mut nejblizsi: Option<(&spravce::PodezrelyBod, f32)> = None;
    for b in podezrele {
        let c = na(b.x, b.y);
        let barva = if b.dz.abs() > 2.0 { CERVENA } else { ORANZOVA };
        let r = (3.0 + b.dz.abs() as f32 * 1.5).min(9.0);
        p.circle_stroke(c, r, egui::Stroke::new(1.6, barva));
        p.circle_filled(c, 2.2, barva);
        if let Some(h) = mozny {
            let d = c.distance(h);
            if d < 12.0 && nejblizsi.is_none_or(|n| d < n.1) {
                nejblizsi = Some((b, d));
            }
        }
    }
    if let Some((b, _)) = nejblizsi {
        ui.ctx().data_mut(|d| d.insert_temp(id_hl, Some(b.handle)));
        p.circle_stroke(na(b.x, b.y), 11.0, egui::Stroke::new(1.5, Color32::WHITE));
        let t = trf!("bod {} · výška {:.2} · odchylka {:+.2} m – klik ukáže ve výkrese", "point {} · elevation {:.2} · deviation {:+.2} m – click to show in the drawing", if b.cislo.is_empty() { "–" } else { &b.cislo }, b.z, b.dz);
        resp.clone().on_hover_text(t);
        if resp.clicked() {
            spravce::posli(Akce::Ukazat([b.x, b.y], 10.0, vec![b.handle]));
        }
    }
}

fn nazev_stylu_povrchu(stav: &StavSpravce, id: &str) -> String {
    stav.styly_povrchu.iter().find(|s| s.id.eq_ignore_ascii_case(id)).map_or_else(|| id.to_string(), |s| s.jmeno.clone())
}

/// Přibližná barva ACI pro náhled.
fn rgb_aci(b: Barva) -> Color32 {
    match b {
        1 => Color32::from_rgb(255, 0, 0),
        2 => Color32::from_rgb(255, 255, 0),
        3 => Color32::from_rgb(0, 255, 0),
        4 => Color32::from_rgb(0, 255, 255),
        5 => Color32::from_rgb(0, 0, 255),
        6 => Color32::from_rgb(255, 0, 255),
        7 => Color32::from_rgb(235, 235, 235),
        8 => Color32::from_rgb(128, 128, 128),
        9 => Color32::from_rgb(192, 192, 192),
        22 => Color32::from_rgb(165, 82, 40),
        30 => Color32::from_rgb(255, 127, 0),
        32 => Color32::from_rgb(204, 102, 0),
        140 => Color32::from_rgb(0, 165, 255),
        150 => Color32::from_rgb(0, 127, 255),
        _ => Color32::from_rgb(110, 110, 110),
    }
}

fn ctverecek(ui: &mut egui::Ui, b: Barva) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
    if b == styly::PODLE_HLADINY {
        ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(1.0, SEDA), egui::StrokeKind::Inside);
    } else {
        ui.painter().rect_filled(r, 2.0, rgb_aci(b));
    }
}

/// Výběr barvy (seznam běžných barev + číslo ACI).
/// Posuvník v mm (s polem pro přesnou hodnotu); větší hodnotu jde napsat.
fn posuvnik_mm(ui: &mut egui::Ui, v: &mut f64, rozsah: std::ops::RangeInclusive<f64>) -> bool {
    ui.add(
        egui::Slider::new(v, rozsah)
            .step_by(0.1)
            .clamping(egui::SliderClamping::Never)
            .suffix(" mm")
            .custom_formatter(|v, _| cz(v, 2))
            .custom_parser(|t| t.replace(',', ".").trim().parse().ok()),
    )
    .changed()
}

/// Barva jako řada vzorků (+ přesné číslo ACI); „podle hladiny“ je přeškrtnutý vzorek.
fn vzorky_barev(ui: &mut egui::Ui, id: &str, b: &mut Barva) -> bool {
    let mut zmena = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (x, n) in BARVY {
            let (rect, r) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
            let p = ui.painter();
            if x == styly::PODLE_HLADINY {
                p.rect_filled(rect.shrink(1.0), 4.0, Color32::from_gray(60));
                p.line_segment([rect.left_bottom() + egui::vec2(4.0, -4.0), rect.right_top() + egui::vec2(-4.0, 4.0)], egui::Stroke::new(1.5, Color32::from_gray(200)));
            } else {
                p.rect_filled(rect.shrink(1.0), 4.0, rgb_aci(x));
            }
            if *b == x {
                p.rect_stroke(rect, 5.0, egui::Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
            } else if r.hovered() {
                p.rect_stroke(rect, 5.0, egui::Stroke::new(1.0, Color32::from_gray(180)), egui::StrokeKind::Outside);
            }
            if r.on_hover_text(crate::lang::tr_tab(n)).clicked() && *b != x {
                *b = x;
                zmena = true;
            }
        }
        ui.add_space(6.0);
        if !BARVY.iter().any(|(x, _)| x == b) || *b != styly::PODLE_HLADINY {
            let mut aci = if *b == styly::PODLE_HLADINY { 7 } else { *b };
            if ui
                .add(egui::DragValue::new(&mut aci).range(1..=255).prefix("ACI "))
                .on_hover_text(tr("přesné číslo barvy (ACI 1–255)", "exact colour number (ACI 1–255)"))
                .changed()
            {
                *b = aci;
                zmena = true;
            }
        }
        let _ = id;
    });
    zmena
}

fn vyber_barvy(ui: &mut egui::Ui, id: &str, b: &mut Barva) -> bool {
    let mut zmena = false;
    ui.horizontal(|ui| {
        ctverecek(ui, *b);
        egui::ComboBox::from_id_salt(id).selected_text(styly::nazev_barvy(*b)).width(130.0).show_ui(ui, |ui| {
            for (x, n) in BARVY {
                ui.horizontal(|ui| {
                    ctverecek(ui, x);
                    if ui.selectable_label(*b == x, crate::lang::tr_tab(n)).clicked() && *b != x {
                        *b = x;
                        zmena = true;
                    }
                });
            }
        });
        if *b != styly::PODLE_HLADINY {
            let mut aci = *b;
            if ui.add(egui::DragValue::new(&mut aci).range(1..=255).prefix("ACI ")).changed() {
                *b = aci;
                zmena = true;
            }
        }
    });
    zmena
}

fn tloustka_text(t: i16) -> String {
    if t < 0 { tr("podle hladiny", "ByLayer").into() } else { format!("{:.2} mm", t as f64 / 100.0) }
}

const TLOUSTKY: [i16; 8] = [-1, 0, 13, 18, 25, 35, 50, 70];

fn vyber_tloustky(ui: &mut egui::Ui, id: &str, t: &mut i16) -> bool {
    let mut zmena = false;
    egui::ComboBox::from_id_salt(id).selected_text(tloustka_text(*t)).width(130.0).show_ui(ui, |ui| {
        for x in TLOUSTKY {
            if ui.selectable_label(*t == x, tloustka_text(x)).clicked() && *t != x {
                *t = x;
                zmena = true;
            }
        }
    });
    zmena
}

fn mm(ui: &mut egui::Ui, v: &mut f64, rozsah: std::ops::RangeInclusive<f64>) -> bool {
    ui.add(
        egui::DragValue::new(v)
            .range(rozsah)
            .speed(0.05)
            .max_decimals(2)
            .suffix(" mm")
            .custom_formatter(|v, _| cz(v, 2))
            .custom_parser(|t| t.replace(',', ".").trim().parse().ok()),
    )
    .changed()
}

impl Okno {
    /// Tlačítka šablony stylů (uložit / načíst) a vysvětlivka.
    fn sablona_stylu(&mut self, ui: &mut egui::Ui) {
        if ui.button(tr("Uložit do šablony", "Save to template")).on_hover_text(tr("styly výkresu uloží do šablony ve vašem profilu", "saves the drawing styles to the template in your profile")).clicked() {
            spravce::posli(Akce::SablonaUloz);
        }
        if ui.button(tr("Načíst ze šablony", "Load from template")).on_hover_text(tr("načte styly ze šablony do tohoto výkresu", "loads the styles from the template into this drawing")).clicked() {
            spravce::posli(Akce::SablonaNacti);
        }
        info(
            ui,
            tr("Styl bodu určuje značku a popis bodu, styl povrchu vrstevnice, barvy, popisy a šipky spádu. \
             Velikosti jsou v mm na papíře a mění se s měřítkem výkresu. Vestavěné styly jde upravit \
             (úprava se uloží do výkresu) a kdykoli vrátit k původnímu nastavení.", "A point style sets the point's marker and label, a surface style the contours, colours, labels and slope arrows. Sizes are in mm on paper and change with the drawing scale. Built-in styles can be edited (the change is saved in the drawing) and restored to the original settings at any time."),
        );
    }

    fn detail_styly_bodu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let pocty = pocty_stylu(stav);
        let pouzito = stav.styly_bodu.iter().filter(|st| pocty.get(&st.id).copied().unwrap_or(0) > 0).count();
        karta::nadpis_karty(ui, &strom::Ikona::Svg(strom::ik!("ter_styl_bodu")), tr("Styly bodů", "Point styles"), |ui| {
            karta::pilulka(ui, &trf!("{} · používá se {}", "{} · used {}", stav.styly_bodu.len(), pouzito), SEDA);
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if karta::tlacitko(ui, Some(strom::ik!("ter_styl_bodu")), tr("+ Nový styl bodů", "+ New point style"), true).clicked() {
                let mut st = StylBodu::vestavene().remove(0);
                st.id = String::new();
                st.jmeno = tr("Nový styl bodů", "New point style").into();
                spravce::posli(Akce::UlozStylBodu(st));
            }
            self.sablona_stylu(ui);
        });
        ui.add_space(8.0);
        let mut klik = None;
        for (nadpis, symboly) in [(tr("Popisy", "Labels"), false), (tr("Symboly", "Symbols"), true)] {
            ui.label(RichText::new(nadpis.to_uppercase()).small().color(SEDA));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for st in stav.styly_bodu.iter().filter(|s| s.znacka.je_symbol() == symboly) {
                    let n = pocty.get(&st.id).copied().unwrap_or(0);
                    let mut popis = if n == 0 { tr("nepoužito", "unused").to_string() } else { trf!("{} bodů", "{} points", czi(n)) };
                    if !st.kody.is_empty() {
                        popis = trf!("{} · kód {}", "{} · code {}", popis, st.kody.join(", "));
                    }
                    if stav.ulozene_styly.contains(&st.id) {
                        popis.push_str(if st.je_vestaveny() { tr(" · upravený", " · modified") } else { tr(" · vlastní", " · custom") });
                    }
                    let r = karta::dlazdice_stylu(ui, |ui| ikona_stylu(ui, st, 28.0), &st.jmeno, &popis, n == 0);
                    if r.clicked() {
                        klik = Some(st.id.clone());
                    }
                    let u = Uzel::StylBodu(st.id.clone());
                    r.context_menu(|ui| self.menu_stylu_bodu(ui, stav, st, &u));
                }
            });
            ui.add_space(8.0);
        }
        if let Some(id) = klik {
            self.vyber_uzel(Uzel::StylBodu(id));
        }
    }

    fn detail_styly_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let pouzito = stav.styly_povrchu.iter().filter(|st| stav.povrchy.iter().any(|p| p.styl.eq_ignore_ascii_case(&st.id))).count();
        karta::nadpis_karty(ui, &strom::Ikona::Svg(strom::ik!("ter_styl")), tr("Styly povrchů", "Surface styles"), |ui| {
            karta::pilulka(ui, &trf!("{} · používá se {}", "{} · used {}", stav.styly_povrchu.len(), pouzito), SEDA);
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if karta::tlacitko(ui, Some(strom::ik!("ter_styl")), tr("+ Nový styl povrchu", "+ New surface style"), true).clicked() {
                spravce::posli(Akce::UlozStylPovrchu(StylPovrchu::vlastni("", tr("Nový styl povrchu", "New surface style"))));
            }
            self.sablona_stylu(ui);
        });
        ui.add_space(8.0);
        let mut klik = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for st in &stav.styly_povrchu {
                let pouziva: Vec<&str> =
                    stav.povrchy.iter().filter(|p| p.styl.eq_ignore_ascii_case(&st.id)).map(|p| p.jmeno.as_str()).collect();
                let mut popis = if pouziva.is_empty() { tr("nepoužito", "unused").to_string() } else { pouziva.join(", ") };
                if stav.ulozene_styly.contains(&st.id) {
                    popis.push_str(if st.je_vestaveny() { tr(" · upravený", " · modified") } else { tr(" · vlastní", " · custom") });
                }
                let r = karta::dlazdice_stylu(ui, |ui| ikona_stylu_povrchu(ui, st, 28.0), &st.jmeno, &popis, pouziva.is_empty());
                if r.clicked() {
                    klik = Some(st.id.clone());
                }
                let u = Uzel::StylPovrchu(st.id.clone());
                r.context_menu(|ui| self.menu_stylu_povrchu(ui, stav, st, &u));
            }
        });
        if let Some(id) = klik {
            self.vyber_uzel(Uzel::StylPovrchu(id));
        }
    }

    fn detail_styl_bodu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, id: &str) {
        if self.edit_styl_bodu.as_ref().map(|s| s.id.as_str()) != Some(id) {
            self.edit_styl_bodu = stav.styly_bodu.iter().find(|s| s.id == id).cloned();
            self.styl_zmenen = false;
            self.zalozka_stylu = 0;
        }
        let Some(st) = self.edit_styl_bodu.as_mut() else {
            ui.label(RichText::new(tr("Styl už ve výkrese není.", "The style is no longer in the drawing.")).color(SEDA));
            return;
        };
        let vestaveny = st.je_vestaveny();
        let ulozeny = stav.ulozene_styly.contains(&st.id);
        let pouziti = pocty_stylu(stav).get(&st.id).copied().unwrap_or(0);
        let nahled = st.clone();

        // ── karta: značka stylu, název, stav, akce ──
        ui.horizontal(|ui| {
            ikona_stylu(ui, &nahled, 28.0);
            ui.label(RichText::new(&nahled.jmeno).size(18.0).strong());
            karta::pilulka(ui, if vestaveny { tr("vestavěný", "built-in") } else { tr("vlastní", "custom") }, SEDA);
            if vestaveny && ulozeny {
                karta::pilulka(ui, tr("upravený", "modified"), ORANZOVA).on_hover_text(tr("úprava je uložená ve výkrese; „Vrátit původní nastavení“ ji zruší", "the change is stored in the drawing; \"Restore original settings\" cancels it"));
            }
            karta::pilulka(
                ui,
                &if pouziti == 0 { tr("nepoužito", "unused").to_string() } else { trf!("{} bodů", "{} points", czi(pouziti)) },
                if pouziti == 0 { SEDA } else { ZELENA },
            );
        });
        ui.add_space(4.0);
        let mut akce_kopie = false;
        let mut akce_puvodni = false;
        let mut akce_smazat = false;
        let mut akce_vratit = false;
        ui.horizontal_wrapped(|ui| {
            akce_kopie = ui.button(tr("Kopírovat jako nový", "Copy as new")).clicked();
            if vestaveny {
                akce_puvodni = ulozeny && ui.button(tr("Vrátit původní nastavení", "Restore original settings")).clicked();
            } else {
                akce_smazat = ui.button(RichText::new(tr("Smazat styl", "Delete style")).color(CERVENA)).clicked();
            }
            if self.styl_zmenen {
                ui.label(RichText::new(tr("ukládám…", "saving…")).color(SEDA));
                akce_vratit = ui.button(tr("Vrátit změny", "Revert changes")).clicked();
            }
        });
        ui.add_space(4.0);
        ui.separator();

        let mut z = false;
        let mut zal = self.zalozka_stylu;
        let novy_kod = &mut self.kody_stylu.1;
        ui.horizontal_top(|ui| {
            // ── velký živý náhled vlevo ──
            ui.vertical(|ui| {
                ui.set_width(250.0);
                nahled_bodu(ui, &nahled, egui::vec2(250.0, 220.0), 13.0);
                ui.label(RichText::new(trf!("náhled v měřítku 1:{}", "preview at scale 1:{}", cislo_cz(stav.meritko))).small().color(SEDA));
            });
            ui.add_space(16.0);
            // ── záložky vpravo ──
            ui.vertical(|ui| {
                let zalozky = [(0u8, tr("Značka", "Marker").to_string()), (1, tr("Popis", "Description").to_string()), (2, tr("Použití", "Use").to_string())];
                if let Some(n) = karta::zalozky(ui, zal, &zalozky) {
                    zal = n;
                }
                let mrizka = |id: &str| egui::Grid::new(id).num_columns(2).spacing([14.0, 9.0]).min_col_width(90.0);
                match zal {
                    0 => {
                        mrizka("sb_znacka_g").show(ui, |ui| {
                            ui.label(tr("Jméno", "Name"));
                            z |= ui.add(egui::TextEdit::singleline(&mut st.jmeno).desired_width(240.0)).changed();
                            ui.end_row();
                            ui.label(tr("Tvar", "Shape"));
                            let nazev = if st.blok.is_empty() { st.znacka.nazev().to_string() } else { tr("vlastní – blok výkresu", "custom – drawing block").to_string() };
                            egui::ComboBox::from_id_salt("sb_znacka").selected_text(nazev).width(220.0).show_ui(ui, |ui| {
                                for x in ZNACKY {
                                    if ui.selectable_label(st.blok.is_empty() && st.znacka == x, x.nazev()).clicked() {
                                        st.znacka = x;
                                        st.blok.clear();
                                        z = true;
                                    }
                                }
                                ui.separator();
                                if ui.selectable_label(!st.blok.is_empty(), tr("vlastní – blok výkresu", "custom – drawing block")).clicked() && st.blok.is_empty() {
                                    st.blok = stav.bloky.first().cloned().unwrap_or_else(|| "?".into());
                                    z = true;
                                }
                            });
                            ui.end_row();
                            if !st.blok.is_empty() {
                                ui.label(tr("Blok", "Block"));
                                ui.vertical(|ui| {
                                    if stav.bloky.is_empty() {
                                        ui.label(
                                            RichText::new(tr("Ve výkrese zatím není žádný blok. Vložte svůj blok značky (příkaz INSERT) nebo ho vytvořte (BLOCK) a vyberte ho tady.", "There is no block in the drawing yet. Insert your marker block (INSERT command) or create it (BLOCK) and select it here."))
                                                .small()
                                                .color(ORANZOVA),
                                        );
                                    } else {
                                        egui::ComboBox::from_id_salt("sb_blok").selected_text(&st.blok).width(220.0).show_ui(ui, |ui| {
                                            for b in &stav.bloky {
                                                z |= ui.selectable_value(&mut st.blok, b.clone(), b).changed();
                                            }
                                        });
                                    }
                                })
                                .response
                                .on_hover_text(tr("Základní bod bloku = poloha bodu. Blok se zvětší tak, aby jeho větší rozměr odpovídal velikosti značky; barva podle stylu (v bloku nastavte barvu „podle bloku“).", "The block's base point = the point position. The block is scaled so its larger dimension matches the marker size; colour by style (set the colour to \"ByBlock\" inside the block)."));
                                ui.end_row();
                            }
                            ui.label(tr("Velikost", "Size"));
                            z |= posuvnik_mm(ui, &mut st.velikost, 0.2..=10.0);
                            ui.end_row();
                            ui.label(tr("Barva", "Colour"));
                            z |= vzorky_barev(ui, "sb_bz", &mut st.barva_znacky);
                            ui.end_row();
                        });
                    }
                    1 => {
                        mrizka("sb_popis_g").show(ui, |ui| {
                            ui.label(tr("Popisek", "Label"));
                            egui::ComboBox::from_id_salt("sb_popisek").selected_text(st.popisek.nazev()).width(220.0).show_ui(ui, |ui| {
                                for x in POPISKY {
                                    z |= ui.selectable_value(&mut st.popisek, x, x.nazev()).changed();
                                }
                            });
                            ui.end_row();
                            ui.label(tr("Výška písma", "Text height"));
                            z |= posuvnik_mm(ui, &mut st.pismo, 0.5..=8.0);
                            ui.end_row();
                            ui.label(tr("Výška bodu", "Point elevation"));
                            ui.horizontal(|ui| {
                                let mut m = st.mista as i32;
                                if ui.add(egui::DragValue::new(&mut m).range(0..=4).suffix(tr(" des. míst", " decimals"))).changed() {
                                    st.mista = m as u8;
                                    z = true;
                                }
                                z |= ui.checkbox(&mut st.carka, tr("desetinná čárka", "decimal comma")).changed();
                            });
                            ui.end_row();
                            ui.label(tr("Kód bodu", "Point code"));
                            z |= ui.checkbox(&mut st.popis, tr("zobrazit kód (popis) bodu", "show the point code (description)")).changed();
                            ui.end_row();
                            ui.label(tr("Barva textu", "Text colour"));
                            z |= vzorky_barev(ui, "sb_bt", &mut st.barva_textu);
                            ui.end_row();
                        });
                    }
                    _ => {
                        mrizka("sb_pouziti_g").show(ui, |ui| {
                            ui.label(tr("Pro kódy", "For codes")).on_hover_text(tr("Body s těmito kódy dostanou styl automaticky (přednost má jen styl nastavený přímo bodu).", "Points with these codes get the style automatically (only a style set directly on the point takes precedence)."));
                            ui.horizontal_wrapped(|ui| {
                                let pred = st.kody.clone();
                                cipy_kodu(ui, &mut st.kody, novy_kod, tr("kód", "code"));
                                z |= st.kody != pred;
                            });
                            ui.end_row();
                            ui.label(tr("Zobrazení", "Display"));
                            z |= ui.checkbox(&mut st.skryt, tr("nezobrazovat body (skrýt značku i popis)", "do not display points (hide marker and label)")).changed();
                            ui.end_row();
                        });
                        ui.add_space(8.0);
                        // odkud styl body mají (ručně / podle kódu / podle skupiny / výchozí)
                        let (mut rucne, mut kodem, mut skupinou, mut vychozi) = (0usize, 0usize, 0usize, 0usize);
                        for (id, puvod) in stav.styl_bodu.values() {
                            if id != &st.id {
                                continue;
                            }
                            match puvod {
                                styly::Puvod::Rucne => rucne += 1,
                                styly::Puvod::Kod(_) => kodem += 1,
                                styly::Puvod::Skupina(_) => skupinou += 1,
                                styly::Puvod::Standard => vychozi += 1,
                            }
                        }
                        let skupiny: Vec<&str> =
                            stav.skupiny.iter().filter(|g| g.skupina.styl == st.id).map(|g| g.skupina.jmeno.as_str()).collect();
                        if rucne + kodem + skupinou + vychozi == 0 {
                            let t = if st.id == styly::STANDARD {
                                tr("Výchozí styl – platí pro body, jejichž skupina ani kód styl neurčují.", "Default style – applies to points whose group or code sets no style.")
                            } else {
                                tr("Styl zatím nic nepoužívá – přiřaďte ho skupině, kódům (pole Pro kódy) nebo vybraným bodům.", "Nothing uses this style yet – assign it to a group, to codes (the For codes field) or to selected points.")
                            };
                            ui.label(RichText::new(t).color(SEDA));
                        } else {
                            ui.label(RichText::new(trf!("Styl má {} bodů:", "The style has {} points:", czi(rucne + kodem + skupinou + vychozi))).strong());
                            egui::Grid::new("sb_pouziti_pocty").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
                                for (n, t) in [
                                    (rucne, tr("nastaveno přímo bodům", "set directly on points").to_string()),
                                    (kodem, tr("podle kódu", "by code").to_string()),
                                    (skupinou, if skupiny.is_empty() { tr("podle skupiny", "by group").to_string() } else { trf!("podle skupiny ({})", "by group ({})", skupiny.join(", ")) }),
                                    (vychozi, tr("výchozí styl", "default style").to_string()),
                                ] {
                                    if n > 0 {
                                        ui.label(RichText::new(czi(n)).strong());
                                        ui.label(RichText::new(t).color(SEDA));
                                        ui.end_row();
                                    }
                                }
                            });
                        }
                        ui.label(RichText::new(tr("Přednost má styl nastavený přímo bodu, pak podle kódu, pak podle skupiny.", "A style set directly on the point wins, then by code, then by group.")).small().color(SEDA));
                    }
                }
                ui.add_space(10.0);
                ui.label(RichText::new(tr("Změna se hned ukáže v náhledu a sama uloží (Ctrl+Z ve výkrese ji vrátí).", "The change shows in the preview at once and is saved automatically (Ctrl+Z in the drawing undoes it).")).small().color(SEDA));
            });
        });
        self.zalozka_stylu = zal;
        if z {
            self.styl_zmenen = true;
            ui.ctx().request_repaint();
        }
        let ulozit_hned = self.styl_zmenen && ulozit_ted(ui, &mut self.zmena_stylu, z);
        let st = st.clone();
        if ulozit_hned {
            hlaska(trf!("Styl „{}“ uložen.", "Style \"{}\" saved.", st.jmeno));
            spravce::posli(Akce::UlozStylBodu(st.clone()));
            self.styl_zmenen = false;
        }
        if akce_vratit {
            self.edit_styl_bodu = None;
            self.kody_stylu = Default::default();
            self.styl_zmenen = false;
            self.zmena_stylu = None;
        }
        if akce_kopie {
            let mut n = st.clone();
            n.id = String::new();
            n.jmeno = trf!("{} (kopie)", "{} (copy)", st.jmeno);
            spravce::posli(Akce::UlozStylBodu(n));
        }
        if akce_puvodni {
            spravce::posli(Akce::SmazStyl(st.id.clone()));
            hlaska(trf!("Styl „{}“ má zase původní nastavení.", "Style \"{}\" has its original settings again.", st.jmeno));
        }
        if akce_smazat {
            smazat(trf!("Styl „{}“ je smazaný.", "Style \"{}\" was deleted.", st.jmeno), Akce::SmazStyl(st.id.clone()));
        }
    }

    fn detail_styl_povrchu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, id: &str) {
        if self.edit_styl_povrchu.as_ref().map(|s| s.id.as_str()) != Some(id) {
            self.edit_styl_povrchu = stav.styly_povrchu.iter().find(|s| s.id == id).cloned();
            self.styl_zmenen = false;
        }
        let Some(st) = self.edit_styl_povrchu.as_mut() else {
            ui.label(RichText::new(tr("Styl už ve výkrese není.", "The style is no longer in the drawing.")).color(SEDA));
            return;
        };
        let vestaveny = st.je_vestaveny();
        karta::nadpis_karty(ui, &strom::Ikona::Svg(strom::ik!("ter_styl")), &st.jmeno, |ui| {
            karta::pilulka(ui, if vestaveny { tr("vestavěný", "built-in") } else { tr("vlastní", "custom") }, SEDA);
        });
        ui.add_space(6.0);
        let mut z = false;
        egui::Grid::new("styl_povrchu").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Jméno", "Name"));
            z |= ui.add(egui::TextEdit::singleline(&mut st.jmeno).desired_width(260.0)).changed();
            ui.end_row();

            ui.label(RichText::new(tr("Vrstevnice", "Contours")).strong());
            z |= ui.checkbox(&mut st.vrstevnice, "zobrazit").changed();
            ui.end_row();
            ui.label(tr("krok", "step"));
            ui.horizontal(|ui| {
                let mut podle = st.interval.is_none();
                if ui.checkbox(&mut podle, tr("podle povrchu", "by surface")).changed() {
                    st.interval = if podle { None } else { Some((0.5, 2.5)) };
                    z = true;
                }
                if let Some((i, h)) = st.interval.as_mut() {
                    ui.label(tr("vedlejší", "minor"));
                    z |= ui.add(egui::DragValue::new(i).range(0.01..=50.0).speed(0.01).max_decimals(3).suffix(" m")).changed();
                    ui.label(tr("hlavní", "major"));
                    z |= ui.add(egui::DragValue::new(h).range(0.01..=250.0).speed(0.05).max_decimals(3).suffix(" m")).changed();
                }
            });
            ui.end_row();
            ui.label(tr("vedlejší", "minor"));
            ui.horizontal(|ui| {
                z |= vyber_barvy(ui, "sp_bv", &mut st.barva_vedlejsi);
                ui.label("tl.");
                z |= vyber_tloustky(ui, "sp_tv", &mut st.tloustka_vedlejsi);
            });
            ui.end_row();
            ui.label(tr("hlavní", "major"));
            ui.horizontal(|ui| {
                z |= vyber_barvy(ui, "sp_bh", &mut st.barva_hlavni);
                ui.label("tl.");
                z |= vyber_tloustky(ui, "sp_th", &mut st.tloustka_hlavni);
            });
            ui.end_row();

            ui.label(RichText::new(tr("Popis vrstevnic", "Contour labels")).strong());
            egui::ComboBox::from_id_salt("sp_popis").selected_text(st.popis.nazev()).width(180.0).show_ui(ui, |ui| {
                for x in POPISY_VRSTEVNIC {
                    z |= ui.selectable_value(&mut st.popis, x, x.nazev()).changed();
                }
            });
            ui.end_row();
            ui.label(tr("výška písma", "text height"));
            z |= mm(ui, &mut st.pismo, 0.5..=20.0);
            ui.end_row();
            ui.label(tr("rozestup popisů", "label spacing"));
            z |= ui.add(egui::DragValue::new(&mut st.rozestup).range(20.0..=1000.0).speed(1.0).max_decimals(0).suffix(" mm")).changed();
            ui.end_row();
            ui.label(tr("desetinná místa", "decimal places"));
            let mut m = st.mista as i32;
            if ui.add(egui::DragValue::new(&mut m).range(0..=3)).changed() {
                st.mista = m as u8;
                z = true;
            }
            ui.end_row();
            ui.label(tr("barva popisu", "label colour"));
            z |= vyber_barvy(ui, "sp_bp", &mut st.barva_popisu);
            ui.end_row();

            ui.label(RichText::new(tr("Trojúhelníky", "Triangles")).strong());
            ui.horizontal(|ui| {
                z |= ui.checkbox(&mut st.tin, "zobrazit").changed();
                z |= vyber_barvy(ui, "sp_bt", &mut st.barva_tin);
            });
            ui.end_row();
            ui.label(RichText::new(tr("Obrys", "Outline")).strong());
            ui.horizontal(|ui| {
                z |= ui.checkbox(&mut st.obrys, "zobrazit").changed();
                z |= vyber_barvy(ui, "sp_bo", &mut st.barva_obrys);
            });
            ui.end_row();
            ui.label(RichText::new(tr("Šipky spádu", "Slope arrows")).strong());
            ui.horizontal(|ui| {
                z |= ui.checkbox(&mut st.sipky, "zobrazit").changed();
                ui.label(tr("délka", "length"));
                z |= mm(ui, &mut st.delka_sipky, 1.0..=30.0);
                z |= vyber_barvy(ui, "sp_bs", &mut st.barva_sipek);
            });
            ui.end_row();
        });
        if z {
            self.styl_zmenen = true;
        }
        let ulozit_hned = self.styl_zmenen && ulozit_ted(ui, &mut self.zmena_stylu, z);
        let st = st.clone();
        let ulozeny = stav.ulozene_styly.contains(&st.id);
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ulozit_hned {
                hlaska(trf!("Styl „{}“ uložen – povrchy s ním se překreslí.", "Style \"{}\" saved – surfaces using it are redrawn.", st.jmeno));
                spravce::posli(Akce::UlozStylPovrchu(st.clone()));
                self.styl_zmenen = false;
            }
            if self.styl_zmenen {
                ui.label(RichText::new(tr("ukládám…", "saving…")).color(SEDA));
            }
            if self.styl_zmenen && ui.button(tr("Vrátit změny", "Revert changes")).clicked() {
                self.edit_styl_povrchu = None;
                self.styl_zmenen = false;
            }
            if ui.button(tr("Kopírovat jako nový", "Copy as new")).clicked() {
                let mut n = st.clone();
                n.id = String::new();
                n.jmeno = trf!("{} (kopie)", "{} (copy)", st.jmeno);
                spravce::posli(Akce::UlozStylPovrchu(n));
            }
            if st.je_vestaveny() {
                if ulozeny && ui.button(tr("Vrátit původní nastavení", "Restore original settings")).clicked() {
                    spravce::posli(Akce::SmazStyl(st.id.clone()));
                }
            } else if ui.button(RichText::new(tr("Smazat styl", "Delete style")).color(CERVENA)).clicked() {
                smazat(trf!("Styl „{}“ je smazaný.", "Style \"{}\" was deleted.", st.jmeno), Akce::SmazStyl(st.id.clone()));
            }
        });
        let pouziva: Vec<&str> =
            stav.povrchy.iter().filter(|p| p.styl.eq_ignore_ascii_case(&st.id)).map(|p| p.jmeno.as_str()).collect();
        ui.label(
            RichText::new(if pouziva.is_empty() {
                tr("Styl zatím nepoužívá žádný povrch – vyberte ho u povrchu v poli Styl.", "No surface uses the style yet – choose it at a surface in the Style field.").to_string()
            } else {
                trf!("Používá: {}", "Used by: {}", pouziva.join(", "))
            })
            .color(SEDA),
        );
    }

    /// Ukládá polohu a velikost okna (s malým zpožděním, ať se nezapisuje při každém pohybu).
    fn hlidej_geometrii(&mut self, ctx: &egui::Context) {
        let g = ctx.input(|i| {
            let v = i.viewport();
            match (v.outer_rect, v.inner_rect) {
                (Some(o), Some(n)) => Some([o.min.x, o.min.y, n.width(), n.height()]),
                _ => None,
            }
        });
        let Some(g) = g else { return };
        if self.ulozena_geometrie.is_none() {
            self.ulozena_geometrie = Some(g);
            return;
        }
        let ted = std::time::Instant::now();
        match self.geometrie {
            Some((p, _)) if p != g => self.geometrie = Some((g, ted)),
            Some((p, t)) if ted.duration_since(t).as_millis() > 800 && Some(p) != self.ulozena_geometrie => {
                nastaveni::zmen(|n| n.okno = Some(p));
                self.ulozena_geometrie = Some(p);
            }
            None if Some(g) != self.ulozena_geometrie => self.geometrie = Some((g, ted)),
            _ => {}
        }
    }
}

/// Náhled bodu podle stylu (1 mm na papíře = 7 px).
/// Značka bodu (tvar ze stylu) se středem `c` a poloměrem `r` v pixelech.
/// Značka stylu – vestavěný tvar, nebo symbol vlastního bloku.
fn kresli_znacku_stylu(p: &egui::Painter, c: egui::Pos2, r: f32, st: &StylBodu, s: egui::Stroke) {
    if st.blok.is_empty() {
        kresli_znacku(p, c, r, st.znacka, s);
    } else {
        let o = egui::Rect::from_center_size(c, egui::vec2(r * 1.8, r * 1.8));
        p.rect_stroke(o, 2.0, s, egui::StrokeKind::Middle);
        p.text(c, Align2::CENTER_CENTER, "B", FontId::proportional((r * 1.1).max(8.0)), s.color);
    }
}

fn kresli_znacku(p: &egui::Painter, c: egui::Pos2, r: f32, z: styly::Znacka, s: egui::Stroke) {
    if z == styly::Znacka::Bod {
        p.circle_filled(c, 1.5, s.color);
    }
    let q = |b: (f64, f64)| c + egui::vec2(b.0 as f32 * r, -b.1 as f32 * r);
    for t in z.tvary() {
        match t {
            styly::Tvar::Cara(a, b) => {
                p.line_segment([q(a), q(b)], s);
            }
            styly::Tvar::Kruh(st, rr) => {
                p.circle_stroke(q(st), (rr as f32 * r).max(0.8), s);
            }
            styly::Tvar::Lomena(v, zavrena) => {
                let mut body: Vec<egui::Pos2> = v.iter().map(|b| q(*b)).collect();
                if zavrena {
                    body.push(body[0]);
                }
                p.add(egui::Shape::line(body, s));
            }
        }
    }
}

fn nastaveni_novych_bodu(ui: &mut egui::Ui, stav: &StavSpravce) {
    let mut n = nastaveni::akt();
    let mut zmena = false;
    ui.horizontal(|ui| {
        ui.label(trf!("Další číslo: {}", "Next number: {}", stav.dalsi_cislo));
        ui.add_space(12.0);
        ui.label(tr("Kód", "Code"));
        zmena |= ui.add(egui::TextEdit::singleline(&mut n.kod_bodu).desired_width(100.0)).changed();
        ui.add_space(12.0);
        let mut ma = n.vyska_bodu_nova.is_some();
        if ui.checkbox(&mut ma, tr("Předvolená výška", "Preset elevation")).changed() {
            n.vyska_bodu_nova = if ma { Some(n.vyska_bodu_nova.unwrap_or(0.0)) } else { None };
            zmena = true;
        }
        if let Some(z) = n.vyska_bodu_nova.as_mut() {
            zmena |= ui.add(egui::DragValue::new(z).speed(0.05).max_decimals(3).suffix(" m")).changed();
        }
    });
    info(
        ui,
        tr("Body se vytvářejí tlačítkem Nový bod (ruční / na povrchu) a Nový povrch > Z naklikaných bodů. \
         Výška: zadané x,y,z, jinak předvolená, jinak z aktivního povrchu.", "Points are created with the New point button (manual / on surface) and New surface > From clicked points. Elevation: entered x,y,z, otherwise preset, otherwise from the active surface."),
    );
    if zmena {
        nastaveni::zmen(|x| {
            x.kod_bodu = n.kod_bodu.trim().to_string();
            x.vyska_bodu_nova = n.vyska_bodu_nova;
        });
    }
}

fn nastaveni_importu(ui: &mut egui::Ui) {
    let n = nastaveni::akt();
    let formaty: [(Option<&str>, &str); 6] = [
        (None, tr("Automaticky (rozpozná se)", "Automatic (detected)")),
        (Some("PNEZD"), "PNEZD"),
        (Some("PENZD"), "PENZD"),
        (Some("PYXZD"), tr("PYXZD (S-JTSK kladné)", "PYXZD (S-JTSK positive)")),
        (Some("NEZD"), "NEZ"),
        (Some("ENZD"), "ENZ"),
    ];
    let oddelovace = [
        (Oddelovac::Mezera, "mezera"),
        (Oddelovac::Carka, tr("čárka", "comma")),
        (Oddelovac::Strednik, tr("středník", "semicolon")),
        (Oddelovac::Tabulator, tr("tabulátor", "tab")),
    ];
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("Formát", "Format"));
        let akt = formaty.iter().find(|f| f.0 == n.format.as_deref()).map_or(tr("Automaticky", "Automatic"), |f| f.1);
        egui::ComboBox::from_id_salt("imp_fmt").selected_text(akt).width(190.0).show_ui(ui, |ui| {
            for (f, t) in formaty {
                if ui.selectable_label(n.format.as_deref() == f, t).clicked() {
                    nastaveni::zmen(|x| x.format = f.map(String::from));
                }
            }
        });
        ui.add_space(8.0);
        ui.add_enabled_ui(n.format.is_some(), |ui| {
            ui.label(tr("Oddělovač", "Delimiter"));
            let akt = oddelovace.iter().find(|o| o.0 == n.oddelovac).map_or("mezera", |o| o.1);
            egui::ComboBox::from_id_salt("imp_odd").selected_text(akt).width(110.0).show_ui(ui, |ui| {
                for (o, t) in oddelovace {
                    if ui.selectable_label(n.oddelovac == o, t).clicked() {
                        nastaveni::zmen(|x| x.oddelovac = o);
                    }
                }
            });
        });
    });
    info(ui, tr("Platí pro tlačítko Import bodů. Automaticky se formát i oddělovač rozpoznají ze souboru.", "Applies to the Import points button. With Automatic the format and delimiter are detected from the file."));
}

/// Malé „i“ v kroužku s vysvětlením v tooltipu (místo šedého textu pod volbou).
fn info(ui: &mut egui::Ui, text: &str) {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
    ui.painter().circle_stroke(rect.center(), 7.0, egui::Stroke::new(1.0, SEDA));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, "i", FontId::proportional(11.0), SEDA);
    r.on_hover_text(text);
}

/// Volba: povrch nejde vybrat kliknutím (klik projde na body pod ním).
fn zaskrtnuti_nevybirat(ui: &mut egui::Ui) {
    let mut ano = nastaveni::akt().povrch_nevybirat;
    if ui
        .checkbox(&mut ano, tr("Povrch nejde vybrat kliknutím", "Surface cannot be selected by clicking"))
        .on_hover_text(
            tr("Klik do výkresu projde přes povrch na body a popisy pod ním. \
             Povrch pak vyberete v Prospektoru (aktivní povrch, strom) a nejde omylem posunout ani smazat.", "A click in the drawing passes through the surface to the points and labels under it. You then select the surface in the Prospector (active surface, tree) and it cannot be moved or deleted by mistake."),
        )
        .changed()
    {
        spravce::posli(Akce::PovrchyNevybirat(ano));
    }
}

/// Barva prvku stylu povrchu na tmavém pozadí („podle hladiny“ = výchozí).
fn barva_prvku(b: Barva, vychozi: Color32) -> Color32 {
    if b == styly::PODLE_HLADINY {
        vychozi
    } else {
        let c = rgb_aci(b);
        let jas = 0.3 * c.r() as f32 + 0.59 * c.g() as f32 + 0.11 * c.b() as f32;
        if jas < 110.0 { c.lerp_to_gamma(Color32::WHITE, 0.35) } else { c }
    }
}

/// Malá ikona stylu povrchu: trojúhelníky, vrstevnice, obrys, šipky.
fn ikona_stylu_povrchu(ui: &mut egui::Ui, st: &StylPovrchu, v: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(v, v), Sense::click());
    let p = ui.painter_at(rect);
    dlazdice_ikony(&p, rect, &resp);
    let r = rect.shrink(v * 0.14);
    let bod = |x: f32, y: f32| egui::pos2(r.left() + r.width() * x, r.top() + r.height() * y);
    let w = if v > 24.0 { 1.4 } else { 1.0 };
    if st.id.eq_ignore_ascii_case(styly::SKRYT) {
        p.circle_stroke(rect.center(), v * 0.25, egui::Stroke::new(w, SEDA));
        p.line_segment([bod(0.0, 1.0), bod(1.0, 0.0)], egui::Stroke::new(w, CERVENA));
        return resp;
    }
    if st.obrys {
        let c = barva_prvku(st.barva_obrys, Color32::from_rgb(90, 160, 220));
        p.rect_stroke(r, 2.0, egui::Stroke::new(w, c), egui::StrokeKind::Middle);
    }
    if st.tin {
        let c = barva_prvku(st.barva_tin, Color32::from_rgb(150, 150, 150));
        let s = egui::Stroke::new(w * 0.8, c);
        for (a, b) in [((0.0, 1.0), (0.5, 0.0)), ((0.5, 0.0), (1.0, 1.0)), ((0.0, 1.0), (1.0, 1.0)), ((0.5, 0.0), (0.5, 1.0))] {
            p.line_segment([bod(a.0, a.1), bod(b.0, b.1)], s);
        }
    }
    if st.vrstevnice {
        let ved = barva_prvku(st.barva_vedlejsi, Color32::from_rgb(230, 160, 70));
        let hl = barva_prvku(st.barva_hlavni, Color32::from_rgb(220, 110, 50));
        for (k, y) in [0.25f32, 0.5, 0.75].into_iter().enumerate() {
            let body: Vec<egui::Pos2> = (0..=8).map(|i| {
                let x = i as f32 / 8.0;
                bod(x, y + 0.08 * (x * 6.0 + k as f32).sin())
            }).collect();
            p.add(egui::Shape::line(body, egui::Stroke::new(if k == 1 { w * 1.4 } else { w }, if k == 1 { hl } else { ved })));
        }
    }
    if st.sipky {
        let c = barva_prvku(st.barva_sipek, Color32::from_rgb(110, 200, 240));
        let s = egui::Stroke::new(w, c);
        p.line_segment([bod(0.2, 0.3), bod(0.8, 0.7)], s);
        p.line_segment([bod(0.8, 0.7), bod(0.58, 0.68)], s);
        p.line_segment([bod(0.8, 0.7), bod(0.72, 0.5)], s);
    }
    resp
}

/// Počet bodů podle id stylu.
fn pocty_stylu(stav: &StavSpravce) -> std::collections::HashMap<String, usize> {
    let mut m = std::collections::HashMap::new();
    for (id, _) in stav.styl_bodu.values() {
        *m.entry(id.clone()).or_insert(0) += 1;
    }
    m
}

/// Barva značky stylu na tmavém pozadí.
fn barva_znacky(st: &StylBodu) -> Color32 {
    if st.skryt {
        SEDA
    } else if st.barva_znacky == styly::PODLE_HLADINY {
        Color32::from_rgb(240, 165, 60)
    } else {
        // tmavé barvy (modrá…) zesvětlit, aby byly na tmavém pozadí vidět
        let c = rgb_aci(st.barva_znacky);
        let jas = 0.3 * c.r() as f32 + 0.59 * c.g() as f32 + 0.11 * c.b() as f32;
        if jas < 110.0 {
            c.lerp_to_gamma(Color32::WHITE, 0.35)
        } else {
            c
        }
    }
}

/// Podklad ikony stylu – světlejší dlaždice s okrajem, při najetí zvýrazněná.
fn dlazdice_ikony(p: &egui::Painter, rect: egui::Rect, resp: &egui::Response) {
    let (pozadi, okraj) = if resp.hovered() {
        (Color32::from_rgb(44, 52, 62), MODRA)
    } else {
        (Color32::from_rgb(34, 37, 41), Color32::from_rgb(70, 75, 82))
    };
    p.rect_filled(rect, 4.0, pozadi);
    p.rect_stroke(rect.shrink(0.5), 4.0, egui::Stroke::new(1.0, okraj), egui::StrokeKind::Inside);
}

/// Malá ikona stylu bodu (čtvereček s jeho značkou).
fn ikona_stylu(ui: &mut egui::Ui, st: &StylBodu, velikost: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(velikost, velikost), Sense::click());
    let p = ui.painter_at(rect);
    dlazdice_ikony(&p, rect, &resp);
    let c = rect.center();
    let r = velikost * 0.36;
    let s = egui::Stroke::new(if velikost > 30.0 { 1.8 } else { 1.3 }, barva_znacky(st));
    if st.skryt {
        p.circle_stroke(c, r * 0.8, s);
        p.line_segment([c + egui::vec2(-r, r), c + egui::vec2(r, -r)], egui::Stroke::new(s.width, CERVENA));
    } else if st.znacka == styly::Znacka::Bod {
        p.circle_filled(c, 2.0, s.color);
        p.line_segment([c + egui::vec2(r * 0.4, -r * 0.3), c + egui::vec2(r, -r * 0.3)], egui::Stroke::new(1.0, SEDA));
    } else {
        kresli_znacku_stylu(&p, c, r, st, s);
    }
    resp
}

fn nahled_bodu(ui: &mut egui::Ui, st: &StylBodu, velikost: egui::Vec2, px: f32) {
    let (rect, _) = ui.allocate_exact_size(velikost, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 8.0, Color32::from_rgb(18, 20, 22));
    if st.skryt {
        p.text(rect.center(), Align2::CENTER_CENTER, tr("body se nekreslí", "points are not drawn"), FontId::proportional(13.0), SEDA);
        return;
    }
    let c = egui::pos2(rect.left() + velikost.x * 0.32, rect.center().y);
    let barva = |b: Barva| if b == styly::PODLE_HLADINY { Color32::from_rgb(240, 165, 60) } else { rgb_aci(b) };
    let bz = barva(st.barva_znacky);
    let r = (st.velikost as f32 * px / 2.0).max(1.0);
    let s = egui::Stroke::new(1.2, bz);
    kresli_znacku_stylu(&p, c, r, st, s);
    let h = (st.pismo as f32 * px).max(6.0);
    let dx = if st.znacka == styly::Znacka::Bod { 0.4 * h } else { r + 0.3 * h };
    let bt = if st.barva_textu == styly::PODLE_HLADINY { Color32::from_rgb(235, 235, 235) } else { rgb_aci(st.barva_textu) };
    let f = FontId::monospace(h);
    let vyska = st.text_vysky(313.40);
    let mut popis_y = c.y + 0.5 * h;
    match st.popisek {
        styly::Popisek::CisloVyska => {
            p.text(egui::pos2(c.x + dx, c.y - 0.3 * h), Align2::LEFT_BOTTOM, "1017", f.clone(), bt);
            p.text(egui::pos2(c.x + dx, c.y + 1.1 * h), Align2::LEFT_BOTTOM, &vyska, f.clone(), bt);
            popis_y = c.y + 2.5 * h;
        }
        styly::Popisek::JedenRadek => {
            p.text(egui::pos2(c.x + dx, c.y + 0.5 * h), Align2::LEFT_BOTTOM, format!("1017 {vyska}"), f.clone(), bt);
            popis_y = c.y + 1.9 * h;
        }
        styly::Popisek::JenVyska => {
            p.text(egui::pos2(c.x + dx, c.y + 0.5 * h), Align2::LEFT_BOTTOM, &vyska, f.clone(), bt);
            popis_y = c.y + 1.9 * h;
        }
        styly::Popisek::JenCislo => {
            p.text(egui::pos2(c.x + dx, c.y + 0.5 * h), Align2::LEFT_BOTTOM, "1017", f.clone(), bt);
            popis_y = c.y + 1.9 * h;
        }
        styly::Popisek::Nic => {}
    }
    if st.popis {
        p.text(egui::pos2(c.x + dx, popis_y), Align2::LEFT_BOTTOM, "TER", FontId::monospace(h * 0.8), bt.gamma_multiply(0.7));
    }
}

/// Liší se zvolený jazyk od jazyka, se kterým plugin běží?
fn jazyk_zmenen(volba: &str) -> bool {
    let bezi = crate::lang::jazyk();
    match volba {
        "cs" | "en" => volba != bezi,
        _ => {
            let sys = crate::lang::systemovy_jazyk();
            let auto = if sys.starts_with("cs") || sys.starts_with("sk") { "cs" } else { "en" };
            auto != bezi
        }
    }
}
