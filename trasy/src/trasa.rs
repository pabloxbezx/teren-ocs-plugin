//! Definice trasy, jak se ukládá do výkresu, a styly tras.

use std::collections::BTreeMap;

use crate::geometrie::Vrchol;
use crate::niveleta::Niveleta;
use crate::styl::{Klic, Styl};

/// Verze zápisu definice.
pub const VERZE: &str = "TRASY1";

#[derive(Clone, Debug, PartialEq)]
pub struct DefiniceTrasy {
    /// Identifikátor bez diakritiky (část názvu bloku).
    pub id: String,
    pub jmeno: String,
    /// Počáteční staničení [m].
    pub pocatek: f64,
    pub styl: String,
    /// Výjimky objektu proti stylu.
    pub vyjimky: BTreeMap<String, String>,
    /// Vrcholy tečnového polygonu v souřadnicích bloku.
    pub vrcholy: Vec<Vrchol>,
    /// Základní bod bloku (vložení s nulovým posunem a otočením leží tady).
    pub zaklad: [f64; 2],
    /// Rovnice staničení (staničení zpět, staničení vpřed). Zatím se jen
    /// ukládá – výpočet a UI přijdou později.
    pub rovnice: Vec<(f64, f64)>,
    /// Nivelety (návrhové profily) trasy.
    pub nivelety: Vec<Niveleta>,
}

