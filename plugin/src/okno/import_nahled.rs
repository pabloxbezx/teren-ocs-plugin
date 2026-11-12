//! Okno importu bodů (podle schváleného náhledu, varianta A):
//! úvodní volba zdroje a náhled souboru – souhrnný pás, záhlaví sloupců
//! s výběrem role, karty počtů jako filtr, tabulka se stavem řádků, mapka,
//! duplicity, skupina a styl, patička se souhrnem.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};
use teren::body::{nacti_body, rozdel_radek, Bod, Desetinna, FormatBodu, Oddelovac, Sloupec, VysledekImportu};
use teren::duplicity::{self, Duplicity, Politika};
use teren::formaty::{self, DruhZdroje};
use teren::kodovani::{self, Kodovani};
use teren::preklad::tr;
use teren::rozpoznani::{self, nazev_desetinne, nazev_oddelovace, podezrela_vyska, s_formatem, v_cr, Navrh, Puvod};
use teren::trf;

use super::Okno;
use crate::import::{CilSkupiny, NastaveniImportu};
use crate::nastaveni::{self, SablonaImportu};
use crate::spravce::{self, Akce, StavSpravce};

// ── barvy a rozměry podle náhledu ─────────────────────────────────────────────

const INK: Color32 = Color32::from_rgb(212, 215, 220);
const MUTED: Color32 = Color32::from_rgb(139, 146, 155);
const FAINT: Color32 = Color32::from_rgb(100, 107, 116);
const LINE: Color32 = Color32::from_rgb(51, 56, 63);
const LINE2: Color32 = Color32::from_rgb(42, 46, 51);
const PANEL: Color32 = Color32::from_rgb(30, 33, 37);
const PANEL2: Color32 = Color32::from_rgb(36, 40, 45);
const CARD: Color32 = Color32::from_rgb(42, 46, 52);
const HOVER: Color32 = Color32::from_rgb(48, 53, 60);
const SEL: Color32 = Color32::from_rgb(29, 44, 60);
const SELLINE: Color32 = Color32::from_rgb(59, 119, 184);
const OK: Color32 = Color32::from_rgb(63, 185, 80);
const WARN: Color32 = Color32::from_rgb(227, 179, 65);
const BAD: Color32 = Color32::from_rgb(229, 83, 75);
const VIOLET: Color32 = Color32::from_rgb(179, 146, 240);
const BOD: Color32 = Color32::from_rgb(240, 163, 58);
const ACCENT: Color32 = Color32::from_rgb(88, 166, 255);
const PRAVY_BG: Color32 = Color32::from_rgb(27, 30, 34);
const MAPA_BG: Color32 = Color32::from_rgb(18, 20, 23);
const PRIMARNI: Color32 = Color32::from_rgb(44, 95, 143);

const SIRKA_PRAVEHO: f32 = 300.0;
const VYSKA_RADKU: f32 = 22.0;

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

fn sirka_sloupce(s: Sloupec) -> f32 {
    match s {
        Sloupec::Popis => 150.0,
        Sloupec::Cislo => 86.0,
        Sloupec::Z => 86.0,
        Sloupec::Preskocit => 86.0,
        _ => 104.0,
    }
}

// ── stav ──────────────────────────────────────────────────────────────────────

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

impl ZVykresu {
    fn novy() -> Self {
        Self { hladina: None, jen_vyber: false, tolerance: 0.0, hladiny: Vec::new(), statistika: Default::default(), ceka: true }
    }
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
    /// Krátká zpráva na úvodní stránce (např. prázdná schránka).
    zprava: Option<String>,
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
    /// Další soubory k importu (přetažené nebo vybrané najednou).
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
            zprava: None,
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

    /// Více souborů najednou: první se načte, další čekají ve frontě.
    fn soubory(cesty: Vec<String>) -> Option<Self> {
        let (prvni, dalsi) = cesty.split_first()?;
        let mut n = Self::novy(prvni);
        n.fronta = dalsi.to_vec();
        n.poradi = (1, cesty.len());
        Some(n)
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
        let mut stav = self.z_vykresu.take().unwrap_or_else(ZVykresu::novy);
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

    fn znovu_rozpoznat(&mut self) {
        self.rucne.kodovani = false;
        if self.bajty.is_empty() {
            self.rozpoznej_vse();
        } else {
            self.dekoduj();
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

    fn podezrely(&self, i: usize) -> Option<&str> {
        self.podezrele.get(i).and_then(|p| p.as_deref())
    }

    fn je_duplicita(&self, i: usize) -> bool {
        self.dup.je_v_souboru(i) || self.dup.ve_vykresu_pro(i).is_some()
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
            Politika::Preskocit => tr(" – přeskočí se"),
            Politika::Prepsat => tr(" – přepíše"),
            Politika::Precislovat(_) => tr(" – přečísluje se"),
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
            (Filtr::Podezrele, StavRadku::Bod(i)) => self.podezrely(*i).is_some(),
            (Filtr::Duplicity, StavRadku::Bod(i)) => self.je_duplicita(*i),
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

// ── výběr souboru (dialog systému ve vlastním vlákně) ─────────────────────────

fn vybrane_soubory() -> &'static Mutex<Option<Vec<String>>> {
    static V: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();
    V.get_or_init(|| Mutex::new(None))
}

#[derive(Clone, Copy)]
enum Dialog {
    Text,
    Excel,
    LandXml,
    Pristroj,
    Vice,
}

fn otevri_dialog(d: Dialog, ctx: egui::Context) {
    std::thread::spawn(move || {
        let (nazev, pripony): (&str, &[&str]) = match d {
            Dialog::Text | Dialog::Vice => (tr("Body (TXT, CSV, XYZ, XLSX, XML, GSI)"), &["txt", "csv", "xyz", "xlsx", "xls", "ods", "xml", "jxl", "gsi"]),
            Dialog::Excel => ("Excel", &["xlsx", "xlsm", "xls", "ods"]),
            Dialog::LandXml => ("LandXML", &["xml"]),
            Dialog::Pristroj => (tr("Totální stanice / GNSS"), &["gsi", "jxl", "xml", "csv", "txt"]),
        };
        let mut f = rfd::FileDialog::new().set_title(tr("Import bodů")).add_filter(nazev, pripony).add_filter(tr("Všechny soubory"), &["*"]);
        if let Some(c) = nastaveni::akt().posledni_soubory.first().and_then(|c| std::path::Path::new(c).parent().map(PathBuf::from)) {
            f = f.set_directory(c);
        }
        let vysledek: Option<Vec<PathBuf>> = match d {
            Dialog::Vice => f.pick_files(),
            _ => f.pick_file().map(|p| vec![p]),
        };
        if let Some(v) = vysledek.filter(|v| !v.is_empty()) {
            *vybrane_soubory().lock().unwrap_or_else(|e| e.into_inner()) = Some(v.into_iter().map(|p| p.display().to_string()).collect());
            ctx.request_repaint();
        }
    });
}

fn schranka() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok().filter(|t| !t.trim().is_empty())
}

// ── kreslení: drobné prvky ────────────────────────────────────────────────────

fn ikona(ui: &mut egui::Ui, zdroj: egui::ImageSource<'static>, velikost: f32) {
    ui.add(egui::Image::new(zdroj).fit_to_exact_size(Vec2::splat(velikost)));
}

macro_rules! obr {
    ($n:literal) => {
        egui::include_image!(concat!("../../assets/", $n, ".svg"))
    };
}

fn nadpis(ui: &mut egui::Ui, t: &str) {
    ui.label(RichText::new(t.to_uppercase()).size(11.0).strong().color(MUTED));
}

fn tlacitko(ui: &mut egui::Ui, t: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(t).size(12.5).color(INK)).fill(CARD).stroke(Stroke::new(1.0, LINE)).corner_radius(6.0))
}

fn box_ram(ui: &mut egui::Ui, obsah: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(PANEL2).stroke(Stroke::new(1.0, LINE2)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        obsah(ui);
    });
}

