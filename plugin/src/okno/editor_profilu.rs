//! Editor vzhledu podélného profilu – samostatné okno s živým náhledem.
//!
//! Uprostřed se kreslí pohled stejným kódem jako ve výkrese
//! (`tra::pohled::kresli`), vlevo je struktura (pásy, čáry grafu), písma a
//! čáry, vpravo vlastnosti vybrané části. Upravuje se buď styl profilu (všechny
//! pohledy, které ho používají), nebo jen jeden pohled (výjimky).

use std::sync::{Mutex, OnceLock};

use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use ocs_plugin_api::host::codec::entities::text::{TextHorizontalAlignment as Hz, TextVerticalAlignment as Vt};
use ocs_plugin_api::host::codec::types::Color;
use ocs_plugin_api::host::{EntityType, Handle};
use crate::lang::tr;
use teren::trf;

use trasy::pasy::{DruhPasu, Pas, Poloha, Usek, Zdroj};
use trasy::pohled::{efektivni, nastaveni_s, DefinicePohledu, Osy, StylProfilu};
use trasy::vzhled::{self, Cara, Pismo, CARY, FONTY, PREDLOHY, ROLE, TLOUSTKY};

use super::cz;
use crate::spravce::{self, Akce, StavSpravce};
use crate::tra::pohled::{kresli, Podklady, Rozvrzeni, H_NIVELETA, H_NIV_POLYGON, H_TEREN};

// barvy okna (jako náhled editoru)
const POZADI: Color32 = Color32::from_rgb(22, 24, 28);
const PANEL: Color32 = Color32::from_rgb(31, 34, 39);
const PANEL2: Color32 = Color32::from_rgb(39, 43, 49);
const PANEL3: Color32 = Color32::from_rgb(48, 53, 60);
const CARA_UI: Color32 = Color32::from_rgb(53, 58, 66);
const TEXT: Color32 = Color32::from_rgb(230, 232, 235);
const TLUMENA: Color32 = Color32::from_rgb(143, 151, 161);
const AKCENT: Color32 = Color32::from_rgb(61, 139, 255);
const VAROVANI: Color32 = Color32::from_rgb(240, 164, 58);
const PLATNO_TMAVE: Color32 = Color32::from_rgb(17, 19, 23);
const LIST_TMAVE: Color32 = Color32::from_rgb(26, 29, 34);
const PLATNO_PAPIR: Color32 = Color32::from_rgb(217, 220, 225);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rezim {
    /// Upravuje se styl (všechny pohledy, které ho používají).
    Styl,
    /// Upravuje se jen jeden pohled (výjimky ze stylu).
    Pohled,
}

#[derive(Clone, PartialEq, Debug)]
enum Vyber {
    Pas(usize),
    Graf,
    Nadpis,
    Legenda,
    Pismo(&'static str),
    Cara(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Karta {
    Struktura,
    Pisma,
    Cary,
}

type Stav = (DefinicePohledu, String);

pub struct EditorProfilu {
    rezim: Rezim,
    styl_id: String,
    jmeno: String,
    /// Pohled pro náhled (a v režimu Pohled ten upravovaný): id, vložení.
    pohled: Option<(String, Handle)>,
    /// Upravovaná definice (u stylu `styl` prázdný a nastavení ve `vyjimky`).
    d: DefinicePohledu,
    puvodni: Stav,
    posledni: Stav,
    zpet: Vec<Stav>,
    vpred: Vec<Stav>,
    vyber: Vyber,
    karta: Karta,
    papir: bool,
    zoom: f32,
    posun: Vec2,
    /// Tažení výšky pásu: index, y myši na začátku, výška na začátku.
    tah: Option<(usize, f32, f64)>,
    /// Přetahování pásu ve struktuře.
    tazeny: Option<usize>,
    galerie: bool,
    dotaz_zavrit: bool,
    pozadano: Option<String>,
    cache: Option<(String, Vec<EntityType>, Rozvrzeni, Osy)>,
    ukazka: Option<Podklady>,
    pub otevreno: bool,
}

/// Styl profilu jako definice pro editor (nastavení = „výjimky“ nad schématem).
fn definice_stylu(s: &StylProfilu) -> DefinicePohledu {
    let mut d = DefinicePohledu::novy("", "", "");
    d.styl = String::new();
    d.vyjimky = s.hodnoty.clone();
    d.pasy = s.pasy.clone();
    d.meritko = s.meritko;
    d.prevyseni = s.prevyseni;
    d
}

fn jmeno_stylu(styly: &[StylProfilu], id: &str) -> String {
    styly.iter().find(|s| s.id == id).map_or_else(|| id.to_string(), |s| crate::lang::tr_dyn(&s.jmeno))
}

/// Stejné definice bez ohledu na otisk a osy (ty mění až kreslení).
fn stejne(a: &DefinicePohledu, b: &DefinicePohledu) -> bool {
    let (mut a, mut b) = (a.clone(), b.clone());
    a.otisk.clear();
    b.otisk.clear();
    a.osy = None;
    b.osy = None;
    a == b
}

impl EditorProfilu {
    /// Editor stylu; náhled na prvním pohledu, který styl používá (jinak na jakémkoli).
    pub fn pro_styl(stav: &StavSpravce, id: &str) -> Option<Self> {
        let s = stav.styly_profilu.iter().find(|s| s.id == id)?;
        let pohledy: Vec<_> = stav.trasy.iter().flat_map(|t| t.pohledy.iter()).collect();
        let p = pohledy.iter().find(|p| p.def.styl == id).or(pohledy.first()).map(|p| (p.id.clone(), p.insert));
        let d = definice_stylu(s);
        Some(Self::novy(Rezim::Styl, id.to_string(), crate::lang::tr_dyn(&s.jmeno), p, d))
    }

    /// Editor jednoho pohledu (výjimky ze stylu).
    pub fn pro_pohled(stav: &StavSpravce, id: &str) -> Option<Self> {
        let p = stav.trasy.iter().flat_map(|t| t.pohledy.iter()).find(|p| p.id == id)?;
        let d = efektivni(&p.def, &stav.styly_profilu);
        let jmeno = jmeno_stylu(&stav.styly_profilu, &d.styl);
        Some(Self::novy(Rezim::Pohled, d.styl.clone(), jmeno, Some((p.id.clone(), p.insert)), d))
    }

    fn novy(rezim: Rezim, styl_id: String, jmeno: String, pohled: Option<(String, Handle)>, d: DefinicePohledu) -> Self {
        let puvodni = (d.clone(), jmeno.clone());
        let vyber = d.pasy.iter().position(|p| p.druh.hodnotovy()).map_or(Vyber::Graf, Vyber::Pas);
        Self {
            rezim,
            styl_id,
            jmeno,
            pohled,
            d,
            posledni: puvodni.clone(),
            puvodni,
            zpet: Vec::new(),
            vpred: Vec::new(),
            vyber,
            karta: Karta::Struktura,
            papir: false,
            zoom: 1.0,
            posun: Vec2::ZERO,
            tah: None,
            tazeny: None,
            galerie: false,
            dotaz_zavrit: false,
            pozadano: None,
            cache: None,
            ukazka: None,
            otevreno: true,
        }
    }

    /// Nastavení pro snímky ukázky okna.
    #[cfg(feature = "ukazka")]
    pub fn ukazka_krok(&mut self, krok: &str, stav: &StavSpravce) {
        match krok {
            "4" => {
                self.karta = Karta::Pisma;
                self.vyber = Vyber::Pismo("hodnota");
            }
            "7" => {
                self.zoom = 2.6;
                self.posun = Vec2::new(-40.0, -560.0);
            }
            "5" => {
                self.papir = true;
                for (k, h) in vzhled::predloha("klasicky") {
                    self.nastav(stav, k, h);
                }
                self.vyber = Vyber::Graf;
            }
            _ => {}
        }
    }

    pub fn ma_zmeny(&self) -> bool {
        !stejne(&self.d, &self.puvodni.0) || self.jmeno != self.puvodni.1
    }

    /// Styly pro skládání nastavení (u stylu se skládá jen nad schématem).
    fn styly<'a>(&self, stav: &'a StavSpravce) -> &'a [StylProfilu] {
        match self.rezim {
            Rezim::Styl => &[],
            Rezim::Pohled => &stav.styly_profilu,
        }
    }

    fn hodnota(&self, stav: &StavSpravce, k: &str) -> String {
        nastaveni_s(&self.d, self.styly(stav)).text(k).to_string()
    }

    fn ano(&self, stav: &StavSpravce, k: &str) -> bool {
        matches!(self.hodnota(stav, k).trim(), "1" | "ano" | "true")
    }

    /// Nastaví hodnotu; stejná jako ve stylu (u stylu jako výchozí) = bez výjimky.
    fn nastav(&mut self, stav: &StavSpravce, k: &str, v: &str) {
        let mut c = self.d.clone();
        c.vyjimky.remove(k);
        let zaklad = nastaveni_s(&c, self.styly(stav)).text(k).to_string();
        if zaklad == v {
            self.d.vyjimky.remove(k);
        } else {
            self.d.vyjimky.insert(k.to_string(), v.to_string());
        }
    }

    fn pismo(&self, stav: &StavSpravce, role: &str) -> Pismo {
        Pismo::cti(&self.hodnota(stav, &format!("p.{role}")))
    }

    fn cara(&self, stav: &StavSpravce, k: &str) -> Cara {
        Cara::cti(&self.hodnota(stav, &format!("c.{k}")))
    }

    fn pasy_zmeneny(&mut self) {
        if self.rezim == Rezim::Pohled {
            self.d.pasy_ze_stylu = false;
        }
    }

    /// Definice pro náhled (geometrie z pohledu, vzhled z editoru).
    fn definice_nahledu(&self, stav: &StavSpravce) -> (DefinicePohledu, Vec<StylProfilu>) {
        match self.rezim {
            Rezim::Pohled => {
                let mut d = self.d.clone();
                d.pasy_ze_stylu = false;
                (d, stav.styly_profilu.clone())
            }
            Rezim::Styl => {
                let mut d = stav.podklady.as_ref().filter(|p| self.pohled.as_ref().is_some_and(|x| x.0 == p.pohled)).map_or_else(DefinicePohledu::default, |p| p.def.clone());
                d.styl = String::new();
                d.vyjimky = self.d.vyjimky.clone();
                d.pasy = self.d.pasy.clone();
                d.pasy_ze_stylu = false;
                d.meritko = self.d.meritko;
                d.prevyseni = self.d.prevyseni;
                (d, Vec::new())
            }
        }
    }

    fn uloz(&mut self) {
        match self.rezim {
            Rezim::Styl => {
                let s = StylProfilu {
                    id: self.styl_id.clone(),
                    jmeno: self.jmeno.clone(),
                    hodnoty: self.d.vyjimky.clone(),
                    pasy: self.d.pasy.clone(),
                    meritko: self.d.meritko,
                    prevyseni: self.d.prevyseni,
                };
                spravce::posli(Akce::UlozStylProfilu(Box::new(s)));
            }
            Rezim::Pohled => {
                if let Some((_, h)) = &self.pohled {
                    spravce::posli(Akce::PohledUpravit(*h, Box::new(self.d.clone())));
                }
            }
        }
        self.puvodni = (self.d.clone(), self.jmeno.clone());
    }

    fn zahod(&mut self) {
        self.d = self.puvodni.0.clone();
        self.jmeno = self.puvodni.1.clone();
        self.zpet.clear();
        self.vpred.clear();
        self.posledni = self.puvodni.clone();
    }

    fn krok_zpet(&mut self) {
        if let Some(s) = self.zpet.pop() {
            self.vpred.push((self.d.clone(), self.jmeno.clone()));
            (self.d, self.jmeno) = s;
            self.posledni = (self.d.clone(), self.jmeno.clone());
        }
    }

    fn krok_vpred(&mut self) {
        if let Some(s) = self.vpred.pop() {
            self.zpet.push((self.d.clone(), self.jmeno.clone()));
            (self.d, self.jmeno) = s;
            self.posledni = (self.d.clone(), self.jmeno.clone());
        }
    }

    /// Změny za snímek do historie (souvislé tažení = jeden krok).
    fn zaznamenej(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_down()) || self.tah.is_some() {
            return;
        }
        if !stejne(&self.d, &self.posledni.0) || self.jmeno != self.posledni.1 {
            self.zpet.push(std::mem::replace(&mut self.posledni, (self.d.clone(), self.jmeno.clone())));
            if self.zpet.len() > 200 {
                self.zpet.remove(0);
            }
            self.vpred.clear();
        }
    }

