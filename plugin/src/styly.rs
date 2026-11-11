//! Styly bodů a povrchů.
//!
//! Vestavěné styly jsou v pluginu; když uživatel styl upraví nebo vytvoří
//! nový, uloží se do výkresu jako pomocný bod na vypnuté hladině `TER_STYLY`
//! s definicí v XDATA `TEREN_POVRCH` (první řetězec `STYL_BODU` / `STYL_POVRCHU`).
//! Uložený styl se stejným id přebije vestavěný. Šablona stylů (soubor
//! v %APPDATA%) slouží k přenosu stylů mezi výkresy.

use teren::preklad::tr;
use teren::trf;
use std::path::PathBuf;

use ocs_plugin_api::host::codec::entities::Point;
use ocs_plugin_api::host::codec::types::{Color, LineWeight};
use ocs_plugin_api::host::{Document as CadDocument, EntityType, ExtendedDataRecord, Handle, LayerConfig, XDataValue};

use teren::body::Bod;
use teren::skupiny::{SkupinaBodu, VSECHNY};

use crate::cogo::{self, BodVykresu};
use crate::povrchy::APP_POVRCH;
use crate::vykres::Vykres;

pub const H_STYLY: &str = "TER_STYLY";
const STYL_BODU: &str = "STYL_BODU";
const STYL_POVRCHU: &str = "STYL_POVRCHU";
/// Tvar značky v jednotkách poloměru (r = 1, osa y nahoru).
#[derive(Clone, Debug, PartialEq)]
pub enum Tvar {
    Cara((f64, f64), (f64, f64)),
    Kruh((f64, f64), f64),
    Lomena(Vec<(f64, f64)>, bool),
}

fn vlnka(_n: usize, zuby: f64, hloubka: f64, spicky: bool) -> Tvar {
    let k = 72;
    let body = (0..k)
        .map(|i| {
            let t = i as f64 / k as f64 * std::f64::consts::TAU;
            let v = if spicky {
                // pilovitý obrys (jehličnan)
                let f = (t * zuby / std::f64::consts::TAU).fract();
                1.0 - hloubka * (2.0 * (f - 0.5)).abs()
            } else {
                1.0 - hloubka + hloubka * (t * zuby / 2.0).cos().abs()
            };
            (v * t.cos(), v * t.sin())
        })
        .collect();
    Tvar::Lomena(body, true)
}

impl Znacka {
    /// Symbol (strom, šachta…) – na rozdíl od jednoduchých značek bodu.
    pub fn je_symbol(self) -> bool {
        matches!(
            self,
            Znacka::StromListnaty
                | Znacka::StromJehlicnaty
                | Znacka::Ker
                | Znacka::Sloup
                | Znacka::Hydrant
                | Znacka::Sachta
                | Znacka::Vpust
                | Znacka::Soupe
        )
    }

    /// Geometrie značky (společná pro výkres i náhled).
    pub fn tvary(self) -> Vec<Tvar> {
        let d = std::f64::consts::FRAC_1_SQRT_2;
        let kruh = |r: f64| Tvar::Kruh((0.0, 0.0), r);
        let ctverec = Tvar::Lomena(vec![(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)], true);
        match self {
            Znacka::Bod => vec![kruh(0.18)],
            Znacka::Krizek => vec![Tvar::Cara((-1.0, 0.0), (1.0, 0.0)), Tvar::Cara((0.0, -1.0), (0.0, 1.0))],
            Znacka::KrizekX => vec![Tvar::Cara((-d, -d), (d, d)), Tvar::Cara((-d, d), (d, -d))],
            Znacka::Kolecko => vec![kruh(1.0)],
            Znacka::KoleckoKrizek => {
                vec![kruh(1.0), Tvar::Cara((-1.0, 0.0), (1.0, 0.0)), Tvar::Cara((0.0, -1.0), (0.0, 1.0))]
            }
            Znacka::Ctverec => vec![ctverec],
            Znacka::Trojuhelnik => vec![Tvar::Lomena(vec![(-1.0, -0.6), (1.0, -0.6), (0.0, 1.0)], true)],
            Znacka::StromListnaty => vec![vlnka(0, 9.0, 0.14, false), kruh(0.1)],
            Znacka::StromJehlicnaty => vec![vlnka(0, 14.0, 0.25, true), kruh(0.1)],
            Znacka::Ker => vec![vlnka(0, 6.0, 0.2, false)],
            Znacka::Sloup => vec![kruh(1.0), kruh(0.45)],
            Znacka::Hydrant => vec![
                kruh(1.0),
                Tvar::Cara((-0.4, -0.5), (-0.4, 0.5)),
                Tvar::Cara((0.4, -0.5), (0.4, 0.5)),
                Tvar::Cara((-0.4, 0.0), (0.4, 0.0)),
            ],
            Znacka::Sachta => vec![kruh(1.0), Tvar::Cara((-d, -d), (d, d)), Tvar::Cara((-d, d), (d, -d))],
            Znacka::Vpust => vec![ctverec, Tvar::Cara((-1.0, -1.0), (1.0, 1.0))],
            Znacka::Soupe => vec![Tvar::Lomena(vec![(-1.0, -0.6), (1.0, 0.6), (1.0, -0.6), (-1.0, 0.6)], true)],
        }
    }
}