fn tecka(ui: &egui::Ui, stred: Pos2, barva: Color32) {
    ui.painter().circle_filled(stred, 4.5, barva);
}

/// Combo bez rámečku (uvnitř čipu): tučný text a šipka.
fn prusvitne(ui: &mut egui::Ui) {
    let w = &mut ui.visuals_mut().widgets;
    for s in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.weak_bg_fill = Color32::TRANSPARENT;
        s.bg_fill = Color32::TRANSPARENT;
        s.bg_stroke = Stroke::NONE;
    }
}

/// Čip souhrnného pásu: popisek, obsah a štítek auto / ručně.
fn cip(ui: &mut egui::Ui, popisek: &str, rucne: Option<bool>, obsah: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(PANEL2).stroke(Stroke::new(1.0, LINE2)).corner_radius(16.0).inner_margin(egui::Margin { left: 10, right: 5, top: 2, bottom: 2 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.label(RichText::new(popisek).size(12.5).color(MUTED));
            prusvitne(ui);
            obsah(ui);
            if let Some(r) = rucne {
                let (t, bg, fg) = if r {
                    (tr("ručně"), Color32::from_rgb(58, 50, 24), Color32::from_rgb(240, 207, 106))
                } else {
                    (tr("auto"), Color32::from_rgb(29, 58, 36), Color32::from_rgb(126, 224, 154))
                };
                egui::Frame::new().fill(bg).corner_radius(8.0).inner_margin(egui::Margin::symmetric(5, 0)).show(ui, |ui| {
                    ui.label(RichText::new(t).size(10.0).color(fg));
                });
            }
        });
    });
}

/// Pruh s upozorněním a tlačítkem. Vrací `true` po kliknutí.
fn pruh(ui: &mut egui::Ui, varovani: bool, barva: Color32, text: &str, tlacitko_text: Option<&str>) -> bool {
    let (bg, okraj) = if varovani { (Color32::from_rgb(51, 41, 15), Color32::from_rgb(92, 78, 34)) } else { (Color32::from_rgb(20, 40, 61), Color32::from_rgb(46, 77, 112)) };
    let mut klik = false;
    egui::Frame::new().fill(bg).stroke(Stroke::new(1.0, okraj)).corner_radius(7.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            tecka(ui, r.center(), barva);
            ui.label(RichText::new(text).size(12.5).color(INK));
            if let Some(t) = tlacitko_text {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| klik = tlacitko(ui, t).clicked());
            }
        });
    });
    ui.add_space(6.0);
    klik
}

fn carkovany_ramecek(ui: &egui::Ui, r: Rect, barva: Color32) {
    let s = Stroke::new(1.5, barva);
    let p = ui.painter();
    for (a, b) in [(r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.right_bottom(), r.left_bottom()), (r.left_bottom(), r.left_top())] {
        p.extend(egui::Shape::dashed_line(&[a, b], s, 6.0, 4.0));
    }
}

/// „dnes“, „včera“ nebo „d. m.“ podle data změny souboru.
fn kdy(cesta: &str) -> String {
    let Ok(t) = std::fs::metadata(cesta).and_then(|m| m.modified()) else { return String::new() };
    let teď = std::time::SystemTime::now();
    let dny = |t: std::time::SystemTime| t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400) as i64;
    match dny(teď) - dny(t) {
        0 => tr("dnes").into(),
        1 => tr("včera").into(),
        _ => {
            // kalendářní datum ze dnů od 1970 (H. Hinnant)
            let z = dny(t) + 719_468;
            let doe = z.rem_euclid(146_097);
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            format!("{d}. {m}.")
        }
    }
}

// ── okno ──────────────────────────────────────────────────────────────────────

impl Okno {
    pub(super) fn okno_importu(&mut self, ctx: &egui::Context, stav: &StavSpravce) {
        kresli(ctx, &mut self.nahled_importu, stav);
    }
}

/// Vstupy okna (přetažení, schránka, dialog, výkres) a kreslení.
fn kresli(ctx: &egui::Context, slot: &mut Option<NahledImportu>, stav: &StavSpravce) {
    let Some(n) = slot.as_mut() else { return };
    let pretazene: Vec<String> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().display().to_string()).filter(|p| !p.is_empty()).collect());
    if let Some(nove) = NahledImportu::soubory(pretazene) {
        *n = nove;
    }
    if let Some(v) = vybrane_soubory().lock().unwrap_or_else(|e| e.into_inner()).take() {
        if let Some(nove) = NahledImportu::soubory(v) {
            *n = nove;
        }
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
    let ram = egui::Frame::window(&ctx.global_style()).fill(PANEL).stroke(Stroke::new(1.0, Color32::from_rgb(68, 74, 82))).inner_margin(0.0);
    let obrazovka = ctx.content_rect();
    egui::Window::new(titulek)
        .id(egui::Id::new("teren_import"))
        .frame(ram)
        .open(&mut otevreno)
        .pivot(Align2::CENTER_CENTER)
        .default_pos(obrazovka.center())
        .default_size([1120.0_f32.min(obrazovka.width() - 24.0), 700.0_f32.min(obrazovka.height() - 24.0)])
        .min_size([760.0, 480.0])
        .max_width(obrazovka.width() - 16.0)
        .max_height(obrazovka.height() - 16.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.visuals_mut().override_text_color = Some(INK);
            if !n.ma_zdroj() {
                egui::Frame::new().inner_margin(14.0).show(ui, |ui| volba_zdroje(ui, n, stav));
                return;
            }
            hotovo = nahled(ui, n, stav);
        });
    if hotovo {
        let mut fronta = std::mem::take(&mut n.fronta);
        if fronta.is_empty() {
            *slot = None;
        } else {
            let c = fronta.remove(0);
            let poradi = (n.poradi.0 + 1, n.poradi.1);
            *n = NahledImportu::novy(&c);
            n.fronta = fronta;
            n.poradi = poradi;
        }
    } else if !otevreno {
        *slot = None;
    }
}

// ── úvodní volba zdroje ───────────────────────────────────────────────────────

