//! Okno importu bodů: volba zdroje, rozpoznání (formát, oddělovač,
//! kódování, desetinná čárka, záhlaví), tabulka se záhlavím sloupců,
//! kontrola chyb a S-JTSK, mapka polohy, duplicity, skupina a styl.

use std::collections::{BTreeSet, HashMap};

use eframe::egui::{self, Color32, FontId, RichText, Sense, Stroke};
use teren::body::{nacti_body, rozdel_radek, Bod, Desetinna, FormatBodu, Oddelovac, Sloupec, VysledekImportu};
use teren::duplicity::{self, Duplicity, Politika};
use teren::formaty::{self, DruhZdroje};
use teren::kodovani::{self, Kodovani};
use teren::preklad::tr;
use teren::rozpoznani::{self, nazev_desetinne, nazev_oddelovace, podezrela_vyska, s_formatem, v_cr, Navrh, Puvod};
use teren::trf;

use super::{Okno, CERVENA, MODRA, ORANZOVA, SEDA, ZELENA};
use crate::import::{CilSkupiny, NastaveniImportu};
use crate::nastaveni::{self, SablonaImportu};
use crate::spravce::{self, Akce, StavSpravce};

const FIALOVA: Color32 = Color32::from_rgb(180, 145, 240);
const KARTA: Color32 = Color32::from_rgb(36, 40, 45);
const LINKA: Color32 = Color32::from_rgb(52, 57, 64);

/// Rychlé předvolby pořadí sloupců.
const VZORY: [(&str, &str); 7] = [
    ("PNEZD", "PNEZD – číslo, N, E, Z, popis"),
    ("PENZD", "PENZD – číslo, E, N, Z, popis"),
    ("PYXZD", "PYXZD – číslo, Y, X, Z, popis (S-JTSK)"),
    ("PXYZD", "PXYZD – číslo, X, Y, Z, popis (S-JTSK)"),
    ("NEZD", "NEZ – bez čísla"),
    ("ENZD", "ENZ – bez čísla"),
    ("YXZD", "YXZ – bez čísla (S-JTSK)"),
];
const ODDELOVACE: [Oddelovac; 4] = [Oddelovac::Strednik, Oddelovac::Mezera, Oddelovac::Tabulator, Oddelovac::Carka];
const ROLE: [Sloupec; 8] =
    [Sloupec::Cislo, Sloupec::SjtskY, Sloupec::SjtskX, Sloupec::E, Sloupec::N, Sloupec::Z, Sloupec::Popis, Sloupec::Preskocit];