    /// Podklady pro náhled: z výkresu (vyžádá je), jinak ukázková data.
    fn podklady<'a>(&'a mut self, stav: &'a StavSpravce) -> Result<&'a Podklady, String> {
        if let Some((pid, _)) = &self.pohled {
            if let Some(p) = stav.podklady.as_ref().filter(|p| p.pohled == *pid) {
                return p.vysledek.as_ref().map_err(|e| e.clone());
            }
            if self.pozadano.as_deref() != Some(pid.as_str()) {
                self.pozadano = Some(pid.clone());
                spravce::posli(Akce::PodkladyPohledu(pid.clone()));
            }
            return Err(tr("Načítám pohled z výkresu…", "Loading the view from the drawing…").to_string());
        }
        Ok(self.ukazka.get_or_insert_with(ukazkove_podklady))
    }

    /// Celé okno (samostatné okno systému, u ukázky vložené).
    pub fn okno(&mut self, ctx: &egui::Context, stav: &StavSpravce) {
        nacti_pisma(ctx);
        let titulek = match self.rezim {
            Rezim::Styl => trf!("Vzhled profilu – styl {}", "Profile appearance – style {}", self.jmeno),
            Rezim::Pohled => trf!("Vzhled profilu – pohled {}", "Profile appearance – view {}", self.pohled.as_ref().map_or("", |p| p.0.as_str())),
        };
        let vlozene = ctx.embed_viewports();
        let okno = egui::ViewportBuilder::default().with_title(titulek.clone()).with_inner_size([1400.0, 860.0]).with_min_inner_size([980.0, 560.0]);
        let id = egui::ViewportId::from_hash_of("teren_editor_profilu");
        let mut obsah = |ui: &mut egui::Ui| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, POZADI);
            styl_ui(ui);
            self.obsah(ui, stav);
            if ui.input(|i| i.viewport().close_requested()) {
                if self.ma_zmeny() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.dotaz_zavrit = true;
                } else {
                    self.otevreno = false;
                }
            }
        };
        if vlozene {
            egui::Area::new(egui::Id::new("editor_profilu_vlozeny")).fixed_pos(Pos2::ZERO).order(egui::Order::Foreground).show(ctx, |ui| {
                let r = ctx.content_rect();
                ui.set_min_size(r.size());
                ui.set_max_size(r.size());
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
                obsah(&mut child);
            });
        } else {
            ctx.show_viewport_immediate(id, okno, |ui, _| obsah(ui));
        }
    }

    fn obsah(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let ctx = ui.ctx().clone();
        // klávesy
        if ctx.memory(|m| m.focused().is_none()) {
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z) && !i.modifiers.shift) {
                self.krok_zpet();
            }
            if ctx.input(|i| i.modifiers.command && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z)))) {
                self.krok_vpred();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                if let Vyber::Pas(i) = self.vyber {
                    self.odeber_pas(i);
                }
            }
        }
        egui::Panel::top("ep_horni").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 8))).show(ui, |ui| self.horni_lista(ui, stav));
        egui::Panel::bottom("ep_dolni").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 4))).show(ui, |ui| self.dolni_lista(ui));
        egui::Panel::left("ep_levy").exact_size(272.0).resizable(false).frame(egui::Frame::new().fill(PANEL)).show(ui, |ui| self.levy_panel(ui, stav));
        egui::Panel::right("ep_pravy").exact_size(310.0).resizable(false).frame(egui::Frame::new().fill(PANEL)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("ep_insp").auto_shrink([false, false]).show(ui, |ui| self.inspektor(ui, stav));
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(if self.papir { PLATNO_PAPIR } else { PLATNO_TMAVE })).show(ui, |ui| self.platno(ui, stav));
        if self.dotaz_zavrit {
            self.dialog_zavreni(&ctx);
        }
        if self.galerie {
            self.galerie_pasu(&ctx);
        }
        self.zaznamenej(&ctx);
    }

    // ── horní a dolní lišta ─────────────────────────────────────────────

    fn horni_lista(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        ui.horizontal(|ui| {
            match self.rezim {
                Rezim::Styl => {
                    ui.add(egui::TextEdit::singleline(&mut self.jmeno).desired_width(230.0).font(FontId::proportional(14.0)))
                        .on_hover_text(tr("jméno stylu", "style name"));
                    let n = stav.trasy.iter().flat_map(|t| t.pohledy.iter()).filter(|p| p.def.styl == self.styl_id).count();
                    chip(ui, &trf!("pohledů: {}", "views: {}", n), TLUMENA);
                }
                Rezim::Pohled => {
                    ui.label(RichText::new(tr("Styl", "Style")).color(TLUMENA));
                    let vybrany = jmeno_stylu(&stav.styly_profilu, &self.d.styl);
                    let mut novy = None;
                    egui::ComboBox::from_id_salt("ep_styl").selected_text(vybrany).width(220.0).show_ui(ui, |ui| {
                        for s in &stav.styly_profilu {
                            if ui.selectable_label(self.d.styl == s.id, crate::lang::tr_dyn(&s.jmeno)).clicked() && self.d.styl != s.id {
                                novy = Some(s.clone());
                            }
                        }
                    });
                    if let Some(s) = novy {
                        self.d.styl = s.id.clone();
                        self.d.vyjimky.clear();
                        self.d.pasy = s.pasy.clone();
                        self.d.pasy_ze_stylu = true;
                        self.styl_id = s.id;
                    }
                    let vyj = self.d.vyjimky.len();
                    if vyj > 0 {
                        chip(ui, &trf!("výjimky: {}", "exceptions: {}", vyj), VAROVANI);
                        if ui.small_button(tr("zrušit výjimky", "clear overrides")).clicked() {
                            self.d.vyjimky.clear();
                        }
                    }
                    if !self.d.pasy_ze_stylu {
                        if let Some(s) = stav.styly_profilu.iter().find(|s| s.id == self.d.styl) {
                            chip(ui, tr("vlastní pásy", "custom bands"), VAROVANI);
                            if ui.small_button(tr("vrátit pásy ze stylu", "restore bands from style")).clicked() {
                                self.d.pasy = s.pasy.clone();
                                self.d.pasy_ze_stylu = true;
                            }
                        }
                    }
                }
            }
            ui.add_space(8.0);
            let akt = self.hodnota(stav, "predloha");
            let mut volba = None;
            segment(ui, &PREDLOHY.iter().map(|p| (p.0, crate::lang::tr_tab(p.1))).collect::<Vec<_>>(), &akt, |v| volba = Some(v));
            if let Some(v) = volba {
                for (k, h) in vzhled::predloha(v) {
                    self.nastav(stav, k, h);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let zmeny = self.ma_zmeny();
                let popis = match self.rezim {
                    Rezim::Styl => tr("Uložit a překreslit", "Save and redraw"),
                    Rezim::Pohled => tr("Použít na pohled", "Apply to the view"),
                };
                if ui.add_enabled(zmeny, egui::Button::new(RichText::new(popis).strong().color(Color32::WHITE)).fill(AKCENT).min_size(Vec2::new(0.0, 28.0))).clicked() {
                    self.uloz();
                }
                if ui.add_enabled(zmeny, egui::Button::new(tr("Zahodit", "Discard")).min_size(Vec2::new(0.0, 28.0))).clicked() {
                    self.zahod();
                }
                ui.add_space(6.0);
                if ui.add_enabled(!self.vpred.is_empty(), egui::Button::new(tr("Znovu", "Redo"))).on_hover_text(tr("Znovu (Ctrl+Y)", "Redo (Ctrl+Y)")).clicked() {
                    self.krok_vpred();
                }
                if ui.add_enabled(!self.zpet.is_empty(), egui::Button::new(tr("Zpět", "Back"))).on_hover_text(tr("Zpět (Ctrl+Z)", "Undo (Ctrl+Z)")).clicked() {
                    self.krok_zpet();
                }
            });
        });
    }

    fn dolni_lista(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let mv = self.d.meritko / self.d.prevyseni.max(0.1);
            ui.label(RichText::new(trf!("M 1:{} / 1:{} · převýšení {}×", "S 1:{} / 1:{} · exaggeration {}×", cz(self.d.meritko, 0), cz(mv, 0), cz(self.d.prevyseni, 1))).color(TLUMENA).size(12.0));
            if let Some((_, _, r, _)) = &self.cache {
                let (s, v) = rozmer(r);
                ui.label(RichText::new(trf!("rozměr {} × {} mm", "size {} × {} mm", cz(s / self.d.meritko * 1000.0, 0), cz(v / self.d.meritko * 1000.0, 0))).color(TLUMENA).size(12.0));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.ma_zmeny() {
                    ui.label(RichText::new(tr("• neuložené změny", "• unsaved changes")).color(VAROVANI).size(12.0));
                } else {
                    ui.label(RichText::new(tr("bez neuložených změn", "no unsaved changes")).color(TLUMENA).size(12.0));
                }
            });
        });
    }

    // ── levý panel ──────────────────────────────────────────────────────

    fn levy_panel(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            for (k, n) in [(Karta::Struktura, tr("Struktura", "Structure")), (Karta::Pisma, tr("Písma", "Fonts")), (Karta::Cary, tr("Čáry a barvy", "Lines and colours"))] {
                let on = self.karta == k;
                let r = ui.add(egui::Button::new(RichText::new(n).color(if on { TEXT } else { TLUMENA })).frame(false));
                if on {
                    let b = r.rect;
                    ui.painter().line_segment([b.left_bottom() + Vec2::new(0.0, 3.0), b.right_bottom() + Vec2::new(0.0, 3.0)], Stroke::new(2.0, AKCENT));
                }
                if r.clicked() {
                    self.karta = k;
                    if k == Karta::Pisma && !matches!(self.vyber, Vyber::Pismo(_)) {
                        self.vyber = Vyber::Pismo("hodnota");
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.separator();
        egui::ScrollArea::vertical().id_salt("ep_levy_s").auto_shrink([false, false]).show(ui, |ui| match self.karta {
            Karta::Struktura => self.struktura(ui, stav),
            Karta::Pisma => self.seznam_pisem(ui, stav),
            Karta::Cary => self.seznam_car(ui, stav),
        });
    }

    fn struktura(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let mut klik: Option<Vyber> = None;
        let mut oko: Option<usize> = None;
        skupina(ui, tr("Výkres", "Drawing"), None);
        if radek(ui, matches!(self.vyber, Vyber::Nadpis), "H", tr("Nadpis a měřítko", "Title and scale"), "", false, None).0.clicked() {
            klik = Some(Vyber::Nadpis);
        }
        let mut obdelniky: Vec<(usize, Rect)> = Vec::new();
        let mut zony: Vec<(Poloha, Rect)> = Vec::new();
        for (poloha, nazev) in [(Poloha::Nad, tr("Pásy nad grafem", "Bands above the graph")), (Poloha::Pod, tr("Pásy pod grafem", "Bands below the graph"))] {
            if poloha == Poloha::Pod {
                skupina(ui, tr("Graf", "Graph"), None);
                if radek(ui, self.vyber == Vyber::Graf, "G", tr("Graf, mřížka a stupnice", "Graph, grid and scale"), "", false, None).0.clicked() {
                    klik = Some(Vyber::Graf);
                }
                for (k, n) in CARY.iter().filter(|c| !matches!(c.0, "pasy" | "mrizka")) {
                    let c = self.cara(stav, k);
                    let r = radek_cary(ui, self.vyber == Vyber::Cara(k), &c, crate::lang::tr_tab(n));
                    if r.clicked() {
                        klik = Some(Vyber::Cara(k));
                    }
                }
            }
            let pocet = self.d.pasy.iter().filter(|p| p.poloha == poloha).count();
            let z = skupina(ui, nazev, Some(pocet));
            zony.push((poloha, z));
            for (i, p) in self.d.pasy.iter().enumerate().filter(|(_, p)| p.poloha == poloha) {
                let vy = cz(p.vyska, if p.vyska.fract().abs() > 1e-9 { 1 } else { 0 });
                let vyj = self.rezim == Rezim::Pohled && !self.d.pasy_ze_stylu;
                let (r, o) = radek(ui, self.vyber == Vyber::Pas(i), zkratka(p.druh), &nazev_pasu(p), &vy, p.skryty, Some(!p.skryty));
                let _ = vyj;
                if o {
                    oko = Some(i);
                } else if r.drag_started() {
                    self.tazeny = Some(i);
                } else if r.clicked() {
                    klik = Some(Vyber::Pas(i));
                }
                obdelniky.push((i, r.rect));
            }
            if pocet == 0 {
                ui.label(RichText::new(tr("   přetáhněte sem pás", "   drag a band here")).color(TLUMENA).size(11.0));
            }
        }
        skupina(ui, tr("Pod profilem", "Below the profile"), None);
        let leg = self.ano(stav, "legenda");
        let (r, o) = radek(ui, self.vyber == Vyber::Legenda, "L", tr("Legenda", "Legend"), "", !leg, Some(leg));
        if o {
            self.nastav(stav, "legenda", if leg { "0" } else { "1" });
        } else if r.clicked() {
            klik = Some(Vyber::Legenda);
        }
        ui.add_space(8.0);
        if ui.add_sized([ui.available_width() - 16.0, 30.0], egui::Button::new(RichText::new(tr("+ Přidat pás", "+ Add band")).color(AKCENT))).clicked() {
            self.galerie = true;
        }
        // přetahování pásů
        if let Some(t) = self.tazeny {
            let mys = ui.ctx().pointer_latest_pos();
            if let Some(m) = mys {
                // čára vložení
                if let Some((j, r)) = obdelniky.iter().find(|(_, r)| r.y_range().contains(m.y)) {
                    let za = m.y > r.center().y;
                    let y = if za { r.bottom() } else { r.top() };
                    ui.painter().line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], Stroke::new(2.0, AKCENT));
                    if ui.input(|i| i.pointer.any_released()) {
                        self.presun_pas(t, Some((*j, za)), None);
                        self.tazeny = None;
                    }
                } else if let Some((pol, _)) = zony.iter().find(|(_, r)| r.expand2(Vec2::new(0.0, 6.0)).contains(m)) {
                    if ui.input(|i| i.pointer.any_released()) {
                        self.presun_pas(t, None, Some(*pol));
                        self.tazeny = None;
                    }
                }
            }
            if ui.input(|i| i.pointer.any_released()) {
                self.tazeny = None;
            }
        }
        if let Some(i) = oko {
            self.d.pasy[i].skryty = !self.d.pasy[i].skryty;
            self.pasy_zmeneny();
        }
        if let Some(v) = klik {
            self.vyber = v;
        }
    }

    fn presun_pas(&mut self, i: usize, cil: Option<(usize, bool)>, zona: Option<Poloha>) {
        if i >= self.d.pasy.len() {
            return;
        }
        let mut p = self.d.pasy.remove(i);
        let novy = match (cil, zona) {
            (Some((j, za)), _) => {
                let j = if j > i { j - 1 } else { j };
                if j >= self.d.pasy.len() {
                    self.d.pasy.push(p);
                    self.d.pasy.len() - 1
                } else {
                    p.poloha = self.d.pasy[j].poloha;
                    let k = if za { j + 1 } else { j };
                    self.d.pasy.insert(k, p);
                    k
                }
            }
            (None, Some(pol)) => {
                p.poloha = pol;
                if pol == Poloha::Nad {
                    self.d.pasy.insert(0, p);
                    0
                } else {
                    self.d.pasy.push(p);
                    self.d.pasy.len() - 1
                }
            }
            _ => {
                self.d.pasy.insert(i, p);
                i
            }
        };
        self.vyber = Vyber::Pas(novy);
        self.pasy_zmeneny();
    }

    fn odeber_pas(&mut self, i: usize) {
        if i < self.d.pasy.len() {
            self.d.pasy.remove(i);
            self.pasy_zmeneny();
            self.vyber = if self.d.pasy.is_empty() { Vyber::Graf } else { Vyber::Pas(i.min(self.d.pasy.len() - 1)) };
        }
    }

    fn seznam_pisem(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Každá role písma se ve výkrese stane textovým stylem TER_PP_… s písmem TrueType. Změna platí pro všechny pásy, které roli používají.", "Each text role becomes a TER_PP_… text style with a TrueType font in the drawing. A change applies to all bands that use the role.")).color(TLUMENA).size(11.5));
        ui.add_space(6.0);
        for (k, n) in ROLE {
            let p = self.pismo(stav, k);
            let on = self.vyber == Vyber::Pismo(k);
            let ukazka = match k {
                "nadpis" => "Podélný profil",
                "zahlavi" => "Kóta dna potrubí",
                "hodnota" => "385,62  0,165 00",
                "usek" => "DN300 – PVC – 83,00",
                "graf" => "Š3  S.R. 383,00", // i18n:ne
                _ => "M 1:500 / 1:50",
            };
            let r = egui::Frame::new()
                .fill(if on { Color32::from_rgba_unmultiplied(61, 139, 255, 40) } else { PANEL2 })
                .stroke(Stroke::new(1.0, if on { AKCENT } else { CARA_UI }))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(ukazka).font(FontId::new(17.0, rodina(&p.font, p.tucne))).color(TEXT).italics_if(p.kurziva));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lang::tr_tab(n)).color(TLUMENA).size(11.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("{} · {} mm", p.font, cz(p.vyska, 1))).color(TLUMENA).size(11.0));
                        });
                    });
                })
                .response
                .interact(Sense::click());
            if r.clicked() {
                self.vyber = Vyber::Pismo(k);
            }
            ui.add_space(4.0);
        }
    }

    fn seznam_car(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        ui.add_space(6.0);
        ui.label(RichText::new(tr("Tloušťky čar", "Line weights")).strong());
        ui.label(RichText::new(tr("Tři tloušťky pro celý profil; čáry je jen odkazují.", "Three weights for the whole profile; lines only refer to them.")).color(TLUMENA).size(11.5));
        for (k, n) in TLOUSTKY {
            let klic = format!("tl.{k}");
            let mut v: f64 = self.hodnota(stav, &klic).replace(',', ".").parse().unwrap_or(0.25);
            ui.horizontal(|ui| {
                let sila = match k {
                    "tenka" => 1.0,
                    "stredni" => 2.0,
                    _ => 3.0,
                };
                let (r, _) = ui.allocate_exact_size(Vec2::new(26.0, 14.0), Sense::hover());
                ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(sila, TEXT));
                ui.add_sized([90.0, 18.0], egui::Label::new(crate::lang::tr_tab(n)));
                if ui.add(egui::DragValue::new(&mut v).range(0.0..=2.11).speed(0.01).max_decimals(2).suffix(" mm")).changed() {
                    self.nastav(stav, &klic, &format!("{v}"));
                }
            });
        }
        ui.add_space(10.0);
        ui.label(RichText::new(tr("Čáry", "Lines")).strong());
        for (k, n) in CARY {
            let c = self.cara(stav, k);
            if radek_cary(ui, self.vyber == Vyber::Cara(k), &c, crate::lang::tr_tab(n)).clicked() {
                self.vyber = Vyber::Cara(k);
            }
        }
        ui.add_space(10.0);
        ui.label(RichText::new(tr("Barva textů", "Text colour")).strong());
        let mut b = vzhled::barva(&self.hodnota(stav, "c.text"));
        if vyber_barvy(ui, "ep_text", &mut b) {
            self.nastav(stav, "c.text", &b.map(vzhled::hex).unwrap_or_default());
        }
        ui.label(RichText::new(tr("„Podle hladiny“ = bílá na obrazovce, černá při tisku.", "“By layer” = white on screen, black when printed.")).color(TLUMENA).size(11.0));
    }

    // ── náhled ──────────────────────────────────────────────────────────

    fn platno(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        let (rect, odpoved) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        // lišta nad náhledem
        let lista = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 34.0));
        {
            let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(lista.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
            let mut papir = self.papir;
            segment(&mut lui, &[("ocs", tr("Pozadí OCS", "OCS background")), ("papir", tr("Papír", "Paper"))], if papir { "papir" } else { "ocs" }, |v| papir = v == "papir");
            self.papir = papir;
            lui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Na šířku", "Fit width")).clicked() {
                    self.zoom = 1.0;
                    self.posun = Vec2::ZERO;
                }
                ui.label(RichText::new(format!("{} %", (self.zoom * 100.0).round())).color(if self.papir { Color32::from_gray(60) } else { TLUMENA }));
                ui.label(RichText::new(tr("klik = výběr · táhni modrý úchyt = výška pásu · kolečko = lupa · pravé tlačítko = posun", "click = select · drag the blue handle = band height · wheel = zoom · right button = pan")).color(if self.papir { Color32::from_gray(80) } else { TLUMENA }).size(11.5));
            });
        }
        let oblast = Rect::from_min_max(Pos2::new(rect.left(), lista.bottom()), rect.max);
        let (d, styly) = self.definice_nahledu(stav);
        let klic = format!("{}|{:?}|{}", d.zapis().join("|"), self.pohled.as_ref().map(|p| &p.0), styly.iter().map(|s| s.otisk()).collect::<String>());
        let papir = self.papir;
        let pk = match self.podklady(stav) {
            Ok(p) => p.clone(),
            Err(e) => {
                ui.painter().text(oblast.center(), Align2::CENTER_CENTER, e, FontId::proportional(15.0), TLUMENA);
                return;
            }
        };
        if self.cache.as_ref().is_none_or(|c| c.0 != klic) {
            let nast = nastaveni_s(&d, &styly);
            let (ent, rozv, osy) = kresli(&pk, &d, &nast);
            self.cache = Some((klic, ent, rozv, osy));
        }
        let Some((_, ent, rozv, osy)) = &self.cache else { return };
        let (bx0, by0, bx1, by1) = meze(rozv, &d);
        let m = d.meritko / 1000.0;
        let okraj = 28.0;
        let s0 = ((oblast.width() - 2.0 * okraj) / (bx1 - bx0) as f32).min((oblast.height() - 2.0 * okraj) / (by1 - by0) as f32 * 3.0).max(0.01);
        let s = s0 * self.zoom;
        // lupa kolečkem kolem myši, posun pravým tlačítkem
        if let Some(mys) = odpoved.hover_pos() {
            let kolecko = ui.input(|i| i.smooth_scroll_delta.y);
            if kolecko.abs() > 0.1 {
                let z0 = self.zoom;
                self.zoom = (self.zoom * (1.0 + kolecko * 0.0015)).clamp(0.3, 12.0);
                let f = self.zoom / z0;
                let stred = oblast.left_top() + Vec2::new(okraj, okraj) + self.posun;
                self.posun += (mys - stred) * (1.0 - f);
            }
        }
        if odpoved.dragged_by(egui::PointerButton::Secondary) || odpoved.dragged_by(egui::PointerButton::Middle) {
            self.posun += odpoved.drag_delta();
        }
        let pocatek = oblast.left_top() + Vec2::new(okraj, okraj) + self.posun;
        let na_obr = |x: f64, y: f64| -> Pos2 { pocatek + Vec2::new(((x - bx0) as f32) * s, ((by1 - y) as f32) * s) };
        let p = ui.painter_at(oblast);
        // list papíru
        let list = Rect::from_min_max(na_obr(bx0, by1) - Vec2::splat(14.0), na_obr(bx1, by0) + Vec2::splat(14.0));
        p.rect_filled(list, 2.0, if papir { Color32::WHITE } else { LIST_TMAVE });
        if !papir {
            p.rect_stroke(list, 2.0, Stroke::new(1.0, Color32::from_rgb(43, 47, 54)), egui::StrokeKind::Outside);
        }
        let px_mm = s * m as f32;
        for e in ent {
            kresli_entitu(&p, e, &na_obr, s, px_mm, papir);
        }
        // výběr
        let obd = |r: [f64; 4]| Rect::from_two_pos(na_obr(r[0], r[1]), na_obr(r[2], r[3]));
        let sirka_x1 = rozv.graf[2];
        let mut uchyt: Option<(usize, Rect)> = None;
        match &self.vyber {
            Vyber::Pas(i) => {
                if let Some((_, y0, y1)) = rozv.pasy.iter().find(|x| x.0 == *i) {
                    let r = obd([rozv.vlevo, *y1, sirka_x1, *y0]);
                    p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(61, 139, 255, 26));
                    p.rect_stroke(r, 0.0, Stroke::new(1.5, AKCENT), egui::StrokeKind::Inside);
                    let u = Rect::from_center_size(Pos2::new(na_obr(rozv.vlevo, 0.0).x + 26.0 * px_mm.min(1.0) + 30.0, r.bottom()), Vec2::new(36.0, 7.0));
                    p.rect_filled(u, 3.5, AKCENT);
                    uchyt = Some((*i, u.expand2(Vec2::new(8.0, 4.0))));
                }
            }
            Vyber::Graf => {
                p.rect_stroke(obd(rozv.graf), 0.0, Stroke::new(1.5, AKCENT), egui::StrokeKind::Outside);
            }
            Vyber::Nadpis => {
                if let Some(r) = rozv.nadpis {
                    p.rect_stroke(obd(r).expand(3.0), 3.0, Stroke::new(1.5, AKCENT), egui::StrokeKind::Outside);
                }
            }
            Vyber::Legenda => {
                if let Some(r) = rozv.legenda {
                    p.rect_stroke(obd(r).expand(3.0), 3.0, Stroke::new(1.5, AKCENT), egui::StrokeKind::Outside);
                }
            }
            Vyber::Cara(k) => {
                for b in body_cary(&pk, osy, k) {
                    let pts: Vec<Pos2> = b.iter().map(|q| na_obr(q[0], q[1])).collect();
                    p.add(egui::Shape::line(pts, Stroke::new(6.0, Color32::from_rgba_unmultiplied(61, 139, 255, 70))));
                }
            }
            Vyber::Pismo(_) => {}
        }
        // tažení výšky pásu
        let mys = ui.ctx().pointer_latest_pos();
        if let Some((i, u)) = uchyt {
            if mys.is_some_and(|m| u.contains(m)) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if odpoved.drag_started_by(egui::PointerButton::Primary) && odpoved.interact_pointer_pos().is_some_and(|m| u.contains(m)) {
                self.tah = Some((i, odpoved.interact_pointer_pos().map_or(0.0, |m| m.y), self.d.pasy.get(i).map_or(5.0, |p| p.vyska)));
            }
        }
        if let Some((i, y0, v0)) = self.tah {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            if let Some(m) = mys {
                let hustota: f64 = self.hodnota(stav, "hustota").replace(',', ".").parse().unwrap_or(1.0);
                let dy = (m.y - y0) as f64 / px_mm as f64 / hustota.max(0.1);
                let nova = ((v0 + dy) * 2.0).round() / 2.0;
                if let Some(p) = self.d.pasy.get_mut(i) {
                    let nova = nova.clamp(3.0, 60.0);
                    if (p.vyska - nova).abs() > 1e-9 {
                        p.vyska = nova;
                        self.pasy_zmeneny();
                    }
                }
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                self.tah = None;
            }
            return;
        }
        // klik = výběr
        if odpoved.clicked() {
            if let Some(m) = odpoved.interact_pointer_pos() {
                let kam = |x: f32, y: f32| -> (f64, f64) { (bx0 + ((x - pocatek.x) / s) as f64, by1 - ((y - pocatek.y) / s) as f64) };
                let (x, y) = kam(m.x, m.y);
                let tol = 6.0 / s as f64;
                let mut novy = None;
                // čáry grafu
                for (k, _) in CARY.iter().filter(|c| matches!(c.0, "teren" | "niveleta" | "polygon" | "povrchy")) {
                    for b in body_cary(&pk, osy, k) {
                        if b.windows(2).any(|w| vzd_usecka([x, y], w[0], w[1]) < tol) {
                            novy = Some(Vyber::Cara(k));
                        }
                    }
                    if novy.is_some() {
                        break;
                    }
                }
                let v = |r: [f64; 4]| x >= r[0].min(r[2]) && x <= r[0].max(r[2]) && y >= r[1].min(r[3]) && y <= r[1].max(r[3]);
                if novy.is_none() {
                    novy = rozv.pasy.iter().find(|q| x >= rozv.vlevo && x <= sirka_x1 && y <= q.1 && y >= q.2).map(|q| Vyber::Pas(q.0));
                }
                if novy.is_none() && rozv.nadpis.is_some_and(v) {
                    novy = Some(Vyber::Nadpis);
                }
                if novy.is_none() && rozv.legenda.is_some_and(v) {
                    novy = Some(Vyber::Legenda);
                }
                if novy.is_none() && v([rozv.vlevo, rozv.graf[1], rozv.graf[2], rozv.graf[3]]) {
                    novy = Some(Vyber::Graf);
                }
                if let Some(n) = novy {
                    if matches!(n, Vyber::Cara(_)) && self.karta == Karta::Pisma {
                        self.karta = Karta::Struktura;
                    }
                    self.vyber = n;
                }
            }
        }
    }

    // ── inspektor ───────────────────────────────────────────────────────

    fn inspektor(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        ui.add_space(12.0);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match self.vyber.clone() {
                Vyber::Pas(i) => self.insp_pasu(ui, stav, i),
                Vyber::Graf => self.insp_grafu(ui, stav),
                Vyber::Nadpis => self.insp_nadpisu(ui, stav),
                Vyber::Legenda => {
                    hlava(ui, tr("Legenda", "Legend"), tr("Legenda čar", "Line legend"));
                    let mut b = self.ano(stav, "legenda");
                    if ui.checkbox(&mut b, tr("Zobrazit legendu pod profilem", "Show the legend below the profile")).changed() {
                        self.nastav(stav, "legenda", if b { "1" } else { "0" });
                    }
                    poznamka(ui, tr("Legenda vypíše čáry grafu (terén, niveleta, další povrchy) v jejich barvě.", "The legend lists the graph lines (terrain, vertical alignment, other surfaces) in their colours."));
                    self.odkaz_pisma(ui, stav, "legenda");
                }
                Vyber::Pismo(k) => self.insp_pisma(ui, stav, k),
                Vyber::Cara(k) => self.insp_cary(ui, stav, k),
            }
            if self.rezim == Rezim::Pohled {
                ui.add_space(10.0);
                poznamka(ui, tr("Upravujete jen tento pohled: změny nastavení jsou výjimky ze stylu, ostatní pohledy zůstanou beze změny.", "You are editing this view only: changes are exceptions to the style, other views stay unchanged."));
            }
        });
    }

    fn insp_pasu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, i: usize) {
        let povrchy: Vec<(String, String)> = match self.podklady(stav) {
            Ok(p) => p.povrchy.clone(),
            Err(_) => stav.povrchy.iter().map(|z| (z.id.clone(), z.jmeno.clone())).collect(),
        };
        let Some(pred) = self.d.pasy.get(i).cloned() else {
            ui.label(RichText::new(tr("Vyberte část profilu v náhledu nebo vlevo ve struktuře.", "Select a part of the profile in the preview or in the structure on the left.")).color(TLUMENA));
            return;
        };
        let mut p = pred.clone();
        ui.label(RichText::new(format!("{} · {}", tr("Pás", "Band"), crate::lang::tr_tab(p.druh.nazev())).to_uppercase()).color(TLUMENA).size(11.0));
        let mut nazev = nazev_pasu(&p);
        if ui.add(egui::TextEdit::singleline(&mut nazev).font(FontId::proportional(15.0)).desired_width(f32::INFINITY)).changed() {
            p.nazev = nazev;
        }
        sekce(ui, tr("Obsah", "Content"));
        pole(ui, tr("Typ", "Type"), |ui| {
            egui::ComboBox::from_id_salt(("ep_druh", i)).selected_text(crate::lang::tr_tab(p.druh.nazev())).width(ui.available_width()).show_ui(ui, |ui| {
                for d in DruhPasu::VSE {
                    if ui.selectable_label(p.druh == d, crate::lang::tr_tab(d.nazev())).clicked() && p.druh != d {
                        let n = Pas::novy(d);
                        p.druh = d;
                        p.zdroj = n.zdroj;
                        p.zdroj2 = n.zdroj2;
                    }
                }
            });
        });
        if matches!(p.druh, DruhPasu::Kota | DruhPasu::Rozdil) {
            pole(ui, if p.druh == DruhPasu::Rozdil { tr("Výška A", "Elevation A") } else { tr("Čte z", "Reads from") }, |ui| vyber_zdroje(ui, ("ep_z1", i), &mut p.zdroj, &povrchy));
        }
        if p.druh == DruhPasu::Rozdil {
            pole(ui, tr("minus B", "minus B"), |ui| vyber_zdroje(ui, ("ep_z2", i), &mut p.zdroj2, &povrchy));
        }
        if p.druh.hodnotovy() || p.druh == DruhPasu::Vzdalenosti {
            ui.label(RichText::new(tr("Kde psát hodnoty", "Where to write values")).color(TLUMENA).size(12.0));
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut p.lomy, tr("lomové body", "break points"));
                ui.checkbox(&mut p.body_trasy, tr("hlavní body trasy", "main alignment points"));
            });
            pole(ui, tr("Po intervalu", "By interval"), |ui| {
                ui.add(egui::DragValue::new(&mut p.krok).range(0.0..=1000.0).speed(1.0).suffix(" m")).on_hover_text(tr("0 = bez pravidelného kroku", "0 = no regular interval"));
            });
        }
        if p.druh.hodnotovy() || p.druh == DruhPasu::Sklony {
            pole(ui, tr("Des. místa", "Decimals"), |ui| {
                let mut v = p.desetin.to_string();
                segment(ui, &[("0", "0"), ("1", "0,0"), ("2", "0,00"), ("3", "0,000")], &v.clone(), |x| v = x.to_string());
                p.desetin = v.parse().unwrap_or(p.desetin);
            });
        }
        if p.druh == DruhPasu::Text {
            pole(ui, tr("Text", "Text"), |ui| {
                ui.add(egui::TextEdit::singleline(&mut p.text).desired_width(f32::INFINITY));
            });
        }
        if p.druh == DruhPasu::Useky {
            ui.label(RichText::new(tr("Úseky (od – do, text)", "Sections (from – to, text)")).color(TLUMENA).size(12.0));
            let mut smazat = None;
            for (j, u) in p.useky.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut u.od).speed(1.0).max_decimals(2));
                    ui.label("–");
                    ui.add(egui::DragValue::new(&mut u.do_).speed(1.0).max_decimals(2).custom_formatter(|v, _| if v >= 1e8 { tr("konec", "end").to_string() } else { cz(v, 2) }));
                    ui.add(egui::TextEdit::singleline(&mut u.text).desired_width(110.0));
                    if ui.small_button("×").clicked() {
                        smazat = Some(j);
                    }
                });
            }
            if let Some(j) = smazat {
                p.useky.remove(j);
            }
            if ui.small_button(tr("+ úsek", "+ segment")).clicked() {
                let od = p.useky.last().map_or(0.0, |u| if u.do_ >= 1e8 { u.od } else { u.do_ });
                p.useky.push(Usek { od, do_: 1e9, text: String::new() });
            }
        }
        sekce(ui, tr("Vzhled", "Appearance"));
        pole(ui, tr("Poloha", "Position"), |ui| {
            let mut v = if p.poloha == Poloha::Nad { "nad" } else { "pod" };
            segment(ui, &[("nad", tr("Nad grafem", "Above the graph")), ("pod", tr("Pod grafem", "Below the graph"))], v, |x| v = x);
            p.poloha = if v == "nad" { Poloha::Nad } else { Poloha::Pod };
        });
        pole(ui, tr("Výška", "Elevation"), |ui| {
            ui.add(egui::Slider::new(&mut p.vyska, 3.0..=40.0).step_by(0.5).suffix(" mm"));
        });
        pole(ui, tr("Písmo", "Font"), |ui| {
            let akt = p.role_pisma().to_string();
            let popis = ROLE.iter().find(|r| r.0 == akt).map_or(akt.clone(), |r| crate::lang::tr_tab(r.1).to_string());
            egui::ComboBox::from_id_salt(("ep_pismo", i)).selected_text(popis).width(ui.available_width()).show_ui(ui, |ui| {
                for (k, n) in ROLE.iter().filter(|r| matches!(r.0, "hodnota" | "usek" | "graf")) {
                    let pis = self.pismo(stav, k);
                    if ui.selectable_label(akt == *k, format!("{} ({} {})", crate::lang::tr_tab(n), pis.font, cz(pis.vyska, 1))).clicked() {
                        p.pismo = k.to_string();
                    }
                }
            });
        });
        if p.druh.textovy() {
            pole(ui, tr("Zarovnání", "Alignment"), |ui| {
                let mut v = if p.na_stred { "stred" } else { "vlevo" };
                segment(ui, &[("vlevo", tr("Vlevo", "Left")), ("stred", tr("Na střed", "Centre"))], v, |x| v = x);
                p.na_stred = v == "stred";
            });
        }
        ui.checkbox(&mut p.podbarvit, tr("Podbarvit pás", "Shade the band"));
        ui.checkbox(&mut p.skryty, tr("Skrýt pás (zůstane ve stylu)", "Hide band (stays in the style)"));
        ui.add_space(10.0);
        let mut dupl = false;
        let mut odebrat = false;
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 8.0) / 2.0;
            dupl = ui.add_sized([w, 28.0], egui::Button::new(tr("Duplikovat", "Duplicate"))).clicked();
            odebrat = ui.add_sized([w, 28.0], egui::Button::new(RichText::new(tr("Odebrat pás", "Remove band")).color(Color32::from_rgb(255, 138, 128)))).clicked();
        });
        if p != pred {
            self.d.pasy[i] = p;
            self.pasy_zmeneny();
        }
        if dupl {
            let mut n = self.d.pasy[i].clone();
            n.nazev = trf!("{} (kopie)", "{} (copy)", nazev_pasu(&n));
            self.d.pasy.insert(i + 1, n);
            self.vyber = Vyber::Pas(i + 1);
            self.pasy_zmeneny();
        }
        if odebrat {
            self.odeber_pas(i);
        }
    }

    fn insp_grafu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        hlava(ui, tr("Graf", "Graph"), tr("Graf, mřížka a stupnice", "Graph, grid and scale"));
        sekce(ui, tr("Měřítko", "Scale"));
        pole(ui, tr("Délky 1:", "Lengths 1:"), |ui| {
            let akt = format!("{}", self.d.meritko);
            let mut v = None;
            segment(ui, &[("250", "250"), ("500", "500"), ("1000", "1000"), ("2000", "2000")], &akt, |x| v = Some(x));
            if let Some(x) = v {
                self.d.meritko = x.parse().unwrap_or(self.d.meritko);
            }
        });
        pole(ui, "", |ui| {
            ui.add(egui::DragValue::new(&mut self.d.meritko).range(10.0..=100000.0).speed(10.0).max_decimals(0).prefix(tr("vlastní 1:", "custom 1:")));
        });
        pole(ui, tr("Převýšení", "Exaggeration"), |ui| {
            ui.add(egui::Slider::new(&mut self.d.prevyseni, 1.0..=50.0).step_by(0.5).suffix(" ×"));
        });
        poznamka(ui, &trf!("Výšky vycházejí v měřítku 1:{}.", "Heights are at scale 1:{}.", cz(self.d.meritko / self.d.prevyseni.max(0.1), 0)));
        if self.rezim == Rezim::Styl {
            poznamka(ui, tr("U stylu je měřítko výchozí pro nové pohledy; náhled ho použije.", "In a style the scale is the default for new views; the preview uses it."));
        }
        sekce(ui, tr("Mřížka a stupnice", "Grid and scale"));
        self.zaskrt(ui, stav, "mrizka", tr("Mřížka", "Grid"));
        pole(ui, tr("Výšk. stupnice", "Elevation scale"), |ui| {
            let akt = self.hodnota(stav, "stupnice");
            let mut v = None;
            segment(ui, &[("skryt", tr("Skrýt", "Hide")), ("vlevo", tr("Vlevo", "Left")), ("obe", tr("Obě", "Both"))], &akt, |x| v = Some(x));
            if let Some(x) = v {
                self.nastav(stav, "stupnice", x);
            }
        });
        self.zaskrt(ui, stav, "ram", tr("Rámeček grafu", "Graph frame"));
        self.zaskrt(ui, stav, "svislice", tr("Svislice z lomových bodů do grafu", "Verticals from break points into the graph"));
        self.zaskrt(ui, stav, "popis_oblouku", tr("Popis zakružovacích oblouků nivelety (R · L · K)", "Vertical curve labels (R · L · K)"));
        pole(ui, tr("Pod terénem", "Below the terrain"), |ui| {
            let akt = self.hodnota(stav, "vypln_terenu");
            let mut v = None;
            segment(ui, &[("zadna", tr("Nic", "Nothing")), ("plna", tr("Tónování", "Tint"))], &akt, |x| v = Some(x));
            if let Some(x) = v {
                self.nastav(stav, "vypln_terenu", x);
            }
        });
        sekce(ui, tr("Pásy", "Bands"));
        pole(ui, tr("Čáry pásů", "Band lines"), |ui| {
            let akt = self.hodnota(stav, "cary_pasu");
            let mut v = None;
            segment(ui, &[("svislice", tr("Jen svislice", "Verticals only")), ("ramecek", tr("Rámeček", "Frame")), ("vykres", tr("Výkresové", "Drawing style"))], &akt, |x| v = Some(x));
            if let Some(x) = v {
                self.nastav(stav, "cary_pasu", x);
            }
        });
        self.zaskrt(ui, stav, "svislice_pasu", tr("Svislice v pásech s hodnotami", "Verticals in value bands"));
        self.zaskrt(ui, stav, "podbarvit_zahlavi", tr("Podbarvit sloupec záhlaví", "Shade the header column"));
        self.cislo(ui, stav, "sirka_zahlavi", tr("Šířka záhlaví", "Header width"), 10.0..=150.0, " mm");
        self.cislo(ui, stav, "hustota", tr("Výška pásů ×", "Band height ×"), 0.5..=2.0, "");
        sekce(ui, tr("Formáty", "Formats"));
        pole(ui, tr("Staničení", "Station"), |ui| {
            let akt = self.hodnota(stav, "staniceni");
            let mut v = None;
            segment(ui, &[("km", "0,040 00"), ("plus", "0+040.00"), ("m", "40.00")], &akt, |x| v = Some(x));
            if let Some(x) = v {
                self.nastav(stav, "staniceni", x);
            }
        });
        pole(ui, tr("Sklon", "Grade"), |ui| {
            let akt = self.hodnota(stav, "sklon");
            let mut v = None;
            segment(ui, &[("promile", "‰"), ("procento", "%")], &akt, |x| v = Some(x));
            if let Some(x) = v {
                self.nastav(stav, "sklon", x);
            }
        });
    }

    fn insp_nadpisu(&mut self, ui: &mut egui::Ui, stav: &StavSpravce) {
        hlava(ui, tr("Nadpis", "Title"), tr("Nadpis a měřítko", "Title and scale"));
        sekce(ui, tr("Text", "Text"));
        let mut t = self.hodnota(stav, "nadpis");
        if ui.add(egui::TextEdit::singleline(&mut t).desired_width(f32::INFINITY)).changed() {
            self.nastav(stav, "nadpis", &t);
        }
        poznamka(ui, tr("Zástupné texty: {trasa}, {povrch}, {meritko}. Prázdný nadpis se nekreslí.", "Placeholders: {trasa} (alignment), {povrch} (surface), {meritko} (scale). An empty title is not drawn."));
        self.zaskrt(ui, stav, "stitek", tr("Měřítko ve štítku", "Scale in the label"));
        self.odkaz_pisma(ui, stav, "nadpis");
        self.odkaz_pisma(ui, stav, "legenda");
    }

    fn odkaz_pisma(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, role: &'static str) {
        let p = self.pismo(stav, role);
        let n = ROLE.iter().find(|r| r.0 == role).map_or(role, |r| r.1);
        if ui.add_sized([ui.available_width(), 26.0], egui::Button::new(format!("{}: {} · {} mm  ›", crate::lang::tr_tab(n), p.font, cz(p.vyska, 1)))).clicked() {
            self.vyber = Vyber::Pismo(role);
            self.karta = Karta::Pisma;
        }
    }

    fn insp_pisma(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, k: &'static str) {
        let pred = self.pismo(stav, k);
        let mut p = pred.clone();
        let n = ROLE.iter().find(|r| r.0 == k).map_or(k, |r| r.1);
        hlava(ui, tr("Styl písma", "Font style"), crate::lang::tr_tab(n));
        ui.label(RichText::new(p.styl_textu()).color(TLUMENA).size(11.0));
        sekce(ui, tr("Písmo", "Font"));
        egui::Grid::new("ep_fonty").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
            for (j, (f, _, _)) in FONTY.iter().enumerate() {
                let on = p.font.eq_ignore_ascii_case(f);
                let ma = ma_pismo(f);
                let txt = RichText::new(format!("Ab 0,123\n{}{}", f, if ma { "" } else { " ⚠" })).font(FontId::new(14.0, rodina(f, false)));
                let b = egui::Button::new(txt).min_size(Vec2::new(128.0, 44.0)).fill(if on { Color32::from_rgba_unmultiplied(61, 139, 255, 40) } else { PANEL2 }).stroke(Stroke::new(1.0, if on { AKCENT } else { CARA_UI }));
                let r = ui.add(b);
                let r = if ma { r } else { r.on_hover_text(tr("Písmo není v tomto počítači nainstalované – OCS použije náhradní.", "The font is not installed on this computer – OCS will use a substitute.")) };
                if r.clicked() {
                    p.font = f.to_string();
                }
                if j % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        sekce(ui, tr("Velikost a tvar", "Size and shape"));
        pole(ui, tr("Výška", "Elevation"), |ui| {
            let akt = format!("{}", p.vyska);
            let mut v = None;
            segment(ui, &[("1.8", "1,8"), ("2", "2"), ("2.5", "2,5"), ("3.5", "3,5"), ("5", "5")], &akt, |x| v = Some(x));
            if let Some(x) = v {
                p.vyska = x.parse().unwrap_or(p.vyska);
            }
        });
        pole(ui, "", |ui| {
            ui.add(egui::DragValue::new(&mut p.vyska).range(0.5..=20.0).speed(0.05).max_decimals(2).suffix(tr(" mm vlastní", " mm custom")));
        });
        pole(ui, tr("Šířka", "Width"), |ui| {
            ui.add(egui::Slider::new(&mut p.sirka, 0.6..=1.3).step_by(0.05));
        });
        pole(ui, tr("Řez", "Section"), |ui| {
            ui.checkbox(&mut p.tucne, RichText::new(tr("Tučné", "Bold")).strong());
            ui.checkbox(&mut p.kurziva, RichText::new(tr("Kurzíva", "Italic")).italics());
        });
        pole(ui, tr("Barva", "Colour"), |ui| {
            vyber_barvy(ui, "ep_barva_pisma", &mut p.barva);
        });
        // ukázka
        ui.add_space(6.0);
        egui::Frame::new().fill(if self.papir { Color32::WHITE } else { LIST_TMAVE }).corner_radius(8.0).inner_margin(12.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let b = p.barva.map(|b| Color32::from_rgb(b[0], b[1], b[2])).unwrap_or(if self.papir { Color32::BLACK } else { TEXT });
            ui.label(RichText::new("Kóta dna 385,62").font(FontId::new((p.vyska as f32 * 6.0).clamp(10.0, 34.0), rodina(&p.font, p.tucne))).color(b).italics_if(p.kurziva)); // i18n:ne
        });
        poznamka(ui, &trf!("Výška je na papíře – v modelu se násobí měřítkem (1:{} = {} m). Řada pro výkresy: 1,8 · 2,5 · 3,5 · 5 · 7 mm.", "Height is on paper – in the model it is multiplied by the scale (1:{} = {} m). Drawing series: 1.8 · 2.5 · 3.5 · 5 · 7 mm.", cz(self.d.meritko, 0), cz(p.vyska * self.d.meritko / 1000.0, 2)));
        if p != pred {
            self.nastav(stav, &format!("p.{k}"), &p.zapis());
        }
    }

    fn insp_cary(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, k: &'static str) {
        let pred = self.cara(stav, k);
        let mut c = pred.clone();
        let n = CARY.iter().find(|x| x.0 == k).map_or(k, |x| x.1);
        hlava(ui, tr("Čára", "Line"), crate::lang::tr_tab(n));
        sekce(ui, tr("Čára", "Line"));
        pole(ui, tr("Barva", "Colour"), |ui| {
            vyber_barvy(ui, "ep_barva_cary", &mut c.barva);
        });
        pole(ui, tr("Tloušťka", "Weight"), |ui| {
            let akt = c.tloustka.clone();
            let mut v = None;
            segment(ui, &TLOUSTKY.iter().map(|t| (t.0, crate::lang::tr_tab(t.1))).collect::<Vec<_>>(), &akt, |x| v = Some(x));
            if let Some(x) = v {
                c.tloustka = x.to_string();
            }
        });
        let tl = self.hodnota(stav, &format!("tl.{}", c.tloustka));
        poznamka(ui, &trf!("{} mm – tloušťky změníte v kartě Čáry a barvy.", "{} mm – change the weights on the Lines and colours tab.", tl));
        if k == "teren" {
            pole(ui, tr("Pod terénem", "Below the terrain"), |ui| {
                let akt = self.hodnota(stav, "vypln_terenu");
                let mut v = None;
                segment(ui, &[("zadna", tr("Nic", "Nothing")), ("plna", tr("Tónování", "Tint"))], &akt, |x| v = Some(x));
                if let Some(x) = v {
                    self.nastav(stav, "vypln_terenu", x);
                }
            });
        }
        if c != pred {
            self.nastav(stav, &format!("c.{k}"), &c.zapis());
        }
    }

    fn zaskrt(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, k: &str, popis: &str) {
        let mut b = self.ano(stav, k);
        if ui.checkbox(&mut b, popis).changed() {
            self.nastav(stav, k, if b { "1" } else { "0" });
        }
    }

    fn cislo(&mut self, ui: &mut egui::Ui, stav: &StavSpravce, k: &str, popis: &str, rozsah: std::ops::RangeInclusive<f64>, pripona: &str) {
        let mut v: f64 = self.hodnota(stav, k).replace(',', ".").parse().unwrap_or(1.0);
        pole(ui, popis, |ui| {
            if ui.add(egui::DragValue::new(&mut v).range(rozsah).speed(0.05).max_decimals(2).suffix(pripona)).changed() {
                self.nastav(stav, k, &format!("{v}"));
            }
        });
    }

    // ── dialogy ─────────────────────────────────────────────────────────

    fn galerie_pasu(&mut self, ctx: &egui::Context) {
        let mut zavrit = false;
        egui::Window::new(tr("Nový pás", "New band"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::LEFT_BOTTOM, Vec2::new(280.0, -60.0))
            .show(ctx, |ui| {
                ui.label(RichText::new(tr("Vyberte, co má pás ukazovat", "Choose what the band shows")).color(TLUMENA));
                egui::Grid::new("ep_galerie").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
                    for (j, d) in DruhPasu::VSE.iter().enumerate() {
                        let txt = format!("{}  {}\n{}", zkratka(*d), crate::lang::tr_tab(d.nazev()), crate::lang::tr_tab(popis_druhu(*d)));
                        if ui.add(egui::Button::new(txt).min_size(Vec2::new(200.0, 42.0))).clicked() {
                            let mut p = Pas::novy(*d);
                            let i = match self.vyber {
                                Vyber::Pas(i) if i < self.d.pasy.len() => {
                                    p.poloha = self.d.pasy[i].poloha;
                                    i + 1
                                }
                                _ => self.d.pasy.len(),
                            };
                            self.d.pasy.insert(i, p);
                            self.vyber = Vyber::Pas(i);
                            self.karta = Karta::Struktura;
                            self.pasy_zmeneny();
                            zavrit = true;
                        }
                        if j % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
                if ui.button(tr("Zavřít", "Close")).clicked() {
                    zavrit = true;
                }
            });
        if zavrit {
            self.galerie = false;
        }
    }

    fn dialog_zavreni(&mut self, ctx: &egui::Context) {
        egui::Window::new(tr("Neuložené změny", "Unsaved changes")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.label(tr("Vzhled profilu má neuložené změny.", "The profile appearance has unsaved changes."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new(tr("Uložit", "Save")).strong()).clicked() {
                    self.uloz();
                    self.otevreno = false;
                    self.dotaz_zavrit = false;
                }
                if ui.button(tr("Zahodit", "Discard")).clicked() {
                    self.otevreno = false;
                    self.dotaz_zavrit = false;
                }
                if ui.button(tr("Zpět do editoru", "Back to the editor")).clicked() {
                    self.dotaz_zavrit = false;
                }
            });
        });
    }
}

