//! Import bodů: rozpoznání formátu, kontrola, náhled a potvrzení.

use teren::preklad::tr;
use teren::trf;
use ocs_plugin_api::host::{Document as CadDocument, CommandStep, EntityType, InteractiveCommand, PreviewWire};

use teren::body::{nacti_body, Bod, FormatBodu, Oddelovac};
use teren::rozpoznani::{nazev_oddelovace, rozpoznej, shodne_s_existujicimi, souhrn};
use teren::skupiny::SkupinaBodu;

use crate::cogo;
use crate::povrchy;
use crate::vykres::Vykres;

/// Jméno souboru (bez cesty) – ukládá se k bodům jako zdroj importu.
pub fn jmeno_souboru(cesta: &str) -> String {
    std::path::Path::new(cesta).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| cesta.to_string())
}

/// Skupina pro body z importovaného souboru (pokud ještě není).
pub fn skupina_importu(doc: &CadDocument, soubor: &str) -> Option<SkupinaBodu> {
    if cogo::ulozene_skupiny(doc).iter().any(|(_, s)| s.zdroje.iter().any(|z| z.eq_ignore_ascii_case(soubor))) {
        return None;
    }
    let kmen = std::path::Path::new(soubor).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let jmeno = cogo::volne_jmeno_skupiny(doc, &trf!("Import {}", kmen));
    Some(SkupinaBodu {
        id: cogo::volne_id_skupiny(doc, &jmeno),
        jmeno,
        popis: trf!("body ze souboru {}", soubor),
        zdroje: vec![soubor.to_string()],
        ..Default::default()
    })
}

/// Načte soubor a připraví body k importu. Vypíše souhrn do příkazové řádky.
/// `format` = `None` → automatické rozpoznání.
/// Text souboru bodů (UTF-8, jinak Windows-1250).
pub fn text_souboru(cesta: &str) -> Result<String, String> {
    let bajty = std::fs::read(cesta).map_err(|e| trf!("nelze číst soubor {}: {}", cesta, e))?;
    Ok(match String::from_utf8(bajty) {
        Ok(t) => t.trim_start_matches('\u{feff}').to_string(),
        Err(e) => teren::dekoduj_cp1250(e.as_bytes()),
    })
}

pub fn priprav(v: &mut dyn Vykres, cesta: &str, format: Option<(&str, Oddelovac, usize)>) -> Option<Vec<Bod>> {
    let bajty = match std::fs::read(cesta) {
        Ok(b) => b,
        Err(e) => {
            v.chyba(&trf!("Terén: nelze číst soubor {}: {}", cesta, e));
            return None;
        }
    };
    let text = match String::from_utf8(bajty) {
        Ok(t) => t.trim_start_matches('\u{feff}').to_string(),
        Err(e) => teren::dekoduj_cp1250(e.as_bytes()),
    };

    let (fmt, popis_formatu) = match format {
        Some((f, o, hl)) => match FormatBodu::z_retezce(f, o) {
            Ok(mut fb) => {
                fb.preskocit_radku = hl;
                (fb, trf!("{}, oddělovač {} (zvoleno ručně)", f, nazev_oddelovace(o)))
            }
            Err(e) => {
                v.chyba(&trf!("Terén: {}", e));
                return None;
            }
        },
        None => {
            let kandidati = rozpoznej(&text);
            let Some(k) = kandidati.first() else {
                v.chyba(tr("Terén: v souboru se nepodařilo najít body v žádném podporovaném formátu."));
                v.vystup(tr("  podporované formáty: PNEZD, PENZD, PYXZD, NEZ, ENZ, YXZ; oddělovač mezera, čárka, středník, tabulátor"));
                return None;
            };
            let mut popis = trf!("{}, oddělovač {}{} (rozpoznáno automaticky)", k.format, nazev_oddelovace(k.oddelovac), if k.hlavicka > 0 { trf!(", hlavička {} ř.", k.hlavicka) } else { String::new() });
            if !k.sjtsk() {
                popis.push_str(tr(" – body neleží v S-JTSK pro ČR, formát ověřte"));
            }
            (k.format_bodu(), popis)
        }
    };

    let import = nacti_body(&text, &fmt);
    if import.body.is_empty() {
        v.chyba(&trf!("Terén: v souboru nebyl nalezen žádný bod ({}).", popis_formatu));
        for ch in import.chyby.iter().take(5) {
            v.vystup(&format!("  {ch}"));
        }
        return None;
    }
    let s = souhrn(&import);
    let preskoceno = match import.chyby.first() {
        None => String::new(),
        Some(ch) if s.chyby == 1 => trf!(" · 1 řádek přeskočen ({})", ch),
        Some(ch) => trf!(" · {} řádků přeskočeno (první: {})", s.chyby, ch),
    };
    v.info(&trf!("Terén: import {} bodů ze souboru {} – {}{}", s.pocet, jmeno_souboru(cesta), popis_formatu, preskoceno));
    if s.mimo_cr > 0 {
        v.chyba(&trf!("  pozor: {} bodů leží mimo území ČR v S-JTSK", s.mimo_cr));
    }
    if s.podezrele_z > 0 {
        v.chyba(&trf!("  pozor: {} bodů má nesmyslnou výšku (mimo −50 až 1700 m)", s.podezrele_z));
    }
    if !s.duplicitni_cisla.is_empty() {
        let ukazka: Vec<&str> = s.duplicitni_cisla.iter().take(8).map(String::as_str).collect();
        v.vystup(&trf!("  opakující se čísla bodů ({}): {}", s.duplicitni_cisla.len(), ukazka.join(", ")));
    }
    if let Ok(doc) = v.dokument() {
        let n = shodne_s_existujicimi(&import.body, &povrchy::body_z_vykresu(&doc));
        if n > 0 {
            v.chyba(&trf!("  pozor: {} bodů už ve výkrese je na stejném místě (import dvakrát?)", n));
        }
    }
    Some(import.body)
}