/// Výchozí styl bodů (platí pro body bez stylu ve skupině).
pub const STANDARD: &str = "STANDARD";
/// Styl „Nezobrazovat“ (pro body i povrchy).
pub const SKRYT: &str = "SKRYT";

/// Barva: 256 = podle hladiny, 1–255 = index barvy DWG (ACI).
pub type Barva = i16;
pub const PODLE_HLADINY: Barva = 256;

pub fn barva(b: Barva) -> Color {
    if (1..=255).contains(&b) { Color::from_index(b) } else { Color::ByLayer }
}

/// Tloušťka čáry v setinách mm, záporná = podle hladiny.
pub fn tloustka(t: i16) -> LineWeight {
    if t >= 0 { LineWeight::Value(t) } else { LineWeight::ByLayer }
}

/// Barvy nabízené v okně (ACI, název).
pub const BARVY: [(Barva, &str); 12] = [
    (PODLE_HLADINY, "podle hladiny"),
    (1, "červená"),
    (2, "žlutá"),
    (3, "zelená"),
    (4, "azurová"),
    (5, "modrá"),
    (6, "purpurová"),
    (7, "bílá / černá"),
    (8, "tmavě šedá"),
    (9, "světle šedá"),
    (30, "oranžová"),
    (22, "hnědá"),
];

pub fn nazev_barvy(b: Barva) -> String {
    BARVY.iter().find(|(x, _)| *x == b).map_or_else(|| trf!("barva {}", b), |(_, n)| tr(n).to_string())
}

// ── Styl bodu ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Znacka {
    /// Jen bod (POINT) bez vlastní značky.
    Bod,
    Krizek,
    KrizekX,
    Kolecko,
    KoleckoKrizek,
    Ctverec,
    Trojuhelnik,
    StromListnaty,
    StromJehlicnaty,
    Ker,
    Sloup,
    Hydrant,
    Sachta,
    Vpust,
    Soupe,
}

pub const ZNACKY: [Znacka; 15] = [
    Znacka::Bod,
    Znacka::Krizek,
    Znacka::KrizekX,
    Znacka::Kolecko,
    Znacka::KoleckoKrizek,
    Znacka::Ctverec,
    Znacka::Trojuhelnik,
    Znacka::StromListnaty,
    Znacka::StromJehlicnaty,
    Znacka::Ker,
    Znacka::Sloup,
    Znacka::Hydrant,
    Znacka::Sachta,
    Znacka::Vpust,
    Znacka::Soupe,
];