fn nazev_role(s: Sloupec) -> &'static str {
    match s {
        Sloupec::Cislo => tr("Číslo"),
        Sloupec::SjtskY => tr("Y (S-JTSK)"),
        Sloupec::SjtskX => tr("X (S-JTSK)"),
        Sloupec::E => tr("E (výkres)"),
        Sloupec::N => tr("N (výkres)"),
        Sloupec::Z => "Z",
        Sloupec::Popis => tr("Kód/popis"),
        Sloupec::Preskocit => tr("Ignorovat"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filtr {
    Vse,
    Nacte,
    Chyby,
    Podezrele,
    Duplicity,
}

#[derive(Clone, PartialEq)]
enum StavRadku {
    Hlavicka,
    /// Index bodu ve výsledku.
    Bod(usize),
    Chyba(Option<usize>, String),
}

struct Radek {
    cislo: usize,
    pole: Vec<String>,
    stav: StavRadku,
}

/// Body z výkresu – nastavení převodu.
struct ZVykresu {
    hladina: Option<String>,
    jen_vyber: bool,
    tolerance: f64,
    hladiny: Vec<(String, usize)>,
    statistika: teren::z_vykresu::Statistika,
    ceka: bool,
}

/// Co je nastavené ručně (jinak rozpoznáno).
#[derive(Default, Clone, Copy)]
struct Rucne {
    oddelovac: bool,
    kodovani: bool,
    desetinna: bool,
    hlavicka: bool,
    sloupce: bool,
}

pub(super) struct NahledImportu {
    /// Plná cesta souboru („“ = schránka / výkres / zatím nic).
    cesta: String,
    cesta_edit: String,
    /// Jméno zdroje ukládané k bodům.
    soubor: String,
    bajty: Vec<u8>,
    druh: Option<DruhZdroje>,
    kodovani: Option<Kodovani>,
    listy: Vec<String>,
    list: usize,
    z_vykresu: Option<ZVykresu>,
    text: String,
    chyba: Option<String>,
    sloupce: Vec<Sloupec>,
    oddelovac: Oddelovac,
    hlavicka: usize,
    desetinna: Desetinna,
    desetinna_zjistena: Desetinna,
    rucne: Rucne,
    popis_rozpoznani: String,
    varovani: bool,
    // výsledek rozboru
    chyba_formatu: Option<String>,
    vysledek: VysledekImportu,
    radky: Vec<Radek>,
    ncol: usize,
    zahlavi: Vec<String>,
    dup: Duplicity,
    odlehle: BTreeSet<usize>,
    vetsina_v_cr: bool,
    /// Důvod podezření ke každému bodu (`None` = v pořádku).
    podezrele: Vec<Option<String>>,
    navrh: Option<(Navrh, Vec<Sloupec>)>,
    plan: duplicity::Plan,
    existujici: Vec<Bod>,
    zmeneno: bool,
    verze: u64,
    // volby
    filtr: Filtr,
    politika: usize,
    precislovat_od: u64,
    skupina: CilSkupiny,
    styl: String,
    priblizit: bool,
    zapamatovat: bool,
    vynechat: BTreeSet<usize>,
    nova_sablona: Option<String>,
    /// Další soubory k importu (přetažené najednou).
    fronta: Vec<String>,
    poradi: (usize, usize),
}

impl NahledImportu {
    fn prazdny() -> Self {
        let n = nastaveni::akt();
        let politika = match Politika::z_klice(&n.import_duplicity, 0) {
            Politika::Preskocit => 0,
            Politika::Prepsat => 1,
            Politika::Precislovat(_) => 2,
            Politika::Ponechat => 3,
        };
        Self {
            cesta: String::new(),
            cesta_edit: String::new(),
            soubor: String::new(),
            bajty: Vec::new(),
            druh: None,
            kodovani: None,
            listy: Vec::new(),
            list: 0,
            z_vykresu: None,
            text: String::new(),
            chyba: None,
            sloupce: Vec::new(),
            oddelovac: Oddelovac::Mezera,
            hlavicka: 0,
            desetinna: Desetinna::Auto,
            desetinna_zjistena: Desetinna::Tecka,
            rucne: Rucne::default(),
            popis_rozpoznani: String::new(),
            varovani: false,
            chyba_formatu: None,
            vysledek: VysledekImportu::default(),
            radky: Vec::new(),
            ncol: 0,
            zahlavi: Vec::new(),
            dup: Duplicity::default(),
            odlehle: BTreeSet::new(),
            vetsina_v_cr: false,
            podezrele: Vec::new(),
            navrh: None,
            plan: duplicity::Plan::default(),
            existujici: Vec::new(),
            zmeneno: true,
            verze: u64::MAX,
            filtr: Filtr::Vse,
            politika,
            precislovat_od: 0,
            skupina: CilSkupiny::Nova,
            styl: String::new(),
            priblizit: n.import_priblizit,
            zapamatovat: true,
            vynechat: BTreeSet::new(),
            nova_sablona: None,
            fronta: Vec::new(),
            poradi: (1, 1),
        }
    }

    /// Okno pro soubor (prázdná cesta = volba zdroje).
    pub(super) fn novy(cesta: &str) -> Self {
        let mut n = Self::prazdny();
        let cesta = cesta.trim().trim_matches('"');
        if !cesta.is_empty() {
            n.nacti_soubor(cesta);
        }
        n
    }

    fn ma_zdroj(&self) -> bool {
        self.druh.is_some() || self.z_vykresu.is_some() || self.chyba.is_some()
    }

    fn nacti_soubor(&mut self, cesta: &str) {
        self.cesta = cesta.to_string();
        self.cesta_edit = cesta.to_string();
        self.soubor = crate::import::jmeno_souboru(cesta);
        self.z_vykresu = None;
        self.kodovani = None;
        self.list = 0;
        self.rucne = Rucne::default();
        match std::fs::read(cesta) {
            Ok(b) => {
                self.bajty = b;
                self.dekoduj();
            }
            Err(e) => {
                self.chyba = Some(trf!("nelze číst soubor {}: {}", cesta, e));
                self.druh = None;
            }
        }
    }

    /// Znovu převede bajty souboru (po změně kódování nebo listu).
    fn dekoduj(&mut self) {
        let vynutit = if self.rucne.kodovani { self.kodovani } else { None };
        match formaty::nacti(&self.cesta, &self.bajty, vynutit, self.list) {
            Ok(z) => {
                self.chyba = None;
                self.druh = Some(z.druh);
                self.kodovani = z.kodovani;
                self.listy = z.listy;
                self.text = z.text;
                self.rozpoznej_vse();
            }
            Err(e) => {
                self.chyba = Some(e);
                self.druh = None;
            }
        }
    }

    fn ze_schranky(&mut self, text: String, zdroje: &[String]) {
        let n = (1..).find(|i| !zdroje.iter().any(|z| z.eq_ignore_ascii_case(&trf!("schránka {}", i)))).unwrap_or(1);
        *self = Self { fronta: std::mem::take(&mut self.fronta), ..Self::prazdny() };
        self.soubor = trf!("schránka {}", n);
        self.druh = Some(DruhZdroje::Text);
        self.text = text.replace("\r\n", "\n");
        self.rozpoznej_vse();
    }

    fn z_vykresu_hotovo(&mut self, z: crate::import::ZVykresu) {
        let mut stav = self.z_vykresu.take().unwrap_or(ZVykresu {
            hladina: None,
            jen_vyber: false,
            tolerance: 0.0,
            hladiny: Vec::new(),
            statistika: Default::default(),
            ceka: false,
        });
        stav.ceka = false;
        stav.hladiny = z.hladiny;
        stav.statistika = z.statistika;
        self.cesta.clear();
        self.soubor = tr("výkres").into();
        self.druh = Some(DruhZdroje::Text);
        self.kodovani = None;
        self.chyba = None;
        self.text = z.text;
        self.z_vykresu = Some(stav);
        self.rozpoznej_vse();
    }

    /// Automatické rozpoznání (nebo pevný formát z Nastavení).
    fn rozpoznej_vse(&mut self) {
        self.rucne = Rucne { kodovani: self.rucne.kodovani, ..Default::default() };
        self.vynechat.clear();
        self.desetinna = Desetinna::Auto;
        self.zmeneno = true;
        let pref = nastaveni::akt();
        let k = match pref.format.as_deref().and_then(|f| FormatBodu::z_retezce(f, pref.oddelovac).ok()) {
            Some(f) if self.z_vykresu.is_none() && self.druh == Some(DruhZdroje::Text) => {
                self.popis_rozpoznani = tr("podle Nastavení (pevný formát)").into();
                Some(s_formatem(&self.text, &f, Puvod::Vzor))
            }
            _ => {
                let k = rozpoznani::rozpoznej(&self.text).into_iter().next();
                self.popis_rozpoznani = match &k {
                    Some(k) if k.sjtsk() => tr("rozpoznáno automaticky").into(),
                    Some(_) => tr("rozpoznáno automaticky – body neleží v S-JTSK pro ČR, ověřte sloupce").into(),
                    None => tr("formát se nepodařilo rozpoznat – nastavte sloupce ručně").into(),
                };
                k
            }
        };
        self.varovani = k.as_ref().is_none_or(|k| !k.sjtsk());
        match k {
            Some(k) => {
                self.sloupce = k.sloupce.clone();
                self.oddelovac = k.oddelovac;
                self.hlavicka = k.hlavicka;
                self.desetinna_zjistena = k.desetinna;
            }
            None => {
                self.sloupce = FormatBodu::z_retezce("PNEZD", Oddelovac::Mezera).map(|f| f.sloupce).unwrap_or_default();
                self.oddelovac = Oddelovac::Mezera;
                self.hlavicka = 0;
            }
        }
    }

    fn pouzij_sablonu(&mut self, s: &SablonaImportu) {
        if let Ok(f) = FormatBodu::z_retezce(&s.vzor, s.oddelovac) {
            if s.kodovani.is_some() && s.kodovani != self.kodovani && self.druh == Some(DruhZdroje::Text) && !self.bajty.is_empty() {
                self.kodovani = s.kodovani;
                self.rucne.kodovani = true;
                self.dekoduj();
            }
            self.sloupce = f.sloupce;
            self.oddelovac = s.oddelovac;
            self.desetinna = s.desetinna;
            self.hlavicka = s.hlavicka;
            self.rucne = Rucne { oddelovac: true, kodovani: self.rucne.kodovani, desetinna: true, hlavicka: true, sloupce: true };
            self.popis_rozpoznani = trf!("šablona „{}“", s.jmeno);
            self.varovani = false;
            self.zmeneno = true;
        }
    }

    fn sablona(&self, jmeno: &str) -> Option<SablonaImportu> {
        let f = FormatBodu::ze_sloupcu(self.sloupce.clone(), self.oddelovac).ok()?;
        Some(SablonaImportu {
            jmeno: jmeno.to_string(),
            vzor: f.vzor(),
            oddelovac: self.oddelovac,
            kodovani: if self.rucne.kodovani { self.kodovani } else { None },
            desetinna: self.desetinna,
            hlavicka: self.hlavicka,
        })
    }

    fn politika(&self) -> Politika {
        match self.politika {
            1 => Politika::Prepsat,
            2 => Politika::Precislovat(self.precislovat_od.max(1)),
            3 => Politika::Ponechat,
            _ => Politika::Preskocit,
        }
    }

    /// Přepočítá rozbor, když se něco změnilo (nebo body ve výkrese).
    fn prepocti(&mut self, stav: &StavSpravce) {
        if stav.verze != self.verze {
            self.verze = stav.verze;
            self.existujici = stav.body.iter().map(|b| b.bod.clone()).collect();
            if self.precislovat_od == 0 {
                self.precislovat_od = stav.dalsi_cislo.max(1);
            }
            self.zmeneno = true;
        }
        if !self.zmeneno || !self.ma_zdroj() {
            return;
        }
        self.zmeneno = false;
        let posledni_popis = self.sloupce.iter().rposition(|s| *s != Sloupec::Preskocit).map(|i| self.sloupce[i]) == Some(Sloupec::Popis);
        let format = FormatBodu::ze_sloupcu(self.sloupce.clone(), self.oddelovac).map(|mut f| {
            f.preskocit_radku = self.hlavicka;
            f.desetinna = self.desetinna;
            f
        });
        self.chyba_formatu = format.as_ref().err().cloned();
        self.vysledek = format.as_ref().map(|f| nacti_body(&self.text, f)).unwrap_or_default();
        let v = &self.vysledek;
        let bod_radku: HashMap<usize, usize> = v.radky.iter().enumerate().map(|(i, r)| (*r, i)).collect();
        let chyby: HashMap<usize, (Option<usize>, String)> = v.chyby.iter().map(|c| (c.radek, (c.sloupec, c.duvod.clone()))).collect();
        let n_fmt = format.as_ref().map_or(0, |f| f.sloupce.len());
        self.radky.clear();
        for (i, l) in self.text.lines().enumerate() {
            let t = l.trim();
            let cislo = i + 1;
            if t.is_empty() {
                continue;
            }
            let stav_radku = if i < self.hlavicka {
                StavRadku::Hlavicka
            } else if let Some(&b) = bod_radku.get(&cislo) {
                StavRadku::Bod(b)
            } else if let Some((s, d)) = chyby.get(&cislo) {
                StavRadku::Chyba(*s, d.clone())
            } else if self.chyba_formatu.is_some() && !t.starts_with('#') {
                StavRadku::Chyba(None, String::new())
            } else {
                continue; // komentář
            };
            let pole = rozdel_radek(l, self.oddelovac, if stav_radku == StavRadku::Hlavicka { 0 } else { n_fmt }, posledni_popis);
            self.radky.push(Radek { cislo, pole: pole.into_iter().map(str::to_string).collect(), stav: stav_radku });
        }
        self.ncol = self.radky.iter().take(500).map(|r| r.pole.len()).max().unwrap_or(0).max(self.sloupce.len()).clamp(1, 40);
        self.sloupce.resize(self.ncol, Sloupec::Preskocit);
        self.zahlavi = match self.hlavicka {
            0 => Vec::new(),
            h => self.text.lines().nth(h - 1).map(|l| rozdel_radek(l, self.oddelovac, 0, false).into_iter().map(str::to_string).collect()).unwrap_or_default(),
        };
        self.dup = duplicity::najdi(&v.body, &self.existujici);
        self.odlehle = rozpoznani::odlehle(&v.body).into_iter().collect();
        self.vetsina_v_cr = v.body.iter().filter(|b| v_cr(b.x, b.y)).count() * 2 > v.body.len();
        self.navrh = rozpoznani::navrh_opravy(&v.body, &self.sloupce);
        self.podezrele = (0..self.vysledek.body.len()).map(|i| self.urci_podezreni(i)).collect();
        self.prepocti_plan();
    }

    fn prepocti_plan(&mut self) {
        let body: Vec<Bod> = self
            .vysledek
            .body
            .iter()
            .zip(&self.vysledek.radky)
            .filter(|(_, r)| !self.vynechat.contains(r))
            .map(|(b, _)| b.clone())
            .collect();
        self.plan = duplicity::naplanuj(&body, &self.existujici, self.politika());
    }

    fn podezrely(&self, i: usize) -> Option<String> {
        self.podezrele.get(i).cloned().flatten()
    }

    fn je_podezrely(&self, i: usize) -> bool {
        self.podezrele.get(i).is_some_and(Option::is_some)
    }

    fn urci_podezreni(&self, i: usize) -> Option<String> {
        let b = self.vysledek.body.get(i)?;
        let mut d = Vec::new();
        if podezrela_vyska(b.z) {
            d.push(trf!("výška {:.2} m – překlep?", b.z));
        }
        if self.odlehle.contains(&i) {
            d.push(tr("daleko od ostatních bodů").to_string());
        } else if self.vetsina_v_cr && !v_cr(b.x, b.y) {
            d.push(tr("mimo ČR (S-JTSK)").to_string());
        }
        (!d.is_empty()).then(|| d.join(" · "))
    }

    fn duplicita(&self, i: usize) -> Option<String> {
        let b = self.vysledek.body.get(i)?;
        let akce = match self.politika() {
            Politika::Preskocit => tr(" → přeskočí se"),
            Politika::Prepsat => tr(" → přepíše"),
            Politika::Precislovat(_) => tr(" → přečísluje se"),
            Politika::Ponechat => "",
        };
        if self.dup.je_v_souboru(i) {
            return Some(trf!("číslo {} je v souboru víckrát{}", b.cislo, akce));
        }
        let e = self.dup.ve_vykresu_pro(i)?;
        let d = self.existujici.get(e).map_or(0.0, |x| (x.x - b.x).hypot(x.y - b.y));
        Some(trf!("{} už je ve výkresu ({:.2} m jinde){}", b.cislo, d, akce))
    }

    fn pocty(&self) -> (usize, usize, usize, usize, usize) {
        let body = self.vysledek.body.len();
        let chyby = self.vysledek.chyby.len();
        let podezrele = self.podezrele.iter().filter(|p| p.is_some()).count();
        (body + chyby, body, chyby, podezrele, self.dup.pocet())
    }

    fn viditelny(&self, r: &Radek) -> bool {
        match (self.filtr, &r.stav) {
            (Filtr::Vse, _) => true,
            (Filtr::Nacte, StavRadku::Bod(_)) => true,
            (Filtr::Chyby, StavRadku::Chyba(..)) => true,
            (Filtr::Podezrele, StavRadku::Bod(i)) => self.je_podezrely(*i),
            (Filtr::Duplicity, StavRadku::Bod(i)) => self.dup.je_v_souboru(*i) || self.dup.ve_vykresu_pro(*i).is_some(),
            _ => false,
        }
    }

    fn je_sjtsk(&self) -> bool {
        self.sloupce.contains(&Sloupec::SjtskY)
    }

    fn nastaveni_importu(&self) -> Option<NastaveniImportu> {
        let f = FormatBodu::ze_sloupcu(self.sloupce.clone(), self.oddelovac).ok()?;
        Some(NastaveniImportu {
            soubor: self.soubor.clone(),
            cesta: self.cesta.clone(),
            text: self.text.clone(),
            format: f.vzor(),
            oddelovac: self.oddelovac,
            hlavicka: self.hlavicka,
            desetinna: self.desetinna,
            duplicity: self.politika(),
            skupina: self.skupina.clone(),
            styl: self.styl.clone(),
            priblizit: self.priblizit,
            vynechat: self.vynechat.iter().copied().collect(),
        })
    }
}

// ── kreslení ─────────────────────────────────────────────────────────────────

fn karta(ui: &mut egui::Ui, obsah: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    egui::Frame::new().fill(KARTA).stroke(Stroke::new(1.0, LINKA)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, obsah).response
}

fn nadpis(ui: &mut egui::Ui, t: &str) {
    ui.label(RichText::new(t.to_uppercase()).small().strong().color(SEDA));
}

/// Čip v souhrnném pásu: popisek, obsah a štítek auto/ručně.
fn cip(ui: &mut egui::Ui, popisek: &str, rucne: Option<bool>, obsah: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(KARTA).stroke(Stroke::new(1.0, LINKA)).corner_radius(14.0).inner_margin(egui::Margin::symmetric(9, 3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.label(RichText::new(popisek).color(SEDA));
            obsah(ui);
            match rucne {
                Some(true) => ui.label(RichText::new(tr("ručně")).small().color(ORANZOVA)),
                Some(false) => ui.label(RichText::new(tr("auto")).small().color(ZELENA)),
                None => ui.label(""),
            };
        });
    });
}