// ── kreslení entit do náhledu ───────────────────────────────────────────

fn barva_entity(c: &Color, vrstva: &str, papir: bool) -> Color32 {
    let b = match c {
        Color::Rgb { r, g, b } => Color32::from_rgb(*r, *g, *b),
        _ => match vrstva {
            H_TEREN => Color32::from_rgb(204, 102, 0),
            H_NIVELETA => Color32::from_rgb(255, 0, 0),
            H_NIV_POLYGON => Color32::from_gray(128),
            _ => {
                if papir {
                    Color32::from_gray(25)
                } else {
                    TEXT
                }
            }
        },
    };
    if papir && (b.r() as u32 * 3 + b.g() as u32 * 6 + b.b() as u32) / 10 > 175 {
        Color32::from_gray(30)
    } else {
        b
    }
}

fn sila(e: &EntityType, px_mm: f32) -> f32 {
    let lw = match e.common().line_weight {
        ocs_plugin_api::host::codec::types::LineWeight::Value(v) => v as f32 / 100.0,
        _ => 0.25,
    };
    (lw * px_mm).clamp(0.7, 4.0)
}

fn kresli_entitu(p: &egui::Painter, e: &EntityType, na: &dyn Fn(f64, f64) -> Pos2, s: f32, px_mm: f32, papir: bool) {
    let k = e.common();
    let barva = barva_entity(&k.color, &k.layer, papir);
    match e {
        EntityType::Line(l) => {
            p.line_segment([na(l.start.x, l.start.y), na(l.end.x, l.end.y)], Stroke::new(sila(e, px_mm), barva));
        }
        EntityType::LwPolyline(pl) => {
            let mut body: Vec<Pos2> = Vec::new();
            let n = pl.vertices.len();
            let pocet = if pl.is_closed { n } else { n.saturating_sub(1) };
            if n > 0 {
                body.push(na(pl.vertices[0].location.x, pl.vertices[0].location.y));
            }
            for i in 0..pocet {
                let a = &pl.vertices[i];
                let b = &pl.vertices[(i + 1) % n];
                if a.bulge.abs() > 1e-9 {
                    // oblouk z vydutí
                    let (ax, ay, bx, by) = (a.location.x, a.location.y, b.location.x, b.location.y);
                    let th = 4.0 * a.bulge.atan();
                    let d = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
                    if d > 1e-12 {
                        let r = d / (2.0 * (th / 2.0).sin());
                        let (mx, my) = ((ax + bx) / 2.0, (ay + by) / 2.0);
                        let h = r * (th / 2.0).cos();
                        let (nx, ny) = (-(by - ay) / d, (bx - ax) / d);
                        let (cx, cy) = (mx + nx * h, my + ny * h);
                        let a0 = (ay - cy).atan2(ax - cx);
                        for j in 1..=16 {
                            let t = a0 + th * j as f64 / 16.0;
                            body.push(na(cx + r.abs() * t.cos(), cy + r.abs() * t.sin()));
                        }
                        continue;
                    }
                }
                body.push(na(b.location.x, b.location.y));
            }
            if body.len() >= 2 {
                p.add(egui::Shape::line(body, Stroke::new(sila(e, px_mm), barva)));
            }
        }
        EntityType::Hatch(h) => {
            let alfa = match k.transparency {
                ocs_plugin_api::host::codec::types::Transparency::Explicit(t) => 1.0 - t as f32 / 255.0,
                _ => 1.0,
            };
            let b = Color32::from_rgba_unmultiplied(barva.r(), barva.g(), barva.b(), (alfa * 255.0 * if papir { 1.6 } else { 1.0 }).clamp(0.0, 255.0) as u8);
            for cesta in &h.paths {
                for hrana in &cesta.edges {
                    if let ocs_plugin_api::host::codec::entities::BoundaryEdge::Polyline(pe) = hrana {
                        let v: Vec<Pos2> = pe.vertices.iter().map(|q| na(q.x, q.y)).collect();
                        vypln(p, &v, b);
                    }
                }
            }
        }
        EntityType::Text(t) => {
            let h = t.height as f32 * s;
            if h < 2.0 || t.value.is_empty() {
                return;
            }
            let (font, tucne) = font_ze_stylu(&t.style);
            let velikost = h / 0.70;
            let galley = p.layout_no_wrap(t.value.clone(), FontId::new(velikost, rodina(&font, tucne)), barva);
            let bod = t.alignment_point.unwrap_or(t.insertion_point);
            let kotva = na(bod.x, bod.y);
            let (w, gh) = (galley.size().x, galley.size().y);
            let ax = match t.horizontal_alignment {
                Hz::Center | Hz::Middle => w / 2.0,
                Hz::Right => w,
                _ => 0.0,
            };
            // výška velkých písmen ≈ 0,7 velikosti; účaří ≈ 0,8 výšky řádku
            let horni = gh * 0.5 - velikost * 0.36;
            let ay = match t.vertical_alignment {
                Vt::Top => horni,
                Vt::Middle => gh * 0.5,
                Vt::Bottom => gh * 0.5 + velikost * 0.36 + velikost * 0.18,
                Vt::Baseline => gh * 0.5 + velikost * 0.36,
            };
            let uhel = -(t.rotation as f32);
            let rot = egui::emath::Rot2::from_angle(uhel);
            let pos = kotva - rot * Vec2::new(ax, ay);
            p.add(egui::epaint::TextShape::new(pos, galley, barva).with_angle(uhel));
        }
        _ => {}
    }
}