impl Znacka {
    pub fn kod(self) -> &'static str {
        match self {
            Znacka::Bod => "BOD",
            Znacka::Krizek => "KRIZ",
            Znacka::KrizekX => "X",
            Znacka::Kolecko => "KRUH",
            Znacka::KoleckoKrizek => "KRUHKRIZ",
            Znacka::Ctverec => "CTVEREC",
            Znacka::Trojuhelnik => "TROJ",
            Znacka::StromListnaty => "STROM_L",
            Znacka::StromJehlicnaty => "STROM_J",
            Znacka::Ker => "KER",
            Znacka::Sloup => "SLOUP",
            Znacka::Hydrant => "HYDRANT",
            Znacka::Sachta => "SACHTA",
            Znacka::Vpust => "VPUST",
            Znacka::Soupe => "SOUPE",
        }
    }
    pub fn nazev(self) -> &'static str {
        match self {
            Znacka::Bod => tr("jen bod"),
            Znacka::Krizek => tr("křížek +"),
            Znacka::KrizekX => tr("křížek ×"),
            Znacka::Kolecko => tr("kolečko"),
            Znacka::KoleckoKrizek => tr("kolečko s křížkem"),
            Znacka::Ctverec => tr("čtverec"),
            Znacka::Trojuhelnik => tr("trojúhelník"),
            Znacka::StromListnaty => tr("strom listnatý"),
            Znacka::StromJehlicnaty => tr("strom jehličnatý"),
            Znacka::Ker => tr("keř"),
            Znacka::Sloup => tr("sloup"),
            Znacka::Hydrant => tr("hydrant"),
            Znacka::Sachta => tr("šachta"),
            Znacka::Vpust => tr("vpusť"),
            Znacka::Soupe => tr("šoupě"),
        }
    }
    fn z_kodu(k: &str) -> Self {
        ZNACKY.into_iter().find(|z| z.kod() == k).unwrap_or(Znacka::Bod)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Popisek {
    /// Číslo nad výškou.
    CisloVyska,
    /// Číslo a výška na jednom řádku.
    JedenRadek,
    JenVyska,
    JenCislo,
    Nic,
}

pub const POPISKY: [Popisek; 5] = [Popisek::CisloVyska, Popisek::JedenRadek, Popisek::JenVyska, Popisek::JenCislo, Popisek::Nic];

impl Popisek {
    pub fn kod(self) -> &'static str {
        match self {
            Popisek::CisloVyska => "CV",
            Popisek::JedenRadek => "C_V",
            Popisek::JenVyska => "V",
            Popisek::JenCislo => "C",
            Popisek::Nic => "-",
        }
    }
    pub fn nazev(self) -> &'static str {
        match self {
            Popisek::CisloVyska => tr("číslo nad výškou"),
            Popisek::JedenRadek => tr("číslo a výška vedle sebe"),
            Popisek::JenVyska => tr("jen výška"),
            Popisek::JenCislo => tr("jen číslo"),
            Popisek::Nic => tr("bez popisu"),
        }
    }
    fn z_kodu(k: &str) -> Self {
        POPISKY.into_iter().find(|z| z.kod() == k).unwrap_or(Popisek::CisloVyska)
    }
    pub fn cislo(self) -> bool {
        matches!(self, Popisek::CisloVyska | Popisek::JedenRadek | Popisek::JenCislo)
    }
    pub fn vyska(self) -> bool {
        matches!(self, Popisek::CisloVyska | Popisek::JedenRadek | Popisek::JenVyska)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StylBodu {
    pub id: String,
    pub jmeno: String,
    pub znacka: Znacka,
    /// Velikost značky na papíře [mm].
    pub velikost: f64,
    pub popisek: Popisek,
    /// Výška písma na papíře [mm].
    pub pismo: f64,
    /// Desetinná místa výšky.
    pub mista: u8,
    /// Desetinná čárka místo tečky.
    pub carka: bool,
    /// Zobrazit popis (kód) bodu.
    pub popis: bool,
    pub barva_znacky: Barva,
    pub barva_textu: Barva,
    /// Kódy bodů, které dostanou tento styl automaticky (např. STR).
    pub kody: Vec<String>,
    /// Nezobrazovat bod (ani značku, ani popis).
    pub skryt: bool,
    /// Vlastní značka – blok výkresu (prázdné = vestavěný tvar `znacka`).
    pub blok: String,
}

impl StylBodu {
    fn novy(id: &str, jmeno: &str, znacka: Znacka, popisek: Popisek, popis: bool) -> Self {
        Self {
            id: id.into(),
            jmeno: jmeno.into(),
            znacka,
            velikost: 1.5,
            popisek,
            pismo: 2.0,
            mista: 2,
            carka: false,
            popis,
            barva_znacky: PODLE_HLADINY,
            barva_textu: PODLE_HLADINY,
            kody: Vec::new(),
            skryt: false,
            blok: String::new(),
        }
    }

    fn symbol(id: &str, jmeno: &str, znacka: Znacka, velikost: f64, barva: Barva, kody: &[&str]) -> Self {
        Self {
            velikost,
            barva_znacky: barva,
            kody: kody.iter().map(|k| k.to_string()).collect(),
            ..Self::novy(id, jmeno, znacka, Popisek::Nic, false)
        }
    }

    pub fn vestavene() -> Vec<StylBodu> {
        vec![
            Self::novy(STANDARD, tr("Standard (číslo, výška, popis)"), Znacka::Bod, Popisek::CisloVyska, true),
            Self::novy("KRIZ_VYSKA", tr("Křížek s výškou"), Znacka::Krizek, Popisek::JenVyska, false),
            Self::novy("KRIZ_RADEK", tr("Křížek, číslo a výška vedle sebe"), Znacka::Krizek, Popisek::JedenRadek, false),
            Self::novy("JEN_ZNACKA", tr("Jen značka"), Znacka::KrizekX, Popisek::Nic, false),
            Self::symbol("STROM_L", tr("Strom listnatý"), Znacka::StromListnaty, 6.0, 3, &[]),
            Self::symbol("STROM_J", tr("Strom jehličnatý"), Znacka::StromJehlicnaty, 6.0, 3, &[]),
            Self::symbol("KER", tr("Keř"), Znacka::Ker, 4.0, 3, &[]),
            Self::symbol("SLOUP", tr("Sloup"), Znacka::Sloup, 2.5, PODLE_HLADINY, &[]),
            Self::symbol("HYDRANT", tr("Hydrant"), Znacka::Hydrant, 3.0, 5, &[]),
            Self::symbol("SACHTA", tr("Šachta"), Znacka::Sachta, 3.0, PODLE_HLADINY, &[]),
            Self::symbol("VPUST", tr("Vpusť"), Znacka::Vpust, 3.0, PODLE_HLADINY, &[]),
            Self::symbol("SOUPE", tr("Šoupě"), Znacka::Soupe, 2.5, 5, &[]),
            Self { skryt: true, ..Self::novy(SKRYT, tr("Nezobrazovat"), Znacka::Bod, Popisek::Nic, false) },
        ]
    }

    pub fn je_vestaveny(&self) -> bool {
        Self::vestavene().iter().any(|s| s.id == self.id)
    }

    /// Text výšky podle stylu.
    pub fn text_vysky(&self, z: f64) -> String {
        let t = format!("{z:.*}", self.mista as usize);
        if self.carka { t.replace('.', ",") } else { t }
    }

    pub(crate) fn zapis(&self) -> Vec<String> {
        vec![
            format!("id={}", self.id),
            format!("jmeno={}", self.jmeno),
            format!("znacka={}", self.znacka.kod()),
            format!("velikost={}", self.velikost),
            format!("popisek={}", self.popisek.kod()),
            format!("pismo={}", self.pismo),
            format!("mista={}", self.mista),
            format!("carka={}", self.carka as u8),
            format!("popis={}", self.popis as u8),
            format!("bznacky={}", self.barva_znacky),
            format!("btextu={}", self.barva_textu),
            format!("kody={}", self.kody.join(",")),
            format!("skryt={}", self.skryt as u8),
            format!("blok={}", self.blok),
        ]
    }

    fn precti<S: AsRef<str>>(radky: &[S]) -> Option<Self> {
        let s = Self::uprav(Self::novy("", "", Znacka::Bod, Popisek::CisloVyska, true), radky);
        (!s.id.is_empty()).then_some(s)
    }

    /// Změní hodnoty stylu podle řádků `klíč=hodnota`.
    pub fn uprav<S: AsRef<str>>(mut s: Self, radky: &[S]) -> Self {
        for r in radky {
            let Some((k, v)) = r.as_ref().split_once('=') else { continue };
            match k {
                "id" => s.id = v.into(),
                "jmeno" => s.jmeno = v.into(),
                "znacka" => s.znacka = Znacka::z_kodu(v),
                "velikost" => s.velikost = v.parse().unwrap_or(1.5),
                "popisek" => s.popisek = Popisek::z_kodu(v),
                "pismo" => s.pismo = v.parse().unwrap_or(2.0),
                "mista" => s.mista = v.parse().unwrap_or(2),
                "carka" => s.carka = v == "1",
                "popis" => s.popis = v == "1",
                "bznacky" => s.barva_znacky = v.parse().unwrap_or(PODLE_HLADINY),
                "btextu" => s.barva_textu = v.parse().unwrap_or(PODLE_HLADINY),
                "kody" => s.kody = teren::skupiny::kody_z_textu(v),
                "skryt" => s.skryt = v == "1",
                "blok" => s.blok = v.trim().to_string(),
                _ => {}
            }
        }
        s
    }
}

// ── Styl povrchu ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PopisVrstevnic {
    Nic,
    Hlavni,
    Vse,
}