fn karta_zdroje(ui: &mut egui::Ui, sirka: f32, obr: egui::ImageSource<'static>, titul: &str, popis: &str) -> bool {
    let (r, odezva) = ui.allocate_exact_size(Vec2::new(sirka, 66.0), Sense::click());
    let nad = odezva.hovered();
    ui.painter().rect_filled(r, 9.0, if nad { Color32::from_rgb(38, 43, 49) } else { PANEL2 });
    ui.painter().rect_stroke(r, 9.0, Stroke::new(1.0, if nad { SELLINE } else { LINE2 }), StrokeKind::Inside);
    ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(12.0, 11.0))), |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ikona(ui, obr, 28.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(titul).size(13.5).strong().color(INK));
                ui.add(egui::Label::new(RichText::new(popis).size(12.0).color(MUTED)).wrap());
            });
        });
    });
    odezva.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn volba_zdroje(ui: &mut egui::Ui, n: &mut NahledImportu, stav: &StavSpravce) {
    let ctx = ui.ctx().clone();
    let sirka = ui.available_width();
    let pravy = 260.0;
    let levy = (sirka - pravy - 14.0).max(300.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        ui.allocate_ui_with_layout(Vec2::new(levy, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(levy);
            // zóna pro přetažení
            let (r, odezva) = ui.allocate_exact_size(Vec2::new(levy, 78.0), Sense::click());
            let nad = odezva.hovered() || ctx.input(|i| !i.raw.hovered_files.is_empty());
            carkovany_ramecek(ui, r, if nad { SELLINE } else { Color32::from_rgb(74, 82, 92) });
            ui.painter().text(r.center() - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, tr("Přetáhněte sem soubor (nebo více souborů)"), FontId::proportional(14.5), INK);
            ui.painter().text(
                r.center() + Vec2::new(0.0, 11.0),
                Align2::CENTER_CENTER,
                tr("TXT, CSV, XYZ, XLSX, XML, GSI · nebo stiskněte Ctrl+V pro vložení tabulky ze schránky"),
                FontId::proportional(12.5),
                MUTED,
            );
            if odezva.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                otevri_dialog(Dialog::Text, ctx.clone());
            }
            ui.add_space(12.0);
            // karty zdrojů v mřížce
            let mezera = 10.0;
            let sloupcu = (((levy + mezera) / (210.0 + mezera)).floor() as usize).clamp(1, 4);
            let w = (levy - mezera * (sloupcu as f32 - 1.0)) / sloupcu as f32;
            let karty: [(egui::ImageSource<'static>, &str, &str, u8); 7] = [
                (obr!("ter_body_import"), tr("Textový soubor…"), tr("TXT, CSV, XYZ – libovolné pořadí sloupců, rozpozná se samo"), 0),
                (obr!("ter_zdroj_schranka"), tr("Schránka"), tr("tabulka zkopírovaná z Excelu, e-mailu nebo webu"), 1),
                (obr!("ter_zdroj_excel"), tr("Excel (.xlsx)…"), tr("výběr listu, sloupce stejně jako u textu"), 2),
                (obr!("ter_bod_cogo"), tr("Z výkresu"), tr("POINT, bloky s atributy, texty výšek u křížku – převod na body Terénu"), 3),
                (obr!("ter_zdroj_xml"), tr("LandXML…"), tr("body CgPoints"), 4),
                (obr!("ter_zdroj_gnss"), tr("Totální stanice / GNSS…"), tr("Leica GSI, Trimble JobXML, CSV přijímačů"), 5),
                (obr!("ter_skupina_bodu"), tr("Více souborů…"), tr("každý soubor do vlastní skupiny"), 6),
            ];
            let mut klik = None;
            for radek in karty.chunks(sloupcu) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = mezera;
                    for (obr, titul, popis, id) in radek {
                        if karta_zdroje(ui, w, obr.clone(), titul, popis) {
                            klik = Some(*id);
                        }
                    }
                });
                ui.add_space(mezera - 5.0);
            }
            match klik {
                Some(0) => otevri_dialog(Dialog::Text, ctx.clone()),
                Some(1) => match schranka() {
                    Some(t) => n.ze_schranky(t, &stav.zdroje),
                    None => n.zprava = Some(tr("Schránka je prázdná – zkopírujte tabulku (např. v Excelu Ctrl+C) a zkuste to znovu.").into()),
                },
                Some(2) => otevri_dialog(Dialog::Excel, ctx.clone()),
                Some(3) => {
                    n.z_vykresu = Some(ZVykresu::novy());
                    n.soubor = tr("výkres").into();
                    spravce::posli(Akce::BodyZVykresu(None, false, 0.0));
                }
                Some(4) => otevri_dialog(Dialog::LandXml, ctx.clone()),
                Some(5) => otevri_dialog(Dialog::Pristroj, ctx.clone()),
                Some(6) => otevri_dialog(Dialog::Vice, ctx.clone()),
                _ => {}
            }
            if let Some(z) = &n.zprava {
                ui.add_space(4.0);
                ui.label(RichText::new(z).size(12.5).color(WARN));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("nebo cesta k souboru")).size(12.5).color(MUTED));
                let r = ui.add(egui::TextEdit::singleline(&mut n.cesta_edit).hint_text(r"C:\…\body.txt").desired_width((ui.available_width() - 70.0).max(120.0)));
                if tlacitko(ui, tr("Načíst")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    let c = n.cesta_edit.trim().trim_matches('"').to_string();
                    if !c.is_empty() {
                        n.nacti_soubor(&c);
                    }
                }
            });
        });
        ui.allocate_ui_with_layout(Vec2::new(pravy, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(pravy);
            let nast = nastaveni::akt();
            nadpis(ui, tr("Poslední soubory"));
            box_ram(ui, |ui| {
                if nast.posledni_soubory.is_empty() {
                    ui.label(RichText::new(tr("zatím žádné")).size(12.5).color(MUTED));
                }
                for c in &nast.posledni_soubory {
                    let r = ui
                        .horizontal(|ui| {
                            ikona(ui, obr!("ter_body_import"), 18.0);
                            ui.label(RichText::new(crate::import::jmeno_souboru(c)).size(12.5));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(kdy(c)).size(12.0).color(MUTED)));
                        })
                        .response
                        .interact(Sense::click())
                        .on_hover_text(c)
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() {
                        n.nacti_soubor(c);
                    }
                }
            });
            ui.add_space(12.0);
            nadpis(ui, tr("Šablony formátu"));
            box_ram(ui, |ui| {
                if nast.import_sablony.is_empty() {
                    ui.label(RichText::new(tr("zatím žádné")).size(12.5).color(MUTED));
                }
                for s in &nast.import_sablony {
                    ui.horizontal(|ui| {
                        ikona(ui, obr!("ter_sablona"), 18.0);
                        ui.label(RichText::new(format!("{} – {} {}", s.jmeno, s.vzor, nazev_oddelovace(s.oddelovac))).size(12.5));
                    });
                }
            });
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    RichText::new(tr("Šablona = uložené přiřazení sloupců, oddělovač, kódování, desetinná čárka a záhlaví. Uložíte ji v náhledu souboru."))
                        .size(12.0)
                        .color(MUTED),
                )
                .wrap(),
            );
        });
    });
}

// ── náhled souboru (varianta A) ───────────────────────────────────────────────

