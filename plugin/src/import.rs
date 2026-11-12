//! Import bodů: načtení zdroje, rozpoznání formátu, kontrola, potvrzení
//! a vložení do výkresu (s volbou duplicit, skupiny a stylu).

use std::sync::{Mutex, OnceLock};

use ocs_plugin_api::host::{CommandStep, Document as CadDocument, EntityType, InteractiveCommand, PreviewWire};
use teren::preklad::tr;
use teren::trf;

use teren::body::{nacti_body, Bod, Desetinna, FormatBodu, Oddelovac};
use teren::duplicity::{self, Politika};
use teren::formaty::{self, DruhZdroje};
use teren::rozpoznani::{nazev_oddelovace, rozpoznej, s_formatem, shodne_s_existujicimi, souhrn, Puvod};
use teren::skupiny::SkupinaBodu;
use teren::z_vykresu::{self, Statistika, Text, Znacka};

use crate::cogo;
use crate::nastaveni;
use crate::povrchy;
use crate::spravce::{self, Akce};
use crate::styly;
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

/// Načte soubor bodů (text v rozpoznaném kódování, nebo převedený
/// LandXML / JobXML / GSI / Excel).
pub fn nacti_zdroj(cesta: &str, kodovani: Option<teren::kodovani::Kodovani>, list: usize) -> Result<formaty::Zdroj, String> {
    let bajty = std::fs::read(cesta).map_err(|e| trf!("nelze číst soubor {}: {}", cesta, e))?;
    formaty::nacti(cesta, &bajty, kodovani, list)
}