impl PopisVrstevnic {
    pub fn nazev(self) -> &'static str {
        match self {
            PopisVrstevnic::Nic => tr("bez popisu"),
            PopisVrstevnic::Hlavni => tr("hlavní vrstevnice"),
            PopisVrstevnic::Vse => tr("všechny vrstevnice"),
        }
    }
    fn kod(self) -> &'static str {
        match self {
            PopisVrstevnic::Nic => "0",
            PopisVrstevnic::Hlavni => "H",
            PopisVrstevnic::Vse => "V",
        }
    }
    fn z_kodu(k: &str) -> Self {
        match k {
            "H" => PopisVrstevnic::Hlavni,
            "V" => PopisVrstevnic::Vse,
            _ => PopisVrstevnic::Nic,
        }
    }
}

pub const POPISY_VRSTEVNIC: [PopisVrstevnic; 3] = [PopisVrstevnic::Hlavni, PopisVrstevnic::Vse, PopisVrstevnic::Nic];

#[derive(Clone, Debug, PartialEq)]
pub struct StylPovrchu {
    pub id: String,
    pub jmeno: String,
    pub tin: bool,
    pub barva_tin: Barva,
    pub obrys: bool,
    pub barva_obrys: Barva,
    pub vrstevnice: bool,
    /// Krok vrstevnic (vedlejší, hlavní) [m]; `None` = podle povrchu.
    pub interval: Option<(f64, f64)>,
    pub barva_vedlejsi: Barva,
    pub barva_hlavni: Barva,
    /// Tloušťka v setinách mm, záporná = podle hladiny.
    pub tloustka_vedlejsi: i16,
    pub tloustka_hlavni: i16,
    pub popis: PopisVrstevnic,
    /// Výška popisu na papíře [mm].
    pub pismo: f64,
    /// Rozestup popisů na papíře [mm].
    pub rozestup: f64,
    pub mista: u8,
    pub barva_popisu: Barva,
    /// Šipky směru spádu na trojúhelnících.
    pub sipky: bool,
    /// Délka šipky na papíře [mm].
    pub delka_sipky: f64,
    pub barva_sipek: Barva,
}