/// Celý náhled. Vrací `true`, když se má okno zavřít (import nebo zrušení).
fn nahled(ui: &mut egui::Ui, n: &mut NahledImportu, stav: &StavSpravce) -> bool {
    let celkem = ui.available_size();
    let vyska_paticky = 48.0;
    let vyska = (celkem.y - vyska_paticky).max(300.0);
    let levy = (celkem.x - SIRKA_PRAVEHO).max(420.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        // levá část
        ui.allocate_ui_with_layout(Vec2::new(levy, vyska), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_size(Vec2::new(levy, vyska));
            egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 8 }).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                radek_zdroje(ui, n);
                if let Some(e) = &n.chyba {
                    ui.add_space(8.0);
                    ui.label(RichText::new(e).color(BAD));
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
                let zbyva = ui.available_height() - 34.0;
                tabulka(ui, n, zbyva.max(140.0));
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(tr("Nad každým sloupcem vyberte, co obsahuje; pod ním je původní název ze záhlaví. Najetím na číslo řádku se ukáže původní text."))
                            .size(11.5)
                            .color(MUTED),
                    )
                    .wrap(),
                );
            });
        });
        // svislá čára a pravý panel
        let (cara, _) = ui.allocate_exact_size(Vec2::new(1.0, vyska), Sense::hover());
        ui.painter().rect_filled(cara, 0.0, LINE);
        ui.allocate_ui_with_layout(Vec2::new(SIRKA_PRAVEHO - 1.0, vyska), egui::Layout::top_down(egui::Align::Min), |ui| {
            let r = ui.max_rect();
            ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(SIRKA_PRAVEHO - 1.0, vyska)), 0.0, PRAVY_BG);
            ui.set_min_size(Vec2::new(SIRKA_PRAVEHO - 1.0, vyska));
            egui::ScrollArea::vertical().id_salt("imp_pravy").max_height(vyska).auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(12.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                    pravy_panel(ui, n, stav);
                });
            });
        });
    });
    paticka(ui, n)
}

fn radek_zdroje(ui: &mut egui::Ui, n: &mut NahledImportu) {
    ui.horizontal(|ui| {
        ikona(ui, obr!("ter_body_import"), 18.0);
        ui.label(RichText::new(&n.soubor).size(14.0).strong());
        let mut info = Vec::new();
        if !n.cesta.is_empty() {
            if let Some(slozka) = std::path::Path::new(&n.cesta).parent() {
                info.push(slozka.display().to_string());
            }
            info.push(format!("{:.1} kB", n.bajty.len() as f64 / 1024.0).replace('.', ","));
        }
        if let Some(d) = n.druh.filter(|d| *d != DruhZdroje::Text) {
            info.push(d.nazev().to_string());
        }
        info.push(trf!("{} řádků", n.text.lines().count()));
        if n.poradi.1 > 1 {
            info.push(trf!("soubor {} z {}", n.poradi.0, n.poradi.1));
        }
        ui.add(egui::Label::new(RichText::new(info.join(" · ")).size(12.5).color(MUTED)).truncate()).on_hover_text(&n.cesta);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(jmeno) = &mut n.nova_sablona {
                let mut ulozit = false;
                if tlacitko(ui, tr("Zrušit")).clicked() {
                    n.nova_sablona = None;
                    return;
                }
                if tlacitko(ui, tr("Uložit")).clicked() {
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
            if tlacitko(ui, tr("Uložit šablonu…")).on_hover_text(tr("Uložit sloupce, oddělovač, kódování a desetinnou čárku jako šablonu")).clicked() {
                n.nova_sablona = Some(String::new());
            }
            let nast = nastaveni::akt();
            let mut volba: Option<Option<SablonaImportu>> = None;
            egui::ComboBox::from_id_salt("imp_sablona").selected_text(RichText::new(tr("Šablona: rozpoznáno automaticky")).size(12.0)).width(200.0).show_ui(ui, |ui| {
                if ui.selectable_label(false, tr("Rozpoznat automaticky")).clicked() {
                    volba = Some(None);
                }
                if let Some(p) = &nast.import_posledni {
                    if ui.selectable_label(false, trf!("Naposledy použité ({})", p.vzor)).clicked() {
                        volba = Some(Some(p.clone()));
                    }
                }
                for s in &nast.import_sablony {
                    if ui.selectable_label(false, format!("{} – {}", s.jmeno, s.vzor)).clicked() {
                        volba = Some(Some(s.clone()));
                    }
                }
            });
            match volba {
                Some(Some(s)) => n.pouzij_sablonu(&s),
                Some(None) => n.znovu_rozpoznat(),
                None => {}
            }
            if tlacitko(ui, tr("Jiný zdroj…")).clicked() {
                let fronta = std::mem::take(&mut n.fronta);
                *n = NahledImportu::prazdny();
                n.fronta = fronta;
            }
        });
    });
    ui.add_space(4.0);
}

