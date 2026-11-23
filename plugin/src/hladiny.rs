//! Názvy hladin pluginu.
//!
//! Hladiny jsou anglicky ve tvaru `TER-<druh>-<prvek>` (PNT body, SRF povrch,
//! ALN trasa, PRF podélný profil), aby jim rozuměli i uživatelé jinde. Popis
//! hladiny je česky a v anglickém Open CAD Studiu se přeloží (`tr`).
//!
//! Výkresy ze starších verzí mají hladiny `TER_…` / `TRA_…` – při první
//! práci s výkresem se přejmenují ([`prejmenuj_stare`]); objekty na nich
//! zůstanou.

use ocs_plugin_api::host::codec::types::Color;
use ocs_plugin_api::host::{Document as CadDocument, LayerConfig, TableOperation};
use crate::lang::formatuj;
use teren::trf;

use crate::vykres::Vykres;

/// Předpona všech hladin pluginu (import a výběr značek je přeskakují).
pub const PREDPONA: &str = "TER-";

/// Je hladina pluginu (nová i stará jména)?
pub fn je_pluginu(h: &str) -> bool {
    let h = h.to_ascii_uppercase();
    h.starts_with(PREDPONA) || h.starts_with("TER_") || h.starts_with("TRA_")
}

/// Předpona hladin povrchu (`TER-SRF-<id>`).
pub fn povrch(id: &str) -> String {
    format!("TER-SRF-{id}")
}

/// Prvky povrchu: (přípona nové hladiny, přípona staré hladiny, popis).
const PRVKY_POVRCHU: [(&str, &str, &str); 10] = [
    ("TIN", "TIN", "Povrch {} – trojúhelníková síť"),
    ("CONTOUR-MAJOR", "VRST_HLAVNI", "Povrch {} – hlavní vrstevnice"),
    ("CONTOUR-MINOR", "VRST_VEDLEJSI", "Povrch {} – vedlejší vrstevnice"),
    ("OUTLINE", "OBRYS", "Povrch {} – obrys"),
    ("LABEL", "POPIS", "Povrch {} – popis vrstevnic"),
    ("BREAKLINE", "SPOJNICE", "Povrch {} – spojnice (lomové hrany)"),
    ("BOUNDARY", "HRANICE", "Povrch {} – hranice"),
    ("HOLE", "DIRY", "Povrch {} – díry"),
    ("EDIT", "UPRAVY", "Povrch {} – úpravy sítě"),
    ("SLOPE", "SPAD", "Povrch {} – šipky spádu"),
];

