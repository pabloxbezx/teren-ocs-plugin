//! Vzhled pohledu podélného profilu: styly písma, čáry a předlohy vzhledu.
//!
//! Vše je uložené jako klíče stylu profilu (`SCHEMA` v `pohled.rs`), takže
//! funguje dědění ze stylu i výjimky jednoho pohledu. Výšky písma a tloušťky
//! čar jsou v mm na papíře.

use crate::styl::Hodnoty;

/// Role písma: klíč (bez předpony `p.`) a český název.
pub const ROLE: [(&str, &str); 6] = [
    ("nadpis", "Nadpis profilu"),
    ("zahlavi", "Záhlaví pásů"),
    ("hodnota", "Hodnoty v pásech"),
    ("usek", "Texty úseků"),
    ("graf", "Popisy v grafu"),
    ("legenda", "Legenda a měřítko"),
];

/// Čáry profilu: klíč (bez předpony `c.`) a český název.
pub const CARY: [(&str, &str); 6] = [
    ("teren", "Terén (povrch pohledu)"),
    ("niveleta", "Niveleta"),
    ("polygon", "Tečnový polygon nivelety"),
    ("povrchy", "Další povrchy"),
    ("pasy", "Čáry pásů"),
    ("mrizka", "Mřížka a svislice"),
];

/// Tři tloušťky čar, na které čáry odkazují.
pub const TLOUSTKY: [(&str, &str); 3] = [("tenka", "Tenká"), ("stredni", "Střední"), ("silna", "Silná")];

/// Písma k výběru: rodina, soubor obyčejného a tučného řezu (Windows).
pub const FONTY: [(&str, &str, &str); 7] = [
    ("Bahnschrift", "bahnschrift.ttf", "bahnschrift.ttf"),
    ("Arial", "arial.ttf", "arialbd.ttf"),
    ("Arial Narrow", "arialn.ttf", "arialnb.ttf"),
    ("Segoe UI", "segoeui.ttf", "segoeuib.ttf"),
    ("Calibri", "calibri.ttf", "calibrib.ttf"),
    ("ISOCPEUR", "isocpeur.ttf", "isocpeur.ttf"),
    ("Consolas", "consola.ttf", "consolab.ttf"),
];

/// Soubor písma pro textový styl výkresu.
pub fn soubor_fontu(font: &str, tucne: bool) -> String {
    match FONTY.iter().find(|f| f.0.eq_ignore_ascii_case(font)) {
        Some(f) => (if tucne { f.2 } else { f.1 }).to_string(),
        None => format!("{}.ttf", font.trim()),
    }
}

/// Jméno textového stylu výkresu pro písmo (sdílené všemi profily).
pub fn styl_textu(font: &str, tucne: bool) -> String {
    let jmeno: String = font.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase();
    format!("TER_PP_{}{}", if jmeno.is_empty() { "PISMO".into() } else { jmeno }, if tucne { "_B" } else { "" })
}