fn f(v: f64) -> String {
    // dost míst, aby se poloha neposouvala opakovaným ukládáním
    let t = format!("{v:.6}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() || t == "-" { "0".into() } else { t.into() }
}

fn cti(t: &str) -> Option<f64> {
    t.trim().replace(',', ".").parse().ok()
}

impl DefiniceTrasy {
    pub fn nova(id: &str, jmeno: &str, vrcholy: Vec<Vrchol>, styl: &str) -> Self {
        let zaklad = vrcholy.first().map_or([0.0, 0.0], |v| [v.x, v.y]);
        Self {
            id: id.into(),
            jmeno: jmeno.into(),
            pocatek: 0.0,
            styl: styl.into(),
            vyjimky: BTreeMap::new(),
            vrcholy,
            zaklad,
            rovnice: Vec::new(),
            nivelety: Vec::new(),
        }
    }

    /// Řádky pro uložení (každý kratší než 255 znaků – limit XDATA).
    pub fn zapis(&self) -> Vec<String> {
        let mut r = vec![VERZE.to_string()];
        r.push(format!("id={}", self.id));
        r.push(format!("jmeno={}", self.jmeno.chars().take(200).collect::<String>()));
        r.push(format!("pocatek={}", f(self.pocatek)));
        r.push(format!("styl={}", self.styl));
        r.push(format!("zaklad={};{}", f(self.zaklad[0]), f(self.zaklad[1])));
        for v in &self.vrcholy {
            // přechodnice jen když jsou (starší verze čtou jen x;y;r)
            if v.l1 > 0.0 || v.l2 > 0.0 {
                r.push(format!("v={};{};{};{};{}", f(v.x), f(v.y), f(v.r), f(v.l1), f(v.l2)));
            } else {
                r.push(format!("v={};{};{}", f(v.x), f(v.y), f(v.r)));
            }
        }
        for (a, b) in &self.rovnice {
            r.push(format!("rovnice={};{}", f(*a), f(*b)));
        }
        for (k, v) in &self.vyjimky {
            r.push(format!("x.{k}={}", v.chars().take(200).collect::<String>()));
        }
        for n in &self.nivelety {
            r.extend(n.zapis());
        }
        r
    }

    /// Načte definici z řádků; `None`, když to není definice trasy.
    pub fn cti(radky: &[&str]) -> Option<Self> {
        if radky.first().copied() != Some(VERZE) {
            return None;
        }
        let mut d = DefiniceTrasy::nova("", "", Vec::new(), "");
        let mut zaklad = None;
        for r in &radky[1..] {
            let Some((k, h)) = r.split_once('=') else { continue };
            match k {
                "id" => d.id = h.into(),
                "jmeno" => d.jmeno = h.into(),
                "pocatek" => d.pocatek = cti(h).unwrap_or(0.0),
                "styl" => d.styl = h.into(),
                "zaklad" => {
                    let c: Vec<f64> = h.split(';').filter_map(cti).collect();
                    if c.len() == 2 {
                        zaklad = Some([c[0], c[1]]);
                    }
                }
                "v" => {
                    let c: Vec<f64> = h.split(';').filter_map(cti).collect();
                    if c.len() >= 2 {
                        let g = |i: usize| c.get(i).copied().unwrap_or(0.0);
                        d.vrcholy.push(Vrchol::s_prechodnicemi(c[0], c[1], g(2), g(3), g(4)));
                    }
                }
                "rovnice" => {
                    let c: Vec<f64> = h.split(';').filter_map(cti).collect();
                    if c.len() == 2 {
                        d.rovnice.push((c[0], c[1]));
                    }
                }
                k if k.starts_with("x.") => {
                    d.vyjimky.insert(k[2..].to_string(), h.to_string());
                }
                _ => {} // neznámé klíče novějších verzí se přeskočí
            }
        }
        if d.id.is_empty() || d.vrcholy.len() < 2 {
            return None;
        }
        d.nivelety = Niveleta::cti_vse(&radky[1..]);
        d.zaklad = zaklad.unwrap_or([d.vrcholy[0].x, d.vrcholy[0].y]);
        Some(d)
    }

    /// Vrcholy ve světových souřadnicích při daném vložení bloku
    /// (bod vložení, otočení; měřítko vložení se neuvažuje).
    pub fn svetove(&self, vlozeni: [f64; 2], otoceni: f64) -> Vec<Vrchol> {
        let (s, c) = otoceni.sin_cos();
        self.vrcholy
            .iter()
            .map(|v| {
                let (dx, dy) = (v.x - self.zaklad[0], v.y - self.zaklad[1]);
                Vrchol::s_prechodnicemi(vlozeni[0] + dx * c - dy * s, vlozeni[1] + dx * s + dy * c, v.r, v.l1, v.l2)
            })
            .collect()
    }
}

// ── Styl trasy ──────────────────────────────────────────────────────────────

/// Klíče stylu trasy (výchozí hodnoty = základní styl).
pub const SCHEMA: &[Klic] = &[
    Klic { klic: "meritko", vychozi: "500", popis: "Měřítko výkresu 1:" },
    Klic { klic: "vyska_textu", vychozi: "2", popis: "Výška textu [mm na papíře]" },
    Klic { klic: "krok_hlavni", vychozi: "100", popis: "Krok hlavního staničení [m]" },
    Klic { klic: "krok_vedlejsi", vychozi: "20", popis: "Krok vedlejšího staničení [m] (0 = nekreslit)" },
    Klic { klic: "format", vychozi: "km", popis: "Formát staničení" },
    Klic { klic: "desetin_hlavni", vychozi: "0", popis: "Desetinná místa hlavního staničení" },
    Klic { klic: "desetin_body", vychozi: "2", popis: "Desetinná místa u hlavních bodů" },
    Klic { klic: "carka", vychozi: "1", popis: "Desetinná čárka" },
    Klic { klic: "popis_body", vychozi: "1", popis: "Popisovat hlavní body (ZÚ, KÚ, TK, KT, TP, PK, KP, PT)" },
    Klic { klic: "popis_vrcholy", vychozi: "0", popis: "Popisovat vrcholy (V) a poloměry" },
    Klic { klic: "zkratky", vychozi: "ZÚ,KÚ,TK,KT,V,TP,PK,KP,PT", popis: "Zkratky bodů ZÚ,KÚ,TK,KT,V,TP,PK,KP,PT" }, // i18n:ne
    Klic { klic: "polygon", vychozi: "1", popis: "Kreslit tečnový polygon" },
    // barva = index ACI 1–255, 256 = podle hladiny; tloušťka v setinách mm, -1 = podle hladiny
    Klic { klic: "barva_osa", vychozi: "256", popis: "Barva osy" },
    Klic { klic: "tloustka_osa", vychozi: "-1", popis: "Tloušťka osy" },
    Klic { klic: "barva_staniceni", vychozi: "256", popis: "Barva staničení" },
    Klic { klic: "tloustka_staniceni", vychozi: "-1", popis: "Tloušťka staničení" },
    Klic { klic: "barva_body", vychozi: "256", popis: "Barva hlavních bodů" },
    Klic { klic: "tloustka_body", vychozi: "-1", popis: "Tloušťka hlavních bodů" },
    Klic { klic: "barva_polygon", vychozi: "256", popis: "Barva tečnového polygonu" },
    Klic { klic: "tloustka_polygon", vychozi: "-1", popis: "Tloušťka tečnového polygonu" },
    Klic { klic: "typ_osa", vychozi: "plna", popis: "Typ čáry osy (plna / cerchovana)" },
    // 0 = jako osa
    Klic { klic: "barva_oblouk", vychozi: "0", popis: "Barva oblouků osy (0 = jako osa)" },
    Klic { klic: "polomery", vychozi: "0", popis: "Rozkreslit oblouky (poloměry ke středu)" },
    Klic { klic: "netisk", vychozi: "0", popis: "Netisknout (hladina, která se netiskne)" },
];

/// Styl nové trasy.
pub const STYL_ZAKLAD: &str = "CERVENA_PLNA";
/// Základ všech vestavěných stylů (situace 1:500).
const ZAKLAD: &str = "SITUACE_500";

/// Dřívější vestavěné styly: v nabídkách už nejsou (pokud je trasa ve výkrese
/// nepoužívá), trasy ze starších výkresů s nimi dál fungují.
pub const SKRYTE_STYLY: [&str; 4] = ["SITUACE_500", "SITUACE_250", "SITUACE_1000", "JEN_OSA"];

pub fn je_skryty(id: &str) -> bool {
    SKRYTE_STYLY.contains(&id)
}

/// Vestavěné styly tras (v kódu; vlastní styly jsou ve výkrese).
pub fn vestavene_styly() -> Vec<Styl> {
    vec![
        Styl::odvozeny(
            "NETISK",
            "Netisk (šedá)",
            ZAKLAD,
            &[("barva_osa", "8"), ("barva_staniceni", "8"), ("barva_body", "8"), ("barva_polygon", "8"), ("netisk", "1")],
        ),
        Styl::odvozeny("CERVENA_CERCHOVANA", "Červená čerchovaná", ZAKLAD, &[("barva_osa", "1"), ("typ_osa", "cerchovana")]),
        Styl::odvozeny(STYL_ZAKLAD, "Červená plná", ZAKLAD, &[("barva_osa", "1")]),
        Styl::odvozeny(
            "PRIMKY_OBLOUKY",
            "Přímé červeně, oblouky modře",
            ZAKLAD,
            &[("barva_osa", "1"), ("barva_oblouk", "5"), ("barva_polygon", "8"), ("polygon", "1"), ("polomery", "1")],
        ),
        // dřívější (skryté)
        Styl::novy(ZAKLAD, "Situace 1:500"),
        Styl::odvozeny("SITUACE_250", "Situace 1:250", ZAKLAD, &[("meritko", "250"), ("krok_vedlejsi", "10")]),
        Styl::odvozeny("SITUACE_1000", "Situace 1:1000", ZAKLAD, &[("meritko", "1000"), ("krok_vedlejsi", "50")]),
        Styl::odvozeny(
            "JEN_OSA",
            "Jen osa bez popisů",
            ZAKLAD,
            &[("krok_hlavni", "0"), ("krok_vedlejsi", "0"), ("popis_body", "0"), ("polygon", "0")],
        ),
    ]
}

/// Vestavěné styly bez dědění: každý nese všechny hodnoty odlišné od
/// schématu (tak se ukládají do výkresu a upravují v Prospektoru).
pub fn vestavene_ploche() -> Vec<Styl> {
    let styly = vestavene_styly();
    styly.iter().map(|s| plochy(s, &styly)).collect()
}

/// Styl bez rodiče s výslednými hodnotami odlišnými od schématu.
pub fn plochy(s: &Styl, styly: &[Styl]) -> Styl {
    let h = crate::styl::vysledne(SCHEMA, styly, &s.id, &BTreeMap::new());
    Styl {
        id: s.id.clone(),
        nazev: s.nazev.clone(),
        rodic: None,
        hodnoty: SCHEMA.iter().filter(|k| h.text(k.klic) != k.vychozi).map(|k| (k.klic.to_string(), h.text(k.klic).to_string())).collect(),
    }
}

pub fn je_vestaveny(id: &str) -> bool {
    vestavene_styly().iter().any(|s| s.id == id)
}

/// Řádky stylu trasy pro uložení do výkresu.
pub fn zapis_stylu(s: &Styl) -> Vec<String> {
    let mut r = vec![format!("id={}", s.id), format!("jmeno={}", s.nazev)];
    for (k, v) in &s.hodnoty {
        r.push(format!("k.{k}={v}"));
    }
    r
}

pub fn precti_styl<S: AsRef<str>>(radky: &[S]) -> Option<Styl> {
    let mut s = Styl::default();
    for r in radky {
        let Some((k, h)) = r.as_ref().split_once('=') else { continue };
        match k {
            "id" => s.id = h.into(),
            "jmeno" => s.nazev = h.into(),
            k if k.starts_with("k.") => {
                s.hodnoty.insert(k[2..].to_string(), h.to_string());
            }
            _ => {}
        }
    }
    (!s.id.is_empty()).then_some(s)
}

/// Další volné id trasy („T1“, „T2“…).
pub fn volne_id<'a>(obsazena: impl IntoIterator<Item = &'a str>) -> String {
    let o: Vec<&str> = obsazena.into_iter().collect();
    (1..).map(|i| format!("T{i}")).find(|k| !o.iter().any(|x| x.eq_ignore_ascii_case(k))).unwrap_or_default()
}