/// Pruh s upozorněním a tlačítkem. Vrací `true` po kliknutí.
fn pruh(ui: &mut egui::Ui, barva: Color32, text: &str, tlacitko: Option<&str>) -> bool {
    let mut klik = false;
    egui::Frame::new()
        .fill(barva.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0, barva.gamma_multiply(0.6)))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("●").color(barva));
                ui.label(text);
                if let Some(t) = tlacitko {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| klik = ui.button(t).clicked());
                }
            });
        });
    klik
}

fn mapka(ui: &mut egui::Ui, n: &NahledImportu) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 210.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, Color32::from_gray(17));
    let body: Vec<(usize, &Bod)> = n.vysledek.body.iter().enumerate().filter(|(i, _)| !n.odlehle.contains(i)).collect();
    if body.is_empty() {
        p.text(rect.center(), egui::Align2::CENTER_CENTER, tr("žádné body"), FontId::proportional(12.0), SEDA);
        return;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (_, b) in &body {
        x0 = x0.min(b.x);
        x1 = x1.max(b.x);
        y0 = y0.min(b.y);
        y1 = y1.max(b.y);
    }
    // body výkresu v okolí importu
    let okraj = ((x1 - x0).max(y1 - y0) * 0.5).max(20.0);
    let okoli: Vec<&Bod> = n
        .existujici
        .iter()
        .filter(|b| b.x > x0 - okraj && b.x < x1 + okraj && b.y > y0 - okraj && b.y < y1 + okraj)
        .take(5000)
        .collect();
    for b in &okoli {
        x0 = x0.min(b.x);
        x1 = x1.max(b.x);
        y0 = y0.min(b.y);
        y1 = y1.max(b.y);
    }
    let pad = 14.0;
    let s = ((rect.width() as f64 - 2.0 * pad) / (x1 - x0).max(1e-3)).min((rect.height() as f64 - 2.0 * pad) / (y1 - y0).max(1e-3));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let bod = |x: f64, y: f64| egui::pos2(rect.center().x + ((x - cx) * s) as f32, rect.center().y - ((y - cy) * s) as f32);
    for b in &okoli {
        p.circle_filled(bod(b.x, b.y), 2.0, Color32::from_gray(95));
    }
    let krok = (body.len() / 4000).max(1);
    for (i, b) in body.iter().step_by(krok) {
        let barva = if n.dup.je_v_souboru(*i) || n.dup.ve_vykresu_pro(*i).is_some() {
            FIALOVA
        } else if n.je_podezrely(*i) {
            ORANZOVA.gamma_multiply(1.2)
        } else {
            Color32::from_rgb(240, 163, 58)
        };
        let c = bod(b.x, b.y);
        p.line_segment([c - egui::vec2(4.0, 0.0), c + egui::vec2(4.0, 0.0)], Stroke::new(1.2, barva));
        p.line_segment([c - egui::vec2(0.0, 4.0), c + egui::vec2(0.0, 4.0)], Stroke::new(1.2, barva));
    }
    if !n.odlehle.is_empty() {
        p.text(rect.right_center() - egui::vec2(8.0, 0.0), egui::Align2::RIGHT_CENTER, trf!("mimo: {} ›", n.odlehle.len()), FontId::proportional(11.0), ORANZOVA);
    }
    // měřítko
    let delka = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0]
        .into_iter()
        .find(|d| d * s > 40.0)
        .unwrap_or(10000.0);
    let a = egui::pos2(rect.left() + 10.0, rect.bottom() - 10.0);
    p.line_segment([a, a + egui::vec2((delka * s) as f32, 0.0)], Stroke::new(1.0, SEDA));
    p.text(a + egui::vec2((delka * s) as f32 + 4.0, 0.0), egui::Align2::LEFT_CENTER, format!("{delka} m"), FontId::proportional(10.0), SEDA);
    p.text(rect.right_top() + egui::vec2(-8.0, 8.0), egui::Align2::RIGHT_TOP, tr("S ↑"), FontId::proportional(10.0), SEDA);
    let poznamka = if n.existujici.is_empty() {
        RichText::new(tr("Ve výkrese zatím nejsou body.")).color(SEDA)
    } else if okoli.is_empty() {
        RichText::new(tr("Body leží daleko od bodů výkresu – zkontrolujte sloupce a soustavu.")).color(ORANZOVA)
    } else {
        RichText::new(tr("Body leží u bodů výkresu (šedě).")).color(SEDA)
    };
    ui.label(poznamka.small());
}