/// Výplň mnohoúhelníku: obdélník nebo „terén + spodní hrana“ (svislé pruhy).
fn vypln(p: &egui::Painter, v: &[Pos2], barva: Color32) {
    if v.len() < 3 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    if v.len() == 4 {
        for q in v {
            mesh.colored_vertex(*q, barva);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
    } else {
        // posledních dvou bodů je spodní hrana (b_n-2 pod koncem, b_n-1 pod začátkem)
        let n = v.len() - 2;
        let dole = v[n].y;
        for i in 0..n.saturating_sub(1) {
            let (a, b) = (v[i], v[i + 1]);
            let z = mesh.vertices.len() as u32;
            mesh.colored_vertex(a, barva);
            mesh.colored_vertex(b, barva);
            mesh.colored_vertex(Pos2::new(b.x, dole), barva);
            mesh.colored_vertex(Pos2::new(a.x, dole), barva);
            mesh.add_triangle(z, z + 1, z + 2);
            mesh.add_triangle(z, z + 2, z + 3);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

/// Body čar grafu v souřadnicích pohledu (pro výběr a zvýraznění).
fn body_cary(pk: &Podklady, osy: &Osy, k: &str) -> Vec<Vec<[f64; 2]>> {
    match k {
        "teren" => pk.useky.iter().map(|u| u.iter().map(|p| osy.bod(p[0], p[1])).collect()).collect(),
        "povrchy" => pk.dalsi.iter().flat_map(|(_, u)| u.iter().map(|x| x.iter().map(|p| osy.bod(p[0], p[1])).collect())).collect(),
        "niveleta" => pk.niveleta.as_ref().map_or(Vec::new(), |n| vec![n.body().into_iter().map(|p| osy.bod(p[0], p[1])).collect()]),
        "polygon" => pk.niveleta.as_ref().map_or(Vec::new(), |n| n.tecny_oblouku().iter().map(|t| t.iter().map(|p| osy.bod(p[0], p[1])).collect()).collect()),
        _ => Vec::new(),
    }
}

fn vzd_usecka(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

/// Meze nakresleného pohledu (x0, y0, x1, y1).
fn meze(r: &Rozvrzeni, d: &DefinicePohledu) -> (f64, f64, f64, f64) {
    let m = d.meritko / 1000.0;
    let mut y0 = r.pasy.iter().map(|p| p.2).fold(r.graf[1], f64::min);
    let mut y1 = r.pasy.iter().map(|p| p.1).fold(r.graf[3], f64::max);
    if let Some(n) = r.nadpis {
        y1 = y1.max(n[3]);
    }
    if let Some(l) = r.legenda {
        y0 = y0.min(l[1]);
    }
    (r.vlevo - 2.0 * m, y0 - 2.0 * m, r.graf[2] + 10.0 * m, y1 + 2.0 * m)
}

fn rozmer(r: &Rozvrzeni) -> (f64, f64) {
    let y0 = r.pasy.iter().map(|p| p.2).fold(r.graf[1], f64::min).min(r.legenda.map_or(f64::MAX, |l| l[1]));
    let y1 = r.pasy.iter().map(|p| p.1).fold(r.graf[3], f64::max).max(r.nadpis.map_or(f64::MIN, |n| n[3]));
    (r.graf[2] - r.vlevo, y1 - y0)
}

// ── písma ───────────────────────────────────────────────────────────────

fn nactena() -> &'static Mutex<Vec<String>> {
    static N: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    N.get_or_init(|| Mutex::new(Vec::new()))
}

fn soubor_v_systemu(soubor: &str) -> Option<std::path::PathBuf> {
    let mut slozky = vec![std::path::PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())).join("Fonts")];
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        slozky.push(std::path::PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
    }
    slozky.into_iter().map(|s| s.join(soubor)).find(|p| p.exists())
}

fn ma_pismo(font: &str) -> bool {
    FONTY.iter().find(|f| f.0 == font).is_some_and(|f| soubor_v_systemu(f.1).is_some())
}

/// Načte písma ze seznamu `FONTY` z Windows (jednou) jako rodiny egui.
fn nacti_pisma(ctx: &egui::Context) {
    // písmo přidané do egui platí až od dalšího snímku – do té doby se nepoužívá
    static CEKAJICI: Mutex<Option<(u64, Vec<String>)>> = Mutex::new(None);
    let mut c = CEKAJICI.lock().unwrap_or_else(|e| e.into_inner());
    match c.as_mut() {
        Some((snimek, cekaji)) => {
            if !cekaji.is_empty() && ctx.cumulative_pass_nr() > *snimek + 1 {
                nactena().lock().unwrap_or_else(|e| e.into_inner()).append(cekaji);
            }
            return;
        }
        None => *c = Some((ctx.cumulative_pass_nr(), Vec::new())),
    }
    let Some((_, n)) = c.as_mut() else { return };
    for (f, obyc, tucny) in FONTY {
        for (soubor, b) in [(obyc, false), (tucny, true)] {
            let jmeno = jmeno_rodiny(f, b);
            if n.contains(&jmeno) {
                continue;
            }
            let Some(cesta) = soubor_v_systemu(soubor) else { continue };
            let Ok(data) = std::fs::read(cesta) else { continue };
            ctx.add_font(egui::epaint::text::FontInsert::new(
                &jmeno,
                egui::FontData::from_owned(data),
                vec![egui::epaint::text::InsertFontFamily { family: FontFamily::Name(jmeno.clone().into()), priority: egui::epaint::text::FontPriority::Highest }],
            ));
            n.push(jmeno);
        }
    }
}

fn jmeno_rodiny(font: &str, tucne: bool) -> String {
    format!("pp:{font}{}", if tucne { ":b" } else { "" })
}

/// Rodina egui pro písmo (nenačtené = běžné písmo okna).
fn rodina(font: &str, tucne: bool) -> FontFamily {
    let n = nactena().lock().unwrap_or_else(|e| e.into_inner());
    let j = jmeno_rodiny(font, tucne);
    if n.contains(&j) {
        return FontFamily::Name(j.into());
    }
    let j = jmeno_rodiny(font, false);
    if n.contains(&j) {
        return FontFamily::Name(j.into());
    }
    FontFamily::Proportional
}

/// Písmo a tučnost z jména textového stylu `TER_PP_…`.
fn font_ze_stylu(styl: &str) -> (String, bool) {
    let tucne = styl.ends_with("_B");
    let zaklad = styl.trim_start_matches("TER_PP_").trim_end_matches("_B");
    let font = FONTY.iter().find(|f| f.0.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase() == zaklad).map_or("Arial", |f| f.0);
    (font.to_string(), tucne)
}

trait Kurziva {
    fn italics_if(self, b: bool) -> Self;
}
impl Kurziva for RichText {
    fn italics_if(self, b: bool) -> Self {
        if b { self.italics() } else { self }
    }
}

// ── drobné prvky UI ─────────────────────────────────────────────────────

fn styl_ui(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(TEXT);
    v.widgets.inactive.weak_bg_fill = PANEL2;
    v.widgets.inactive.bg_fill = PANEL2;
    v.widgets.hovered.weak_bg_fill = PANEL3;
    v.widgets.active.weak_bg_fill = PANEL3;
    v.extreme_bg_color = PANEL2;
    v.selection.bg_fill = AKCENT;
    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
    ui.spacing_mut().button_padding = Vec2::new(8.0, 4.0);
}

fn chip(ui: &mut egui::Ui, t: &str, barva: Color32) {
    egui::Frame::new().stroke(Stroke::new(1.0, CARA_UI)).fill(PANEL2).corner_radius(10.0).inner_margin(egui::Margin::symmetric(8, 1)).show(ui, |ui| {
        ui.label(RichText::new(t).color(barva).size(11.0));
    });
}

/// Segmentový přepínač; `zmena` dostane klíč zvolené možnosti.
fn segment<'a>(ui: &mut egui::Ui, volby: &[(&'a str, &str)], akt: &str, mut zmena: impl FnMut(&'a str)) {
    egui::Frame::new().fill(PANEL2).stroke(Stroke::new(1.0, CARA_UI)).corner_radius(7.0).inner_margin(2.0).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal(|ui| {
            for (k, n) in volby {
                let on = *k == akt || akt.replace(',', ".").parse::<f64>().ok().zip(k.parse::<f64>().ok()).is_some_and(|(a, b)| (a - b).abs() < 1e-9);
                let b = egui::Button::new(RichText::new(*n).color(if on { TEXT } else { TLUMENA }).size(12.5)).fill(if on { PANEL3 } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(5.0);
                if ui.add(b).clicked() && !on {
                    zmena(k);
                }
            }
        });
    });
}