#[cfg(test)]
mod testy {
    use super::*;
    use crate::styl::vysledne;

    #[test]
    fn zapis_a_cteni() {
        let mut d = DefiniceTrasy::nova(
            "T1",
            "Ulice Nová = hlavní",
            vec![
                Vrchol::new(-740123.456, -1045678.9, 0.0),
                Vrchol::new(-740000.0, -1045600.0, 150.0),
                Vrchol::s_prechodnicemi(-739900.0, -1045700.0, 80.0, 20.5, 35.0),
                Vrchol::new(-739800.0, -1045650.0, 0.0),
            ],
            STYL_ZAKLAD,
        );
        d.pocatek = 120.5;
        d.vyjimky.insert("krok_vedlejsi".into(), "10".into());
        d.rovnice.push((300.0, 310.0));
        d.nivelety.push(crate::niveleta::Niveleta::nova("N1", "Niveleta", vec![
            crate::niveleta::VrcholNivelety::new(0.0, 250.0, 0.0),
            crate::niveleta::VrcholNivelety::new(120.0, 251.5, 3000.0),
            crate::niveleta::VrcholNivelety::new(300.0, 249.0, 0.0),
        ]));
        let r = d.zapis();
        assert!(r.iter().all(|x| x.len() < 255));
        let rr: Vec<&str> = r.iter().map(String::as_str).collect();
        assert_eq!(DefiniceTrasy::cti(&rr), Some(d));
    }