impl Okno {
    pub(super) fn okno_importu(&mut self, ctx: &egui::Context, stav: &StavSpravce) {
        let Some(n) = self.nahled_importu.as_mut() else { return };
        // přetažené soubory: první se načte, další čekají ve frontě
        let pretazene: Vec<String> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().display().to_string()).filter(|p| !p.is_empty()).collect());
        if let Some((prvni, dalsi)) = pretazene.split_first() {
            *n = NahledImportu::novy(prvni);
            n.fronta = dalsi.to_vec();
            n.poradi = (1, pretazene.len());
        }
        // Ctrl+V mimo textové pole = tabulka ze schránky
        if ctx.memory(|m| m.focused().is_none()) {
            if let Some(t) = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Paste(t) = e { Some(t.clone()) } else { None })) {
                if !t.trim().is_empty() {
                    n.ze_schranky(t, &stav.zdroje);
                }
            }
        }
        if let Some(z) = crate::import::vezmi_z_vykresu() {
            match z {
                Ok(z) => n.z_vykresu_hotovo(z),
                Err(e) => n.chyba = Some(e),
            }
        }
        n.prepocti(stav);

        let mut otevreno = true;
        let mut hotovo = false;
        let titulek = if n.soubor.is_empty() { tr("Import bodů").to_string() } else { trf!("Import bodů – {}", n.soubor) };
        egui::Window::new(titulek)
            .id(egui::Id::new("teren_import"))
            .open(&mut otevreno)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([980.0, 660.0])
            .collapsible(false)
            .max_width(ctx.content_rect().width() - 24.0)
            .show(ctx, |ui| {
                if !n.ma_zdroj() {
                    volba_zdroje(ui, n);
                    return;
                }
                horni_radek(ui, n, stav);
                if let Some(e) = &n.chyba {
                    ui.add_space(8.0);
                    ui.label(RichText::new(e).color(CERVENA));
                    return;
                }
                if n.z_vykresu.as_ref().is_some_and(|z| z.ceka) {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr("Čtu body z výkresu…"));
                    });
                    return;
                }
                souhrnny_pas(ui, n);
                pruhy(ui, n, stav);
                karty_poctu(ui, n);
                ui.add_space(4.0);
                let vyska = (ui.available_height() - 48.0).max(220.0);
                let pravy = 290.0;
                ui.horizontal_top(|ui| {
                    let levy = (ui.available_width() - pravy - 12.0).max(300.0);
                    ui.allocate_ui(egui::vec2(levy, vyska), |ui| tabulka(ui, n, vyska));
                    ui.add_space(4.0);
                    ui.allocate_ui(egui::vec2(pravy, vyska), |ui| {
                        egui::ScrollArea::vertical().id_salt("imp_pravy").max_height(vyska).show(ui, |ui| pravy_panel(ui, n, stav));
                    });
                });
                ui.separator();
                hotovo = paticka(ui, n);
            });
        if hotovo {
            let dalsi = n.fronta.first().cloned();
            if let Some(c) = dalsi {
                let mut fronta = std::mem::take(&mut n.fronta);
                fronta.remove(0);
                let poradi = (n.poradi.0 + 1, n.poradi.1);
                *n = NahledImportu::novy(&c);
                n.fronta = fronta;
                n.poradi = poradi;
            } else {
                self.nahled_importu = None;
            }
        } else if !otevreno {
            self.nahled_importu = None;
        }
    }
}