impl StylPovrchu {
    fn zaklad(id: &str, jmeno: &str) -> Self {
        Self {
            id: id.into(),
            jmeno: jmeno.into(),
            tin: false,
            barva_tin: PODLE_HLADINY,
            obrys: true,
            barva_obrys: PODLE_HLADINY,
            vrstevnice: true,
            interval: None,
            barva_vedlejsi: PODLE_HLADINY,
            barva_hlavni: PODLE_HLADINY,
            tloustka_vedlejsi: -1,
            tloustka_hlavni: -1,
            popis: PopisVrstevnic::Hlavni,
            pismo: 2.5,
            rozestup: 120.0,
            mista: 2,
            barva_popisu: PODLE_HLADINY,
            sipky: false,
            delka_sipky: 5.0,
            barva_sipek: PODLE_HLADINY,
        }
    }

    /// Vestavěné styly (odpovídají stylům z verzí 0.2–0.4, krok podle povrchu).
    pub fn vestavene() -> Vec<StylPovrchu> {
        let mut v = Vec::new();
        v.push(Self::zaklad("VRSTP", tr("Vrstevnice s popisem")));
        v.push(Self { popis: PopisVrstevnic::Nic, ..Self::zaklad("VRST", tr("Vrstevnice")) });
        v.push(Self { tin: true, popis: PopisVrstevnic::Nic, ..Self::zaklad("TINVRST", tr("Trojúhelníky + vrstevnice")) });
        v.push(Self { tin: true, vrstevnice: false, popis: PopisVrstevnic::Nic, ..Self::zaklad("TIN", tr("Trojúhelníky")) });
        v.push(Self { vrstevnice: false, popis: PopisVrstevnic::Nic, ..Self::zaklad("OBRYS", tr("Obrys")) });
        v.push(Self { tin: true, vrstevnice: false, popis: PopisVrstevnic::Nic, sipky: true, ..Self::zaklad("SPAD", tr("Spád (šipky)")) });
        v.push(Self {
            obrys: false,
            vrstevnice: false,
            popis: PopisVrstevnic::Nic,
            ..Self::zaklad(SKRYT, tr("Nezobrazovat"))
        });
        v
    }

    pub fn je_vestaveny(&self) -> bool {
        Self::vestavene().iter().any(|s| s.id == self.id)
    }

    /// Nový vlastní styl (výchozí hodnoty pro úpravy).
    pub fn vlastni(id: &str, jmeno: &str) -> Self {
        Self {
            interval: Some((0.5, 2.5)),
            barva_vedlejsi: 32,
            barva_hlavni: 22,
            tloustka_hlavni: 35,
            ..Self::zaklad(id, jmeno)
        }
    }

    pub fn text_vysky(&self, z: f64) -> String {
        let t = format!("{z:.*}", self.mista as usize);
        let t = if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_string() } else { t };
        t.replace('.', ",")
    }

    pub(crate) fn zapis(&self) -> Vec<String> {
        let (i, h) = self.interval.unwrap_or((0.0, 0.0));
        vec![
            format!("id={}", self.id),
            format!("jmeno={}", self.jmeno),
            format!("tin={}|{}", self.tin as u8, self.barva_tin),
            format!("obrys={}|{}", self.obrys as u8, self.barva_obrys),
            format!("vrst={}", self.vrstevnice as u8),
            format!("krok={i}|{h}"),
            format!("vedl={}|{}", self.barva_vedlejsi, self.tloustka_vedlejsi),
            format!("hlav={}|{}", self.barva_hlavni, self.tloustka_hlavni),
            format!("popis={}|{}|{}|{}|{}", self.popis.kod(), self.pismo, self.rozestup, self.mista, self.barva_popisu),
            format!("sipky={}|{}|{}", self.sipky as u8, self.delka_sipky, self.barva_sipek),
        ]
    }

    fn precti<S: AsRef<str>>(radky: &[S]) -> Option<Self> {
        let s = Self::uprav(Self::zaklad("", ""), radky);
        (!s.id.is_empty()).then_some(s)
    }

    /// Změní hodnoty stylu podle řádků `klíč=hodnota`.
    pub fn uprav<S: AsRef<str>>(mut s: Self, radky: &[S]) -> Self {
        for r in radky {
            let Some((k, v)) = r.as_ref().split_once('=') else { continue };
            let c: Vec<&str> = v.split('|').collect();
            let f = |i: usize, d: f64| c.get(i).and_then(|x| x.parse::<f64>().ok()).unwrap_or(d);
            let n = |i: usize, d: i16| c.get(i).and_then(|x| x.parse::<i16>().ok()).unwrap_or(d);
            let b = |i: usize| c.get(i).map(|x| *x == "1").unwrap_or(false);
            match k {
                "id" => s.id = v.into(),
                "jmeno" => s.jmeno = v.into(),
                "tin" => (s.tin, s.barva_tin) = (b(0), n(1, PODLE_HLADINY)),
                "obrys" => (s.obrys, s.barva_obrys) = (b(0), n(1, PODLE_HLADINY)),
                "vrst" => s.vrstevnice = b(0),
                "krok" => {
                    let (i, h) = (f(0, 0.0), f(1, 0.0));
                    s.interval = (i > 0.0 && h > 0.0).then_some((i, h));
                }
                "vedl" => (s.barva_vedlejsi, s.tloustka_vedlejsi) = (n(0, PODLE_HLADINY), n(1, -1)),
                "hlav" => (s.barva_hlavni, s.tloustka_hlavni) = (n(0, PODLE_HLADINY), n(1, -1)),
                "popis" => {
                    s.popis = PopisVrstevnic::z_kodu(c.first().copied().unwrap_or("0"));
                    s.pismo = f(1, 2.5);
                    s.rozestup = f(2, 120.0);
                    s.mista = n(3, 2).clamp(0, 3) as u8;
                    s.barva_popisu = n(4, PODLE_HLADINY);
                }
                "sipky" => {
                    s.sipky = b(0);
                    s.delka_sipky = f(1, 5.0);
                    s.barva_sipek = n(2, PODLE_HLADINY);
                }
                _ => {}
            }
        }
        s
    }
}

