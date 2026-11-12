//! Nastavení pluginu uložené mezi spuštěními.
//!
//! Soubor `%APPDATA%\OpenCADStudio\teren_nastaveni.txt` (na Linuxu
//! `~/.config/OpenCADStudio/…`), jednoduché řádky `klíč = hodnota`.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use teren::body::{Desetinna, Oddelovac};
use teren::kodovani::Kodovani;

/// Uložené nastavení čtení souboru bodů (šablona formátu).
#[derive(Clone, Debug, PartialEq)]
pub struct SablonaImportu {
    pub jmeno: String,
    /// Vzor sloupců (např. `PYXZD`, `P-YXZ`).
    pub vzor: String,
    pub oddelovac: Oddelovac,
    /// `None` = rozpoznat automaticky.
    pub kodovani: Option<Kodovani>,
    pub desetinna: Desetinna,
    pub hlavicka: usize,
}

impl SablonaImportu {
    fn zapis(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.jmeno.replace('|', "/"),
            self.vzor,
            nazev_odd(self.oddelovac),
            self.kodovani.map_or("auto", |k| k.klic()),
            self.desetinna.klic(),
            self.hlavicka
        )
    }
    fn precti(v: &str) -> Option<Self> {
        let c: Vec<&str> = v.split('|').map(str::trim).collect();
        if c.len() < 6 || teren::body::FormatBodu::z_retezce(c[1], Oddelovac::Mezera).is_err() {
            return None;
        }
        Some(Self {
            jmeno: c[0].to_string(),
            vzor: c[1].to_string(),
            oddelovac: Oddelovac::z_nazvu(c[2]).unwrap_or(Oddelovac::Mezera),
            kodovani: Kodovani::z_klice(c[3]),
            desetinna: Desetinna::z_klice(c[4]),
            hlavicka: c[5].parse().unwrap_or(0),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Nastaveni {
    /// `None` = automatické rozpoznání formátu.
    pub format: Option<String>,
    pub oddelovac: Oddelovac,
    /// Měřítko výkresu (jmenovatel: 500 = 1:500).
    pub meritko: f64,
    /// Kódy bodů, které se do nového povrchu neberou.
    pub vyloucene_kody: Vec<String>,
    /// Tolerance přichycení na bod u spojnic [m].
    pub tolerance_bodu: f64,
    /// Práh kontroly zaměření [m].
    pub prah_kontroly: f64,
    /// Kód (popis) nově vytvářených bodů.
    pub kod_bodu: String,
    /// Předvolená výška nově vytvářených bodů (`None` = zadat / z povrchu).
    pub vyska_bodu_nova: Option<f64>,
    /// Poloha a velikost okna Prospektoru (x, y, šířka, výška).
    pub okno: Option<[f32; 4]>,
    /// Prospektor sleduje výběr ve výkrese.
    pub sledovat_vyber: bool,
    /// Prospektor se otevírá v režimu Výkres (jinak Výběr).
    pub rezim_vykres: bool,
    /// Neukazovat postup „Jak na to“.
    pub postup_skryt: bool,
    /// Okno Prospektoru zůstává nad Open CAD Studiem.
    pub okno_navrchu: bool,
    /// Povrch nejde vybrat kliknutím (klik projde na body pod ním).
    pub povrch_nevybirat: bool,
    /// Před importem bodů ukázat náhled souboru (formát, sloupce, první body).
    pub nahled_importu: bool,
    /// Překrývající se popisy bodů automaticky odsunout (s vodicí čarou).
    pub odsazeni_popisku: bool,
    /// Posunutím popisu bodu se posune jen popis (jinak celý bod).
    pub posun_jen_popis: bool,
    /// Barvy v prohlížeči 3D: spojnice, obrys, vrstevnice, body, hrany.
    pub barvy_3d: [[u8; 3]; 5],
    /// Jazyk: „auto“ (podle systému), „cs“, „en“.
    pub jazyk: String,
    /// Vlastní šablony formátu pro import.
    pub import_sablony: Vec<SablonaImportu>,
    /// Naposledy použité nastavení čtení souboru.
    pub import_posledni: Option<SablonaImportu>,
    /// Co s duplicitními čísly (klíč `teren::duplicity::Politika`).
    pub import_duplicity: String,
    /// Po importu přiblížit na body.
    pub import_priblizit: bool,
    /// Naposledy importované soubory (nejnovější první).
    pub posledni_soubory: Vec<String>,
}

pub const BARVY_3D: [[u8; 3]; 5] = [[240, 220, 60], [90, 200, 255], [150, 80, 30], [240, 165, 60], [15, 15, 15]];

impl Default for Nastaveni {
    fn default() -> Self {
        Self {
            format: None,
            oddelovac: Oddelovac::Mezera,
            meritko: 500.0,
            vyloucene_kody: vec!["STR".into()],
            tolerance_bodu: 0.5,
            prah_kontroly: 0.5,
            kod_bodu: String::new(),
            vyska_bodu_nova: None,
            okno: None,
            sledovat_vyber: true,
            rezim_vykres: false,
            postup_skryt: false,
            okno_navrchu: false,
            povrch_nevybirat: true,
            nahled_importu: true,
            odsazeni_popisku: true,
            posun_jen_popis: false,
            barvy_3d: BARVY_3D,
            jazyk: "auto".into(),
            import_sablony: Vec::new(),
            import_posledni: None,
            import_duplicity: "preskocit".into(),
            import_priblizit: true,
            posledni_soubory: Vec::new(),
        }
    }
}

/// Měřítko výkresu: měřítko poznámek nastavené v Open CAD Studiu (stavový
/// řádek), pokud je jiné než 1:1, jinak měřítko z pásu karet Terén.
pub fn meritko(doc: &ocs_plugin_api::host::Document) -> f64 {
    meritko_z_nazvu(&doc.header.current_annotation_scale).unwrap_or_else(|| akt().meritko)
}

/// „1:500“ → 500 (1:1 a měřítka zvětšení → `None`).
pub fn meritko_z_nazvu(n: &str) -> Option<f64> {
    let (a, b) = n.split_once(':')?;
    let a: f64 = a.trim().replace(',', ".").parse().ok()?;
    let b: f64 = b.trim().replace(',', ".").parse().ok()?;
    let m = b / a;
    (a > 0.0 && m > 1.0 + 1e-9 && m.is_finite()).then_some(m)
}

fn soubor() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)?
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
    };
    Some(base.join("OpenCADStudio").join("teren_nastaveni.txt"))
}