fn volba_zdroje(ui: &mut egui::Ui, n: &mut NahledImportu) {
    ui.add_space(4.0);
    egui::Frame::new().stroke(Stroke::new(1.5, Color32::from_gray(80))).corner_radius(10.0).inner_margin(18.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(tr("Přetáhněte sem soubor (nebo více souborů)")).strong().size(15.0));
            ui.label(RichText::new(tr("TXT, CSV, XYZ, XLSX, LandXML, JobXML, GSI · nebo stiskněte Ctrl+V pro vložení tabulky ze schránky")).color(SEDA));
        });
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(tr("Soubor"));
        let r = ui.add(egui::TextEdit::singleline(&mut n.cesta_edit).hint_text(tr("cesta k souboru")).desired_width((ui.available_width() - 80.0).max(120.0)));
        if ui.button(tr("Načíst")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            let c = n.cesta_edit.trim().trim_matches('"').to_string();
            if !c.is_empty() {
                n.nacti_soubor(&c);
            }
        }
    });
    ui.label(RichText::new(tr("Soubor vyberete i tlačítkem Import bodů na pásu karet.")).small().color(SEDA));
    ui.add_space(8.0);
    ui.columns(2, |sl| {
        let ui = &mut sl[0];
        nadpis(ui, tr("Zdroj"));
        let polozka = |ui: &mut egui::Ui, titul: &str, popis: &str| {
            karta(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(titul).strong());
                ui.label(RichText::new(popis).small().color(SEDA));
            })
        };
        polozka(ui, tr("Textový soubor"), tr("TXT, CSV, XYZ – libovolné pořadí sloupců, rozpozná se samo"));
        polozka(ui, tr("Schránka (Ctrl+V)"), tr("tabulka zkopírovaná z Excelu, e-mailu nebo webu"));
        polozka(ui, tr("Excel"), tr("XLSX, XLS, ODS – výběr listu, sloupce stejně jako u textu"));
        polozka(ui, tr("LandXML, Trimble JobXML, Leica GSI"), tr("body CgPoints, redukované body, GSI-8/16"));
        let r = polozka(ui, tr("Z výkresu…"), tr("POINT, bloky s atributy, texty výšek u značky → body Terénu"));
        if r.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            n.z_vykresu = Some(ZVykresu { hladina: None, jen_vyber: false, tolerance: 0.0, hladiny: Vec::new(), statistika: Default::default(), ceka: true });
            n.soubor = tr("výkres").into();
            spravce::posli(Akce::BodyZVykresu(None, false, 0.0));
        }
        polozka(ui, tr("Více souborů"), tr("přetáhněte je najednou – každý do vlastní skupiny"));
        let ui = &mut sl[1];
        let nast = nastaveni::akt();
        nadpis(ui, tr("Poslední soubory"));
        if nast.posledni_soubory.is_empty() {
            ui.label(RichText::new(tr("zatím žádné")).color(SEDA));
        }
        for c in &nast.posledni_soubory {
            if ui.selectable_label(false, crate::import::jmeno_souboru(c)).on_hover_text(c).clicked() {
                n.nacti_soubor(c);
            }
        }
        ui.add_space(8.0);
        nadpis(ui, tr("Šablony formátu"));
        if nast.import_sablony.is_empty() {
            ui.label(RichText::new(tr("Šablonu uložíte v náhledu tlačítkem Uložit šablonu.")).small().color(SEDA));
        }
        for s in &nast.import_sablony {
            ui.label(format!("{} · {} · {}", s.jmeno, s.vzor, nazev_oddelovace(s.oddelovac)));
        }
    });
}

fn horni_radek(ui: &mut egui::Ui, n: &mut NahledImportu, stav: &StavSpravce) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&n.soubor).strong().size(15.0));
        let mut info = Vec::new();
        if let Some(d) = n.druh.filter(|d| *d != DruhZdroje::Text) {
            info.push(d.nazev().to_string());
        }
        if !n.cesta.is_empty() {
            info.push(format!("{:.1} kB", n.bajty.len() as f64 / 1024.0));
        }
        info.push(trf!("{} řádků", n.text.lines().count()));
        if n.poradi.1 > 1 {
            info.push(trf!("soubor {} z {}", n.poradi.0, n.poradi.1));
        }
        ui.label(RichText::new(info.join(" · ")).color(SEDA)).on_hover_text(&n.cesta);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut nast = nastaveni::akt();
            if let Some(jmeno) = &mut n.nova_sablona {
                let mut ulozit = false;
                if ui.button(tr("Zrušit")).clicked() {
                    n.nova_sablona = None;
                    return;
                }
                if ui.button(tr("Uložit")).clicked() {
                    ulozit = true;
                }
                let r = ui.add(egui::TextEdit::singleline(jmeno).desired_width(150.0).hint_text(tr("jméno šablony")));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ulozit = true;
                }
                if ulozit && !jmeno.trim().is_empty() {
                    let jmeno = jmeno.trim().to_string();
                    if let Some(s) = n.sablona(&jmeno) {
                        nastaveni::zmen(|x| {
                            x.import_sablony.retain(|t| t.jmeno != s.jmeno);
                            x.import_sablony.push(s);
                        });
                    }
                    n.nova_sablona = None;
                }
                return;
            }
            if ui.button(tr("Uložit šablonu…")).on_hover_text(tr("Uložit sloupce, oddělovač, kódování a desetinnou čárku jako šablonu")).clicked() {
                n.nova_sablona = Some(String::new());
            }
            let mut volba: Option<Option<SablonaImportu>> = None;
            egui::ComboBox::from_id_salt("imp_sablona").selected_text(tr("Šablona…")).width(150.0).show_ui(ui, |ui| {
                if ui.selectable_label(false, tr("Rozpoznat automaticky")).clicked() {
                    volba = Some(None);
                }
                if let Some(p) = nast.import_posledni.take() {
                    if ui.selectable_label(false, trf!("Naposledy použité ({})", p.vzor)).clicked() {
                        volba = Some(Some(p));
                    }
                }
                for s in &nast.import_sablony {
                    if ui.selectable_label(false, format!("{} · {}", s.jmeno, s.vzor)).clicked() {
                        volba = Some(Some(s.clone()));
                    }
                }
            });
            match volba {
                Some(Some(s)) => n.pouzij_sablonu(&s),
                Some(None) => {
                    n.rucne.kodovani = false;
                    if n.bajty.is_empty() {
                        n.rozpoznej_vse();
                    } else {
                        n.dekoduj();
                    }
                }
                None => {}
            }
            if ui.button(tr("Jiný zdroj…")).clicked() {
                let fronta = std::mem::take(&mut n.fronta);
                *n = NahledImportu::prazdny();
                n.fronta = fronta;
                n.verze = u64::MAX;
            }
            let _ = stav;
        });
    });
}