/// Pevné hladiny: (nový název, starý název, popis).
const PEVNE: [(&str, &str, &str); 27] = [
    ("TER-PNT", "TER_BODY", "Body (plugin Terén)"),
    ("TER-PNT-HIDDEN", "TER_BODY_SKRYTE", "Skryté body (styl Nezobrazovat)"),
    ("TER-PNT-NUMBER", "TER_BODY_CISLO", "Čísla bodů"),
    ("TER-PNT-ELEVATION", "TER_BODY_VYSKA", "Výšky bodů"),
    ("TER-PNT-DESCRIPTION", "TER_BODY_POPIS", "Popisy (kódy) bodů"),
    ("TER-PNT-SYMBOL", "TER_BODY_ZNACKA", "Značky bodů"),
    ("TER-PNT-GROUPS", "TER_SKUPINY", "Definice skupin bodů (plugin Terén) – nemazat"),
    ("TER-STYLES", "TER_STYLY", "Styly bodů, povrchů a profilů (plugin Terén) – nemazat"),
    ("TER-CHECK", "TER_KONTROLA", "Značky kontroly zaměření (plugin Terén)"),
    ("TER-SRF-HIDDEN", "TER_POVRCH_SKRYTY", "Skryté povrchy (styl Nezobrazovat)"),
    ("TER-ALN", "TRA_TRASY", "Trasy (plugin Terén)"),
    ("TER-ALN-CENTERLINE", "TRA_OSA", "Trasa – osa"),
    ("TER-ALN-STATIONING", "TRA_STANICENI", "Trasa – staničení"),
    ("TER-ALN-KEY-POINTS", "TRA_HLAVNI_BODY", "Trasa – hlavní body"),
    ("TER-ALN-TANGENTS", "TRA_POLYGON", "Trasa – tečnový polygon"),
    ("TER-ALN-NOPLOT", "TRA_NETISK", "Trasy – netiskne se"),
    ("TER-PRF", "TRA_POHLED", "Podélné profily (plugin Terén)"),
    ("TER-PRF-FRAME", "TRA_POHLED_RAM", "Podélný profil – rám"),
    ("TER-PRF-GRID", "TRA_POHLED_MRIZKA", "Podélný profil – mřížka"),
    ("TER-PRF-LABEL", "TRA_POHLED_POPIS", "Podélný profil – popisy"),
    ("TER-PRF-GROUND", "TRA_PROFIL_TEREN", "Podélný profil – terén"),
    ("TER-PRF-BANDS", "TRA_POHLED_PASY", "Podélný profil – pásy"),
    ("TER-PRF-GRADE", "TRA_PROFIL_NIVELETA", "Podélný profil – niveleta"),
    ("TER-PRF-GRADE-TANGENTS", "TRA_PROFIL_NIVELETA_POLYGON", "Podélný profil – tečnový polygon nivelety"),
    // hladiny exportu do DXF (soubor mimo výkres)
    ("TER-SRF-TIN", "TER_TIN", "Povrch – trojúhelníková síť"),
    ("TER-SRF-CONTOUR-MAJOR", "TER_VRSTEVNICE_HLAVNI", "Povrch – hlavní vrstevnice"),
    ("TER-SRF-CONTOUR-MINOR", "TER_VRSTEVNICE_VEDLEJSI", "Povrch – vedlejší vrstevnice"),
];

/// Česky popis hladiny pluginu (přeložený), nebo `None` u cizí hladiny.
pub fn popis(nazev: &str) -> Option<String> {
    let n = nazev.to_ascii_uppercase();
    if let Some((_, _, p)) = PEVNE.iter().find(|(h, _, _)| *h == n) {
        return Some(crate::lang::tr_tab(p).to_string());
    }
    let zbytek = n.strip_prefix("TER-SRF-")?;
    // povrch: TER-SRF-<id> nebo TER-SRF-<id>-<prvek>; id je z původního názvu
    let id_puvodni = &nazev["TER-SRF-".len()..];
    for (pripona, _, p) in PRVKY_POVRCHU {
        if let Some(id) = zbytek.strip_suffix(pripona).and_then(|s| s.strip_suffix('-')).filter(|s| !s.is_empty()) {
            return Some(formatuj(crate::lang::tr_tab(p), &[&&id_puvodni[..id.len()]]));
        }
    }
    Some(trf!("Povrch {} (plugin Terén)", "Surface {} (Terén plugin)", id_puvodni))
}

/// Konfigurace nové hladiny pluginu (barva a popis).
pub fn konfigurace(nazev: &str, barva: i16) -> LayerConfig {
    LayerConfig { name: nazev.to_string(), color: Some(Color::from_index(barva)), description: popis(nazev), ..Default::default() }
}

/// Nový název hladiny ze starší verze (`None` = není stará hladina pluginu).
/// `povrchy` jsou id povrchů výkresu (jejich hladiny měly tvar `TER_<id>_…`).
pub fn novy_nazev(stary: &str, povrchy: &[String]) -> Option<String> {
    let s = stary.to_ascii_uppercase();
    if let Some((h, _, _)) = PEVNE.iter().find(|(_, st, _)| !st.is_empty() && *st == s) {
        return Some(h.to_string());
    }
    for id in povrchy {
        let pref = format!("TER_{}", id.to_ascii_uppercase());
        let Some(zbytek) = s.strip_prefix(&pref) else { continue };
        if zbytek.is_empty() {
            return Some(povrch(id));
        }
        if let Some(prv) = zbytek.strip_prefix('_') {
            if let Some((nova, _, _)) = PRVKY_POVRCHU.iter().find(|(_, st, _)| *st == prv) {
                return Some(format!("{}-{nova}", povrch(id)));
            }
        }
    }
    None
}