fn nazev_odd(o: Oddelovac) -> &'static str {
    match o {
        Oddelovac::Mezera => "mezera",
        Oddelovac::Carka => "carka",
        Oddelovac::Strednik => "strednik",
        Oddelovac::Tabulator => "tab",
    }
}

fn hex(t: &str) -> Option<[u8; 3]> {
    let t = t.trim_start_matches('#');
    if t.len() != 6 {
        return None;
    }
    let k = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok();
    Some([k(0)?, k(2)?, k(4)?])
}

fn nacti() -> Nastaveni {
    let mut n = Nastaveni::default();
    let Some(text) = soubor().and_then(|p| std::fs::read_to_string(p).ok()) else { return n };
    for radek in text.lines() {
        let Some((k, v)) = radek.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "format" => n.format = (!v.is_empty() && v != "auto").then(|| v.to_string()),
            "oddelovac" => n.oddelovac = Oddelovac::z_nazvu(v).unwrap_or(Oddelovac::Mezera),
            "meritko" => n.meritko = v.parse().unwrap_or(500.0),
            "vyloucene_kody" => {
                n.vyloucene_kody = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
            }
            "tolerance_bodu" => n.tolerance_bodu = v.parse().unwrap_or(0.5),
            "prah_kontroly" => n.prah_kontroly = v.parse().unwrap_or(0.5),
            "kod_bodu" => n.kod_bodu = v.to_string(),
            "vyska_bodu_nova" => n.vyska_bodu_nova = v.parse().ok(),
            "sledovat_vyber" => n.sledovat_vyber = v != "0",
            "rezim_vykres" => n.rezim_vykres = v == "1",
            "postup_skryt" => n.postup_skryt = v == "1",
            "okno_navrchu" => n.okno_navrchu = v == "1",
            // od 1.0.4 nový klíč s výchozím „ano“ (starý „povrch_nevybirat“ se ignoruje)
            "povrch_propustit" => n.povrch_nevybirat = v != "0",
            "nahled_importu" => n.nahled_importu = v != "0",
            "odsazeni_popisku" => n.odsazeni_popisku = v != "0",
            "posun_jen_popis" => n.posun_jen_popis = v == "1",
            "jazyk" => n.jazyk = v.to_string(),
            "sablona_importu" => n.import_sablony.extend(SablonaImportu::precti(v)),
            "posledni_import" => n.import_posledni = SablonaImportu::precti(v),
            "import_duplicity" => n.import_duplicity = v.to_string(),
            "import_priblizit" => n.import_priblizit = v != "0",
            "posledni_soubor" if n.posledni_soubory.len() < POSLEDNICH => n.posledni_soubory.push(v.to_string()),
            "barvy_3d" => {
                for (i, c) in v.split(';').take(5).enumerate() {
                    if let Some(b) = hex(c.trim()) {
                        n.barvy_3d[i] = b;
                    }
                }
            }
            "okno" => {
                let c: Vec<f32> = v.split(';').filter_map(|x| x.trim().parse().ok()).collect();
                if c.len() == 4 && c[2] >= 200.0 && c[3] >= 150.0 {
                    n.okno = Some([c[0], c[1], c[2], c[3]]);
                }
            }
            _ => {}
        }
    }
    n
}