fn souhrnny_pas(ui: &mut egui::Ui, n: &mut NahledImportu) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        let vzor = FormatBodu::ze_sloupcu(n.sloupce.clone(), n.oddelovac).map(|f| f.vzor()).unwrap_or_else(|_| "?".into());
        cip(ui, tr("Formát"), Some(n.rucne.sloupce), |ui| {
            egui::ComboBox::from_id_salt("imp_vzor").selected_text(RichText::new(&vzor).strong()).width(70.0).show_ui(ui, |ui| {
                for (v, popis) in VZORY {
                    if ui.selectable_label(vzor == v, tr(popis)).clicked() {
                        if let Ok(f) = FormatBodu::z_retezce(v, n.oddelovac) {
                            n.sloupce = f.sloupce;
                            n.rucne.sloupce = true;
                            n.zmeneno = true;
                        }
                    }
                }
            });
        });
        cip(ui, tr("Oddělovač"), Some(n.rucne.oddelovac), |ui| {
            egui::ComboBox::from_id_salt("imp_odd").selected_text(RichText::new(nazev_oddelovace(n.oddelovac)).strong()).width(80.0).show_ui(ui, |ui| {
                for o in ODDELOVACE {
                    if ui.selectable_value(&mut n.oddelovac, o, nazev_oddelovace(o)).clicked() {
                        n.rucne.oddelovac = true;
                        n.zmeneno = true;
                    }
                }
            });
        });
        if n.druh == Some(DruhZdroje::Excel) {
            cip(ui, tr("List"), None, |ui| {
                let akt = n.listy.get(n.list).cloned().unwrap_or_default();
                let mut novy = None;
                egui::ComboBox::from_id_salt("imp_list").selected_text(RichText::new(akt).strong()).width(110.0).show_ui(ui, |ui| {
                    for (i, l) in n.listy.iter().enumerate() {
                        if ui.selectable_label(i == n.list, l).clicked() {
                            novy = Some(i);
                        }
                    }
                });
                if let Some(i) = novy.filter(|i| *i != n.list) {
                    n.list = i;
                    n.dekoduj();
                }
            });
        } else if let Some(k) = n.kodovani {
            cip(ui, tr("Kódování"), Some(n.rucne.kodovani), |ui| {
                let mut novy = None;
                egui::ComboBox::from_id_salt("imp_kod").selected_text(RichText::new(k.nazev()).strong()).width(105.0).show_ui(ui, |ui| {
                    for x in kodovani::VSECHNA {
                        if ui.selectable_label(x == k, x.nazev()).clicked() {
                            novy = Some(x);
                        }
                    }
                });
                if let Some(x) = novy.filter(|x| *x != k) {
                    n.kodovani = Some(x);
                    n.rucne.kodovani = true;
                    let r = n.rucne;
                    let (s, o, h, d) = (n.sloupce.clone(), n.oddelovac, n.hlavicka, n.desetinna);
                    n.dekoduj();
                    // ručně nastavené volby zůstanou
                    if r.sloupce || r.oddelovac || r.hlavicka || r.desetinna {
                        (n.sloupce, n.oddelovac, n.hlavicka, n.desetinna) = (s, o, h, d);
                        n.rucne = r;
                    }
                }
            });
        }
        cip(ui, tr("Desetinná"), Some(n.rucne.desetinna), |ui| {
            let text = match n.desetinna {
                Desetinna::Auto => nazev_desetinne(n.desetinna_zjistena),
                d => nazev_desetinne(d),
            };
            egui::ComboBox::from_id_salt("imp_des").selected_text(RichText::new(text).strong()).width(70.0).show_ui(ui, |ui| {
                for d in [Desetinna::Auto, Desetinna::Tecka, Desetinna::Carka] {
                    if ui.selectable_value(&mut n.desetinna, d, nazev_desetinne(d)).clicked() {
                        n.rucne.desetinna = d != Desetinna::Auto;
                        n.zmeneno = true;
                    }
                }
            });
        });
        cip(ui, tr("Záhlaví"), Some(n.rucne.hlavicka), |ui| {
            if ui.add(egui::DragValue::new(&mut n.hlavicka).range(0..=50).suffix(tr(" ř."))).on_hover_text(tr("počet úvodních řádků, které se nenačtou")).changed() {
                n.rucne.hlavicka = true;
                n.zmeneno = true;
            }
        });
        cip(ui, tr("Souřadnice"), None, |ui| {
            ui.label(RichText::new(if n.je_sjtsk() { tr("S-JTSK → výkres −Y/−X") } else { tr("výkres E/N (beze změny)") }).strong());
        });
    });
    ui.label(RichText::new(&n.popis_rozpoznani).small().color(if n.varovani { ORANZOVA } else { SEDA }));
}

fn pruhy(ui: &mut egui::Ui, n: &mut NahledImportu, stav: &StavSpravce) {
    if let Some(e) = &n.chyba_formatu {
        pruh(ui, CERVENA, &trf!("Sloupce nedávají platný formát: {}", e), None);
    }
    if let Some((navrh, sloupce)) = n.navrh.clone() {
        let (text, tl) = match navrh {
            Navrh::Prohodit => (tr("Y a X (N a E) jsou nejspíš prohozené – po prohození leží body v ČR."), tr("Prohodit")),
            Navrh::Znamenko => (tr("Souřadnice mají opačné znaménko než sloupce – po převodu leží body v ČR."), tr("Převést")),
            Navrh::ProhoditZnamenko => (tr("Souřadnice jsou prohozené a mají opačné znaménko – po opravě leží body v ČR."), tr("Opravit")),
        };
        if pruh(ui, ORANZOVA, text, Some(tl)) {
            n.sloupce = sloupce;
            n.rucne.sloupce = true;
            n.zmeneno = true;
        }
    }
    if let Some(k) = n.kodovani {
        if !n.rucne.kodovani && kodovani::podezrele_znaky(&n.text) > 0 {
            let jine = if k == Kodovani::Cp1250 { Kodovani::Cp852 } else { Kodovani::Cp1250 };
            if pruh(ui, MODRA, &trf!("V textu jsou nečitelné znaky – soubor je možná v kódování {}.", jine.nazev()), Some(&trf!("Použít {}", jine.nazev()))) {
                n.kodovani = Some(jine);
                n.rucne.kodovani = true;
                n.dekoduj();
            }
        }
    }
    let (_, body, chyby, _, _) = n.pocty();
    if chyby > 0 && chyby > body && pruh(ui, CERVENA, tr("Většina řádků nejde přečíst – zkontrolujte oddělovač, desetinnou čárku a sloupce."), Some(tr("Vrátit rozpoznané"))) {
        n.rucne.kodovani = false;
        if n.bajty.is_empty() {
            n.rozpoznej_vse();
        } else {
            n.dekoduj();
        }
    }
    let uz = stav.body.iter().filter(|b| !n.soubor.is_empty() && b.zdroj.eq_ignore_ascii_case(&n.soubor)).count();
    if uz > 0 && n.politika != 1 && pruh(ui, MODRA, &trf!("Soubor už byl importován ({} bodů ve výkrese) – body se stejným číslem jde aktualizovat.", uz), Some(tr("Aktualizovat podle čísla"))) {
        n.politika = 1;
        n.prepocti_plan();
    }
    if let Some(z) = &mut n.z_vykresu {
        let st = &z.statistika;
        let mut znovu = false;
        egui::Frame::new().fill(KARTA).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(trf!(
                    "Značek {} · výška z textu {} · z atributu {} · ze značky {} · bez výšky {}",
                    st.znacek, st.vyska_z_textu, st.vyska_z_atributu, st.vyska_ze_znacky, st.bez_vysky
                ));
                ui.separator();
                ui.label(tr("Hladina"));
                let akt = z.hladina.clone().unwrap_or_else(|| tr("všechny").into());
                egui::ComboBox::from_id_salt("imp_hladina").selected_text(akt).width(140.0).show_ui(ui, |ui| {
                    if ui.selectable_label(z.hladina.is_none(), tr("všechny")).clicked() {
                        z.hladina = None;
                        znovu = true;
                    }
                    for (h, pocet) in &z.hladiny {
                        if ui.selectable_label(z.hladina.as_ref() == Some(h), format!("{h} ({pocet})")).clicked() {
                            z.hladina = Some(h.clone());
                            znovu = true;
                        }
                    }
                });
                znovu |= ui.checkbox(&mut z.jen_vyber, tr("jen vybrané")).changed();
                ui.label(tr("Tolerance textu"));
                if ui.add(egui::DragValue::new(&mut z.tolerance).range(0.0..=100.0).speed(0.1).suffix(" m")).on_hover_text(tr("0 = trojnásobek výšky textu")).changed() {
                    znovu = true;
                }
            });
        });
        if znovu {
            z.ceka = true;
            spravce::posli(Akce::BodyZVykresu(z.hladina.clone(), z.jen_vyber, z.tolerance));
        }
    }
}