fn souhrnny_pas(ui: &mut egui::Ui, n: &mut NahledImportu) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        let vzor = FormatBodu::ze_sloupcu(n.sloupce.clone(), n.oddelovac).map(|f| f.vzor()).unwrap_or_else(|_| "?".into());
        cip(ui, tr("Formát"), Some(n.rucne.sloupce), |ui| {
            egui::ComboBox::from_id_salt("imp_vzor").selected_text(RichText::new(&vzor).strong()).width(64.0).show_ui(ui, |ui| {
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
            egui::ComboBox::from_id_salt("imp_odd").selected_text(RichText::new(nazev_oddelovace(n.oddelovac)).strong()).width(78.0).show_ui(ui, |ui| {
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
                egui::ComboBox::from_id_salt("imp_kod").selected_text(RichText::new(k.nazev()).strong()).width(104.0).show_ui(ui, |ui| {
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
            egui::ComboBox::from_id_salt("imp_des").selected_text(RichText::new(text).strong()).width(56.0).show_ui(ui, |ui| {
                for d in [Desetinna::Auto, Desetinna::Tecka, Desetinna::Carka] {
                    if ui.selectable_value(&mut n.desetinna, d, nazev_desetinne(d)).clicked() {
                        n.rucne.desetinna = d != Desetinna::Auto;
                        n.zmeneno = true;
                    }
                }
            });
        });
        cip(ui, tr("Záhlaví"), Some(n.rucne.hlavicka), |ui| {
            let text = if n.hlavicka == 0 { tr("žádné").to_string() } else if n.hlavicka == 1 { tr("řádek 1").to_string() } else { trf!("řádky 1–{}", n.hlavicka) };
            egui::ComboBox::from_id_salt("imp_hl").selected_text(RichText::new(text).strong()).width(70.0).show_ui(ui, |ui| {
                for h in 0..=5usize {
                    let t = if h == 0 { tr("žádné").to_string() } else if h == 1 { tr("řádek 1").to_string() } else { trf!("řádky 1–{}", h) };
                    if ui.selectable_value(&mut n.hlavicka, h, t).clicked() {
                        n.rucne.hlavicka = true;
                        n.zmeneno = true;
                    }
                }
            });
        });
        cip(ui, tr("Souřadnice"), None, |ui| {
            ui.label(RichText::new(if n.je_sjtsk() { tr("S-JTSK kladné – výkres -Y/-X") } else { tr("výkres E/N (beze změny)") }).strong());
        });
    });
    ui.label(RichText::new(&n.popis_rozpoznani).size(11.5).color(if n.varovani { WARN } else { MUTED }));
    ui.add_space(2.0);
}

fn pruhy(ui: &mut egui::Ui, n: &mut NahledImportu, stav: &StavSpravce) {
    if let Some(e) = n.chyba_formatu.clone() {
        pruh(ui, true, BAD, &trf!("Sloupce nedávají platný formát: {}", e), None);
    }
    if let Some((navrh, sloupce)) = n.navrh.clone() {
        let (text, tl) = match navrh {
            Navrh::Prohodit => (tr("Y a X jsou nejspíš prohozené – po prohození leží body v S-JTSK."), tr("Prohodit Y a X")),
            Navrh::Znamenko => (tr("Souřadnice mají opačné znaménko než sloupce – po převodu leží body v S-JTSK."), tr("Převést")),
            Navrh::ProhoditZnamenko => (tr("Souřadnice jsou prohozené a mají opačné znaménko – po opravě leží body v S-JTSK."), tr("Opravit")),
        };
        if pruh(ui, true, WARN, text, Some(tl)) {
            n.sloupce = sloupce;
            n.rucne.sloupce = true;
            n.zmeneno = true;
        }
    }
    if let Some(k) = n.kodovani {
        if !n.rucne.kodovani && kodovani::podezrele_znaky(&n.text) > 0 {
            let jine = if k == Kodovani::Cp1250 { Kodovani::Cp852 } else { Kodovani::Cp1250 };
            if pruh(ui, false, MUTED, &trf!("V kódech jsou nečitelné znaky – soubor je možná v kódování {}.", jine.nazev()), Some(&trf!("Použít {}", jine.nazev()))) {
                n.kodovani = Some(jine);
                n.rucne.kodovani = true;
                n.dekoduj();
            }
        }
    }
    let (_, body, chyby, _, _) = n.pocty();
    if chyby > 0 && chyby > body && pruh(ui, true, BAD, tr("Většina řádků nejde přečíst – zkontrolujte oddělovač a desetinnou čárku."), Some(tr("Vrátit rozpoznané"))) {
        n.znovu_rozpoznat();
    }
    let uz = stav.body.iter().filter(|b| !n.soubor.is_empty() && b.zdroj.eq_ignore_ascii_case(&n.soubor)).count();
    if uz > 0 && n.politika != 1 && pruh(ui, false, ACCENT, &trf!("Soubor už byl importován ({} bodů ve výkrese) – body se stejným číslem jde aktualizovat.", uz), Some(tr("Aktualizovat podle čísla"))) {
        n.politika = 1;
        n.prepocti_plan();
    }
    if let Some(z) = &mut n.z_vykresu {
        let st = &z.statistika;
        let mut znovu = false;
        egui::Frame::new().fill(PANEL2).stroke(Stroke::new(1.0, LINE2)).corner_radius(7.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(trf!(
                    "Značek {} · výška z textu {} · z atributu {} · ze značky {} · bez výšky {}",
                    st.znacek, st.vyska_z_textu, st.vyska_z_atributu, st.vyska_ze_znacky, st.bez_vysky
                )).size(12.5));
                ui.label(RichText::new(tr("Hladina")).size(12.5).color(MUTED));
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
                ui.label(RichText::new(tr("Tolerance textu")).size(12.5).color(MUTED));
                if ui.add(egui::DragValue::new(&mut z.tolerance).range(0.0..=100.0).speed(0.1).suffix(" m")).on_hover_text(tr("0 = trojnásobek výšky textu")).changed() {
                    znovu = true;
                }
            });
        });
        ui.add_space(6.0);
        if znovu {
            z.ceka = true;
            spravce::posli(Akce::BodyZVykresu(z.hladina.clone(), z.jen_vyber, z.tolerance));
        }
    }
}

fn karty_poctu(ui: &mut egui::Ui, n: &mut NahledImportu) {
    let (radku, body, chyby, podezrele, dup) = n.pocty();
    let dup_popis = trf!("duplicitní čísla ({} v souboru, {} ve výkresu)", n.dup.v_souboru.len(), n.dup.ve_vykresu.len());
    let karty = [
        (Filtr::Vse, radku, tr("řádků s daty").to_string(), INK),
        (Filtr::Nacte, body, tr("načte se").to_string(), OK),
        (Filtr::Chyby, chyby, tr("nečitelné").to_string(), BAD),
        (Filtr::Podezrele, podezrele, tr("podezřelé").to_string(), WARN),
        (Filtr::Duplicity, dup, dup_popis, VIOLET),
    ];
    let mezera = 8.0;
    let w = ((ui.available_width() - mezera * 4.0) / 5.0).max(80.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = mezera;
        for (f, pocet, popis, barva) in karty {
            let (r, odezva) = ui.allocate_exact_size(Vec2::new(w, 50.0), Sense::click());
            let vybrano = n.filtr == f;
            let p = ui.painter();
            p.rect_filled(r, 8.0, if vybrano { SEL } else if odezva.hovered() { CARD } else { PANEL2 });
            p.rect_stroke(r, 8.0, Stroke::new(1.0, if vybrano { SELLINE } else { LINE2 }), StrokeKind::Inside);
            p.text(r.left_top() + Vec2::new(10.0, 6.0), Align2::LEFT_TOP, pocet.to_string(), FontId::proportional(19.0), barva);
            p.with_clip_rect(r.shrink(2.0)).text(r.left_top() + Vec2::new(10.0, 31.0), Align2::LEFT_TOP, popis, FontId::proportional(11.5), MUTED);
            if odezva.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr("klikněte pro filtr tabulky")).clicked() {
                n.filtr = if vybrano { Filtr::Vse } else { f };
            }
        }
    });
    ui.add_space(4.0);
}