fn skupina(ui: &mut egui::Ui, t: &str, pocet: Option<usize>) -> Rect {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(t.to_uppercase()).color(TLUMENA).size(11.0));
        if let Some(n) = pocet {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(n.to_string()).color(TLUMENA).size(11.0));
            });
        }
    })
    .response
    .rect
}

/// Řádek struktury: (odpověď řádku, kliknuto na oko).
fn radek(ui: &mut egui::Ui, on: bool, zkratka: &str, nazev: &str, vpravo: &str, skryty: bool, oko: Option<bool>) -> (egui::Response, bool) {
    let sirka = ui.available_width() - 8.0;
    let (rect, r) = ui.allocate_exact_size(Vec2::new(sirka, 26.0), Sense::click_and_drag());
    let rect = rect.translate(Vec2::new(4.0, 0.0));
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 6.0, Color32::from_rgba_unmultiplied(61, 139, 255, 40));
        p.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_rgba_unmultiplied(61, 139, 255, 115)), egui::StrokeKind::Inside);
    } else if r.hovered() {
        p.rect_filled(rect, 6.0, PANEL2);
    }
    let a = if skryty { 100 } else { 255 };
    // úchyt pro přetažení: šest teček
    for (dx, dy) in [(0.0, -4.0), (0.0, 0.0), (0.0, 4.0), (4.0, -4.0), (4.0, 0.0), (4.0, 4.0)] {
        p.circle_filled(rect.left_center() + Vec2::new(8.0 + dx, dy), 1.1, Color32::from_gray(95));
    }
    let ikona = Rect::from_center_size(rect.left_center() + Vec2::new(36.0, 0.0), Vec2::new(26.0, 18.0));
    p.rect_filled(ikona, 4.0, PANEL3);
    p.text(ikona.center(), Align2::CENTER_CENTER, zkratka, FontId::proportional(10.0), Color32::from_rgba_unmultiplied(185, 192, 200, a));
    let maxw = rect.width() - 110.0;
    let g = p.layout(nazev.to_string(), FontId::proportional(13.0), Color32::from_rgba_unmultiplied(230, 232, 235, a), maxw);
    p.galley(Pos2::new(rect.left() + 56.0, rect.center().y - g.size().y / 2.0), g, TEXT);
    p.text(rect.right_center() - Vec2::new(30.0, 0.0), Align2::RIGHT_CENTER, vpravo, FontId::proportional(11.0), TLUMENA);
    let mut klik_oko = false;
    if let Some(vid) = oko {
        let o = Rect::from_center_size(rect.right_center() - Vec2::new(14.0, 0.0), Vec2::splat(18.0));
        // oko: plné kolečko = viditelný, prázdné = skrytý
        p.circle_stroke(o.center(), 5.5, Stroke::new(1.2, TLUMENA));
        if vid {
            p.circle_filled(o.center(), 2.8, TLUMENA);
        }
        if r.clicked() && ui.ctx().pointer_interact_pos().is_some_and(|m| o.contains(m)) {
            klik_oko = true;
        }
    }
    (r, klik_oko)
}