/// Potvrzení importu s náhledem bodů u kurzoru.
pub struct PotvrditImport {
    pub pocet: usize,
    pub skupina: Option<String>,
    pub objekty: Option<Vec<EntityType>>,
    pub nahled: Vec<PreviewWire>,
}

/// Objekty importovaných bodů (POINT, značky a popisy, nosič skupiny).
pub fn objekty_importu(body: &[Bod], ro: &crate::styly::Rozlisovac, m: f64, soubor: &str, skupina: Option<SkupinaBodu>) -> Vec<EntityType> {
    let polozky: Vec<(Bod, crate::styly::StylBodu)> = body.iter().map(|b| (b.clone(), ro.styl(b, soubor, ""))).collect();
    let mut objekty: Vec<EntityType> = polozky.iter().map(|(b, s)| cogo::bod_entita(b, soubor, s)).collect();
    objekty.extend(cogo::grafika_rozmistena(&polozky, &[], m));
    if let Some(s) = skupina {
        let n = body.len().max(1) as f64;
        let stred = [body.iter().map(|b| b.x).sum::<f64>() / n, body.iter().map(|b| b.y).sum::<f64>() / n, 0.0];
        objekty.push(cogo::nosic_skupiny(&s, stred));
    }
    objekty
}

impl PotvrditImport {
    pub fn new(body: &[Bod], ro: &crate::styly::Rozlisovac, m: f64, soubor: &str, skupina: Option<SkupinaBodu>) -> Self {
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for b in body {
            x0 = x0.min(b.x);
            x1 = x1.max(b.x);
            y0 = y0.min(b.y);
            y1 = y1.max(b.y);
        }
        let r = ((x1 - x0).max(y1 - y0) / 200.0).clamp(0.05, 5.0);
        let c = [0.95, 0.65, 0.25, 1.0];
        let mut nahled = vec![PreviewWire::with_color(
            vec![[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0], [x0, y0, 0.0]],
            [0.6, 0.6, 0.6, 1.0],
        )];
        let krok = (body.len() / 3000).max(1);
        for b in body.iter().step_by(krok) {
            nahled.push(PreviewWire::line([b.x - r, b.y, 0.0], [b.x + r, b.y, 0.0], Some(c)));
            nahled.push(PreviewWire::line([b.x, b.y - r, 0.0], [b.x, b.y + r, 0.0], Some(c)));
        }
        let polozky: Vec<(Bod, crate::styly::StylBodu)> = body.iter().map(|b| (b.clone(), ro.styl(b, soubor, ""))).collect();
        let mut objekty: Vec<EntityType> = polozky.iter().map(|(b, s)| cogo::bod_entita(b, soubor, s)).collect();
        objekty.extend(cogo::grafika_rozmistena(&polozky, &[], m));
        let jmeno_skupiny = skupina.as_ref().map(|s| s.jmeno.clone());
        if let Some(s) = skupina {
            let n = body.len().max(1) as f64;
            let stred = [body.iter().map(|b| b.x).sum::<f64>() / n, body.iter().map(|b| b.y).sum::<f64>() / n, 0.0];
            objekty.push(cogo::nosic_skupiny(&s, stred));
        }
        Self { pocet: body.len(), skupina: jmeno_skupiny, objekty: Some(objekty), nahled }
    }
}

impl InteractiveCommand for PotvrditImport {
    fn prompt(&self) -> String {
        let skupina = self.skupina.as_ref().map(|s| trf!(" do skupiny „{}“", s)).unwrap_or_default();
        trf!("Importovat {} bodů{}? Enter = ano, Esc = zrušit (pohnutím myší se zobrazí náhled):", self.pocet, skupina)
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> CommandStep {
        self.on_enter()
    }
    fn on_cursor_move(&mut self, _pt: [f64; 3]) -> Vec<PreviewWire> {
        self.nahled.clone()
    }
    fn on_enter(&mut self) -> CommandStep {
        match self.objekty.take() {
            Some(o) => CommandStep::CommitManyAndEnd(o),
            None => CommandStep::Done,
        }
    }
}