fn tabulka(ui: &mut egui::Ui, n: &mut NahledImportu, vyska: f32) {
    let w_cislo = 38.0;
    let w_stav = 22.0;
    let w_duvod = 240.0;
    let sirky: Vec<f32> = (0..n.ncol).map(|j| sirka_sloupce(n.sloupce.get(j).copied().unwrap_or(Sloupec::Preskocit))).collect();
    let sirka_dat: f32 = w_cislo + w_stav + sirky.iter().sum::<f32>();
    let viditelne: Vec<usize> = (0..n.radky.len()).filter(|&i| n.viditelny(&n.radky[i])).collect();
    let mut prepnout: Option<usize> = None;
    let sirka_okna = ui.available_width();
    let celkova = (sirka_dat + w_duvod).max(sirka_okna - 2.0);
    let (ram, _) = ui.allocate_exact_size(Vec2::new(sirka_okna, vyska), Sense::hover());
    ui.painter().rect_stroke(ram, 8.0, Stroke::new(1.0, LINE2), StrokeKind::Inside);
    ui.scope_builder(egui::UiBuilder::new().max_rect(ram.shrink(1.0)), |ui| {
        egui::ScrollArea::horizontal().id_salt("imp_tab_h").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(celkova);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            // záhlaví s výběrem role sloupce
            let (hr, _) = ui.allocate_exact_size(Vec2::new(celkova, 46.0), Sense::hover());
            ui.painter().rect_filled(hr, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, Color32::from_rgb(35, 39, 44));
            ui.painter().hline(hr.x_range(), hr.bottom(), Stroke::new(1.0, LINE));
            let mut x = hr.left() + w_cislo + w_stav;
            for (j, &w) in sirky.iter().enumerate() {
                let bunka = Rect::from_min_size(Pos2::new(x + 3.0, hr.top() + 4.0), Vec2::new(w - 6.0, 40.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(bunka), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let akt = n.sloupce[j];
                    let text = RichText::new(nazev_role(akt)).size(12.5).strong().color(if akt == Sloupec::Preskocit { FAINT } else { INK });
                    egui::ComboBox::from_id_salt(("imp_role", j)).selected_text(text).width(w - 8.0).show_ui(ui, |ui| {
                        for r in ROLE {
                            if ui.selectable_label(akt == r, nazev_role(r)).clicked() && akt != r {
                                // roli má jen jeden sloupec – původní držitel se ignoruje
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
                    ui.add(egui::Label::new(RichText::new(puvodni).size(10.5).color(FAINT)).truncate());
                });
                x += w;
            }
            ui.painter().text(Pos2::new(x + 8.0, hr.bottom() - 8.0), Align2::LEFT_BOTTOM, tr("Stav"), FontId::proportional(12.5), MUTED);
            // řádky (kreslí se jen viditelné)
            egui::ScrollArea::vertical()
                .id_salt("imp_tab_v")
                .max_height(vyska - 48.0)
                .auto_shrink([false, false])
                .show_rows(ui, VYSKA_RADKU, viditelne.len(), |ui, rozsah| {
                    for &ri in &viditelne[rozsah] {
                        let r = &n.radky[ri];
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(celkova, VYSKA_RADKU), Sense::hover());
                        let vyrazen = n.vynechat.contains(&r.cislo);
                        let (pozadi, barva_tecky, duvod, spatny, bod) = match &r.stav {
                            StavRadku::Hlavicka => (Color32::TRANSPARENT, Color32::from_gray(85), tr("záhlaví – nenačítá se").to_string(), None, None),
                            StavRadku::Chyba(s, d) => (Color32::from_rgb(42, 26, 26), BAD, d.clone(), *s, None),
                            StavRadku::Bod(i) => {
                                let dup = n.duplicita(*i);
                                let pod = n.podezrely(*i).map(str::to_string);
                                let (bg, t) = if dup.is_some() {
                                    (Color32::from_rgb(37, 31, 51), VIOLET)
                                } else if pod.is_some() {
                                    (Color32::from_rgb(41, 36, 15), WARN)
                                } else {
                                    (Color32::TRANSPARENT, OK)
                                };
                                (bg, t, [pod, dup].into_iter().flatten().collect::<Vec<_>>().join(" · "), None, Some(*i))
                            }
                        };
                        let p = ui.painter().clone();
                        p.rect_filled(rect, 0.0, pozadi);
                        p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, Color32::from_rgb(36, 40, 45)));
                        let stred_y = rect.center().y;
                        let font = FontId::proportional(12.5);
                        // číslo řádku (vpravo zarovnané) s původním textem v bublině
                        let r_cislo = Rect::from_min_size(rect.min, Vec2::new(w_cislo, VYSKA_RADKU));
                        p.text(Pos2::new(r_cislo.right() - 6.0, stred_y), Align2::RIGHT_CENTER, r.cislo.to_string(), FontId::proportional(11.5), FAINT);
                        ui.interact(r_cislo, egui::Id::new(("imp_rn", r.cislo)), Sense::hover()).on_hover_text(n.text.lines().nth(r.cislo - 1).unwrap_or(""));
                        p.circle_filled(Pos2::new(rect.left() + w_cislo + w_stav / 2.0, stred_y), 4.5, barva_tecky);
                        let mut x = rect.left() + w_cislo + w_stav;
                        for (j, &w) in sirky.iter().enumerate() {
                            let bunka = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, VYSKA_RADKU));
                            let h = r.pole.get(j).map(String::as_str).unwrap_or("");
                            let ignor = n.sloupce.get(j) == Some(&Sloupec::Preskocit);
                            let mut barva = if r.stav == StavRadku::Hlavicka || ignor { FAINT } else { INK };
                            if spatny == Some(j) {
                                barva = BAD;
                            }
                            if vyrazen {
                                barva = FAINT;
                            }
                            let text = if h.is_empty() && spatny == Some(j) { "—" } else { h };
                            let tr_ = p.with_clip_rect(bunka.shrink2(Vec2::new(4.0, 0.0))).text(Pos2::new(x + 6.0, stred_y), Align2::LEFT_CENTER, text, font.clone(), barva);
                            if spatny == Some(j) {
                                p.hline(bunka.x_range().shrink(3.0), rect.bottom() - 2.0, Stroke::new(2.0, BAD));
                            }
                            if vyrazen {
                                p.hline(tr_.x_range(), stred_y, Stroke::new(1.0, FAINT));
                            }
                            x += w;
                        }
                        // stav a důvod
                        let mut xd = x + 8.0;
                        if let Some(i) = bod {
                            if n.podezrely(i).is_some() || vyrazen {
                                let cb = Rect::from_min_size(Pos2::new(xd, rect.top() + 1.0), Vec2::new(80.0, VYSKA_RADKU - 2.0));
                                let mut v = vyrazen;
                                if ui.put(cb, egui::Checkbox::new(&mut v, RichText::new(tr("vyřadit")).size(11.5))).changed() {
                                    prepnout = Some(r.cislo);
                                }
                                xd += 84.0;
                            }
                        }
                        let (t, b) = if duvod.is_empty() { (tr("v pořádku").to_string(), OK) } else { (duvod, MUTED) };
                        p.with_clip_rect(Rect::from_min_max(Pos2::new(xd, rect.top()), rect.right_bottom())).text(Pos2::new(xd, stred_y), Align2::LEFT_CENTER, t, FontId::proportional(11.5), b);
                    }
                    if viditelne.is_empty() {
                        ui.add_space(10.0);
                        ui.label(RichText::new(tr("Žádné řádky – zkuste jiný filtr.")).color(MUTED));
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

fn mapka(ui: &mut egui::Ui, n: &NahledImportu) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 220.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 8.0, MAPA_BG);
    p.rect_stroke(rect, 8.0, Stroke::new(1.0, LINE2), StrokeKind::Inside);
    let body: Vec<(usize, &Bod)> = n.vysledek.body.iter().enumerate().filter(|(i, _)| !n.odlehle.contains(i)).collect();
    if body.is_empty() {
        p.text(rect.center(), Align2::CENTER_CENTER, tr("žádné body"), FontId::proportional(12.0), MUTED);
        return;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (_, b) in &body {
        x0 = x0.min(b.x);
        x1 = x1.max(b.x);
        y0 = y0.min(b.y);
        y1 = y1.max(b.y);
    }
    let okraj = ((x1 - x0).max(y1 - y0) * 0.5).max(20.0);
    let okoli: Vec<&Bod> = n.existujici.iter().filter(|b| b.x > x0 - okraj && b.x < x1 + okraj && b.y > y0 - okraj && b.y < y1 + okraj).take(5000).collect();
    for b in &okoli {
        x0 = x0.min(b.x);
        x1 = x1.max(b.x);
        y0 = y0.min(b.y);
        y1 = y1.max(b.y);
    }
    let pad = 16.0;
    let s = ((rect.width() as f64 - 2.0 * pad) / (x1 - x0).max(1e-3)).min((rect.height() as f64 - 2.0 * pad) / (y1 - y0).max(1e-3));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let bod = |x: f64, y: f64| Pos2::new(rect.center().x + ((x - cx) * s) as f32, rect.center().y - ((y - cy) * s) as f32);
    // rámeček rozsahu
    let ra = Rect::from_two_pos(bod(x0, y0), bod(x1, y1)).expand(8.0);
    for (a, b) in [(ra.left_top(), ra.right_top()), (ra.right_top(), ra.right_bottom()), (ra.right_bottom(), ra.left_bottom()), (ra.left_bottom(), ra.left_top())] {
        p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.0, LINE), 3.0, 3.0));
    }
    for b in &okoli {
        p.circle_filled(bod(b.x, b.y), 3.0, Color32::from_rgb(90, 95, 102));
    }
    let krok = (body.len() / 4000).max(1);
    for (i, b) in body.iter().step_by(krok) {
        let barva = if n.je_duplicita(*i) {
            VIOLET
        } else if n.podezrely(*i).is_some() {
            WARN
        } else {
            BOD
        };
        let c = bod(b.x, b.y);
        p.line_segment([c - Vec2::new(4.0, 0.0), c + Vec2::new(4.0, 0.0)], Stroke::new(1.2, barva));
        p.line_segment([c - Vec2::new(0.0, 4.0), c + Vec2::new(0.0, 4.0)], Stroke::new(1.2, barva));
        p.circle_filled(c, 1.8, barva);
    }
    if let Some((_, e)) = n.dup.ve_vykresu.first().map(|&(i, e)| (i, e)) {
        for &(i, e2) in n.dup.ve_vykresu.iter() {
            if let (Some(a), Some(b)) = (n.vysledek.body.get(i), n.existujici.get(e2)) {
                p.extend(egui::Shape::dashed_line(&[bod(a.x, a.y), bod(b.x, b.y)], Stroke::new(1.0, VIOLET), 2.0, 2.0));
            }
        }
        let _ = e;
    }
    // sever
    let s_pos = Pos2::new(rect.right() - 12.0, rect.top() + 12.0);
    p.text(s_pos, Align2::RIGHT_TOP, "S", FontId::proportional(10.0), FAINT);
    p.line_segment([s_pos + Vec2::new(-12.0, 10.0), s_pos + Vec2::new(-12.0, 0.0)], Stroke::new(1.0, FAINT));
    p.line_segment([s_pos + Vec2::new(-12.0, 0.0), s_pos + Vec2::new(-15.0, 4.0)], Stroke::new(1.0, FAINT));
    p.line_segment([s_pos + Vec2::new(-12.0, 0.0), s_pos + Vec2::new(-9.0, 4.0)], Stroke::new(1.0, FAINT));
    // body mimo
    if !n.odlehle.is_empty() {
        let c = Pos2::new(rect.right() - 20.0, rect.center().y);
        p.line_segment([c + Vec2::new(-6.0, -10.0), c + Vec2::new(6.0, 0.0)], Stroke::new(1.6, WARN));
        p.line_segment([c + Vec2::new(6.0, 0.0), c + Vec2::new(-6.0, 10.0)], Stroke::new(1.6, WARN));
        p.text(c + Vec2::new(-10.0, 22.0), Align2::RIGHT_CENTER, trf!("mimo: {}", n.odlehle.len()), FontId::proportional(10.0), WARN);
    }
    // měřítko
    let delka = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0].into_iter().find(|d| d * s > 30.0).unwrap_or(10000.0);
    let a = Pos2::new(rect.left() + 14.0, rect.bottom() - 10.0);
    p.line_segment([a, a + Vec2::new((delka * s) as f32, 0.0)], Stroke::new(1.0, MUTED));
    p.text(a + Vec2::new((delka * s) as f32 + 4.0, 0.0), Align2::LEFT_CENTER, format!("{delka} m"), FontId::proportional(10.0), MUTED);

    // legenda
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for (barva, t) in [(OK, tr("nové")), (WARN, tr("podezřelé")), (VIOLET, tr("duplicita")), (Color32::from_gray(85), tr("ve výkresu"))] {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                tecka(ui, r.center(), barva);
                ui.label(RichText::new(t).size(11.5).color(MUTED));
            });
        }
    });
    let poznamka = if n.existujici.is_empty() {
        RichText::new(tr("Ve výkrese zatím nejsou body.")).color(MUTED)
    } else if okoli.is_empty() {
        RichText::new(tr("Body leží daleko od bodů výkresu – zkontrolujte sloupce a soustavu.")).color(WARN)
    } else {
        RichText::new(tr("Body leží mezi body výkresu – import sedí.")).color(MUTED)
    };
    ui.add(egui::Label::new(poznamka.size(12.0)).wrap());
    if !n.odlehle.is_empty() {
        let t: Vec<String> = n.odlehle.iter().take(5).filter_map(|&i| n.vysledek.body.get(i)).map(|b| b.cislo.clone()).collect();
        ui.add(egui::Label::new(RichText::new(trf!("Mimo náhled: {}", t.join(", "))).size(12.0).color(WARN)).wrap());
    }
}