fn karty_poctu(ui: &mut egui::Ui, n: &mut NahledImportu) {
    let (radku, body, chyby, podezrele, dup) = n.pocty();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let sirka = ((ui.available_width() - 4.0 * 8.0) / 5.0).max(90.0);
        for (f, pocet, popis, barva) in [
            (Filtr::Vse, radku, tr("řádků s daty"), ui.visuals().strong_text_color()),
            (Filtr::Nacte, body, tr("načte se"), ZELENA),
            (Filtr::Chyby, chyby, tr("nečitelné"), CERVENA),
            (Filtr::Podezrele, podezrele, tr("podezřelé"), ORANZOVA),
            (Filtr::Duplicity, dup, tr("duplicitní čísla"), FIALOVA),
        ] {
            let vybrano = n.filtr == f;
            let r = egui::Frame::new()
                .fill(if vybrano { Color32::from_rgb(29, 44, 60) } else { KARTA })
                .stroke(Stroke::new(1.0, if vybrano { Color32::from_rgb(59, 119, 184) } else { LINKA }))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.set_width(sirka - 22.0);
                    ui.label(RichText::new(pocet.to_string()).size(18.0).strong().color(if pocet == 0 && f != Filtr::Vse && f != Filtr::Nacte { SEDA } else { barva }));
                    ui.label(RichText::new(popis).small().color(SEDA));
                })
                .response
                .interact(Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tr("klikněte pro filtr tabulky"));
            if r.clicked() {
                n.filtr = if vybrano { Filtr::Vse } else { f };
            }
        }
    });
}

fn tabulka(ui: &mut egui::Ui, n: &mut NahledImportu, vyska: f32) {
    let w_cislo = 40.0;
    let w_stav = 16.0;
    let w_duvod = 280.0;
    let sirky: Vec<f32> = (0..n.ncol).map(|j| if n.sloupce.get(j) == Some(&Sloupec::Popis) { 170.0 } else { 104.0 }).collect();
    let viditelne: Vec<usize> = (0..n.radky.len()).filter(|&i| n.viditelny(&n.radky[i])).collect();
    let mut prepnout: Option<usize> = None;
    egui::Frame::new().stroke(Stroke::new(1.0, LINKA)).corner_radius(8.0).inner_margin(4.0).show(ui, |ui| {
        egui::ScrollArea::horizontal().id_salt("imp_tab_h").auto_shrink([false, true]).show(ui, |ui| {
            // záhlaví s výběrem role sloupce
            ui.horizontal(|ui| {
                ui.add_space(w_cislo + w_stav + 8.0);
                for (j, &w) in sirky.iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        let akt = n.sloupce[j];
                        let text = RichText::new(nazev_role(akt)).strong().color(if akt == Sloupec::Preskocit { SEDA } else { ui.visuals().strong_text_color() });
                        egui::ComboBox::from_id_salt(("imp_role", j)).selected_text(text).width(w - 6.0).show_ui(ui, |ui| {
                            for r in ROLE {
                                if ui.selectable_label(akt == r, nazev_role(r)).clicked() && akt != r {
                                    // role může mít jen jeden sloupec – původní držitel se ignoruje
                                    if r != Sloupec::Preskocit {
                                        for s in n.sloupce.iter_mut().filter(|s| **s == r) {
                                            *s = Sloupec::Preskocit;
                                        }
                                    }
                                    n.sloupce[j] = r;
                                    n.rucne.sloupce = true;
                                    n.zmeneno = true;
                                }
                            }
                        });
                        let puvodni = n.zahlavi.get(j).map(|t| format!("„{t}“")).unwrap_or_else(|| trf!("sloupec {}", j + 1));
                        ui.label(RichText::new(puvodni).small().color(SEDA));
                    });
                }
                ui.label(RichText::new(tr("Stav")).color(SEDA));
            });
            ui.separator();
            let vyska_radku = 20.0;
            egui::ScrollArea::vertical()
                .id_salt("imp_tab_v")
                .max_height((vyska - 70.0).max(120.0))
                .auto_shrink([false, false])
                .show_rows(ui, vyska_radku, viditelne.len(), |ui, rozsah| {
                    for &ri in &viditelne[rozsah] {
                        let r = &n.radky[ri];
                        let (barva, tecka, duvod, chybny_sloupec, bod) = match &r.stav {
                            StavRadku::Hlavicka => (Color32::TRANSPARENT, SEDA, tr("záhlaví – nenačítá se").to_string(), None, None),
                            StavRadku::Chyba(s, d) => (CERVENA.gamma_multiply(0.13), CERVENA, d.clone(), *s, None),
                            StavRadku::Bod(i) => {
                                let dup = n.duplicita(*i);
                                let pod = n.podezrely(*i);
                                let (barva, tecka) = if dup.is_some() {
                                    (FIALOVA.gamma_multiply(0.12), FIALOVA)
                                } else if pod.is_some() {
                                    (ORANZOVA.gamma_multiply(0.12), ORANZOVA)
                                } else {
                                    (Color32::TRANSPARENT, ZELENA)
                                };
                                let text = [pod, dup].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                                (barva, tecka, text, None, Some(*i))
                            }
                        };
                        let vyrazen = n.vynechat.contains(&r.cislo);
                        egui::Frame::new().fill(barva).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.set_height(vyska_radku - 2.0);
                                ui.add_sized([w_cislo, 16.0], egui::Label::new(RichText::new(r.cislo.to_string()).color(SEDA).small()))
                                    .on_hover_text(n.text.lines().nth(r.cislo - 1).unwrap_or(""));
                                ui.add_sized([w_stav, 16.0], egui::Label::new(RichText::new("●").color(tecka)));
                                for (j, &w) in sirky.iter().enumerate() {
                                    let h = r.pole.get(j).map(String::as_str).unwrap_or("");
                                    let mut t = if h.is_empty() && matches!(r.stav, StavRadku::Chyba(..)) && chybny_sloupec == Some(j) {
                                        RichText::new("—").color(CERVENA)
                                    } else {
                                        RichText::new(h)
                                    };
                                    if r.stav == StavRadku::Hlavicka || n.sloupce.get(j) == Some(&Sloupec::Preskocit) {
                                        t = t.color(SEDA);
                                    }
                                    if chybny_sloupec == Some(j) {
                                        t = t.color(CERVENA).underline();
                                    }
                                    if vyrazen {
                                        t = t.strikethrough().color(SEDA);
                                    }
                                    ui.add_sized([w, 16.0], egui::Label::new(t).truncate());
                                }
                                if let Some(i) = bod {
                                    if n.je_podezrely(i) || vyrazen {
                                        let mut v = vyrazen;
                                        if ui.checkbox(&mut v, tr("vyřadit")).changed() {
                                            prepnout = Some(r.cislo);
                                        }
                                    }
                                }
                                let duvod = if duvod.is_empty() { RichText::new(tr("v pořádku")).color(ZELENA) } else { RichText::new(duvod).color(SEDA) };
                                ui.add_sized([w_duvod, 16.0], egui::Label::new(duvod.small()).truncate());
                            });
                        });
                    }
                    if viditelne.is_empty() {
                        ui.label(RichText::new(tr("Žádné řádky – zkuste jiný filtr.")).color(SEDA));
                    }
                });
        });
    });
    if let Some(c) = prepnout {
        if !n.vynechat.remove(&c) {
            n.vynechat.insert(c);
        }
        n.prepocti_plan();
    }
}