/// Načte soubor a připraví body k importu. Vypíše souhrn do příkazové řádky.
/// `format` = `None` → automatické rozpoznání; hlavička 0 = rozpoznat.
pub fn priprav(v: &mut dyn Vykres, cesta: &str, format: Option<(&str, Oddelovac, usize)>) -> Option<Vec<Bod>> {
    let zdroj = match nacti_zdroj(cesta, None, 0) {
        Ok(z) => z,
        Err(e) => {
            v.chyba(&trf!("Terén: {}", e));
            return None;
        }
    };
    let text = zdroj.text;

    let (fmt, popis_formatu) = match format {
        Some((f, o, hl)) => match FormatBodu::z_retezce(f, o) {
            Ok(mut fb) => {
                fb.preskocit_radku = if hl == 0 { s_formatem(&text, &fb, Puvod::Vzor).hlavicka } else { hl };
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
            if zdroj.druh != DruhZdroje::Text {
                popis = trf!("{} převedený na {}", zdroj.druh.nazev(), popis);
            }
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
    nastaveni::pridej_posledni(cesta);
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
/// `styl` = vlastní styl bodů („“ = podle skupiny a kódu).
pub fn objekty_importu(body: &[Bod], ro: &styly::Rozlisovac, m: f64, soubor: &str, skupina: Option<SkupinaBodu>, styl: &str) -> Vec<EntityType> {
    let polozky: Vec<(Bod, styly::StylBodu)> = body.iter().map(|b| (b.clone(), ro.styl(b, soubor, styl))).collect();
    let mut objekty: Vec<EntityType> = polozky
        .iter()
        .map(|(b, s)| cogo::bod_entita_styl(b, soubor, s, styl))
        .collect();
    objekty.extend(cogo::grafika_rozmistena(&polozky, &[], m));
    if let Some(s) = skupina {
        let n = body.len().max(1) as f64;
        let stred = [body.iter().map(|b| b.x).sum::<f64>() / n, body.iter().map(|b| b.y).sum::<f64>() / n, 0.0];
        objekty.push(cogo::nosic_skupiny(&s, stred));
    }
    objekty
}

impl PotvrditImport {
    pub fn new(body: &[Bod], ro: &styly::Rozlisovac, m: f64, soubor: &str, skupina: Option<SkupinaBodu>) -> Self {
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
        let jmeno_skupiny = skupina.as_ref().map(|s| s.jmeno.clone());
        let objekty = objekty_importu(body, ro, m, soubor, skupina, "");
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

// ── Import z okna náhledu ────────────────────────────────────────────────────

/// Kam body zařadit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CilSkupiny {
    /// Nová skupina „Import <soubor>“ (pokud soubor ještě žádná skupina nemá).
    Nova,
    /// Existující skupina (id) – soubor se přidá k jejím zdrojům.
    Existujici(String),
    Zadna,
}

/// Vše, co okno náhledu předá k importu.
#[derive(Debug, Clone)]
pub struct NastaveniImportu {
    /// Jméno zdroje ukládané k bodům (soubor nebo „schránka …“).
    pub soubor: String,
    /// Plná cesta (prázdná u schránky a výkresu).
    pub cesta: String,
    /// Text v podobě, jak ho ukázal náhled.
    pub text: String,
    pub format: String,
    pub oddelovac: Oddelovac,
    pub hlavicka: usize,
    pub desetinna: Desetinna,
    pub duplicity: Politika,
    pub skupina: CilSkupiny,
    /// Vlastní styl bodů („“ = podle skupiny a kódu).
    pub styl: String,
    pub priblizit: bool,
    /// Čísla řádků, které uživatel vyřadil.
    pub vynechat: Vec<usize>,
}

/// Vloží body podle nastavení z náhledu. Vrací zprávu pro uživatele.
pub fn proved_import(k: &mut dyn Vykres, n: NastaveniImportu) -> Result<String, String> {
    let mut f = FormatBodu::z_retezce(&n.format, n.oddelovac)?;
    f.preskocit_radku = n.hlavicka;
    f.desetinna = n.desetinna;
    let v = nacti_body(&n.text, &f);
    let chyb = v.chyby.len();
    let body: Vec<Bod> = v.body.into_iter().zip(v.radky).filter(|(_, r)| !n.vynechat.contains(r)).map(|(b, _)| b).collect();
    let vynechano = n.vynechat.len();
    if body.is_empty() {
        return Err(tr("není co importovat – žádný bod se nenačetl").into());
    }
    cogo::priprav_vykres(k)?;
    let doc = k.dokument()?;
    let vse = cogo::body(&doc);
    let existujici: Vec<Bod> = vse.iter().map(|b| b.bod.clone()).collect();
    let plan = duplicity::naplanuj(&body, &existujici, n.duplicity);

    let nova_skupina = match &n.skupina {
        CilSkupiny::Nova => skupina_importu(&doc, &n.soubor),
        CilSkupiny::Existujici(id) => {
            if let Some(mut s) = cogo::najdi_skupinu(&doc, id) {
                if !s.vse && !s.zdroje.iter().any(|z| z.eq_ignore_ascii_case(&n.soubor)) {
                    s.zdroje.push(n.soubor.clone());
                    cogo::uloz_skupinu(k, &doc, &s)?;
                }
            }
            None
        }
        CilSkupiny::Zadna => None,
    };
    let jmeno_skupiny = match &n.skupina {
        CilSkupiny::Nova => nova_skupina.as_ref().map(|s| s.jmeno.clone()),
        CilSkupiny::Existujici(id) => cogo::najdi_skupinu(&doc, id).map(|s| s.jmeno),
        CilSkupiny::Zadna => None,
    };

    // přepsat existující body (poloha, výška; popis jen když soubor nějaký má)
    if !plan.prepsat.is_empty() {
        let zmeny: Vec<cogo::ZmenaBodu> = plan
            .prepsat
            .iter()
            .map(|(i, b)| {
                let bv = vse[*i].clone();
                let mut novy = b.clone();
                novy.cislo = bv.bod.cislo.clone();
                if novy.popis.is_empty() {
                    novy.popis = bv.bod.popis.clone();
                }
                let styl = bv.styl.clone();
                (bv, novy, styl)
            })
            .collect();
        cogo::uprav_body(k, &doc, &zmeny, tr("Přepsat body"))?;
    }
    if !plan.pridat.is_empty() {
        let doc = k.dokument()?;
        let ro = styly::Rozlisovac::new(&doc).s_skupinou(nova_skupina.as_ref());
        let m = nastaveni::meritko(&doc);
        let obj = objekty_importu(&plan.pridat, &ro, m, &n.soubor, nova_skupina, &n.styl);
        k.zpet(tr("Import bodů"));
        k.pridej(obj)?;
    }
    k.hotovo();
    if !n.cesta.is_empty() {
        nastaveni::pridej_posledni(&n.cesta);
    }

    if n.priblizit {
        let vsechny: Vec<&Bod> = plan.pridat.iter().chain(plan.prepsat.iter().map(|(_, b)| b)).collect();
        if !vsechny.is_empty() {
            let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for b in &vsechny {
                x0 = x0.min(b.x);
                x1 = x1.max(b.x);
                y0 = y0.min(b.y);
                y1 = y1.max(b.y);
            }
            spravce::posli(Akce::Ukazat([(x0 + x1) / 2.0, (y0 + y1) / 2.0], ((x1 - x0).max(y1 - y0) / 2.0).max(5.0), Vec::new()));
        }
    }

    let mut t = match jmeno_skupiny {
        Some(s) => trf!("Importováno {} bodů do skupiny „{}“", plan.pridat.len(), s),
        None => trf!("Importováno {} bodů ze zdroje {}", plan.pridat.len(), n.soubor),
    };
    if !plan.prepsat.is_empty() {
        t.push_str(&trf!(" · přepsáno {}", plan.prepsat.len()));
    }
    if plan.preskoceno > 0 {
        t.push_str(&trf!(" · duplicit přeskočeno {}", plan.preskoceno));
    }
    if !plan.precislovano.is_empty() {
        t.push_str(&trf!(" · přečíslováno {}", plan.precislovano.len()));
    }
    if chyb > 0 {
        t.push_str(&trf!(" · nečitelných řádků {}", chyb));
    }
    if vynechano > 0 {
        t.push_str(&trf!(" · vyřazeno {}", vynechano));
    }
    Ok(t)
}

// ── Body z výkresu ───────────────────────────────────────────────────────────

/// Výsledek převodu bodů nakreslených ve výkrese (pro okno náhledu).
#[derive(Debug, Clone, Default)]
pub struct ZVykresu {
    pub text: String,
    pub statistika: Statistika,
    /// Hladiny se značkami a jejich počet.
    pub hladiny: Vec<(String, usize)>,
}

fn slot_z_vykresu() -> &'static Mutex<Option<Result<ZVykresu, String>>> {
    static S: OnceLock<Mutex<Option<Result<ZVykresu, String>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// Výsledek převodu, jakmile je hotový (jen jednou).
pub fn vezmi_z_vykresu() -> Option<Result<ZVykresu, String>> {
    slot_z_vykresu().lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Je hladina Terénu (vlastní body a grafika se nepřevádějí).
fn hladina_terenu(h: &str) -> bool {
    h.to_ascii_uppercase().starts_with("TER_")
}

/// Převede značky a texty z výkresu (celého nebo výběru, jen zvolené hladiny)
/// a výsledek uloží pro okno náhledu.
pub fn priprav_z_vykresu(k: &mut dyn Vykres, hladina: Option<&str>, jen_vyber: bool, tolerance: f64) -> Result<String, String> {
    let doc = k.dokument()?;
    let vyber: Vec<_> = if jen_vyber { k.vybrane() } else { Vec::new() };
    let mut znacky = Vec::new();
    let mut texty = Vec::new();
    let mut hladiny: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for e in doc.model_space_entities() {
        let c = e.common();
        if hladina_terenu(&c.layer) || (jen_vyber && !vyber.contains(&c.handle)) {
            continue;
        }
        if c.extended_data.get_record(povrchy::APP_BOD).is_some() {
            continue;
        }
        let na_hladine = hladina.is_none_or(|h| c.layer.eq_ignore_ascii_case(h));
        match e {
            EntityType::Point(p) => {
                *hladiny.entry(c.layer.clone()).or_default() += 1;
                if na_hladine {
                    znacky.push(Znacka { x: p.location.x, y: p.location.y, z: p.location.z, atributy: Vec::new() });
                }
            }
            EntityType::Insert(i) => {
                *hladiny.entry(c.layer.clone()).or_default() += 1;
                if na_hladine {
                    znacky.push(Znacka {
                        x: i.insert_point.x,
                        y: i.insert_point.y,
                        z: i.insert_point.z,
                        atributy: i.attributes.iter().map(|a| (a.tag.clone(), a.value.clone())).collect(),
                    });
                }
            }
            // texty se párují ze všech hladin (výšky bývají ve vlastní hladině)
            EntityType::Text(t) => texty.push(Text { x: t.insertion_point.x, y: t.insertion_point.y, vyska: t.height, hodnota: t.value.clone() }),
            EntityType::MText(t) => texty.push(Text {
                x: t.insertion_point.x,
                y: t.insertion_point.y,
                vyska: t.height,
                hodnota: z_vykresu::cisty_mtext(&t.value),
            }),
            _ => {}
        }
    }
    let (text, statistika) = z_vykresu::preved(&znacky, &texty, tolerance);
    let pocet = statistika.znacek;
    *slot_z_vykresu().lock().unwrap_or_else(|e| e.into_inner()) =
        Some(Ok(ZVykresu { text, statistika, hladiny: hladiny.into_iter().collect() }));
    Ok(trf!("Z výkresu připraveno {} bodů k náhledu", pocet))
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn hladiny_terenu_se_neprevadeji() {
        assert!(hladina_terenu("TER_BODY"));
        assert!(hladina_terenu("ter_body_popis"));
        assert!(!hladina_terenu("vytycovaci_body"));
    }

    #[test]
    fn jmeno_zdroje() {
        assert_eq!(jmeno_souboru(r"C:\Zakazky\Ulice\zamereni.txt"), "zamereni.txt");
        assert_eq!(jmeno_souboru("schránka"), "schránka");
    }
}