fn uloz(n: &Nastaveni) {
    let Some(p) = soubor() else { return };
    let text = format!(
        "# Nastavení pluginu Terén\nformat = {}\noddelovac = {}\nmeritko = {}\nvyloucene_kody = {}\ntolerance_bodu = {}\nprah_kontroly = {}\nkod_bodu = {}\nvyska_bodu_nova = {}\nokno = {}\nsledovat_vyber = {}\nrezim_vykres = {}\npostup_skryt = {}\npovrch_propustit = {}\nnahled_importu = {}\nodsazeni_popisku = {}\nposun_jen_popis = {}\nbarvy_3d = {}\njazyk = {}\n", // i18n:ne
        n.format.as_deref().unwrap_or("auto"),
        nazev_odd(n.oddelovac),
        n.meritko,
        n.vyloucene_kody.join(", "),
        n.tolerance_bodu,
        n.prah_kontroly,
        n.kod_bodu,
        n.vyska_bodu_nova.map(|v| v.to_string()).unwrap_or_default(),
        n.okno.map(|o| format!("{};{};{};{}", o[0], o[1], o[2], o[3])).unwrap_or_default(),
        n.sledovat_vyber as u8,
        n.rezim_vykres as u8,
        n.postup_skryt as u8,
        n.povrch_nevybirat as u8,
        n.nahled_importu as u8,
        n.odsazeni_popisku as u8,
        n.posun_jen_popis as u8,
        n.barvy_3d.iter().map(|c| format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])).collect::<Vec<_>>().join(";"),
        n.jazyk
    );
    let mut text = text;
    for s in &n.import_sablony {
        text.push_str(&format!("sablona_importu = {}
", s.zapis())); // i18n:ne
    }
    if let Some(s) = &n.import_posledni {
        text.push_str(&format!("posledni_import = {}
", s.zapis())); // i18n:ne
    }
    text.push_str(&format!("okno_navrchu = {}
", n.okno_navrchu as u8)); // i18n:ne
    text.push_str(&format!("import_duplicity = {}
import_priblizit = {}
", n.import_duplicity, n.import_priblizit as u8)); // i18n:ne
    for s in &n.posledni_soubory {
        text.push_str(&format!("posledni_soubor = {}
", s)); // i18n:ne
    }
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, text);
}

/// Kolik naposledy importovaných souborů si pamatovat.
pub const POSLEDNICH: usize = 6;

/// Zapíše soubor mezi naposledy importované.
pub fn pridej_posledni(cesta: &str) {
    if cesta.trim().is_empty() {
        return;
    }
    zmen(|n| {
        n.posledni_soubory.retain(|c| !c.eq_ignore_ascii_case(cesta));
        n.posledni_soubory.insert(0, cesta.to_string());
        n.posledni_soubory.truncate(POSLEDNICH);
    });
}

fn globalni() -> &'static Mutex<Nastaveni> {
    static N: OnceLock<Mutex<Nastaveni>> = OnceLock::new();
    N.get_or_init(|| Mutex::new(nacti()))
}

pub fn akt() -> Nastaveni {
    globalni().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn zmen(f: impl FnOnce(&mut Nastaveni)) {
    let mut g = globalni().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut g);
    uloz(&g);
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn nastaveni_importu_tam_a_zpet() {
        let s = SablonaImportu {
            jmeno: "Geodet | PYXZD".into(),
            vzor: "P-YXZD".into(),
            oddelovac: Oddelovac::Strednik,
            kodovani: Some(Kodovani::Cp1250),
            desetinna: Desetinna::Carka,
            hlavicka: 1,
        };
        let z = SablonaImportu::precti(&s.zapis()).unwrap();
        assert_eq!(z.jmeno, "Geodet / PYXZD");
        assert_eq!(SablonaImportu { jmeno: s.jmeno.clone(), ..z }, s);
        let auto = SablonaImportu { kodovani: None, ..s.clone() };
        assert_eq!(SablonaImportu::precti(&auto.zapis()).unwrap().kodovani, None);
        assert!(SablonaImportu::precti("x|QQ|mezera|auto|auto|0").is_none());
        assert!(SablonaImportu::precti("neúplné").is_none());
    }
}