fn pravy_panel(ui: &mut egui::Ui, n: &mut NahledImportu, stav: &StavSpravce) {
    nadpis(ui, tr("Poloha bodů"));
    mapka(ui, n);
    ui.add_space(6.0);
    let v_souboru = n.dup.v_souboru.len();
    let ve_vykresu = n.dup.ve_vykresu.len();
    let pocet = v_souboru + ve_vykresu;
    box_ram(ui, |ui| {
        ui.horizontal(|ui| {
            nadpis(ui, tr("Duplicitní čísla"));
            ui.label(RichText::new(trf!("· {} v souboru, {} ve výkresu", v_souboru, ve_vykresu)).size(11.5).color(MUTED));
        });
        let mut zmena = false;
        for (i, titul, popis, cislo) in [
            (0, tr("Přeskočit"), tr("ponechat body ve výkrese, z dvojic v souboru vzít první"), trf!("{} přeskočeno", pocet)),
            (1, tr("Přepsat"), tr("aktualizovat polohu a výšku podle souboru"), trf!("{} přepsáno", ve_vykresu)),
            (2, tr("Přečíslovat od"), tr("nová čísla dostanou jen duplicity"), trf!("{} nová čísla", pocet)),
            (3, tr("Importovat i tak"), tr("dosavadní chování – dvě stejná čísla"), trf!("{} zdvojeno", pocet)),
        ] {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                zmena |= ui.radio_value(&mut n.politika, i, "").changed();
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.horizontal(|ui| {
                        if ui.add(egui::Label::new(RichText::new(titul).size(12.5)).sense(Sense::click())).clicked() && n.politika != i {
                            n.politika = i;
                            zmena = true;
                        }
                        if i == 2 {
                            zmena |= ui.add(egui::DragValue::new(&mut n.precislovat_od).range(1..=u64::MAX / 2)).changed();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(cislo).size(11.5).color(MUTED)));
                    });
                    ui.add(egui::Label::new(RichText::new(popis).size(11.5).color(MUTED)).wrap());
                });
            });
            ui.add_space(2.0);
        }
        if zmena {
            n.prepocti_plan();
        }
    });
    ui.add_space(6.0);
    box_ram(ui, |ui| {
        nadpis(ui, tr("Kam"));
        let kmen = std::path::Path::new(&n.soubor).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let jmeno_nove = trf!("Nová skupina „Import {}“", kmen);
        let akt = match &n.skupina {
            CilSkupiny::Nova => jmeno_nove.clone(),
            CilSkupiny::Existujici(id) => stav.skupiny.iter().find(|s| &s.skupina.id == id).map(|s| s.skupina.jmeno.clone()).unwrap_or_default(),
            CilSkupiny::Zadna => tr("Bez skupiny").into(),
        };
        ui.horizontal(|ui| {
            ikona(ui, obr!("ter_skupina_bodu"), 18.0);
            egui::ComboBox::from_id_salt("imp_skupina").selected_text(RichText::new(akt).size(12.5)).width(ui.available_width() - 4.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut n.skupina, CilSkupiny::Nova, jmeno_nove);
                for s in stav.skupiny.iter().filter(|s| !s.skupina.vse) {
                    ui.selectable_value(&mut n.skupina, CilSkupiny::Existujici(s.skupina.id.clone()), trf!("{} ({} bodů)", s.skupina.jmeno, s.pocet));
                }
                ui.selectable_value(&mut n.skupina, CilSkupiny::Zadna, tr("Bez skupiny"));
            });
        });
        let akt = if n.styl.is_empty() {
            tr("Styl podle kódu").to_string()
        } else {
            stav.styly_bodu.iter().find(|s| s.id == n.styl).map(|s| s.jmeno.clone()).unwrap_or_else(|| n.styl.clone())
        };
        ui.horizontal(|ui| {
            ikona(ui, obr!("ter_styl_bodu"), 18.0);
            egui::ComboBox::from_id_salt("imp_styl").selected_text(RichText::new(akt).size(12.5)).width(ui.available_width() - 4.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut n.styl, String::new(), tr("Styl podle kódu"));
                for s in &stav.styly_bodu {
                    ui.selectable_value(&mut n.styl, s.id.clone(), &s.jmeno);
                }
            });
        });
        ui.add_space(4.0);
        ui.checkbox(&mut n.priblizit, RichText::new(tr("Po importu přiblížit na body")).size(12.5));
        ui.checkbox(&mut n.zapamatovat, RichText::new(tr("Zapamatovat nastavení pro příště")).size(12.5));
    });
}