// ── Uložení ve výkrese ───────────────────────────────────────────────────────

fn retezce(r: &ExtendedDataRecord) -> Vec<String> {
    r.values.iter().filter_map(|v| if let XDataValue::String(s) = v { Some(s.clone()) } else { None }).collect()
}

/// Uložené styly: (handle nosiče, druh, řetězce).
fn ulozene(doc: &CadDocument) -> Vec<(Handle, String, Vec<String>)> {
    doc.model_space_entities()
        .filter_map(|e| {
            let r = e.common().extended_data.get_record(APP_POVRCH)?;
            let t = retezce(r);
            let druh = t.first()?.clone();
            (druh == STYL_BODU || druh == STYL_POVRCHU).then(|| (e.common().handle, druh, t[1..].to_vec()))
        })
        .collect()
}

/// Styly bodů výkresu: vestavěné (případně přepsané) a vlastní.
pub fn styly_bodu(doc: &CadDocument) -> Vec<StylBodu> {
    let mut v = StylBodu::vestavene();
    for (_, druh, t) in ulozene(doc) {
        if druh != STYL_BODU {
            continue;
        }
        if let Some(s) = StylBodu::precti(&t) {
            match v.iter_mut().find(|x| x.id == s.id) {
                Some(x) => *x = s,
                None => v.push(s),
            }
        }
    }
    v
}

pub fn styly_povrchu(doc: &CadDocument) -> Vec<StylPovrchu> {
    let mut v = StylPovrchu::vestavene();
    for (_, druh, t) in ulozene(doc) {
        if druh != STYL_POVRCHU {
            continue;
        }
        if let Some(s) = StylPovrchu::precti(&t) {
            match v.iter_mut().find(|x| x.id == s.id) {
                Some(x) => *x = s,
                None => v.push(s),
            }
        }
    }
    v
}

pub fn styl_povrchu(doc: &CadDocument, id: &str) -> StylPovrchu {
    let v = styly_povrchu(doc);
    v.iter().find(|s| s.id.eq_ignore_ascii_case(id)).cloned().unwrap_or_else(|| v[0].clone())
}

/// Je styl uložený ve výkrese (upravený vestavěný nebo vlastní)?
pub fn je_ulozeny(doc: &CadDocument, id: &str) -> bool {
    ulozene(doc).iter().any(|(_, _, t)| t.iter().any(|r| r == &format!("id={id}")))
}

fn nosic(druh: &str, radky: Vec<String>, poloha: [f64; 2]) -> EntityType {
    let mut p = Point::from_coords(poloha[0], poloha[1], 0.0);
    p.common.layer = H_STYLY.into();
    p.common.extended_data.add_record(zaznam(druh, radky));
    EntityType::Point(p)
}

fn zaznam(druh: &str, radky: Vec<String>) -> ExtendedDataRecord {
    let mut r = ExtendedDataRecord::new(APP_POVRCH);
    r.add_value(XDataValue::String(druh.into()));
    for t in radky {
        r.add_value(XDataValue::String(t));
    }
    r
}

fn poloha_nosice(doc: &CadDocument) -> [f64; 2] {
    let b = cogo::body(doc);
    if b.is_empty() {
        return [0.0, 0.0];
    }
    let n = b.len() as f64;
    [b.iter().map(|b| b.bod.x).sum::<f64>() / n, b.iter().map(|b| b.bod.y).sum::<f64>() / n]
}