    #[test]
    fn cizi_zaznam() {
        assert_eq!(DefiniceTrasy::cti(&["TEREN2", "id=X"]), None);
    }

    #[test]
    fn posun_a_otoceni() {
        let d = DefiniceTrasy::nova("T1", "A", vec![Vrchol::new(10.0, 0.0, 0.0), Vrchol::new(20.0, 0.0, 0.0)], STYL_ZAKLAD);
        let w = d.svetove([110.0, 50.0], std::f64::consts::FRAC_PI_2);
        assert!((w[0].x - 110.0).abs() < 1e-9 && (w[0].y - 50.0).abs() < 1e-9);
        assert!((w[1].x - 110.0).abs() < 1e-9 && (w[1].y - 60.0).abs() < 1e-9);
    }

    #[test]
    fn odvozeny_styl() {
        let h = vysledne(SCHEMA, &vestavene_styly(), "SITUACE_1000", &BTreeMap::new());
        assert_eq!(h.cislo("meritko"), 1000.0);
        assert_eq!(h.cislo("krok_hlavni"), 100.0);
    }

    #[test]
    fn styl_zapis_a_ploche() {
        let p = vestavene_ploche();
        let s = p.iter().find(|s| s.id == "JEN_OSA").unwrap();
        assert!(s.rodic.is_none());
        assert_eq!(s.hodnoty.get("polygon").map(String::as_str), Some("0"));
        let mut s = s.clone();
        s.hodnoty.insert("barva_osa".into(), "3".into());
        assert_eq!(precti_styl(&zapis_stylu(&s)), Some(s.clone()));
        assert_eq!(vysledne(SCHEMA, &[s], "JEN_OSA", &BTreeMap::new()).cislo("barva_osa"), 3.0);
    }

    #[test]
    fn id() {
        assert_eq!(volne_id(["T1", "t2"]), "T3");
    }
}