fn radek_cary(ui: &mut egui::Ui, on: bool, c: &Cara, nazev: &str) -> egui::Response {
    let sirka = ui.available_width() - 8.0;
    let (rect, r) = ui.allocate_exact_size(Vec2::new(sirka, 24.0), Sense::click());
    let rect = rect.translate(Vec2::new(4.0, 0.0));
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 6.0, Color32::from_rgba_unmultiplied(61, 139, 255, 40));
    } else if r.hovered() {
        p.rect_filled(rect, 6.0, PANEL2);
    }
    let b = c.barva.map_or(TEXT, |b| Color32::from_rgb(b[0], b[1], b[2]));
    let w = match c.tloustka.as_str() {
        "tenka" => 1.0,
        "silna" => 3.0,
        _ => 2.0,
    };
    p.line_segment([rect.left_center() + Vec2::new(24.0, 0.0), rect.left_center() + Vec2::new(46.0, 0.0)], Stroke::new(w, b));
    p.text(rect.left_center() + Vec2::new(56.0, 0.0), Align2::LEFT_CENTER, nazev, FontId::proportional(13.0), TEXT);
    r
}

fn hlava(ui: &mut egui::Ui, druh: &str, titul: &str) {
    ui.label(RichText::new(druh.to_uppercase()).color(TLUMENA).size(11.0));
    ui.label(RichText::new(titul).strong().size(15.0));
}