fn zajisti_hladinu(v: &mut dyn Vykres, doc: &CadDocument) {
    if !doc.layers.iter().any(|l| l.name.eq_ignore_ascii_case(H_STYLY)) {
        v.hladina(LayerConfig {
            name: H_STYLY.into(),
            color: Some(Color::from_index(8)),
            off: Some(true),
            plottable: Some(false),
            description: Some(tr("Styly bodů a povrchů (plugin Terén) – nemazat").into()),
            ..Default::default()
        });
    }
}

fn uloz(v: &mut dyn Vykres, doc: &CadDocument, druh: &str, id: &str, radky: Vec<String>) -> Result<(), String> {
    cogo::priprav_vykres(v)?;
    zajisti_hladinu(v, doc);
    let klic = format!("id={id}");
    if let Some((h, _, _)) = ulozene(doc).into_iter().find(|(_, d, t)| d == druh && t.contains(&klic)) {
        if v.zapis_xdata(h, zaznam(druh, radky)) {
            return Ok(());
        }
        return Err(tr("styl se nepodařilo uložit").into());
    }
    v.pridej_jednu(nosic(druh, radky, poloha_nosice(doc)))?;
    Ok(())
}

pub fn uloz_styl_bodu(v: &mut dyn Vykres, doc: &CadDocument, s: &StylBodu) -> Result<(), String> {
    uloz(v, doc, STYL_BODU, &s.id, s.zapis())
}

pub fn uloz_styl_povrchu(v: &mut dyn Vykres, doc: &CadDocument, s: &StylPovrchu) -> Result<(), String> {
    uloz(v, doc, STYL_POVRCHU, &s.id, s.zapis())
}

/// Smaže uložený styl (u vestavěného = návrat k původnímu nastavení).
pub fn smaz_styl(v: &mut dyn Vykres, doc: &CadDocument, id: &str) -> bool {
    let klic = format!("id={id}");
    let mut ok = false;
    for (h, _, t) in ulozene(doc) {
        if t.contains(&klic) {
            ok |= v.odeber(h);
        }
    }
    ok
}

/// Volné id stylu odvozené ze jména.
pub fn volne_id(doc: &CadDocument, jmeno: &str) -> String {
    let mut zaklad = cogo::bez_diakritiky(jmeno);
    if zaklad.is_empty() {
        zaklad = "STYL".into();
    }
    let obsazene: Vec<String> = styly_bodu(doc)
        .into_iter()
        .map(|s| s.id)
        .chain(styly_povrchu(doc).into_iter().map(|s| s.id))
        .map(|s| s.to_uppercase())
        .collect();
    if !obsazene.contains(&zaklad) {
        return zaklad;
    }
    (2..).map(|n| format!("{zaklad}_{n}")).find(|id| !obsazene.contains(id)).unwrap()
}

// ── Který styl má bod ────────────────────────────────────────────────────────

/// Odkud má bod svůj styl.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Puvod {
    Rucne,
    Kod(String),
    Skupina(String),
    Standard,
}

/// Určí styl bodu: vlastní styl bodu, jinak první skupina se stylem, jinak Standard.
pub struct Rozlisovac {
    styly: Vec<StylBodu>,
    skupiny: Vec<SkupinaBodu>,
}

impl Rozlisovac {
    pub fn new(doc: &CadDocument) -> Self {
        let skupiny = cogo::ulozene_skupiny(doc).into_iter().map(|(_, s)| s).filter(|s| !s.styl.is_empty()).collect();
        Self { styly: styly_bodu(doc), skupiny }
    }

    /// Přidá skupinu, která ve výkrese ještě není (např. skupina importu).
    pub fn s_skupinou(mut self, s: Option<&SkupinaBodu>) -> Self {
        if let Some(s) = s.filter(|s| !s.styl.is_empty()) {
            self.skupiny.push(s.clone());
        }
        self
    }

    fn podle_id(&self, id: &str) -> Option<&StylBodu> {
        self.styly.iter().find(|s| s.id.eq_ignore_ascii_case(id))
    }

    /// Pořadí: vlastní styl bodu, styl podle kódu, styl skupiny, Standard.
    pub fn styl(&self, b: &Bod, zdroj: &str, vlastni: &str) -> StylBodu {
        self.urci(b, zdroj, vlastni).0
    }