/// Patička se souhrnem a tlačítky. Vrací `true`, když se okno má zavřít.
fn paticka(ui: &mut egui::Ui, n: &mut NahledImportu) -> bool {
    let mut zavrit = false;
    let (_, _, chyby, podezrele, _) = n.pocty();
    let vlozi = n.plan.pridat.len();
    let prepise = n.plan.prepsat.len();
    let sirka = ui.available_width();
    let (r, _) = ui.allocate_exact_size(Vec2::new(sirka, 48.0), Sense::hover());
    ui.painter().rect_filled(r, egui::CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 }, PANEL2);
    ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, LINE));
    ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(14.0, 10.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let kus = |ui: &mut egui::Ui, t: String, b: Color32| {
            ui.label(RichText::new(t).size(12.5).color(b));
        };
        kus(ui, trf!("{} bodů se vloží", vlozi), MUTED);
        if prepise > 0 {
            kus(ui, trf!("· {} se přepíše", prepise), MUTED);
        }
        if n.plan.preskoceno > 0 {
            kus(ui, trf!("· {} duplicit se přeskočí", n.plan.preskoceno), VIOLET);
        }
        if chyby > 0 {
            kus(ui, trf!("· {} nečitelné se vynechají", chyby), BAD);
        }
        if podezrele > 0 {
            kus(ui, trf!("· {} podezřelé se vloží (lze vyřadit)", podezrele), WARN);
        }
        kus(ui, tr("· Zpět jedním krokem").into(), MUTED);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let pocet = vlozi + prepise;
            let tl = egui::Button::new(RichText::new(trf!("Importovat {} bodů", pocet)).size(13.0).strong().color(Color32::WHITE))
                .fill(PRIMARNI)
                .stroke(Stroke::new(1.0, SELLINE))
                .corner_radius(6.0)
                .min_size(Vec2::new(150.0, 28.0));
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
            if tlacitko(ui, tr("Zrušit")).clicked() {
                n.fronta.clear();
                zavrit = true;
            }
            if !n.fronta.is_empty() && tlacitko(ui, tr("Přeskočit soubor")).clicked() {
                zavrit = true;
            }
            let mut ukazovat = nastaveni::akt().nahled_importu;
            if ui.checkbox(&mut ukazovat, RichText::new(tr("Náhled ukazovat i příště")).size(12.0).color(MUTED)).changed() {
                nastaveni::zmen(|x| x.nahled_importu = ukazovat);
            }
        });
    });
    zavrit
}

// ── zkušební zobrazení mimo Open CAD Studio ───────────────────────────────────

/// Otevře samotné okno importu s daty ze souboru (prázdná cesta = volba
/// zdroje), po vykreslení uloží snímek do `png` a skončí. Slouží ke kontrole
/// vzhledu bez Open CAD Studia (`cargo run --example import_okno`).
#[doc(hidden)]
pub fn ukazka(cesta: &str, png: &str, sirka: f32, vyska: f32, filtr: &str) -> eframe::Result {
    struct App {
        slot: Option<NahledImportu>,
        stav: StavSpravce,
        snimek: String,
        snimkovano: bool,
        snimku: u32,
    }
    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
            let ctx = &ui.ctx().clone();
            ui.painter().rect_filled(ui.max_rect(), 0.0, Color32::from_rgb(27, 27, 27));
            if self.slot.is_none() {
                self.slot = Some(NahledImportu::prazdny());
            }
            kresli(ctx, &mut self.slot, &self.stav);
            self.snimku += 1;
            for e in ctx.input(|i| i.raw.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = e {
                    let soubor = std::fs::File::create(&self.snimek).expect("soubor snímku");
                    let mut enc = png::Encoder::new(std::io::BufWriter::new(soubor), image.size[0] as u32, image.size[1] as u32);
                    enc.set_color(png::ColorType::Rgba);
                    enc.set_depth(png::BitDepth::Eight);
                    let data: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    enc.write_header().and_then(|mut w| w.write_image_data(&data)).expect("zápis snímku");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            if self.snimku == 25 && !self.snimkovano {
                self.snimkovano = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint();
        }
    }
    let mut stav = StavSpravce { verze: 1, dalsi_cislo: 5001, ..Default::default() };
    stav.styly_bodu = Vec::new();
    let mut slot = if cesta.is_empty() { Some(NahledImportu::prazdny()) } else { NahledImportu::soubory(vec![cesta.to_string()]) };
    if let Some(n) = slot.as_mut() {
        n.prepocti(&stav);
        // body „ve výkrese“ pro ukázku duplicit a mapky
        if let Ok(t) = std::env::var("TEREN_UKAZKA_VYKRES") {
            n.existujici = t
                .split(';')
                .filter_map(|b| {
                    let c: Vec<&str> = b.split(',').collect();
                    if c.len() == 3 { Some(Bod { cislo: c[0].into(), x: -c[1].parse::<f64>().ok()?, y: -c[2].parse::<f64>().ok()?, z: 0.0, popis: String::new() }) } else { None }
                })
                .collect();
            n.zmeneno = true;
            n.prepocti(&stav);
        }
        n.filtr = match filtr {
            "chyby" => Filtr::Chyby,
            "podezrele" => Filtr::Podezrele,
            "duplicity" => Filtr::Duplicity,
            _ => Filtr::Vse,
        };
    }
    let moznosti = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([sirka, vyska]), ..Default::default() };
    let snimek = png.to_string();
    eframe::run_native(
        "Terén – import (ukázka)",
        moznosti,
        Box::new(move |cc| {
            super::nastav_vzhled(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(App { slot, stav, snimek, snimkovano: false, snimku: 0 }))
        }),
    )
}