fn sekce(ui: &mut egui::Ui, t: &str) {
    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new(t.to_uppercase()).color(TLUMENA).size(11.0));
}

fn pole(ui: &mut egui::Ui, popis: &str, obsah: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized([92.0, 20.0], egui::Label::new(RichText::new(popis).color(Color32::from_rgb(195, 201, 208))).truncate());
        obsah(ui);
    });
}

fn poznamka(ui: &mut egui::Ui, t: &str) {
    ui.label(RichText::new(t).color(TLUMENA).size(11.5));
}

const PALETA: [[u8; 3]; 12] = [
    [95, 211, 95],
    [255, 90, 77],
    [224, 90, 216],
    [79, 179, 255],
    [240, 164, 58],
    [201, 162, 74],
    [154, 140, 255],
    [143, 151, 161],
    [91, 98, 108],
    [230, 232, 235],
    [30, 30, 30],
    [0, 150, 120],
];

/// Výběr barvy: „podle hladiny“, paleta a vlastní barva.
fn vyber_barvy(ui: &mut egui::Ui, id: &str, b: &mut Option<[u8; 3]>) -> bool {
    let mut zmena = false;
    ui.vertical(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (r, o) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
            ui.painter().rect_filled(r.shrink(1.0), 4.0, Color32::from_gray(60));
            ui.painter().line_segment([r.left_bottom() + Vec2::new(4.0, -4.0), r.right_top() + Vec2::new(-4.0, 4.0)], Stroke::new(1.5, Color32::from_gray(200)));
            if b.is_none() {
                ui.painter().rect_stroke(r, 5.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
            }
            if o.on_hover_text(tr("podle hladiny", "ByLayer")).clicked() && b.is_some() {
                *b = None;
                zmena = true;
            }
            for c in PALETA {
                let (r, o) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                ui.painter().rect_filled(r.shrink(1.0), 4.0, Color32::from_rgb(c[0], c[1], c[2]));
                if *b == Some(c) {
                    ui.painter().rect_stroke(r, 5.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
                }
                if o.clicked() && *b != Some(c) {
                    *b = Some(c);
                    zmena = true;
                }
            }
            let mut v = b.unwrap_or([200, 200, 200]);
            if ui.color_edit_button_srgb(&mut v).on_hover_text(tr("vlastní barva", "custom colour")).changed() {
                *b = Some(v);
                zmena = true;
            }
            let _ = id;
        });
    });
    zmena
}