fn pravy_panel(ui: &mut egui::Ui, n: &mut NahledImportu, stav: &StavSpravce) {
    nadpis(ui, tr("Poloha bodů"));
    mapka(ui, n);
    ui.add_space(6.0);
    let v_souboru = n.dup.v_souboru.len();
    let ve_vykresu = n.dup.ve_vykresu.len();
    karta(ui, |ui| {
        ui.set_width(ui.available_width());
        nadpis(ui, &trf!("Duplicitní čísla · {} v souboru, {} ve výkresu", v_souboru, ve_vykresu));
        let mut zmena = false;
        let pocet = v_souboru + ve_vykresu;
        for (i, titul, popis, cislo) in [
            (0, tr("Přeskočit"), tr("ponechat body ve výkrese, z dvojic v souboru vzít první"), trf!("{} přeskočeno", pocet)),
            (1, tr("Přepsat"), tr("aktualizovat polohu a výšku podle souboru"), trf!("{} přepsáno", ve_vykresu)),
            (2, tr("Přečíslovat"), tr("nová čísla dostanou jen duplicity"), trf!("{} nových čísel", pocet)),
            (3, tr("Importovat i tak"), tr("dosavadní chování – dvě stejná čísla"), trf!("{} zdvojeno", pocet)),
        ] {
            ui.horizontal(|ui| {
                zmena |= ui.radio_value(&mut n.politika, i, titul).changed();
                if i == 2 && n.politika == 2 {
                    ui.label(tr("od"));
                    zmena |= ui.add(egui::DragValue::new(&mut n.precislovat_od).range(1..=u64::MAX / 2)).changed();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(cislo).small().color(SEDA)));
            });
            ui.label(RichText::new(popis).small().color(SEDA));
        }
        if zmena {
            n.prepocti_plan();
        }
    });
    ui.add_space(6.0);
    karta(ui, |ui| {
        ui.set_width(ui.available_width());
        nadpis(ui, tr("Kam"));
        let jmeno_nove = trf!("Nová skupina „Import {}“", std::path::Path::new(&n.soubor).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        let akt = match &n.skupina {
            CilSkupiny::Nova => jmeno_nove.clone(),
            CilSkupiny::Existujici(id) => stav.skupiny.iter().find(|s| &s.skupina.id == id).map(|s| s.skupina.jmeno.clone()).unwrap_or_default(),
            CilSkupiny::Zadna => tr("Bez skupiny").into(),
        };
        ui.label(RichText::new(tr("Skupina")).small().color(SEDA));
        egui::ComboBox::from_id_salt("imp_skupina").selected_text(akt).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut n.skupina, CilSkupiny::Nova, jmeno_nove);
            for s in stav.skupiny.iter().filter(|s| !s.skupina.vse) {
                ui.selectable_value(&mut n.skupina, CilSkupiny::Existujici(s.skupina.id.clone()), trf!("{} ({} bodů)", s.skupina.jmeno, s.pocet));
            }
            ui.selectable_value(&mut n.skupina, CilSkupiny::Zadna, tr("Bez skupiny"));
        });
        ui.label(RichText::new(tr("Styl bodů")).small().color(SEDA));
        let akt = if n.styl.is_empty() {
            tr("Podle skupiny a kódu").to_string()
        } else {
            stav.styly_bodu.iter().find(|s| s.id == n.styl).map(|s| s.jmeno.clone()).unwrap_or_else(|| n.styl.clone())
        };
        egui::ComboBox::from_id_salt("imp_styl").selected_text(akt).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut n.styl, String::new(), tr("Podle skupiny a kódu"));
            for s in &stav.styly_bodu {
                ui.selectable_value(&mut n.styl, s.id.clone(), &s.jmeno);
            }
        });
        ui.add_space(4.0);
        ui.checkbox(&mut n.priblizit, tr("Po importu přiblížit na body"));
        ui.checkbox(&mut n.zapamatovat, tr("Zapamatovat nastavení pro příště"));
    });
}

/// Patička se souhrnem a tlačítky. Vrací `true`, když se okno má zavřít.
fn paticka(ui: &mut egui::Ui, n: &mut NahledImportu) -> bool {
    let mut zavrit = false;
    let (_, _, chyby, podezrele, _) = n.pocty();
    let vlozi = n.plan.pridat.len();
    let prepise = n.plan.prepsat.len();
    ui.horizontal(|ui| {
        let mut t = trf!("{} bodů se vloží", vlozi);
        if prepise > 0 {
            t.push_str(&trf!(" · {} přepíše", prepise));
        }
        if n.plan.preskoceno > 0 {
            t.push_str(&trf!(" · {} duplicit se přeskočí", n.plan.preskoceno));
        }
        if chyby > 0 {
            t.push_str(&trf!(" · {} nečitelných se vynechá", chyby));
        }
        if podezrele > 0 {
            t.push_str(&trf!(" · {} podezřelých (lze vyřadit)", podezrele));
        }
        ui.label(RichText::new(t).color(SEDA));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let pocet = vlozi + prepise;
            let tl = egui::Button::new(RichText::new(if prepise > 0 { trf!("Importovat {} bodů", pocet) } else { trf!("Importovat {} bodů", vlozi) }).color(Color32::WHITE).strong())
                .fill(Color32::from_rgb(44, 95, 143))
                .min_size(egui::vec2(150.0, 26.0));
            if ui.add_enabled(pocet > 0 && n.chyba_formatu.is_none(), tl).clicked() {
                if let Some(nast) = n.nastaveni_importu() {
                    if n.zapamatovat {
                        let posledni = n.sablona(tr("naposledy"));
                        let (politika, priblizit) = (n.politika().klic().to_string(), n.priblizit);
                        nastaveni::zmen(|x| {
                            if posledni.is_some() {
                                x.import_posledni = posledni;
                            }
                            x.import_duplicity = politika;
                            x.import_priblizit = priblizit;
                        });
                    }
                    spravce::posli(Akce::Import(Box::new(nast)));
                    zavrit = true;
                }
            }
            if ui.button(tr("Zrušit")).clicked() {
                n.fronta.clear();
                zavrit = true;
            }
            if !n.fronta.is_empty() && ui.button(tr("Přeskočit soubor")).clicked() {
                zavrit = true;
            }
            let mut ukazovat = nastaveni::akt().nahled_importu;
            if ui.checkbox(&mut ukazovat, tr("Náhled ukazovat i příště")).changed() {
                nastaveni::zmen(|x| x.nahled_importu = ukazovat);
            }
        });
    });
    zavrit
}