/// Přejmenuje hladiny ze starších verzí na nové (a doplní jim popis).
/// Vrací, jestli se něco změnilo. Hladina se nepřejmenuje, když nová už
/// existuje (výkres by měl obě – objekty na staré zůstanou).
pub fn prejmenuj_stare(v: &mut dyn Vykres, doc: &CadDocument, povrchy: &[String]) -> bool {
    let mut zmena = false;
    for l in doc.layers.iter() {
        let Some(novy) = novy_nazev(&l.name, povrchy) else { continue };
        if doc.layers.iter().any(|x| x.name.eq_ignore_ascii_case(&novy)) {
            continue;
        }
        if v.tabulka(TableOperation::LayerRename { from: l.name.clone(), to: novy.clone() }).is_ok() {
            zmena = true;
            if let Some(p) = popis(&novy) {
                let _ = v.tabulka(TableOperation::LayerModify {
                    config: LayerConfig { name: novy, description: Some(p), ..Default::default() },
                });
            }
        }
    }
    zmena
}

/// Má výkres nějakou hladinu ze starší verze? (rychlá kontrola bez zápisu)
pub fn ma_stare(doc: &CadDocument) -> bool {
    doc.layers.iter().any(|l| {
        let n = l.name.to_ascii_uppercase();
        n.starts_with("TER_") || n.starts_with("TRA_")
    })
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn stare_pevne_hladiny() {
        assert_eq!(novy_nazev("TER_BODY", &[]).as_deref(), Some("TER-PNT"));
        assert_eq!(novy_nazev("ter_body_vyska", &[]).as_deref(), Some("TER-PNT-ELEVATION"));
        assert_eq!(novy_nazev("TRA_PROFIL_NIVELETA_POLYGON", &[]).as_deref(), Some("TER-PRF-GRADE-TANGENTS"));
        assert_eq!(novy_nazev("TRA_OSA", &[]).as_deref(), Some("TER-ALN-CENTERLINE"));
        assert_eq!(novy_nazev("vytycovaci_body", &[]), None);
        assert_eq!(novy_nazev("TER-PNT", &[]), None);
    }

    #[test]
    fn stare_hladiny_povrchu() {
        let ids = vec!["TEREN".to_string(), "P2".to_string()];
        assert_eq!(novy_nazev("TER_TEREN", &ids).as_deref(), Some("TER-SRF-TEREN"));
        assert_eq!(novy_nazev("TER_TEREN_VRST_HLAVNI", &ids).as_deref(), Some("TER-SRF-TEREN-CONTOUR-MAJOR"));
        assert_eq!(novy_nazev("TER_P2_SPOJNICE", &ids).as_deref(), Some("TER-SRF-P2-BREAKLINE"));
        assert_eq!(novy_nazev("TER_P3_SPOJNICE", &ids), None);
        assert_eq!(novy_nazev("TER_TEREN_NECO", &ids), None);
    }

    #[test]
    fn popisy() {
        assert!(popis("TER-PNT-ELEVATION").is_some());
        assert!(popis("TER-SRF-TEREN-CONTOUR-MAJOR").unwrap().contains("TEREN"));
        assert!(popis("TER-SRF-Teren2").unwrap().contains("Teren2"));
        assert!(popis("Stávající stav").is_none());
        // každá pevná hladina má jiný starý název
        let mut stare: Vec<_> = PEVNE.iter().map(|(_, s, _)| *s).filter(|s| !s.is_empty()).collect();
        let n = stare.len();
        stare.sort();
        stare.dedup();
        assert_eq!(stare.len(), n);
    }
}