fn nazev_zdroje(z: &Zdroj, povrchy: &[(String, String)]) -> String {
    match z {
        Zdroj::Teren => tr("Povrch pohledu", "View surface").to_string(),
        Zdroj::Niveleta => tr("Niveleta", "Profile grade line").to_string(),
        Zdroj::Upraveny => tr("Upravený terén", "Modified ground").to_string(),
        Zdroj::Potrubi => tr("Potrubí (zatím bez dat)", "Pipe (no data yet)").to_string(),
        Zdroj::Povrch(id) => povrchy.iter().find(|p| p.0 == *id).map_or_else(|| id.clone(), |p| p.1.clone()),
    }
}

fn vyber_zdroje(ui: &mut egui::Ui, salt: (&str, usize), z: &mut Zdroj, povrchy: &[(String, String)]) {
    egui::ComboBox::from_id_salt(salt).selected_text(nazev_zdroje(z, povrchy)).width(ui.available_width()).show_ui(ui, |ui| {
        let mut moznosti = vec![Zdroj::Teren, Zdroj::Upraveny, Zdroj::Niveleta, Zdroj::Potrubi];
        moznosti.extend(povrchy.iter().map(|p| Zdroj::Povrch(p.0.clone())));
        for m in moznosti {
            let n = nazev_zdroje(&m, povrchy);
            if ui.selectable_label(*z == m, n).clicked() {
                *z = m;
            }
        }
    });
}

fn nazev_pasu(p: &Pas) -> String {
    if p.nazev.trim().is_empty() { crate::lang::tr_tab(p.druh.nazev()).to_string() } else { p.nazev.clone() }
}

fn zkratka(d: DruhPasu) -> &'static str {
    match d {
        DruhPasu::Staniceni => "km",
        DruhPasu::Teren | DruhPasu::Niveleta | DruhPasu::Kota => "1,23",
        DruhPasu::Rozdil => "±",
        DruhPasu::Smer => "R",
        DruhPasu::Sklony => "‰",
        DruhPasu::Vzdalenosti => "↔",
        DruhPasu::Text => "T",
        DruhPasu::Useky => "Aa",
    }
}

fn popis_druhu(d: DruhPasu) -> &'static str {
    match d {
        DruhPasu::Staniceni => "km / m v místech",
        DruhPasu::Teren => "výška povrchu pohledu",
        DruhPasu::Niveleta => "výška nivelety",
        DruhPasu::Rozdil => "hloubka, výkop / násyp",
        DruhPasu::Smer => "přímé a oblouky trasy",
        DruhPasu::Sklony => "sklon a délka úseků",
        DruhPasu::Kota => "výška z libovolného povrchu",
        DruhPasu::Vzdalenosti => "mezi místy zápisu",
        DruhPasu::Text => "jeden řádek textu",
        DruhPasu::Useky => "parcely, povrch, materiál…",
    }
}

// ── ukázková data (náhled bez výkresu) ──────────────────────────────────

/// Vymyšlený pohled pro náhled, když ve výkrese žádný není.
pub fn ukazkove_podklady() -> Podklady {
    use trasy::geometrie::{geometrie, Vrchol};
    use trasy::niveleta::{Niveleta, VrcholNivelety};
    let g = geometrie(&[Vrchol::new(0.0, 0.0, 0.0), Vrchol::new(160.0, 30.0, 150.0), Vrchol::new(330.0, 10.0, 0.0)], 0.0).expect("ukázková trasa"); // i18n:ne
    let l = g.konec();
    let teren = |s: f64| 388.1 + 1.3 * (s / 38.0).sin() + 0.55 * (s / 13.0).cos() + 0.0045 * s - 0.9 * (-((s - 165.0) / 22.0).powi(2)).exp();
    let mut u = Vec::new();
    let mut s = 0.0;
    while s <= l + 1e-9 {
        u.push([s, teren(s)]);
        s += 1.0;
    }
    let jiny: Vec<[f64; 2]> = u.iter().map(|p| [p[0], p[1] + 0.18 * (p[0] / 9.0).sin() - 0.25]).collect();
    let niveleta = Niveleta {
        id: "N1".into(),
        jmeno: "Niveleta".into(),
        vrcholy: vec![VrcholNivelety::new(0.0, 386.6, 0.0), VrcholNivelety::new(150.0, 386.0, 1500.0), VrcholNivelety::new(l, 387.6, 0.0)],
    };
    let zmin = u.iter().map(|p| p[1]).fold(f64::MAX, f64::min).min(386.0);
    let zmax = u.iter().map(|p| p[1]).fold(f64::MIN, f64::max).max(387.6);
    Podklady {
        trasa: "Stoka A".into(),
        povrch: "Stávající terén".into(),
        g,
        useky: vec![u],
        dalsi: vec![("NAVRH".into(), vec![jiny])],
        povrchy: vec![("TEREN".into(), "Stávající terén".into()), ("NAVRH".into(), "Zaměření 2019".into())], // i18n:ne
        niveleta: Some(niveleta),
        upraveny: Some("NAVRH".into()),
        od: 0.0,
        do_: l,
        zmin,
        zmax,
        trojuhelniku: 0,
    }
}