    /// Styl bodu a odkud ho bod má.
    pub fn urci(&self, b: &Bod, zdroj: &str, vlastni: &str) -> (StylBodu, Puvod) {
        if !vlastni.is_empty() {
            if let Some(s) = self.podle_id(vlastni) {
                return (s.clone(), Puvod::Rucne);
            }
        }
        let kod = b.kod();
        if !kod.is_empty() {
            if let Some(s) = self.styly.iter().find(|s| s.kody.iter().any(|k| k.eq_ignore_ascii_case(kod))) {
                return (s.clone(), Puvod::Kod(kod.to_uppercase()));
            }
        }
        for g in &self.skupiny {
            if g.id != VSECHNY && g.obsahuje(b, zdroj) {
                if let Some(s) = self.podle_id(&g.styl) {
                    return (s.clone(), Puvod::Skupina(g.jmeno.clone()));
                }
            }
        }
        (self.podle_id(STANDARD).cloned().unwrap_or_else(|| StylBodu::vestavene().remove(0)), Puvod::Standard)
    }

    pub fn styl_bodu(&self, b: &BodVykresu) -> StylBodu {
        self.styl(&b.bod, &b.zdroj, &b.styl)
    }
}

// ── Šablona stylů ────────────────────────────────────────────────────────────

fn soubor_sablony() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)?
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
    };
    Some(base.join("OpenCADStudio").join("teren_styly.txt"))
}

/// Uloží styly výkresu do šablony (soubor v profilu uživatele).
pub fn uloz_sablonu(doc: &CadDocument) -> Result<(String, usize), String> {
    let p = soubor_sablony().ok_or(tr("nelze určit složku nastavení"))?;
    let mut text = String::from("# Šablona stylů pluginu Terén – jeden styl na řádek\n"); // i18n:ne
    let mut n = 0;
    for s in styly_bodu(doc) {
        text.push_str(&format!("{STYL_BODU}\t{}\n", s.zapis().join("\t")));
        n += 1;
    }
    for s in styly_povrchu(doc) {
        text.push_str(&format!("{STYL_POVRCHU}\t{}\n", s.zapis().join("\t")));
        n += 1;
    }
    text.push_str(&format!("MERITKO\t{}\n", crate::nastaveni::meritko(doc).round()));
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::write(&p, text).map_err(|e| trf!("šablonu nejde zapsat: {}", e))?;
    Ok((p.display().to_string(), n))
}

/// Měřítko uložené v šabloně (jmenovatel).
pub fn meritko_sablony() -> Option<u32> {
    let text = std::fs::read_to_string(soubor_sablony()?).ok()?;
    text.lines().find_map(|r| r.strip_prefix("MERITKO\t")).and_then(|m| m.trim().parse::<f64>().ok()).map(|m| m as u32)
}

/// Je šablona uložená?
pub fn sablona_existuje() -> bool {
    soubor_sablony().is_some_and(|p| p.is_file())
}

/// Načte šablonu a uloží její styly do výkresu (stejná id se přepíšou).
pub fn nacti_sablonu(v: &mut dyn Vykres, doc: &CadDocument) -> Result<usize, String> {
    let p = soubor_sablony().ok_or(tr("nelze určit složku nastavení"))?;
    let text = std::fs::read_to_string(&p).map_err(|_| trf!("šablona stylů zatím není uložená ({})", p.display()))?;
    let mut n = 0;
    for radek in text.lines().filter(|r| !r.starts_with('#')) {
        let pole: Vec<&str> = radek.split('\t').collect();
        match pole.first().copied() {
            Some(STYL_BODU) => {
                if let Some(s) = StylBodu::precti(&pole[1..]) {
                    if s != StylBodu::vestavene().into_iter().find(|x| x.id == s.id).unwrap_or_else(|| s.clone()) || !s.je_vestaveny() {
                        uloz_styl_bodu(v, doc, &s)?;
                        n += 1;
                    }
                }
            }
            Some(STYL_POVRCHU) => {
                if let Some(s) = StylPovrchu::precti(&pole[1..]) {
                    if s != StylPovrchu::vestavene().into_iter().find(|x| x.id == s.id).unwrap_or_else(|| s.clone()) || !s.je_vestaveny() {
                        uloz_styl_povrchu(v, doc, &s)?;
                        n += 1;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(n)
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn styl_bodu_tam_a_zpet() {
        let mut s = StylBodu::vestavene()[1].clone();
        s.carka = true;
        s.mista = 3;
        s.barva_textu = 3;
        s.kody = vec!["STR".into(), "KER".into()];
        s.skryt = true;
        assert_eq!(StylBodu::precti(&s.zapis()).unwrap(), s);
        assert_eq!(s.text_vysky(388.395), "388,395");
    }

    #[test]
    fn styl_povrchu_tam_a_zpet() {
        let mut s = StylPovrchu::vlastni("MUJ", "Můj");
        s.sipky = true;
        s.popis = PopisVrstevnic::Vse;
        assert_eq!(StylPovrchu::precti(&s.zapis()).unwrap(), s);
        for v in StylPovrchu::vestavene() {
            assert_eq!(StylPovrchu::precti(&v.zapis()).unwrap(), v);
        }
        assert_eq!(s.text_vysky(357.5), "357,5");
        assert_eq!(s.text_vysky(355.0), "355");
    }
}