/// Barva `#rrggbb`; prázdná = podle hladiny.
pub fn barva(t: &str) -> Option<[u8; 3]> {
    let t = t.trim().strip_prefix('#')?;
    if t.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(t, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

pub fn hex(b: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
}

fn cislo(t: &str, vychozi: f64) -> f64 {
    t.trim().replace(',', ".").parse().ok().filter(|x: &f64| x.is_finite()).unwrap_or(vychozi)
}

fn f(v: f64) -> String {
    let t = format!("{v:.3}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Styl písma: rodina, výška [mm na papíře], šířka, tučné, kurzíva, barva.
#[derive(Clone, Debug, PartialEq)]
pub struct Pismo {
    pub font: String,
    pub vyska: f64,
    pub sirka: f64,
    pub tucne: bool,
    pub kurziva: bool,
    /// `None` = podle barvy textů profilu.
    pub barva: Option<[u8; 3]>,
}

impl Pismo {
    /// Z textu `písmo;výška;šířka;tučné;kurzíva;barva`.
    pub fn cti(t: &str) -> Self {
        let c: Vec<&str> = t.split(';').collect();
        let g = |i: usize| c.get(i).copied().unwrap_or("");
        Self {
            font: if g(0).trim().is_empty() { "Arial".into() } else { g(0).trim().into() },
            vyska: cislo(g(1), 2.5).clamp(0.5, 20.0),
            sirka: cislo(g(2), 1.0).clamp(0.3, 3.0),
            tucne: g(3) == "1",
            kurziva: g(4) == "1",
            barva: barva(g(5)),
        }
    }

    pub fn zapis(&self) -> String {
        format!(
            "{};{};{};{};{};{}",
            self.font,
            f(self.vyska),
            f(self.sirka),
            u8::from(self.tucne),
            u8::from(self.kurziva),
            self.barva.map(hex).unwrap_or_default()
        )
    }

    pub fn styl_textu(&self) -> String {
        styl_textu(&self.font, self.tucne)
    }
}

/// Čára: barva (`None` = podle hladiny) a tloušťka (odkaz na `TLOUSTKY`).
#[derive(Clone, Debug, PartialEq)]
pub struct Cara {
    pub barva: Option<[u8; 3]>,
    pub tloustka: String,
}

impl Cara {
    pub fn cti(t: &str) -> Self {
        let (b, tl) = t.split_once(';').unwrap_or((t, "stredni"));
        let tl = if TLOUSTKY.iter().any(|x| x.0 == tl.trim()) { tl.trim().to_string() } else { "stredni".into() };
        Self { barva: barva(b), tloustka: tl }
    }
    pub fn zapis(&self) -> String {
        format!("{};{}", self.barva.map(hex).unwrap_or_default(), self.tloustka)
    }
}

/// Předlohy vzhledu (mění písma, čáry a graf; pásy nechají).
pub const PREDLOHY: [(&str, &str, &str); 4] = [
    ("moderni", "Moderní", "písmo Bahnschrift, jen svislice, podbarvené záhlaví, tónovaný terén"),
    ("klasicky", "Klasický", "Arial, rámečky pásů, mřížka a stupnice na obou stranách"),
    ("kompaktni", "Kompaktní", "úzké písmo, nižší pásy, bez stupnice"),
    ("vykresovy", "Výkresový", "ISOCPEUR, záhlaví velkými písmeny bez rámečků, hodnoty u svislic, textové pásy v buňkách"),
];

/// Hodnoty klíčů, které předloha nastaví.
pub fn predloha(id: &str) -> Vec<(&'static str, &'static str)> {
    let mut v = vec![("predloha", "")];
    v.extend_from_slice(match id {
        "klasicky" => &[
            ("predloha", "klasicky"),
            ("p.nadpis", "Arial;5;1;1;0;"),
            ("p.zahlavi", "Arial;2.5;1;0;0;"),
            ("p.hodnota", "Arial;2;1;0;0;"),
            ("p.usek", "Arial;2;1;0;0;"),
            ("p.graf", "Arial;2;1;0;0;"),
            ("p.legenda", "Arial;2;1;0;0;"),
            ("tl.tenka", "0.18"),
            ("tl.stredni", "0.25"),
            ("tl.silna", "0.5"),
            ("cary_pasu", "ramecek"),
            ("mrizka", "1"),
            ("ram", "1"),
            ("stupnice", "obe"),
            ("podbarvit_zahlavi", "0"),
            ("vypln_terenu", "zadna"),
            ("legenda", "0"),
            ("stitek", "0"),
            ("svislice", "0"),
            ("hustota", "1"),
        ][..],
        "vykresovy" => &[
            ("predloha", "vykresovy"),
            ("p.nadpis", "ISOCPEUR;3.5;1;0;0;#e00000"),
            ("p.zahlavi", "ISOCPEUR;2.5;1;0;0;"),
            ("p.hodnota", "ISOCPEUR;2.5;1;0;0;"),
            ("p.usek", "ISOCPEUR;2.5;1;0;0;"),
            ("p.graf", "ISOCPEUR;2.5;1;0;0;"),
            ("p.legenda", "ISOCPEUR;2.5;1;0;0;"),
            ("tl.tenka", "0.13"),
            ("tl.stredni", "0.25"),
            ("tl.silna", "0.5"),
            ("c.teren", "#2fb82f;stredni"),
            ("c.povrchy", "#e040e0;stredni"),
            ("c.niveleta", "#ff2020;silna"),
            // podle hladiny: bílé na obrazovce, černé při tisku
            ("c.pasy", ";tenka"),
            ("c.mrizka", ";tenka"),
            ("cary_pasu", "vykres"),
            // čísla v ose staničení, svislice jen v grafu
            ("svislice_pasu", "0"),
            ("mrizka", "0"),
            ("ram", "0"),
            ("stupnice", "skryt"),
            ("podbarvit_zahlavi", "0"),
            ("vypln_terenu", "zadna"),
            ("legenda", "1"),
            ("stitek", "0"),
            ("svislice", "1"),
            ("hustota", "1"),
            ("sirka_zahlavi", "75"),
        ][..],
        "kompaktni" => &[
            ("predloha", "kompaktni"),
            ("p.nadpis", "Arial Narrow;4;1;1;0;"),
            ("p.zahlavi", "Arial Narrow;2;1;0;0;"),
            ("p.hodnota", "Arial Narrow;1.8;1;0;0;"),
            ("p.usek", "Arial Narrow;1.8;1;0;0;"),
            ("p.graf", "Arial Narrow;1.8;1;0;0;"),
            ("p.legenda", "Arial Narrow;1.8;1;0;0;"),
            ("tl.tenka", "0.13"),
            ("tl.stredni", "0.18"),
            ("tl.silna", "0.35"),
            ("cary_pasu", "svislice"),
            ("mrizka", "0"),
            ("ram", "0"),
            ("stupnice", "skryt"),
            ("podbarvit_zahlavi", "1"),
            ("vypln_terenu", "zadna"),
            ("legenda", "0"),
            ("stitek", "1"),
            ("svislice", "0"),
            ("hustota", "0.8"),
        ][..],
        _ => &[
            ("predloha", "moderni"),
            ("p.nadpis", "Bahnschrift;5;1;1;0;"),
            ("p.zahlavi", "Bahnschrift;2.5;1;0;0;"),
            ("p.hodnota", "Bahnschrift;2;0.9;0;0;"),
            ("p.usek", "Bahnschrift;2;1;0;0;"),
            ("p.graf", "Bahnschrift;2;1;1;0;"),
            ("p.legenda", "Bahnschrift;2;1;0;0;"),
            ("tl.tenka", "0.13"),
            ("tl.stredni", "0.25"),
            ("tl.silna", "0.5"),
            ("cary_pasu", "svislice"),
            ("mrizka", "0"),
            ("ram", "0"),
            ("stupnice", "vlevo"),
            ("podbarvit_zahlavi", "1"),
            ("vypln_terenu", "plna"),
            ("legenda", "1"),
            ("stitek", "1"),
            ("svislice", "1"),
            ("hustota", "1"),
        ][..],
    });
    v.remove(0);
    v
}

/// Výsledný vzhled pohledu z nastavení.
#[derive(Clone, Debug, PartialEq)]
pub struct Vzhled {
    pub pisma: Vec<(String, Pismo)>,
    pub cary: Vec<(String, Cara)>,
    /// Tloušťky tenká, střední, silná [mm].
    pub tloustky: [f64; 3],
    /// Barva textů bez vlastní barvy (`None` = podle hladiny).
    pub barva_textu: Option<[u8; 3]>,
    pub podbarvit_zahlavi: bool,
    /// `plna` (tónování) / `zadna`.
    pub vypln_terenu: String,
    pub svislice: bool,
    pub legenda: bool,
    pub stitek: bool,
    pub sirka_zahlavi: f64,
    pub hustota: f64,
    pub nadpis: String,
}

impl Vzhled {
    pub fn z(n: &Hodnoty) -> Self {
        Self {
            pisma: ROLE.iter().map(|(k, _)| (k.to_string(), Pismo::cti(n.text(&format!("p.{k}"))))).collect(),
            cary: CARY.iter().map(|(k, _)| (k.to_string(), Cara::cti(n.text(&format!("c.{k}"))))).collect(),
            tloustky: [
                n.cislo("tl.tenka").clamp(0.0, 2.11),
                n.cislo("tl.stredni").clamp(0.0, 2.11),
                n.cislo("tl.silna").clamp(0.0, 2.11),
            ],
            barva_textu: barva(n.text("c.text")),
            podbarvit_zahlavi: n.ano("podbarvit_zahlavi"),
            vypln_terenu: n.text("vypln_terenu").to_string(),
            svislice: n.ano("svislice"),
            legenda: n.ano("legenda"),
            stitek: n.ano("stitek"),
            sirka_zahlavi: n.cislo("sirka_zahlavi").clamp(10.0, 200.0),
            hustota: n.cislo("hustota").clamp(0.3, 3.0),
            nadpis: n.text("nadpis").to_string(),
        }
    }

    pub fn pismo(&self, role: &str) -> &Pismo {
        self.pisma.iter().find(|p| p.0 == role).or_else(|| self.pisma.iter().find(|p| p.0 == "hodnota")).map(|p| &p.1).expect("role písma") // i18n:ne
    }

    pub fn cara(&self, k: &str) -> &Cara {
        self.cary.iter().find(|c| c.0 == k).map(|c| &c.1).expect("čára") // i18n:ne
    }

    /// Tloušťka [mm] podle odkazu.
    pub fn tloustka(&self, t: &str) -> f64 {
        match t {
            "tenka" => self.tloustky[0],
            "silna" => self.tloustky[2],
            _ => self.tloustky[1],
        }
    }
}

/// Nadpis s doplněnými zástupnými texty `{trasa}`, `{povrch}`, `{meritko}`.
pub fn dosad_nadpis(sablona: &str, trasa: &str, povrch: &str, meritko: &str) -> String {
    sablona.replace("{trasa}", trasa).replace("{povrch}", povrch).replace("{meritko}", meritko).replace("{měřítko}", meritko) // i18n:ne
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn pismo_a_cara() {
        let p = Pismo::cti("Bahnschrift;2.2;0.9;1;0;#ff0000");
        assert_eq!((p.vyska, p.sirka, p.tucne, p.barva), (2.2, 0.9, true, Some([255, 0, 0])));
        assert_eq!(Pismo::cti(&p.zapis()), p);
        assert_eq!(p.styl_textu(), "TER_PP_BAHNSCHRIFT_B");
        assert_eq!(soubor_fontu("Arial", true), "arialbd.ttf");
        let c = Cara::cti("#5fd35f;silna");
        assert_eq!(Cara::cti(&c.zapis()), c);
        assert_eq!(Cara::cti(";xx").tloustka, "stredni");
        assert_eq!(Cara::cti(";tenka").barva, None);
    }

    #[test]
    fn predlohy() {
        for (id, _, _) in PREDLOHY {
            let v = predloha(id);
            assert!(v.iter().any(|(k, h)| *k == "predloha" && *h == id));
            assert!(v.iter().filter(|(k, _)| k.starts_with("p.")).count() == ROLE.len());
        }
    }
}
